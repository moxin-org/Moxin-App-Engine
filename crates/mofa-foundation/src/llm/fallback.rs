//! LLM Provider Fallback Chain
//!
//! Wraps multiple [`LLMProvider`]s in priority order. When a provider fails
//! with a triggering error (rate-limit, quota, network, timeout, auth), the
//! chain automatically tries the next provider. The chain itself implements
//! [`LLMProvider`], so it is transparent to the rest of the system.
//!
//! # Example
//!
//! ```rust,ignore
//! use mofa_foundation::llm::{FallbackChain, FallbackCondition, CircuitBreakerConfig};
//!
//! let chain = FallbackChain::builder()
//!     .with_circuit_breaker(CircuitBreakerConfig::default()) // 3 failures → 30s cooldown
//!     .add(openai_provider)                                  // try first
//!     .add_with_trigger(anthropic_provider,                  // try if openai is rate-limited
//!         FallbackTrigger::on_conditions(vec![
//!             FallbackCondition::RateLimited,
//!             FallbackCondition::QuotaExceeded,
//!         ]))
//!     .add_last(ollama_provider)                             // last resort, always try
//!     .build();
//!
//! // Use exactly like any other LLMProvider
//! let client = LLMClient::new(Arc::new(chain));
//!
//! // Read metrics
//! let snapshot = chain.metrics();
//! println!("total fallbacks: {}", snapshot.fallbacks_total);
//! ```
//!
//! # Config-driven construction
//!
//! ```yaml
//! # fallback_chain.yaml
//! name: my-chain
//! circuit_breaker:
//!   failure_threshold: 3
//!   cooldown_secs: 30
//! providers:
//!   - provider: openai
//!     api_key: "sk-..."
//!   - provider: anthropic
//!     api_key: "sk-ant-..."
//!     trigger: any_error
//!   - provider: ollama
//!     base_url: "http://localhost:11434"
//!     trigger: never
//! ```
//!
//! ```rust,ignore
//! let config: FallbackChainConfig = serde_yaml::from_str(yaml)?;
//! let chain = config.build(&registry).await?;
//! ```

use super::provider::{LLMConfig, LLMProvider, LLMRegistry};
use super::types::*;
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{info, warn};

// ============================================================================
// FallbackCondition
// ============================================================================

/// Conditions under which the chain moves to the next provider.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FallbackCondition {
    /// Provider returned HTTP 429 / rate-limit response.
    RateLimited,
    /// Provider returned quota-exceeded / billing error.
    QuotaExceeded,
    /// TCP/TLS/DNS failure reaching the provider.
    NetworkError,
    /// Request took too long and was aborted.
    Timeout,
    /// API key rejected / credentials invalid.
    AuthError,
    /// Provider does not support the requested model or feature.
    ProviderUnavailable,
    /// Prompt is too long for the provider's context window.
    ContextLengthExceeded,
    /// Requested model does not exist on this provider.
    ModelNotFound,
}

impl FallbackCondition {
    /// Returns `true` when `error` matches this condition.
    pub fn matches(&self, error: &LLMError) -> bool {
        matches!(
            (self, error),
            (Self::RateLimited, LLMError::RateLimited(_))
                | (Self::QuotaExceeded, LLMError::QuotaExceeded(_))
                | (Self::NetworkError, LLMError::NetworkError(_))
                | (Self::Timeout, LLMError::Timeout(_))
                | (Self::AuthError, LLMError::AuthError(_))
                | (Self::ProviderUnavailable, LLMError::ProviderNotSupported(_))
                | (
                    Self::ContextLengthExceeded,
                    LLMError::ContextLengthExceeded(_)
                )
                | (Self::ModelNotFound, LLMError::ModelNotFound(_))
        )
    }

    /// Default set of conditions used when none are specified.
    ///
    /// Covers transient, provider-side failures that make sense to retry
    /// against a different backend:
    /// - `RateLimited`
    /// - `QuotaExceeded`
    /// - `NetworkError`
    /// - `Timeout`
    /// - `AuthError`
    pub fn defaults() -> Vec<Self> {
        vec![
            Self::RateLimited,
            Self::QuotaExceeded,
            Self::NetworkError,
            Self::Timeout,
            Self::AuthError,
        ]
    }
}

// ============================================================================
// FallbackTrigger
// ============================================================================

/// Controls when a provider entry triggers a fallback to the next provider.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum FallbackTrigger {
    /// Fall back only when the error matches one of the listed conditions.
    OnConditions(Vec<FallbackCondition>),
    /// Fall back on any error.
    OnAnyError,
    /// Never fall back — this entry is always the terminal provider.
    Never,
}

impl FallbackTrigger {
    /// Convenience constructor for condition-based trigger.
    pub fn on_conditions(conditions: Vec<FallbackCondition>) -> Self {
        Self::OnConditions(conditions)
    }

    /// Returns the default trigger using [`FallbackCondition::defaults`].
    pub fn default_conditions() -> Self {
        Self::OnConditions(FallbackCondition::defaults())
    }

    /// Returns `true` if `error` should cause the chain to move to the next
    /// provider.
    pub fn should_fallback(&self, error: &LLMError) -> bool {
        match self {
            Self::OnConditions(conditions) => conditions.iter().any(|c| c.matches(error)),
            Self::OnAnyError => true,
            Self::Never => false,
        }
    }
}

impl Default for FallbackTrigger {
    fn default() -> Self {
        Self::default_conditions()
    }
}

// ============================================================================
// Circuit Breaker
// ============================================================================

/// Configuration for the per-provider circuit breaker.
///
/// When a provider accumulates `failure_threshold` consecutive fallback-
/// triggering failures, the circuit opens and the provider is skipped for
/// `cooldown_secs` seconds. After the cooldown the circuit closes again and
/// the provider is tried on the next request.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct CircuitBreakerConfig {
    /// Number of consecutive fallback-triggering failures before the circuit
    /// opens. Default: 3.
    pub failure_threshold: u32,
    /// Seconds to wait before the circuit closes again. Default: 30.
    pub cooldown_secs: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            cooldown_secs: 30,
        }
    }
}

impl CircuitBreakerConfig {
    /// Build a new config with explicit values.
    pub fn new(failure_threshold: u32, cooldown_secs: u64) -> Self {
        Self {
            failure_threshold,
            cooldown_secs,
        }
    }
}

/// Per-provider circuit-breaker state.  Uses a `std::sync::Mutex` for the
/// inner state because the critical section is tiny (a few integer reads /
/// writes) and we never hold the lock across an `await` point.
struct ProviderCircuitBreaker {
    threshold: u32,
    cooldown: Duration,
    state: Mutex<BreakerState>,
}

#[derive(Default)]
struct BreakerState {
    consecutive_failures: u32,
    /// `Some(instant)` means the circuit is open until that instant.
    open_until: Option<Instant>,
}

impl ProviderCircuitBreaker {
    fn new(config: &CircuitBreakerConfig) -> Self {
        Self {
            threshold: config.failure_threshold,
            cooldown: Duration::from_secs(config.cooldown_secs),
            state: Mutex::new(BreakerState::default()),
        }
    }

    /// Returns `true` if the circuit is currently open (provider should be
    /// skipped).
    fn is_open(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state
            .open_until
            .map(|until| Instant::now() < until)
            .unwrap_or(false)
    }

    /// Record a successful call — resets the failure counter and closes the
    /// circuit.
    fn record_success(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.consecutive_failures = 0;
        state.open_until = None;
    }

    /// Record a failure that triggered a fallback.  Opens the circuit once the
    /// threshold is reached.
    fn record_failure(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        if state.consecutive_failures >= self.threshold {
            state.open_until = Some(Instant::now() + self.cooldown);
        }
    }
}

// ============================================================================
// Metrics
// ============================================================================

/// A point-in-time snapshot of fallback chain metrics.
#[derive(Debug, Clone)]
pub struct FallbackSnapshot {
    /// Name of the chain.
    pub chain_name: String,
    /// Total requests sent to the chain.
    pub requests_total: u64,
    /// Total times the chain moved to a fallback provider.
    pub fallbacks_total: u64,
    /// Per-provider breakdown (ordered by slot).
    pub providers: Vec<ProviderSnapshot>,
}

/// Per-provider metrics snapshot.
#[derive(Debug, Clone)]
pub struct ProviderSnapshot {
    /// Provider name as reported by [`LLMProvider::name`].
    pub name: String,
    /// Times this provider returned a successful response.
    pub successes: u64,
    /// Times this provider failed and triggered a fallback to the next slot.
    pub fallback_failures: u64,
    /// Times this provider was skipped because its circuit breaker was open.
    pub circuit_breaker_skips: u64,
}

/// Internal per-chain counters.  `Vec<AtomicU64>` is safe here because the
/// `Vec` is never resized after the chain is built.
struct FallbackMetrics {
    requests_total: AtomicU64,
    fallbacks_total: AtomicU64,
    provider_names: Vec<String>,
    provider_successes: Vec<AtomicU64>,
    provider_fallback_failures: Vec<AtomicU64>,
    provider_circuit_skips: Vec<AtomicU64>,
}

impl FallbackMetrics {
    fn new(provider_names: Vec<String>) -> Self {
        let n = provider_names.len();
        let make_vec = || (0..n).map(|_| AtomicU64::new(0)).collect::<Vec<_>>();
        Self {
            requests_total: AtomicU64::new(0),
            fallbacks_total: AtomicU64::new(0),
            provider_names,
            provider_successes: make_vec(),
            provider_fallback_failures: make_vec(),
            provider_circuit_skips: make_vec(),
        }
    }

    fn snapshot(&self, chain_name: &str) -> FallbackSnapshot {
        let providers = self
            .provider_names
            .iter()
            .enumerate()
            .map(|(i, name)| ProviderSnapshot {
                name: name.clone(),
                successes: self.provider_successes[i].load(Ordering::Relaxed),
                fallback_failures: self.provider_fallback_failures[i].load(Ordering::Relaxed),
                circuit_breaker_skips: self.provider_circuit_skips[i].load(Ordering::Relaxed),
            })
            .collect();

        FallbackSnapshot {
            chain_name: chain_name.to_string(),
            requests_total: self.requests_total.load(Ordering::Relaxed),
            fallbacks_total: self.fallbacks_total.load(Ordering::Relaxed),
            providers,
        }
    }
}

// ============================================================================
// FallbackEntry
// ============================================================================

struct FallbackEntry {
    provider: Arc<dyn LLMProvider>,
    trigger: FallbackTrigger,
    circuit_breaker: Option<ProviderCircuitBreaker>,
}

// ============================================================================
// FallbackChain
// ============================================================================

/// An [`LLMProvider`] that delegates to a prioritised list of providers and
/// automatically falls back when one fails.
///
/// Build with [`FallbackChain::builder`] or load from a [`FallbackChainConfig`].
pub struct FallbackChain {
    name: String,
    providers: Vec<FallbackEntry>,
    metrics: FallbackMetrics,
}

impl FallbackChain {
    /// Start building a new chain.
    pub fn builder() -> FallbackChainBuilder {
        FallbackChainBuilder::new()
    }

    /// Number of provider slots in the chain.
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// `true` if the chain has no providers.
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Return a point-in-time snapshot of chain metrics.
    pub fn metrics(&self) -> FallbackSnapshot {
        self.metrics.snapshot(&self.name)
    }

    /// Try providers in order for a non-streaming chat request.
    async fn try_chat(&self, request: ChatCompletionRequest) -> LLMResult<ChatCompletionResponse> {
        self.metrics.requests_total.fetch_add(1, Ordering::Relaxed);
        let mut last_error: Option<LLMError> = None;

        for (index, entry) in self.providers.iter().enumerate() {
            // --- circuit breaker: skip open circuits ---
            if entry
                .circuit_breaker
                .as_ref()
                .map(|cb| cb.is_open())
                .unwrap_or(false)
            {
                self.metrics.provider_circuit_skips[index].fetch_add(1, Ordering::Relaxed);
                warn!(
                    chain = %self.name,
                    provider = %entry.provider.name(),
                    slot = index,
                    "FallbackChain: circuit breaker open, skipping provider"
                );
                continue;
            }

            let provider_name = entry.provider.name();
            match entry.provider.chat(request.clone()).await {
                Ok(response) => {
                    if let Some(cb) = &entry.circuit_breaker {
                        cb.record_success();
                    }
                    self.metrics.provider_successes[index].fetch_add(1, Ordering::Relaxed);
                    if index > 0 {
                        info!(
                            chain = %self.name,
                            provider = %provider_name,
                            slot = index,
                            "FallbackChain: succeeded after fallback"
                        );
                    }
                    return Ok(response);
                }
                Err(err) => {
                    let is_last = index + 1 >= self.providers.len();
                    if !is_last && entry.trigger.should_fallback(&err) {
                        if let Some(cb) = &entry.circuit_breaker {
                            cb.record_failure();
                        }
                        self.metrics.provider_fallback_failures[index]
                            .fetch_add(1, Ordering::Relaxed);
                        self.metrics.fallbacks_total.fetch_add(1, Ordering::Relaxed);
                        warn!(
                            chain = %self.name,
                            provider = %provider_name,
                            slot = index,
                            error = %err,
                            "FallbackChain: provider failed, trying next"
                        );
                        last_error = Some(err);
                    } else {
                        return Err(err);
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            LLMError::Other(format!(
                "FallbackChain({}): all providers unavailable (circuit breakers open)",
                self.name
            ))
        }))
    }
}

#[async_trait]
impl LLMProvider for FallbackChain {
    fn name(&self) -> &str {
        &self.name
    }

    fn default_model(&self) -> &str {
        self.providers
            .first()
            .map(|e| e.provider.default_model())
            .unwrap_or("")
    }

    fn supports_streaming(&self) -> bool {
        self.providers
            .iter()
            .any(|e| e.provider.supports_streaming())
    }

    fn supports_tools(&self) -> bool {
        self.providers.iter().any(|e| e.provider.supports_tools())
    }

    fn supports_vision(&self) -> bool {
        self.providers.iter().any(|e| e.provider.supports_vision())
    }

    fn supports_embedding(&self) -> bool {
        self.providers
            .iter()
            .any(|e| e.provider.supports_embedding())
    }

    async fn chat(&self, request: ChatCompletionRequest) -> LLMResult<ChatCompletionResponse> {
        self.try_chat(request).await
    }

    async fn chat_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> LLMResult<super::provider::ChatStream> {
        self.metrics.requests_total.fetch_add(1, Ordering::Relaxed);
        let mut last_error: Option<LLMError> = None;

        for (index, entry) in self.providers.iter().enumerate() {
            if !entry.provider.supports_streaming() {
                continue;
            }

            if entry
                .circuit_breaker
                .as_ref()
                .map(|cb| cb.is_open())
                .unwrap_or(false)
            {
                self.metrics.provider_circuit_skips[index].fetch_add(1, Ordering::Relaxed);
                warn!(
                    chain = %self.name,
                    provider = %entry.provider.name(),
                    slot = index,
                    "FallbackChain: circuit breaker open, skipping streaming provider"
                );
                continue;
            }

            let provider_name = entry.provider.name();
            match entry.provider.chat_stream(request.clone()).await {
                Ok(stream) => {
                    if let Some(cb) = &entry.circuit_breaker {
                        cb.record_success();
                    }
                    self.metrics.provider_successes[index].fetch_add(1, Ordering::Relaxed);
                    if index > 0 {
                        info!(
                            chain = %self.name,
                            provider = %provider_name,
                            slot = index,
                            "FallbackChain: streaming succeeded after fallback"
                        );
                    }
                    return Ok(stream);
                }
                Err(err) => {
                    let is_last = index + 1 >= self.providers.len();
                    if !is_last && entry.trigger.should_fallback(&err) {
                        if let Some(cb) = &entry.circuit_breaker {
                            cb.record_failure();
                        }
                        self.metrics.provider_fallback_failures[index]
                            .fetch_add(1, Ordering::Relaxed);
                        self.metrics.fallbacks_total.fetch_add(1, Ordering::Relaxed);
                        warn!(
                            chain = %self.name,
                            provider = %provider_name,
                            slot = index,
                            error = %err,
                            "FallbackChain: streaming provider failed, trying next"
                        );
                        last_error = Some(err);
                    } else {
                        return Err(err);
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            LLMError::ProviderNotSupported(format!(
                "FallbackChain({}): no provider supports streaming",
                self.name
            ))
        }))
    }

    async fn embedding(&self, request: EmbeddingRequest) -> LLMResult<EmbeddingResponse> {
        self.metrics.requests_total.fetch_add(1, Ordering::Relaxed);
        let mut last_error: Option<LLMError> = None;

        for (index, entry) in self.providers.iter().enumerate() {
            if !entry.provider.supports_embedding() {
                continue;
            }

            if entry
                .circuit_breaker
                .as_ref()
                .map(|cb| cb.is_open())
                .unwrap_or(false)
            {
                self.metrics.provider_circuit_skips[index].fetch_add(1, Ordering::Relaxed);
                warn!(
                    chain = %self.name,
                    provider = %entry.provider.name(),
                    slot = index,
                    "FallbackChain: circuit breaker open, skipping embedding provider"
                );
                continue;
            }

            let provider_name = entry.provider.name();
            match entry.provider.embedding(request.clone()).await {
                Ok(response) => {
                    if let Some(cb) = &entry.circuit_breaker {
                        cb.record_success();
                    }
                    self.metrics.provider_successes[index].fetch_add(1, Ordering::Relaxed);
                    if index > 0 {
                        info!(
                            chain = %self.name,
                            provider = %provider_name,
                            slot = index,
                            "FallbackChain: embedding succeeded after fallback"
                        );
                    }
                    return Ok(response);
                }
                Err(err) => {
                    let is_last = index + 1 >= self.providers.len();
                    if !is_last && entry.trigger.should_fallback(&err) {
                        if let Some(cb) = &entry.circuit_breaker {
                            cb.record_failure();
                        }
                        self.metrics.provider_fallback_failures[index]
                            .fetch_add(1, Ordering::Relaxed);
                        self.metrics.fallbacks_total.fetch_add(1, Ordering::Relaxed);
                        warn!(
                            chain = %self.name,
                            provider = %provider_name,
                            slot = index,
                            error = %err,
                            "FallbackChain: embedding provider failed, trying next"
                        );
                        last_error = Some(err);
                    } else {
                        return Err(err);
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            LLMError::ProviderNotSupported(format!(
                "FallbackChain({}): no provider supports embedding",
                self.name
            ))
        }))
    }

    async fn health_check(&self) -> LLMResult<bool> {
        // Healthy if at least one provider (with a closed circuit) is healthy.
        for entry in &self.providers {
            if entry
                .circuit_breaker
                .as_ref()
                .map(|cb| cb.is_open())
                .unwrap_or(false)
            {
                continue;
            }
            if entry.provider.health_check().await.unwrap_or(false) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for [`FallbackChain`].
pub struct FallbackChainBuilder {
    name: String,
    providers: Vec<(Arc<dyn LLMProvider>, FallbackTrigger)>,
    circuit_breaker: Option<CircuitBreakerConfig>,
}

impl FallbackChainBuilder {
    fn new() -> Self {
        Self {
            name: "fallback-chain".to_string(),
            providers: Vec::new(),
            circuit_breaker: None,
        }
    }

    /// Set the name reported by [`LLMProvider::name`].
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Enable a circuit breaker for every slot using `config`.
    ///
    /// Call this before adding providers; it applies to all of them.
    pub fn with_circuit_breaker(mut self, config: CircuitBreakerConfig) -> Self {
        self.circuit_breaker = Some(config);
        self
    }

    /// Add a provider using the default fallback conditions.
    ///
    /// Triggers fallback on: `RateLimited`, `QuotaExceeded`, `NetworkError`,
    /// `Timeout`, `AuthError`.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, provider: impl LLMProvider + 'static) -> Self {
        self.add_arc(Arc::new(provider), FallbackTrigger::default_conditions())
    }

    /// Add an `Arc<dyn LLMProvider>` using the default fallback conditions.
    pub fn add_shared(self, provider: Arc<dyn LLMProvider>) -> Self {
        self.add_arc(provider, FallbackTrigger::default_conditions())
    }

    /// Add a provider with a custom [`FallbackTrigger`].
    pub fn add_with_trigger(
        self,
        provider: impl LLMProvider + 'static,
        trigger: FallbackTrigger,
    ) -> Self {
        self.add_arc(Arc::new(provider), trigger)
    }

    /// Add the terminal (last-resort) provider — never falls back from here.
    pub fn add_last(self, provider: impl LLMProvider + 'static) -> Self {
        self.add_arc(Arc::new(provider), FallbackTrigger::Never)
    }

    /// Add a shared terminal provider — never falls back from here.
    pub fn add_last_shared(self, provider: Arc<dyn LLMProvider>) -> Self {
        self.add_arc(provider, FallbackTrigger::Never)
    }

    fn add_arc(mut self, provider: Arc<dyn LLMProvider>, trigger: FallbackTrigger) -> Self {
        self.providers.push((provider, trigger));
        self
    }

    /// Build the [`FallbackChain`].
    ///
    /// # Panics
    ///
    /// Panics if no providers were added.
    pub fn build(self) -> FallbackChain {
        assert!(
            !self.providers.is_empty(),
            "FallbackChainBuilder: at least one provider is required"
        );

        let provider_names: Vec<String> = self
            .providers
            .iter()
            .map(|(p, _)| p.name().to_string())
            .collect();

        let entries = self
            .providers
            .into_iter()
            .map(|(provider, trigger)| FallbackEntry {
                circuit_breaker: self
                    .circuit_breaker
                    .as_ref()
                    .map(ProviderCircuitBreaker::new),
                provider,
                trigger,
            })
            .collect();

        FallbackChain {
            metrics: FallbackMetrics::new(provider_names),
            name: self.name,
            providers: entries,
        }
    }
}

// ============================================================================
// Config-driven construction
// ============================================================================

/// YAML/TOML-deserializable configuration for building a [`FallbackChain`].
///
/// # Example YAML
///
/// ```yaml
/// name: my-chain
/// circuit_breaker:
///   failure_threshold: 3
///   cooldown_secs: 30
/// providers:
///   - provider: openai
///     api_key: "sk-..."
///   - provider: anthropic
///     api_key: "sk-ant-..."
///     trigger: any_error
///   - provider: ollama
///     base_url: "http://localhost:11434"
///     trigger: never
/// ```
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FallbackChainConfig {
    /// Name reported by [`LLMProvider::name`].  Defaults to `"fallback-chain"`.
    #[serde(default)]
    pub name: Option<String>,

    /// Optional circuit-breaker settings applied to every provider slot.
    #[serde(default)]
    pub circuit_breaker: Option<CircuitBreakerConfig>,

    /// Ordered list of providers to try, from highest to lowest priority.
    pub providers: Vec<FallbackProviderConfig>,
}

/// Per-provider entry inside [`FallbackChainConfig`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FallbackProviderConfig {
    /// Provider and connection settings (reuses [`LLMConfig`] fields).
    #[serde(flatten)]
    pub llm: LLMConfig,

    /// When to fall back from this provider.
    ///
    /// - omit / `"default"` → default conditions (rate-limit, quota, network,
    ///   timeout, auth)
    /// - `"any_error"` → fall back on any error
    /// - `"never"` → terminal provider; never falls back
    /// - `{ conditions: ["rate_limited", "timeout"] }` → explicit list
    #[serde(default)]
    pub trigger: FallbackTriggerConfig,
}

/// Serializable representation of [`FallbackTrigger`].
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackTriggerConfig {
    /// Use the default set of conditions.
    #[default]
    Default,
    /// Fall back on any error.
    AnyError,
    /// Never fall back (terminal provider).
    Never,
    /// Fall back only on the listed conditions.
    Conditions(Vec<FallbackConditionConfig>),
}

impl From<FallbackTriggerConfig> for FallbackTrigger {
    fn from(cfg: FallbackTriggerConfig) -> Self {
        match cfg {
            FallbackTriggerConfig::Default => FallbackTrigger::default_conditions(),
            FallbackTriggerConfig::AnyError => FallbackTrigger::OnAnyError,
            FallbackTriggerConfig::Never => FallbackTrigger::Never,
            FallbackTriggerConfig::Conditions(conds) => {
                FallbackTrigger::OnConditions(conds.into_iter().map(Into::into).collect())
            }
        }
    }
}

/// Serializable representation of [`FallbackCondition`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackConditionConfig {
    RateLimited,
    QuotaExceeded,
    NetworkError,
    Timeout,
    AuthError,
    ProviderUnavailable,
    ContextLengthExceeded,
    ModelNotFound,
}

impl From<FallbackConditionConfig> for FallbackCondition {
    fn from(cfg: FallbackConditionConfig) -> Self {
        match cfg {
            FallbackConditionConfig::RateLimited => Self::RateLimited,
            FallbackConditionConfig::QuotaExceeded => Self::QuotaExceeded,
            FallbackConditionConfig::NetworkError => Self::NetworkError,
            FallbackConditionConfig::Timeout => Self::Timeout,
            FallbackConditionConfig::AuthError => Self::AuthError,
            FallbackConditionConfig::ProviderUnavailable => Self::ProviderUnavailable,
            FallbackConditionConfig::ContextLengthExceeded => Self::ContextLengthExceeded,
            FallbackConditionConfig::ModelNotFound => Self::ModelNotFound,
        }
    }
}

impl FallbackChainConfig {
    /// Build a [`FallbackChain`] by resolving each provider entry through
    /// `registry`.
    pub async fn build(self, registry: &LLMRegistry) -> LLMResult<FallbackChain> {
        if self.providers.is_empty() {
            return Err(LLMError::Other(
                "FallbackChainConfig: at least one provider is required".into(),
            ));
        }

        let mut builder = FallbackChain::builder();
        if let Some(name) = self.name {
            builder = builder.name(name);
        }
        if let Some(cb) = self.circuit_breaker {
            builder = builder.with_circuit_breaker(cb);
        }

        let last_index = self.providers.len() - 1;
        for (i, entry) in self.providers.into_iter().enumerate() {
            let trigger: FallbackTrigger = entry.trigger.into();
            let provider = registry.create(entry.llm).await?;
            // If this is the last slot AND the trigger is still Default/Any,
            // treat it as Never so we don't silently drop errors.
            let effective_trigger = if i == last_index {
                match trigger {
                    FallbackTrigger::Never => FallbackTrigger::Never,
                    _ => FallbackTrigger::Never,
                }
            } else {
                trigger
            };
            builder = builder.add_arc(provider, effective_trigger);
        }

        Ok(builder.build())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A controllable mock provider that returns pre-set responses in sequence.
    struct MockProvider {
        name: String,
        responses: Vec<LLMResult<ChatCompletionResponse>>,
        call_count: AtomicUsize,
    }

    impl MockProvider {
        fn new(name: &str, responses: Vec<LLMResult<ChatCompletionResponse>>) -> Self {
            Self {
                name: name.to_string(),
                responses,
                call_count: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl LLMProvider for MockProvider {
        fn name(&self) -> &str {
            &self.name
        }

        async fn chat(&self, _request: ChatCompletionRequest) -> LLMResult<ChatCompletionResponse> {
            let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
            self.responses
                .get(idx)
                .cloned()
                .unwrap_or_else(|| Err(LLMError::Other("no more responses".into())))
        }
    }

    fn ok_response(text: &str) -> LLMResult<ChatCompletionResponse> {
        Ok(ChatCompletionResponse {
            id: "id".into(),
            object: "chat.completion".into(),
            created: 0,
            model: "model".into(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage::assistant(text),
                finish_reason: Some(FinishReason::Stop),
                logprobs: None,
            }],
            usage: None,
            system_fingerprint: None,
        })
    }

    fn request() -> ChatCompletionRequest {
        ChatCompletionRequest::new("test-model")
    }

    // ── FallbackCondition::matches ──────────────────────────────────────────

    #[test]
    fn condition_matches_correct_errors() {
        assert!(FallbackCondition::RateLimited.matches(&LLMError::RateLimited("x".into())));
        assert!(FallbackCondition::QuotaExceeded.matches(&LLMError::QuotaExceeded("x".into())));
        assert!(FallbackCondition::NetworkError.matches(&LLMError::NetworkError("x".into())));
        assert!(FallbackCondition::Timeout.matches(&LLMError::Timeout("x".into())));
        assert!(FallbackCondition::AuthError.matches(&LLMError::AuthError("x".into())));
        assert!(FallbackCondition::ModelNotFound.matches(&LLMError::ModelNotFound("x".into())));
        assert!(
            FallbackCondition::ContextLengthExceeded
                .matches(&LLMError::ContextLengthExceeded("x".into()))
        );
        assert!(
            FallbackCondition::ProviderUnavailable
                .matches(&LLMError::ProviderNotSupported("x".into()))
        );
    }

    #[test]
    fn condition_does_not_match_unrelated_error() {
        assert!(!FallbackCondition::RateLimited.matches(&LLMError::AuthError("x".into())));
        assert!(!FallbackCondition::NetworkError.matches(&LLMError::RateLimited("x".into())));
    }

    // ── FallbackTrigger::should_fallback ────────────────────────────────────

    #[test]
    fn trigger_on_any_error_always_falls_back() {
        let t = FallbackTrigger::OnAnyError;
        assert!(t.should_fallback(&LLMError::RateLimited("x".into())));
        assert!(t.should_fallback(&LLMError::Other("x".into())));
        assert!(t.should_fallback(&LLMError::SerializationError("x".into())));
    }

    #[test]
    fn trigger_never_never_falls_back() {
        let t = FallbackTrigger::Never;
        assert!(!t.should_fallback(&LLMError::RateLimited("x".into())));
        assert!(!t.should_fallback(&LLMError::NetworkError("x".into())));
    }

    #[test]
    fn trigger_on_conditions_respects_list() {
        let t = FallbackTrigger::on_conditions(vec![FallbackCondition::RateLimited]);
        assert!(t.should_fallback(&LLMError::RateLimited("x".into())));
        assert!(!t.should_fallback(&LLMError::NetworkError("x".into())));
    }

    // ── FallbackChain basic scenarios ───────────────────────────────────────

    #[tokio::test]
    async fn first_provider_succeeds_no_fallback() {
        let p1 = Arc::new(MockProvider::new("p1", vec![ok_response("hello")]));
        let p2 = Arc::new(MockProvider::new("p2", vec![ok_response("world")]));
        let p1_ref = p1.clone();
        let p2_ref = p2.clone();

        let chain = FallbackChain::builder()
            .add_shared(p1)
            .add_last_shared(p2)
            .build();

        let result = chain.chat(request()).await.unwrap();
        assert_eq!(result.content().unwrap(), "hello");
        assert_eq!(p1_ref.calls(), 1);
        assert_eq!(p2_ref.calls(), 0);
    }

    #[tokio::test]
    async fn falls_back_on_rate_limit() {
        let p1 = MockProvider::new("p1", vec![Err(LLMError::RateLimited("429".into()))]);
        let p2 = MockProvider::new("p2", vec![ok_response("from-p2")]);

        let chain = FallbackChain::builder().add(p1).add_last(p2).build();

        let result = chain.chat(request()).await.unwrap();
        assert_eq!(result.content().unwrap(), "from-p2");
    }

    #[tokio::test]
    async fn falls_back_through_all_providers() {
        let p1 = MockProvider::new("p1", vec![Err(LLMError::RateLimited("rl".into()))]);
        let p2 = MockProvider::new("p2", vec![Err(LLMError::QuotaExceeded("quota".into()))]);
        let p3 = MockProvider::new("p3", vec![ok_response("p3-ok")]);

        let chain = FallbackChain::builder()
            .add(p1)
            .add(p2)
            .add_last(p3)
            .build();

        let result = chain.chat(request()).await.unwrap();
        assert_eq!(result.content().unwrap(), "p3-ok");
    }

    #[tokio::test]
    async fn propagates_non_fallback_error() {
        // SerializationError is not in the default trigger conditions.
        let p1 = MockProvider::new(
            "p1",
            vec![Err(LLMError::SerializationError("bad json".into()))],
        );
        let p2 = MockProvider::new("p2", vec![ok_response("should not reach")]);

        let chain = FallbackChain::builder().add(p1).add_last(p2).build();

        let err = chain.chat(request()).await.unwrap_err();
        assert!(matches!(err, LLMError::SerializationError(_)));
    }

    #[tokio::test]
    async fn last_provider_error_propagates_even_if_fallback_condition() {
        let p1 = MockProvider::new("p1", vec![Err(LLMError::RateLimited("rl1".into()))]);
        let p2 = MockProvider::new("p2", vec![Err(LLMError::RateLimited("rl2".into()))]);

        let chain = FallbackChain::builder().add(p1).add_last(p2).build();

        let err = chain.chat(request()).await.unwrap_err();
        assert!(matches!(err, LLMError::RateLimited(_)));
    }

    #[tokio::test]
    async fn health_check_true_if_any_healthy() {
        struct AlwaysHealthy;
        struct AlwaysUnhealthy;

        #[async_trait]
        impl LLMProvider for AlwaysHealthy {
            fn name(&self) -> &str {
                "healthy"
            }
            async fn chat(&self, _r: ChatCompletionRequest) -> LLMResult<ChatCompletionResponse> {
                unimplemented!()
            }
            async fn health_check(&self) -> LLMResult<bool> {
                Ok(true)
            }
        }

        #[async_trait]
        impl LLMProvider for AlwaysUnhealthy {
            fn name(&self) -> &str {
                "unhealthy"
            }
            async fn chat(&self, _r: ChatCompletionRequest) -> LLMResult<ChatCompletionResponse> {
                unimplemented!()
            }
            async fn health_check(&self) -> LLMResult<bool> {
                Ok(false)
            }
        }

        let chain = FallbackChain::builder()
            .add(AlwaysUnhealthy)
            .add_last(AlwaysHealthy)
            .build();

        assert!(chain.health_check().await.unwrap());
    }

    #[tokio::test]
    async fn health_check_false_if_all_unhealthy() {
        struct AlwaysUnhealthy;

        #[async_trait]
        impl LLMProvider for AlwaysUnhealthy {
            fn name(&self) -> &str {
                "unhealthy"
            }
            async fn chat(&self, _r: ChatCompletionRequest) -> LLMResult<ChatCompletionResponse> {
                unimplemented!()
            }
            async fn health_check(&self) -> LLMResult<bool> {
                Ok(false)
            }
        }

        let chain = FallbackChain::builder()
            .add(AlwaysUnhealthy)
            .add_last(AlwaysUnhealthy)
            .build();

        assert!(!chain.health_check().await.unwrap());
    }

    #[test]
    #[should_panic(expected = "at least one provider is required")]
    fn builder_panics_with_no_providers() {
        FallbackChain::builder().build();
    }

    #[test]
    fn chain_len_and_is_empty() {
        let chain = FallbackChain::builder()
            .add(MockProvider::new("p", vec![]))
            .build();
        assert_eq!(chain.len(), 1);
        assert!(!chain.is_empty());
    }

    // ── Circuit breaker ─────────────────────────────────────────────────────

    #[test]
    fn circuit_breaker_opens_after_threshold() {
        let cb = ProviderCircuitBreaker::new(&CircuitBreakerConfig::new(3, 60));
        assert!(!cb.is_open());
        cb.record_failure();
        cb.record_failure();
        assert!(!cb.is_open(), "should still be closed at 2 failures");
        cb.record_failure();
        assert!(cb.is_open(), "should open at threshold=3");
    }

    #[test]
    fn circuit_breaker_resets_on_success() {
        let cb = ProviderCircuitBreaker::new(&CircuitBreakerConfig::new(2, 60));
        cb.record_failure();
        cb.record_failure();
        assert!(cb.is_open());
        cb.record_success();
        assert!(!cb.is_open(), "success should close the circuit");
    }

    #[tokio::test]
    async fn circuit_breaker_skips_open_provider() {
        // p1 fails twice (threshold=2) → circuit opens → p2 should never be
        // tried on the third call via the normal path.
        let p1 = Arc::new(MockProvider::new(
            "p1",
            vec![
                Err(LLMError::RateLimited("1".into())),
                Err(LLMError::RateLimited("2".into())),
                ok_response("p1-recovered"), // would be used if circuit stays closed
            ],
        ));
        let p2 = Arc::new(MockProvider::new(
            "p2",
            vec![
                ok_response("p2-fallback-1"),
                ok_response("p2-fallback-2"),
                ok_response("p2-while-p1-open"),
            ],
        ));
        let p1_ref = p1.clone();
        let p2_ref = p2.clone();

        let chain = FallbackChain::builder()
            .with_circuit_breaker(CircuitBreakerConfig::new(2, 3600))
            .add_shared(p1)
            .add_last_shared(p2)
            .build();

        // Call 1: p1 fails → fallback to p2
        let r1 = chain.chat(request()).await.unwrap();
        assert_eq!(r1.content().unwrap(), "p2-fallback-1");

        // Call 2: p1 fails → circuit opens → fallback to p2
        let r2 = chain.chat(request()).await.unwrap();
        assert_eq!(r2.content().unwrap(), "p2-fallback-2");

        // Call 3: p1 circuit is open → skipped → goes straight to p2
        let r3 = chain.chat(request()).await.unwrap();
        assert_eq!(r3.content().unwrap(), "p2-while-p1-open");
        // p1 was never called on the third request
        assert_eq!(p1_ref.calls(), 2);
        assert_eq!(p2_ref.calls(), 3);
    }

    // ── Metrics ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn metrics_count_requests_and_fallbacks() {
        let p1 = MockProvider::new(
            "p1",
            vec![Err(LLMError::RateLimited("rl".into())), ok_response("ok")],
        );
        let p2 = MockProvider::new("p2", vec![ok_response("fallback-ok")]);

        let chain = FallbackChain::builder().add(p1).add_last(p2).build();

        // Request 1: p1 fails → fallback to p2
        chain.chat(request()).await.unwrap();
        // Request 2: p1 succeeds
        chain.chat(request()).await.unwrap();

        let snap = chain.metrics();
        assert_eq!(snap.requests_total, 2);
        assert_eq!(snap.fallbacks_total, 1);
        assert_eq!(snap.providers[0].fallback_failures, 1);
        assert_eq!(snap.providers[0].successes, 1);
        assert_eq!(snap.providers[1].successes, 1);
    }

    // ── Config deserialization ───────────────────────────────────────────────

    #[test]
    fn fallback_chain_config_deserializes_from_yaml() {
        let yaml = r#"
name: test-chain
circuit_breaker:
  failure_threshold: 5
  cooldown_secs: 60
providers:
  - provider: openai
    api_key: "sk-test"
  - provider: ollama
    base_url: "http://localhost:11434"
    trigger: never
"#;
        let config: FallbackChainConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name.as_deref(), Some("test-chain"));
        let cb = config.circuit_breaker.as_ref().unwrap();
        assert_eq!(cb.failure_threshold, 5);
        assert_eq!(cb.cooldown_secs, 60);
        assert_eq!(config.providers.len(), 2);
        assert_eq!(config.providers[0].llm.provider, "openai");
        assert!(matches!(
            config.providers[1].trigger,
            FallbackTriggerConfig::Never
        ));
    }

    #[test]
    fn trigger_config_converts_to_trigger() {
        let t: FallbackTrigger = FallbackTriggerConfig::AnyError.into();
        assert!(t.should_fallback(&LLMError::Other("x".into())));

        let t: FallbackTrigger = FallbackTriggerConfig::Never.into();
        assert!(!t.should_fallback(&LLMError::RateLimited("x".into())));

        let t: FallbackTrigger =
            FallbackTriggerConfig::Conditions(vec![FallbackConditionConfig::Timeout]).into();
        assert!(t.should_fallback(&LLMError::Timeout("x".into())));
        assert!(!t.should_fallback(&LLMError::RateLimited("x".into())));
    }
}
