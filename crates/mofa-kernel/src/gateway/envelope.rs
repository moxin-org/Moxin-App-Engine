//! Typed request and response envelopes for the gateway pipeline.
//!
//! [`RequestEnvelope`] is created at gateway admission and flows through every
//! middleware filter and routing layer unchanged.  All pipeline components read
//! context (correlation ID, route, deadline) from this struct instead of
//! re-parsing the raw HTTP request.
//!
//! [`AgentResponse`] is produced by the agent handler and consumed by the
//! access-log, metrics, and admin API layers.  It carries enough information
//! for structured logging and latency tracking without reaching into axum
//! response internals.
//!
//! # Serialisation note
//!
//! Both types derive `Serialize`.  `RequestEnvelope::deadline` is a
//! `std::time::Instant` — a monotonic, non-serialisable type — so it is
//! tagged `#[serde(skip)]`.  Transport layers that need to propagate a
//! deadline across process boundaries should convert to `deadline_unix_ms`
//! (milliseconds since UNIX epoch) via [`RequestEnvelope::deadline_unix_ms`].

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// RequestEnvelope
// ─────────────────────────────────────────────────────────────────────────────

/// A typed envelope wrapping an inbound agent call from admission to handler.
///
/// Created once at the gateway edge with a freshly generated `correlation_id`
/// and passed unchanged through the filter chain and routing layer.
///
/// # Deadline semantics
///
/// When `deadline` is `Some`, middleware and handlers should check
/// [`is_expired`](RequestEnvelope::is_expired) before doing expensive work.
/// A request that has already exceeded its deadline should be short-circuited
/// with a `504 Gateway Timeout` rather than forwarded to the agent.
#[derive(Debug, Clone, Serialize)]
pub struct RequestEnvelope {
    /// UUID v4 generated at gateway admission.  Flows through every layer
    /// for distributed tracing and log correlation.
    pub correlation_id: String,
    /// ID of the [`GatewayRoute`](super::route::GatewayRoute) that matched
    /// this request, set after the routing phase.
    pub route_id: String,
    /// Request payload, parsed from the HTTP body.
    pub payload: serde_json::Value,
    /// Optional per-request deadline.  Skipped during serialisation because
    /// `Instant` is not serialisable; use
    /// [`deadline_unix_ms`](RequestEnvelope::deadline_unix_ms) when a
    /// wire-transferable deadline is needed.
    #[serde(skip)]
    pub deadline: Option<Instant>,
    /// IP address of the originating caller.
    pub origin_ip: IpAddr,
    /// Headers forwarded from the inbound HTTP request.  Keys are lowercased.
    pub headers: HashMap<String, String>,
    /// Wall-clock timestamp (ms since UNIX epoch) at which the envelope was
    /// created.  Used to compute `latency_ms` in [`GatewayResponse`].
    pub created_at_ms: u64,
}

impl RequestEnvelope {
    /// Create a new envelope with a freshly generated correlation ID and the
    /// current wall-clock creation time.
    ///
    /// `route_id` is typically empty at construction time and filled in by
    /// the routing middleware once a matching route has been found.
    pub fn new(route_id: impl Into<String>, payload: serde_json::Value, origin_ip: IpAddr) -> Self {
        Self {
            correlation_id: Uuid::new_v4().to_string(),
            route_id: route_id.into(),
            payload,
            deadline: None,
            origin_ip,
            headers: HashMap::new(),
            created_at_ms: now_ms(),
        }
    }

    /// Set an optional deadline as a monotonic `Instant`.
    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// Insert a forwarded header (key is lowercased automatically).
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into().to_lowercase(), value.into());
        self
    }

    /// Returns `true` if a deadline is set and has already passed.
    pub fn is_expired(&self) -> bool {
        self.deadline.map(|d| Instant::now() > d).unwrap_or(false)
    }

    /// Return the deadline as milliseconds since UNIX epoch, suitable for
    /// propagation over the wire.  Returns `None` if no deadline is set.
    pub fn deadline_unix_ms(&self) -> Option<u64> {
        self.deadline.map(|d| {
            let remaining = d.saturating_duration_since(Instant::now());
            let remaining_ms = u64::try_from(remaining.as_millis()).unwrap_or(u64::MAX);
            now_ms().saturating_add(remaining_ms)
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GatewayResponse
// ─────────────────────────────────────────────────────────────────────────────

/// A typed response produced by an agent handler and consumed by the
/// access-log, metrics, and admin API layers.
///
/// `latency_ms` is computed from the originating [`RequestEnvelope`]'s
/// `created_at_ms` and the wall-clock time at response completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    /// HTTP status code sent back to the caller.
    pub status_code: u16,
    /// Response body returned by the agent.
    pub body: serde_json::Value,
    /// End-to-end latency in milliseconds, measured from envelope creation to
    /// response completion.
    pub latency_ms: u64,
    /// ID of the agent that handled the request.
    pub agent_id: String,
    /// Correlation ID copied from the originating [`RequestEnvelope`] so
    /// access logs and traces can be joined without re-parsing the body.
    pub correlation_id: String,
}

impl AgentResponse {
    /// Create a response, computing `latency_ms` from the envelope's
    /// `created_at_ms`.
    pub fn new(
        status_code: u16,
        body: serde_json::Value,
        agent_id: impl Into<String>,
        envelope: &RequestEnvelope,
    ) -> Self {
        let latency_ms = now_ms().saturating_sub(envelope.created_at_ms);
        Self {
            status_code,
            body,
            latency_ms,
            agent_id: agent_id.into(),
            correlation_id: envelope.correlation_id.clone(),
        }
    }

    /// Returns `true` if the response indicates success (2xx status code).
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status_code)
    }

    /// Returns `true` if the status code is a client error (4xx).
    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status_code)
    }

    /// Returns `true` if the status code is a server or gateway error (5xx).
    pub fn is_server_error(&self) -> bool {
        self.status_code >= 500
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Current wall-clock time as milliseconds since UNIX epoch.
fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::str::FromStr;
    use std::time::{Duration, Instant};

    use serde_json::json;

    use super::{AgentResponse, RequestEnvelope};

    fn loopback() -> IpAddr {
        IpAddr::from_str("127.0.0.1").unwrap()
    }

    // ── RequestEnvelope ──────────────────────────────────────────────────────

    #[test]
    fn new_generates_unique_correlation_ids() {
        let a = RequestEnvelope::new("r1", json!({}), loopback());
        let b = RequestEnvelope::new("r1", json!({}), loopback());
        assert_ne!(a.correlation_id, b.correlation_id);
    }

    #[test]
    fn new_sets_created_at_ms() {
        let env = RequestEnvelope::new("r1", json!({}), loopback());
        assert!(env.created_at_ms > 0);
    }

    #[test]
    fn headers_lowercased() {
        let env = RequestEnvelope::new("r1", json!({}), loopback())
            .with_header("Content-Type", "application/json")
            .with_header("X-Api-Key", "secret");
        assert_eq!(
            env.headers.get("content-type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(env.headers.get("x-api-key"), Some(&"secret".to_string()));
    }

    #[test]
    fn is_expired_false_when_no_deadline() {
        let env = RequestEnvelope::new("r1", json!({}), loopback());
        assert!(!env.is_expired());
    }

    #[test]
    fn is_expired_false_when_deadline_in_future() {
        let env = RequestEnvelope::new("r1", json!({}), loopback())
            .with_deadline(Instant::now() + Duration::from_secs(60));
        assert!(!env.is_expired());
    }

    #[test]
    fn is_expired_true_when_deadline_in_past() {
        let past = Instant::now() - Duration::from_secs(1);
        let env = RequestEnvelope::new("r1", json!({}), loopback()).with_deadline(past);
        assert!(env.is_expired());
    }

    #[test]
    fn deadline_unix_ms_none_when_no_deadline() {
        let env = RequestEnvelope::new("r1", json!({}), loopback());
        assert!(env.deadline_unix_ms().is_none());
    }

    #[test]
    fn deadline_unix_ms_some_when_deadline_set() {
        let env = RequestEnvelope::new("r1", json!({}), loopback())
            .with_deadline(Instant::now() + Duration::from_secs(5));
        assert!(env.deadline_unix_ms().is_some());
    }

    #[test]
    fn envelope_serializes_without_deadline_field() -> Result<(), Box<dyn std::error::Error>> {
        let env = RequestEnvelope::new("r1", json!({"key": "val"}), loopback())
            .with_deadline(Instant::now() + Duration::from_secs(10));
        let json = serde_json::to_string(&env)?;
        assert!(!json.contains("deadline"));
        assert!(json.contains("correlation_id"));
        assert!(json.contains("created_at_ms"));
        Ok(())
    }

    // ── GatewayResponse ──────────────────────────────────────────────────────

    #[test]
    fn gateway_response_latency_computed_from_envelope() {
        let env = RequestEnvelope::new("r1", json!({}), loopback());
        let resp = AgentResponse::new(200, json!({"ok": true}), "agent-a", &env);
        assert_eq!(resp.correlation_id, env.correlation_id);
        // latency_ms must be >= 0 (saturating_sub guarantees this)
        // and realistically < 1000ms for a test
        assert!(resp.latency_ms < 1000);
    }

    #[test]
    fn is_success_2xx() {
        let env = RequestEnvelope::new("r1", json!({}), loopback());
        assert!(AgentResponse::new(200, json!({}), "a", &env).is_success());
        assert!(AgentResponse::new(204, json!({}), "a", &env).is_success());
        assert!(!AgentResponse::new(400, json!({}), "a", &env).is_success());
    }

    #[test]
    fn is_client_error_4xx() {
        let env = RequestEnvelope::new("r1", json!({}), loopback());
        assert!(AgentResponse::new(400, json!({}), "a", &env).is_client_error());
        assert!(AgentResponse::new(404, json!({}), "a", &env).is_client_error());
        assert!(!AgentResponse::new(500, json!({}), "a", &env).is_client_error());
    }

    #[test]
    fn is_server_error_5xx() {
        let env = RequestEnvelope::new("r1", json!({}), loopback());
        assert!(AgentResponse::new(500, json!({}), "a", &env).is_server_error());
        assert!(AgentResponse::new(504, json!({}), "a", &env).is_server_error());
        assert!(!AgentResponse::new(200, json!({}), "a", &env).is_server_error());
    }

    #[test]
    fn gateway_response_serializes() -> Result<(), Box<dyn std::error::Error>> {
        let env = RequestEnvelope::new("r1", json!({}), loopback());
        let resp = AgentResponse::new(200, json!({"result": "ok"}), "agent-a", &env);
        let json = serde_json::to_string(&resp)?;
        assert!(json.contains("status_code"));
        assert!(json.contains("latency_ms"));
        assert!(json.contains("agent_id"));
        assert!(json.contains("correlation_id"));
        Ok(())
    }

    #[test]
    fn now_ms_uses_safe_conversion() {
        // Verify the module-local now_ms() returns a sane value (i.e. the
        // u128→u64 try_from path works in practice).
        let ms = super::now_ms();
        assert!(
            ms > 1_577_836_800_000,
            "now_ms() = {ms}, expected > 2020-01-01 epoch millis"
        );
    }
}
