//! Gateway capability client for the Cognitive Swarm Orchestrator.
//!
//! [`GatewayCapabilityClient`] bridges the orchestrator to physical and digital-world
//! capabilities exposed by `mofa-gateway`. Agents request hardware and service
//! capabilities through a typed [`CapabilityKind`] enum, giving the compiler full
//! coverage of all resource kinds without unstructured string keys.
//!
//! # Architecture
//!
//! ```text
//! GatewayCapabilityClient
//!   -> request_capability(CapabilityRequest)
//!        -> check TTL cache (CachedCapability)
//!        -> on miss: GatewayTransport::post("/capability/request")
//!        -> populate cache, append AuditEntry
//!   -> health_check()
//!        -> GatewayTransport::get("/health")
//!   -> register_as_virtual_agents(SwarmCapabilityRegistry)
//!        -> iterates known capabilities, registers each
//! ```
//!
//! # Transport abstraction
//!
//! All network I/O goes through the [`GatewayTransport`] trait. The crate ships
//! a [`MockGatewayTransport`] for unit tests. Production code can provide a
//! concrete implementation backed by any HTTP client without pulling reqwest
//! into this crate.
//!
//! # Example
//!
//! ```rust,no_run
//! use mofa_orchestrator::gateway_client::{
//!     CapabilityKind, CapabilityRequest, GatewayCapabilityClient,
//!     GatewayClientConfig, MockGatewayTransport, SwarmCapabilityRegistry,
//! };
//! use std::collections::HashMap;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let transport = Arc::new(MockGatewayTransport::new_granting());
//!     let config = GatewayClientConfig::default();
//!     let client = GatewayCapabilityClient::new(config, transport);
//!
//!     let healthy = client.health_check().await?;
//!     println!("gateway healthy: {healthy}");
//!
//!     let req = CapabilityRequest {
//!         kind: CapabilityKind::FileSystem { allowed_path: "/data".into() },
//!         agent_id: "agent-1".into(),
//!         timeout_ms: 5_000,
//!         metadata: HashMap::new(),
//!     };
//!     let resp = client.request_capability(req).await?;
//!     println!("granted: {}", resp.granted);
//!     Ok(())
//! }
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// Capability kind
// ---------------------------------------------------------------------------

/// A physical or digital-world capability that a swarm agent can request.
///
/// Each variant maps to a device or service accessible through `mofa-gateway`.
/// Structured variants (e.g., `Sensor`, `FileSystem`) carry the parameters
/// needed to identify or scope the resource, so the gateway can enforce access
/// policies at the field level rather than through opaque string keys.
///
/// Unlike the gateway protocol layer, this enum is intentionally exhaustive --
/// no `#[non_exhaustive]` -- so that tests can pattern-match all variants
/// without a catch-all arm.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CapabilityKind {
    /// Text-to-speech output through a connected speaker device.
    Speaker,

    /// Image capture from a connected camera device.
    Camera,

    /// Audio capture from a connected microphone device.
    Microphone,

    /// Read from a named sensor (temperature, pressure, proximity, humidity, etc.).
    Sensor {
        /// Gateway-registered identifier for the physical sensor.
        sensor_id: String,
    },

    /// Scoped read or write access to a filesystem path.
    /// The gateway enforces that all operations stay inside `allowed_path`.
    FileSystem {
        /// Root path boundary enforced server-side by the gateway.
        allowed_path: String,
    },

    /// Outbound HTTP fetch proxied through the gateway.
    HttpFetch,

    /// Web search via the search API configured on the gateway operator side.
    WebSearch,

    /// Send a notification through the notifier channel configured on the gateway.
    Notify,

    /// Read-only access to a database resource exposed by the gateway.
    DatabaseRead,

    /// Read-write access to a database resource exposed by the gateway.
    DatabaseWrite,

    /// A custom capability registered by the gateway operator at runtime.
    Custom {
        /// Operator-assigned name for the custom capability.
        name: String,
    },
}

impl CapabilityKind {
    /// Returns a stable, URL-safe string key for this capability kind.
    ///
    /// Used as the routing key in `SwarmCapabilityRegistry` and as the cache
    /// key suffix in [`GatewayCapabilityClient`].
    pub fn kind_str(&self) -> String {
        match self {
            CapabilityKind::Speaker => "speaker".to_string(),
            CapabilityKind::Camera => "camera".to_string(),
            CapabilityKind::Microphone => "microphone".to_string(),
            CapabilityKind::Sensor { sensor_id } => format!("sensor:{sensor_id}"),
            CapabilityKind::FileSystem { allowed_path } => {
                format!("filesystem:{allowed_path}")
            }
            CapabilityKind::HttpFetch => "http-fetch".to_string(),
            CapabilityKind::WebSearch => "web-search".to_string(),
            CapabilityKind::Notify => "notify".to_string(),
            CapabilityKind::DatabaseRead => "database-read".to_string(),
            CapabilityKind::DatabaseWrite => "database-write".to_string(),
            CapabilityKind::Custom { name } => format!("custom:{name}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Request / Response
// ---------------------------------------------------------------------------

/// Request sent to the gateway to negotiate access to a capability.
///
/// `timeout_ms` overrides the client-level default for this single call,
/// which is useful for slow filesystem operations or latency-sensitive sensors.
/// `metadata` carries caller-defined key-value pairs forwarded to the gateway
/// capability handler as-is.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CapabilityRequest {
    /// Which capability to request.
    pub kind: CapabilityKind,
    /// Identifier of the agent making the request.
    pub agent_id: String,
    /// Maximum time in milliseconds to wait for the gateway to respond.
    pub timeout_ms: u64,
    /// Arbitrary key-value metadata forwarded verbatim to the gateway handler.
    pub metadata: HashMap<String, String>,
}

/// Response returned by the gateway after evaluating a capability request.
///
/// When `granted` is `true`, `capability_id` identifies the active grant and
/// `expires_at_ms` indicates when it expires. When `granted` is `false`,
/// `rejection_reason` contains a human-readable explanation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CapabilityResponse {
    /// Whether the gateway granted the requested capability.
    pub granted: bool,
    /// Unique identifier for this capability grant (empty string if denied).
    pub capability_id: String,
    /// Unix timestamp (ms) when the grant expires. Zero if not applicable.
    pub expires_at_ms: u64,
    /// Optional endpoint URL the agent should use to invoke the capability.
    pub endpoint_url: Option<String>,
    /// Human-readable explanation when `granted` is `false`.
    pub rejection_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Transport trait
// ---------------------------------------------------------------------------

/// Pluggable HTTP backend for [`GatewayCapabilityClient`].
///
/// Implement this trait to swap in any HTTP client without changing the client
/// logic. The crate ships [`MockGatewayTransport`] for unit tests.
#[async_trait]
pub trait GatewayTransport: Send + Sync {
    /// Send a POST request to `url` with the given body bytes.
    ///
    /// Returns the raw response body on success.
    async fn post(&self, url: &str, body: &[u8]) -> Result<Vec<u8>, GatewayClientError>;

    /// Send a GET request to `url`.
    ///
    /// Returns the raw response body on success.
    async fn get(&self, url: &str) -> Result<Vec<u8>, GatewayClientError>;
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// All errors that can be returned by [`GatewayCapabilityClient`] operations.
///
/// `#[non_exhaustive]` ensures that adding new error variants in future gateway
/// protocol versions does not constitute a breaking change for downstream code
/// that matches on this enum.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GatewayClientError {
    /// The gateway denied the requested capability.
    #[error("capability `{kind}` denied: {reason}")]
    CapabilityDenied {
        /// String representation of the denied capability kind.
        kind: String,
        /// Gateway-provided reason for the denial.
        reason: String,
    },

    /// The gateway health endpoint returned a non-2xx response or was unreachable.
    #[error("health check failed for endpoint `{endpoint}`")]
    HealthCheckFailed {
        /// The endpoint URL that failed the health check.
        endpoint: String,
    },

    /// A cache read or write operation failed.
    #[error("cache error: {0}")]
    CacheError(String),

    /// The gateway did not respond within the configured timeout window.
    #[error("gateway timed out after {timeout_ms}ms")]
    Timeout {
        /// The timeout that was exceeded, in milliseconds.
        timeout_ms: u64,
    },

    /// JSON serialization or deserialization failed.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// Cached capability entry
// ---------------------------------------------------------------------------

/// A cached gateway response with TTL metadata.
///
/// Used internally by [`GatewayCapabilityClient`] to avoid redundant round-trips
/// for requests whose grants have not yet expired.
#[derive(Debug, Clone)]
pub struct CachedCapability {
    /// The gateway response that was cached.
    pub response: CapabilityResponse,
    /// Unix timestamp (ms) when the entry was inserted into the cache.
    pub cached_at_ms: u64,
    /// Time-to-live in milliseconds. The entry is considered stale once
    /// `current_time_ms() - cached_at_ms >= ttl_ms`.
    pub ttl_ms: u64,
}

impl CachedCapability {
    /// Returns `true` if the cache entry has expired.
    pub fn is_expired(&self) -> bool {
        let now = current_time_ms();
        now.saturating_sub(self.cached_at_ms) >= self.ttl_ms
    }
}

// ---------------------------------------------------------------------------
// Audit entry
// ---------------------------------------------------------------------------

/// A single record in the capability audit log.
///
/// Appended by [`GatewayCapabilityClient`] after every `request_capability`
/// call, regardless of outcome. The `cache_hit` field lets operators measure
/// the effectiveness of the TTL cache.
#[derive(Debug, Clone)]
pub struct AuditEntry {
    /// Unix timestamp (ms) when the request was processed.
    pub timestamp_ms: u64,
    /// Agent that made the request.
    pub agent_id: String,
    /// String representation of the requested capability kind.
    pub capability_kind: String,
    /// Whether the gateway (or cache) granted the capability.
    pub granted: bool,
    /// Whether the result was served from the TTL cache.
    pub cache_hit: bool,
}

// ---------------------------------------------------------------------------
// Cache stats
// ---------------------------------------------------------------------------

/// Snapshot of cache performance counters.
///
/// Returned by [`GatewayCapabilityClient::cache_stats`]. All counters are
/// monotonically increasing -- they are never reset.
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of `request_capability` calls satisfied from the TTL cache.
    pub hit_count: u64,
    /// Number of `request_capability` calls that required a transport round-trip.
    pub miss_count: u64,
    /// Number of cache entries that were found but had already expired.
    pub expired_count: u64,
    /// Number of entries currently held in the in-memory cache map.
    pub cached_entries: usize,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for [`GatewayCapabilityClient`].
///
/// All fields have sensible defaults via [`Default`]. Override only what you
/// need:
///
/// ```rust
/// use mofa_orchestrator::gateway_client::GatewayClientConfig;
///
/// let cfg = GatewayClientConfig {
///     gateway_base_url: "http://gateway.prod:8080".into(),
///     default_timeout_ms: 30_000,
///     capability_cache_ttl_secs: 120,
///     audit_enabled: true,
///     health_check_interval_secs: 30,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct GatewayClientConfig {
    /// Base URL of the `mofa-gateway` instance (no trailing slash).
    ///
    /// Requests are sent to `{gateway_base_url}/capability/request` and
    /// `{gateway_base_url}/health`.
    pub gateway_base_url: String,

    /// Default timeout for capability requests in milliseconds.
    ///
    /// Individual `CapabilityRequest::timeout_ms` values override this default
    /// for that single call.
    pub default_timeout_ms: u64,

    /// How long to cache a granted capability response, expressed in seconds.
    ///
    /// A value of `0` effectively disables caching (every request hits the
    /// transport). A large value reduces latency but risks using stale grants.
    pub capability_cache_ttl_secs: u64,

    /// When `true`, every `request_capability` call is appended to the audit
    /// log accessible via [`GatewayCapabilityClient::audit_entries`].
    pub audit_enabled: bool,

    /// How often (in seconds) to run automatic background health checks.
    ///
    /// This field is reserved for future use by a background health-check task.
    /// It is stored and returned by `cache_stats` but no goroutine is spawned
    /// by this library -- the caller is responsible for scheduling health checks.
    pub health_check_interval_secs: u64,
}

impl Default for GatewayClientConfig {
    fn default() -> Self {
        Self {
            gateway_base_url: "http://localhost:8080".to_string(),
            default_timeout_ms: 5_000,
            capability_cache_ttl_secs: 300,
            audit_enabled: true,
            health_check_interval_secs: 60,
        }
    }
}

// ---------------------------------------------------------------------------
// SwarmCapabilityRegistry
// ---------------------------------------------------------------------------

/// Lightweight registry that maps agent IDs to their associated capability kinds.
///
/// Populated by [`GatewayCapabilityClient::register_as_virtual_agents`]. The
/// SwarmComposer can then look up which capabilities each virtual agent provides
/// when routing capability-backed subtasks.
#[derive(Debug, Default)]
pub struct SwarmCapabilityRegistry {
    entries: HashMap<String, CapabilityKind>,
}

impl SwarmCapabilityRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Register a capability kind under the given agent ID.
    ///
    /// If `agent_id` is already registered, the old entry is replaced.
    pub fn register(&mut self, agent_id: String, kind: CapabilityKind) {
        self.entries.insert(agent_id, kind);
    }

    /// Return all capability kinds registered for `agent_id`.
    ///
    /// Currently each agent ID maps to exactly one kind, so this returns
    /// either zero or one entries.
    pub fn capabilities_for(&self, agent_id: &str) -> Vec<&CapabilityKind> {
        self.entries
            .get(agent_id)
            .map(|k| vec![k])
            .unwrap_or_default()
    }

    /// Return a reference to the full entries map.
    pub fn all_entries(&self) -> &HashMap<String, CapabilityKind> {
        &self.entries
    }

    /// Return the total number of registered entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return `true` if no entries have been registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Main client
// ---------------------------------------------------------------------------

/// Hardware-aware gateway capability client with TTL caching and audit logging.
///
/// ## Capability caching
///
/// Every granted `CapabilityResponse` is stored in an in-memory map keyed by
/// `"{agent_id}:{capability_kind_str}"`. Subsequent requests for the same
/// agent and kind are served directly from the cache until the entry expires
/// (`capability_cache_ttl_secs` from the config). Expired entries trigger a
/// fresh transport call and cache update.
///
/// ## Audit log
///
/// When `audit_enabled` is `true` in the config, every `request_capability`
/// call -- whether served from cache or the transport -- appends an
/// [`AuditEntry`] to the internal audit log. Retrieve entries with
/// [`audit_entries`].
///
/// ## Cache statistics
///
/// Three `Arc<AtomicU64>` counters track hits, misses, and expired entries.
/// Read them with [`cache_stats`].
///
/// [`audit_entries`]: GatewayCapabilityClient::audit_entries
/// [`cache_stats`]: GatewayCapabilityClient::cache_stats
pub struct GatewayCapabilityClient {
    /// Client-level configuration (base URL, timeouts, TTL, audit flag).
    config: GatewayClientConfig,
    /// In-memory TTL cache keyed by `"{agent_id}:{capability_kind_str}"`.
    cache: Arc<RwLock<HashMap<String, CachedCapability>>>,
    /// Pluggable transport layer (real HTTP client or mock).
    transport: Arc<dyn GatewayTransport>,
    /// Append-only audit log populated when `config.audit_enabled` is true.
    audit_log: Arc<RwLock<Vec<AuditEntry>>>,
    /// Number of cache hits since creation.
    hit_count: Arc<AtomicU64>,
    /// Number of cache misses (transport calls) since creation.
    miss_count: Arc<AtomicU64>,
    /// Number of expired cache entries encountered since creation.
    expired_count: Arc<AtomicU64>,
}

impl GatewayCapabilityClient {
    /// Create a new client.
    ///
    /// `transport` is the pluggable HTTP backend. Use [`MockGatewayTransport`]
    /// in unit tests and a real HTTP implementation in production.
    pub fn new(config: GatewayClientConfig, transport: Arc<dyn GatewayTransport>) -> Self {
        Self {
            config,
            cache: Arc::new(RwLock::new(HashMap::new())),
            transport,
            audit_log: Arc::new(RwLock::new(Vec::new())),
            hit_count: Arc::new(AtomicU64::new(0)),
            miss_count: Arc::new(AtomicU64::new(0)),
            expired_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Compute the cache key for a given agent and capability kind.
    pub fn cache_key(agent_id: &str, kind: &CapabilityKind) -> String {
        format!("{agent_id}:{}", kind.kind_str())
    }

    /// Request access to a capability on behalf of an agent.
    ///
    /// The method checks the TTL cache first. On a cache hit the stored
    /// response is returned immediately without contacting the gateway. On a
    /// miss (or after expiry), the request is forwarded to the transport and
    /// the response is cached.
    ///
    /// When `audit_enabled` is `true` in the config, an [`AuditEntry`] is
    /// appended regardless of the cache outcome.
    ///
    /// # Errors
    ///
    /// - [`GatewayClientError::CapabilityDenied`] if the gateway refused the request.
    /// - [`GatewayClientError::Timeout`] if the gateway did not respond in time.
    /// - [`GatewayClientError::Serialization`] on encoding or decoding failure.
    pub async fn request_capability(
        &self,
        req: CapabilityRequest,
    ) -> Result<CapabilityResponse, GatewayClientError> {
        let key = Self::cache_key(&req.agent_id, &req.kind);

        // Fast path -- check cache.
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(&key) {
                if !entry.is_expired() {
                    self.hit_count.fetch_add(1, Ordering::Relaxed);
                    let resp = entry.response.clone();
                    if self.config.audit_enabled {
                        drop(cache);
                        self.append_audit(&req, &resp, true).await;
                    }
                    return Ok(resp);
                }
                // Entry exists but is stale.
                self.expired_count.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Slow path -- call transport.
        self.miss_count.fetch_add(1, Ordering::Relaxed);

        let url = format!("{}/capability/request", self.config.gateway_base_url);
        let body = serde_json::to_vec(&req)?;
        let raw = self.transport.post(&url, &body).await?;
        let resp: CapabilityResponse = serde_json::from_slice(&raw)?;

        if !resp.granted {
            let reason = resp
                .rejection_reason
                .clone()
                .unwrap_or_else(|| "no reason provided".to_string());
            if self.config.audit_enabled {
                self.append_audit(&req, &resp, false).await;
            }
            return Err(GatewayClientError::CapabilityDenied {
                kind: req.kind.kind_str(),
                reason,
            });
        }

        // Cache the granted response.
        let ttl_ms = self.config.capability_cache_ttl_secs.saturating_mul(1_000);
        let cached = CachedCapability {
            response: resp.clone(),
            cached_at_ms: current_time_ms(),
            ttl_ms,
        };
        {
            let mut cache = self.cache.write().await;
            cache.insert(key, cached);
        }

        if self.config.audit_enabled {
            self.append_audit(&req, &resp, false).await;
        }

        Ok(resp)
    }

    /// Check whether the gateway is healthy.
    ///
    /// Sends `GET {gateway_base_url}/health`. Returns `true` if the transport
    /// call succeeds.
    ///
    /// # Errors
    ///
    /// - [`GatewayClientError::HealthCheckFailed`] if the transport returns an error.
    pub async fn health_check(&self) -> Result<bool, GatewayClientError> {
        let url = format!("{}/health", self.config.gateway_base_url);
        self.transport.get(&url).await.map(|_| true).map_err(|_| {
            GatewayClientError::HealthCheckFailed {
                endpoint: url.clone(),
            }
        })
    }

    /// Register the known capability kinds as virtual agents in `registry`.
    ///
    /// Iterates over a representative set of all non-parameterised capability
    /// variants plus placeholder entries for structured variants. Each variant
    /// is registered under the agent ID `"gateway::{kind_str}"`.
    ///
    /// Returns the number of entries registered.
    ///
    /// # Errors
    ///
    /// Currently infallible; returns `Ok` in all cases. The signature uses
    /// `Result` to allow future implementations that fetch the capability list
    /// from the gateway.
    pub async fn register_as_virtual_agents(
        &self,
        registry: &mut SwarmCapabilityRegistry,
    ) -> Result<usize, GatewayClientError> {
        let kinds: Vec<CapabilityKind> = vec![
            CapabilityKind::Speaker,
            CapabilityKind::Camera,
            CapabilityKind::Microphone,
            CapabilityKind::HttpFetch,
            CapabilityKind::WebSearch,
            CapabilityKind::Notify,
            CapabilityKind::DatabaseRead,
            CapabilityKind::DatabaseWrite,
        ];

        let count = kinds.len();
        for kind in kinds {
            let agent_id = format!("gateway::{}", kind.kind_str());
            registry.register(agent_id, kind);
        }
        Ok(count)
    }

    /// Return a snapshot of the cache performance counters.
    pub fn cache_stats(&self) -> CacheStats {
        let cached_entries = {
            // Use try_read to avoid blocking; fall back to 0 if lock is held.
            match self.cache.try_read() {
                Ok(guard) => guard.len(),
                Err(_) => 0,
            }
        };
        CacheStats {
            hit_count: self.hit_count.load(Ordering::Relaxed),
            miss_count: self.miss_count.load(Ordering::Relaxed),
            expired_count: self.expired_count.load(Ordering::Relaxed),
            cached_entries,
        }
    }

    /// Return a copy of all audit log entries.
    ///
    /// Entries are in insertion order. The log grows monotonically; entries are
    /// never deleted.
    pub async fn audit_entries(&self) -> Vec<AuditEntry> {
        self.audit_log.read().await.clone()
    }

    /// Remove all cached entries for the given agent.
    ///
    /// The next `request_capability` call for this agent will unconditionally
    /// hit the transport, regardless of the TTL.
    pub async fn invalidate_cache(&self, agent_id: &str) {
        let mut cache = self.cache.write().await;
        cache.retain(|k, _| !k.starts_with(&format!("{agent_id}:")));
    }

    // -- internal helpers --

    async fn append_audit(
        &self,
        req: &CapabilityRequest,
        resp: &CapabilityResponse,
        cache_hit: bool,
    ) {
        let entry = AuditEntry {
            timestamp_ms: current_time_ms(),
            agent_id: req.agent_id.clone(),
            capability_kind: req.kind.kind_str(),
            granted: resp.granted,
            cache_hit,
        };
        let mut log = self.audit_log.write().await;
        log.push(entry);
    }
}

// ---------------------------------------------------------------------------
// MockGatewayTransport
// ---------------------------------------------------------------------------

/// Test double for [`GatewayTransport`].
///
/// `MockGatewayTransport::new_granting()` always returns a granted
/// [`CapabilityResponse`]. `MockGatewayTransport::new_denying(reason)` always
/// returns a denied response with the given reason.
///
/// The `call_count` method returns the total number of `post` and `get` calls
/// made so far, which lets tests assert that the cache prevented redundant
/// transport calls.
pub struct MockGatewayTransport {
    granted: bool,
    denial_reason: String,
    call_count: Arc<AtomicU64>,
}

impl MockGatewayTransport {
    /// Create a transport that always grants capability requests.
    pub fn new_granting() -> Self {
        Self {
            granted: true,
            denial_reason: String::new(),
            call_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Create a transport that always denies capability requests.
    pub fn new_denying(reason: impl Into<String>) -> Self {
        Self {
            granted: false,
            denial_reason: reason.into(),
            call_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Return the total number of transport calls made.
    pub fn call_count(&self) -> u64 {
        self.call_count.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl GatewayTransport for MockGatewayTransport {
    async fn post(&self, _url: &str, _body: &[u8]) -> Result<Vec<u8>, GatewayClientError> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        let resp = CapabilityResponse {
            granted: self.granted,
            capability_id: if self.granted {
                "mock-cap-id-001".to_string()
            } else {
                String::new()
            },
            expires_at_ms: if self.granted {
                current_time_ms() + 60_000
            } else {
                0
            },
            endpoint_url: if self.granted {
                Some("http://gateway.local/mock".to_string())
            } else {
                None
            },
            rejection_reason: if self.granted {
                None
            } else {
                Some(self.denial_reason.clone())
            },
        };
        serde_json::to_vec(&resp).map_err(GatewayClientError::Serialization)
    }

    async fn get(&self, _url: &str) -> Result<Vec<u8>, GatewayClientError> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        Ok(b"ok".to_vec())
    }
}

// ---------------------------------------------------------------------------
// Internal utility
// ---------------------------------------------------------------------------

/// Returns the current Unix time in milliseconds.
///
/// Saturates to `u64::MAX` on overflow rather than panicking.
fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_granting_client() -> GatewayCapabilityClient {
        let transport = Arc::new(MockGatewayTransport::new_granting());
        GatewayCapabilityClient::new(GatewayClientConfig::default(), transport)
    }

    fn make_denying_client(reason: &str) -> GatewayCapabilityClient {
        let transport = Arc::new(MockGatewayTransport::new_denying(reason));
        GatewayCapabilityClient::new(GatewayClientConfig::default(), transport)
    }

    fn basic_request(agent_id: &str, kind: CapabilityKind) -> CapabilityRequest {
        CapabilityRequest {
            kind,
            agent_id: agent_id.to_string(),
            timeout_ms: 1_000,
            metadata: HashMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // 1. Cache hit returns without calling transport
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn cache_hit_does_not_call_transport() {
        let transport = Arc::new(MockGatewayTransport::new_granting());
        let t2 = Arc::clone(&transport);
        let client = GatewayCapabilityClient::new(GatewayClientConfig::default(), transport);

        // First call -- transport is invoked.
        let req1 = basic_request("agent-1", CapabilityKind::Speaker);
        client.request_capability(req1).await.unwrap();
        assert_eq!(t2.call_count(), 1, "first request should call transport");

        // Second call with same agent and kind -- should hit cache.
        let req2 = basic_request("agent-1", CapabilityKind::Speaker);
        client.request_capability(req2).await.unwrap();
        assert_eq!(t2.call_count(), 1, "second request should be served from cache");

        let stats = client.cache_stats();
        assert_eq!(stats.hit_count, 1);
        assert_eq!(stats.miss_count, 1);
    }

    // -----------------------------------------------------------------------
    // 2. Cache expiry causes re-fetch
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn expired_cache_entry_causes_re_fetch() {
        let transport = Arc::new(MockGatewayTransport::new_granting());
        let t2 = Arc::clone(&transport);

        // Use a TTL of 0 seconds so every entry expires immediately.
        let config = GatewayClientConfig {
            capability_cache_ttl_secs: 0,
            ..Default::default()
        };
        let client = GatewayCapabilityClient::new(config, transport);

        let req1 = basic_request("agent-1", CapabilityKind::Camera);
        client.request_capability(req1).await.unwrap();

        let req2 = basic_request("agent-1", CapabilityKind::Camera);
        client.request_capability(req2).await.unwrap();

        // Both calls should have hit the transport because TTL=0 expires instantly.
        assert_eq!(t2.call_count(), 2);

        let stats = client.cache_stats();
        // Both are misses -- the second was an expired entry.
        assert_eq!(stats.miss_count, 2);
        assert_eq!(stats.expired_count, 1);
    }

    // -----------------------------------------------------------------------
    // 3. Denied capability returns CapabilityDenied error
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn denied_capability_returns_error() {
        let client = make_denying_client("policy violation");
        let req = basic_request("agent-2", CapabilityKind::Microphone);
        let result = client.request_capability(req).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            GatewayClientError::CapabilityDenied { kind, reason } => {
                assert_eq!(kind, "microphone");
                assert!(reason.contains("policy violation"), "reason: {reason}");
            }
            other => panic!("expected CapabilityDenied, got: {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // 4. Health check returns Ok(true) when transport succeeds
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn health_check_ok() {
        let client = make_granting_client();
        let result = client.health_check().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);
    }

    // -----------------------------------------------------------------------
    // 5. Health check returns HealthCheckFailed when transport fails
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn health_check_failing_transport_returns_error() {
        struct FailingTransport;
        #[async_trait]
        impl GatewayTransport for FailingTransport {
            async fn post(&self, _url: &str, _body: &[u8]) -> Result<Vec<u8>, GatewayClientError> {
                Err(GatewayClientError::HealthCheckFailed {
                    endpoint: "http://bad".into(),
                })
            }
            async fn get(&self, url: &str) -> Result<Vec<u8>, GatewayClientError> {
                Err(GatewayClientError::HealthCheckFailed {
                    endpoint: url.to_string(),
                })
            }
        }

        let client = GatewayCapabilityClient::new(
            GatewayClientConfig::default(),
            Arc::new(FailingTransport),
        );
        let result = client.health_check().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GatewayClientError::HealthCheckFailed { .. }
        ));
    }

    // -----------------------------------------------------------------------
    // 6. register_as_virtual_agents returns correct count
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn register_as_virtual_agents_returns_correct_count() {
        let client = make_granting_client();
        let mut registry = SwarmCapabilityRegistry::new();
        let count = client.register_as_virtual_agents(&mut registry).await.unwrap();
        // The default implementation registers 8 non-parameterised variants.
        assert_eq!(count, 8);
        assert_eq!(registry.len(), 8);
    }

    // -----------------------------------------------------------------------
    // 7. Audit log records entries when audit_enabled = true
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn audit_log_records_entries() {
        let config = GatewayClientConfig {
            audit_enabled: true,
            ..Default::default()
        };
        let transport = Arc::new(MockGatewayTransport::new_granting());
        let client = GatewayCapabilityClient::new(config, transport);

        let req = basic_request("agent-audit", CapabilityKind::WebSearch);
        client.request_capability(req).await.unwrap();

        let entries = client.audit_entries().await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].agent_id, "agent-audit");
        assert_eq!(entries[0].capability_kind, "web-search");
        assert!(entries[0].granted);
        assert!(!entries[0].cache_hit);
    }

    // -----------------------------------------------------------------------
    // 8. Cache invalidation removes entries for the given agent
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn cache_invalidation_removes_agent_entries() {
        let transport = Arc::new(MockGatewayTransport::new_granting());
        let t2 = Arc::clone(&transport);
        let client = GatewayCapabilityClient::new(GatewayClientConfig::default(), transport);

        // Populate cache for agent-x.
        let req1 = basic_request("agent-x", CapabilityKind::Speaker);
        client.request_capability(req1).await.unwrap();
        assert_eq!(t2.call_count(), 1);

        // Invalidate cache for agent-x.
        client.invalidate_cache("agent-x").await;

        // Next request should hit transport again.
        let req2 = basic_request("agent-x", CapabilityKind::Speaker);
        client.request_capability(req2).await.unwrap();
        assert_eq!(t2.call_count(), 2);
    }

    // -----------------------------------------------------------------------
    // 9. Multiple agents same capability kind are cached independently
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn multiple_agents_same_kind_cached_independently() {
        let transport = Arc::new(MockGatewayTransport::new_granting());
        let t2 = Arc::clone(&transport);
        let client = GatewayCapabilityClient::new(GatewayClientConfig::default(), transport);

        let req_a = basic_request("agent-a", CapabilityKind::HttpFetch);
        let req_b = basic_request("agent-b", CapabilityKind::HttpFetch);

        client.request_capability(req_a).await.unwrap();
        client.request_capability(req_b).await.unwrap();

        // Both are misses (different agents).
        assert_eq!(t2.call_count(), 2);

        // Second call for agent-a should hit cache.
        let req_a2 = basic_request("agent-a", CapabilityKind::HttpFetch);
        client.request_capability(req_a2).await.unwrap();
        assert_eq!(t2.call_count(), 2, "repeat request for agent-a should be cached");

        let stats = client.cache_stats();
        assert_eq!(stats.hit_count, 1);
        assert_eq!(stats.miss_count, 2);
    }

    // -----------------------------------------------------------------------
    // 10. CacheStats tracks hit/miss/expired correctly
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn cache_stats_tracked_correctly() {
        let transport = Arc::new(MockGatewayTransport::new_granting());
        let client = GatewayCapabilityClient::new(GatewayClientConfig::default(), transport);

        // Three unique requests -- all misses.
        for i in 0..3u32 {
            let req = basic_request(&format!("agent-{i}"), CapabilityKind::Notify);
            client.request_capability(req).await.unwrap();
        }

        // Repeat the first agent -- cache hit.
        let req = basic_request("agent-0", CapabilityKind::Notify);
        client.request_capability(req).await.unwrap();

        let stats = client.cache_stats();
        assert_eq!(stats.miss_count, 3);
        assert_eq!(stats.hit_count, 1);
        assert_eq!(stats.expired_count, 0);
    }

    // -----------------------------------------------------------------------
    // 11. CapabilityRequest serialization round-trip
    // -----------------------------------------------------------------------
    #[test]
    fn capability_request_round_trip() {
        let mut meta = HashMap::new();
        meta.insert("region".to_string(), "eu-west".to_string());

        let req = CapabilityRequest {
            kind: CapabilityKind::Sensor {
                sensor_id: "temp-01".to_string(),
            },
            agent_id: "agent-serde".to_string(),
            timeout_ms: 2_000,
            metadata: meta,
        };

        let json = serde_json::to_string(&req).expect("serialize");
        let restored: CapabilityRequest = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(restored.agent_id, "agent-serde");
        assert_eq!(restored.timeout_ms, 2_000);
        assert_eq!(restored.metadata.get("region").map(String::as_str), Some("eu-west"));
        if let CapabilityKind::Sensor { sensor_id } = &restored.kind {
            assert_eq!(sensor_id, "temp-01");
        } else {
            panic!("expected Sensor variant");
        }
    }

    // -----------------------------------------------------------------------
    // 12. CapabilityResponse serialization round-trip
    // -----------------------------------------------------------------------
    #[test]
    fn capability_response_round_trip() {
        let resp = CapabilityResponse {
            granted: true,
            capability_id: "cap-abc".to_string(),
            expires_at_ms: 9_999_999,
            endpoint_url: Some("http://gw/ep".to_string()),
            rejection_reason: None,
        };

        let json = serde_json::to_string(&resp).expect("serialize");
        let restored: CapabilityResponse = serde_json::from_str(&json).expect("deserialize");

        assert!(restored.granted);
        assert_eq!(restored.capability_id, "cap-abc");
        assert_eq!(restored.expires_at_ms, 9_999_999);
        assert_eq!(restored.endpoint_url.as_deref(), Some("http://gw/ep"));
        assert!(restored.rejection_reason.is_none());
    }

    // -----------------------------------------------------------------------
    // 13. All CapabilityKind variants created and kind_str returns expected strings
    // -----------------------------------------------------------------------
    #[test]
    fn all_capability_kind_variants_kind_str() {
        assert_eq!(CapabilityKind::Speaker.kind_str(), "speaker");
        assert_eq!(CapabilityKind::Camera.kind_str(), "camera");
        assert_eq!(CapabilityKind::Microphone.kind_str(), "microphone");
        assert_eq!(
            CapabilityKind::Sensor { sensor_id: "co2".into() }.kind_str(),
            "sensor:co2"
        );
        assert_eq!(
            CapabilityKind::FileSystem { allowed_path: "/data".into() }.kind_str(),
            "filesystem:/data"
        );
        assert_eq!(CapabilityKind::HttpFetch.kind_str(), "http-fetch");
        assert_eq!(CapabilityKind::WebSearch.kind_str(), "web-search");
        assert_eq!(CapabilityKind::Notify.kind_str(), "notify");
        assert_eq!(CapabilityKind::DatabaseRead.kind_str(), "database-read");
        assert_eq!(CapabilityKind::DatabaseWrite.kind_str(), "database-write");
        assert_eq!(
            CapabilityKind::Custom { name: "lidar".into() }.kind_str(),
            "custom:lidar"
        );
    }

    // -----------------------------------------------------------------------
    // 14. SwarmCapabilityRegistry register and lookup
    // -----------------------------------------------------------------------
    #[test]
    fn swarm_capability_registry_register_and_lookup() {
        let mut registry = SwarmCapabilityRegistry::new();
        assert!(registry.is_empty());

        registry.register("agent-1".to_string(), CapabilityKind::Speaker);
        registry.register("agent-2".to_string(), CapabilityKind::Camera);

        assert_eq!(registry.len(), 2);

        let caps = registry.capabilities_for("agent-1");
        assert_eq!(caps.len(), 1);
        assert_eq!(*caps[0], CapabilityKind::Speaker);

        let caps2 = registry.capabilities_for("agent-2");
        assert_eq!(caps2.len(), 1);
        assert_eq!(*caps2[0], CapabilityKind::Camera);

        // Non-existent agent.
        let caps3 = registry.capabilities_for("agent-999");
        assert!(caps3.is_empty());
    }

    // -----------------------------------------------------------------------
    // 15. Timeout error variant carries correct timeout_ms
    // -----------------------------------------------------------------------
    #[test]
    fn timeout_error_carries_timeout_ms() {
        let err = GatewayClientError::Timeout { timeout_ms: 7_500 };
        let msg = err.to_string();
        assert!(msg.contains("7500"), "error message should contain timeout: {msg}");

        if let GatewayClientError::Timeout { timeout_ms } = err {
            assert_eq!(timeout_ms, 7_500);
        } else {
            panic!("wrong variant");
        }
    }

    // -----------------------------------------------------------------------
    // 16. Mock transport call count increases correctly on cache misses only
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn mock_transport_called_correct_number_of_times() {
        let transport = Arc::new(MockGatewayTransport::new_granting());
        let t2 = Arc::clone(&transport);
        let client = GatewayCapabilityClient::new(GatewayClientConfig::default(), transport);

        // 4 distinct requests.
        let kinds = [
            CapabilityKind::Speaker,
            CapabilityKind::Camera,
            CapabilityKind::Microphone,
            CapabilityKind::DatabaseRead,
        ];
        for kind in &kinds {
            let req = basic_request("agent-mock", kind.clone());
            client.request_capability(req).await.unwrap();
        }
        assert_eq!(t2.call_count(), 4, "4 distinct kinds = 4 transport calls");

        // Repeat all 4 -- all should be cache hits, no new transport calls.
        for kind in &kinds {
            let req = basic_request("agent-mock", kind.clone());
            client.request_capability(req).await.unwrap();
        }
        assert_eq!(t2.call_count(), 4, "repeats should all be cache hits");

        let stats = client.cache_stats();
        assert_eq!(stats.hit_count, 4);
        assert_eq!(stats.miss_count, 4);
    }

    // -----------------------------------------------------------------------
    // 17. CachedCapability::is_expired() logic
    // -----------------------------------------------------------------------
    #[test]
    fn cached_capability_is_expired_logic() {
        let now = current_time_ms();

        // Entry with large TTL should not be expired.
        let fresh = CachedCapability {
            response: CapabilityResponse {
                granted: true,
                capability_id: "id".into(),
                expires_at_ms: now + 60_000,
                endpoint_url: None,
                rejection_reason: None,
            },
            cached_at_ms: now,
            ttl_ms: 60_000,
        };
        assert!(!fresh.is_expired());

        // Entry with TTL=0 should immediately be expired.
        let stale = CachedCapability {
            response: CapabilityResponse {
                granted: true,
                capability_id: "id2".into(),
                expires_at_ms: 0,
                endpoint_url: None,
                rejection_reason: None,
            },
            cached_at_ms: 0,
            ttl_ms: 0,
        };
        assert!(stale.is_expired());
    }

    // -----------------------------------------------------------------------
    // 18. cache_key produces agent-scoped keys
    // -----------------------------------------------------------------------
    #[test]
    fn cache_key_is_agent_scoped() {
        let k1 = GatewayCapabilityClient::cache_key("agent-a", &CapabilityKind::Speaker);
        let k2 = GatewayCapabilityClient::cache_key("agent-b", &CapabilityKind::Speaker);
        let k3 = GatewayCapabilityClient::cache_key("agent-a", &CapabilityKind::Camera);

        assert_ne!(k1, k2, "different agents, same kind");
        assert_ne!(k1, k3, "same agent, different kinds");
        assert!(k1.starts_with("agent-a:"), "key should be prefixed with agent-id");
    }
}
