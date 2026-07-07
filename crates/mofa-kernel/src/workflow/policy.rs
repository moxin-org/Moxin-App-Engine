//! Node-level fault-tolerance policy for StateGraph execution.
//!
//! This module provides opt-in circuit-breaker and retry semantics for nodes
//! in a [`StateGraph`].  The default policy is a no-op (errors propagate
//! unchanged), so adding this module is backward-compatible.
//!
//! # Overview
//!
//! ```text
//! ┌──────────────┐  error   ┌────────────────────┐
//! │   NodeFunc   │─────────▶│   NodePolicy       │
//! │   (call)     │          │ ┌────────────────┐  │
//! └──────────────┘          │ │  RetryCondition│  │
//!                           │ └───────┬────────┘  │
//!                           │         │ retryable? │
//!                           │    yes ─┘    no      │
//!                           │  retry w/      ┌─────┤
//!                           │  back-off      │ CB  │
//!                           │                │open?│
//!                           │                └──┬──┘
//!                           │            yes    │   no
//!                           │      fallback ◀───┘   │
//!                           │      node              │ propagate
//!                           └────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use mofa_kernel::workflow::policy::{NodePolicy, RetryCondition};
//! use std::time::Duration;
//!
//! let policy = NodePolicy {
//!     max_retries: 3,
//!     retry_backoff_ms: 150,
//!     retry_condition: RetryCondition::OnTransient(vec!["timeout".to_string(), "rate limit".to_string()]),
//!     fallback_node: Some("fallback_handler".to_string()),
//!     circuit_open_after: 5,
//!     circuit_reset_after: Duration::from_secs(30),
//! };
//! ```

use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

// ============================================================================
// RetryCondition — which errors are retryable
// ============================================================================

/// Determines which errors are eligible for retry.
///
/// # Variants
///
/// - `Always` — every error is retried up to `max_retries`
/// - `Never` — errors are never retried (circuit breaker still applies)
/// - `OnTransient(patterns)` — retry only when the error message contains
///   at least one of the given substrings (case-insensitive)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
#[derive(Default)]
pub enum RetryCondition {
    /// Retry on every error.
    Always,
    /// Never retry; propagate errors immediately.
    #[default]
    Never,
    /// Retry only when the error message contains one of the listed patterns
    /// (case-insensitive substring match).
    OnTransient(Vec<String>),
}

impl RetryCondition {
    /// Returns `true` if the given error message satisfies the retry condition.
    ///
    /// # Example
    ///
    /// ```rust
    /// use mofa_kernel::workflow::policy::RetryCondition;
    ///
    /// let cond = RetryCondition::OnTransient(vec!["timeout".to_string()]);
    /// assert!(cond.matches("connection timeout"));
    /// assert!(!cond.matches("permission denied"));
    ///
    /// assert!(RetryCondition::Always.matches("anything"));
    /// assert!(!RetryCondition::Never.matches("anything"));
    /// ```
    pub fn matches(&self, error_message: &str) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::OnTransient(patterns) => {
                let lower = error_message.to_lowercase();
                // `p.to_lowercase()` already allocates a `String`; calling
                // `.as_str().to_owned()` on top converts it back to `String`
                // via a needless round-trip (issue #1466).  Use `.as_str()`
                // directly to borrow the temporary and avoid the extra alloc.
                patterns
                    .iter()
                    .any(|p| lower.contains(p.to_lowercase().as_str()))
            }
        }
    }
}

// ============================================================================
// CircuitState — state machine for the circuit breaker
// ============================================================================

/// State of a per-node circuit breaker.
///
/// The circuit follows a standard three-state machine:
///
/// ```text
/// Closed ──(N failures)──▶ Open ──(reset_after)──▶ HalfOpen
///    ▲                                                  │
///    └──────────────── success ────────────────────────┘
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[derive(Default)]
pub enum CircuitState {
    /// Healthy — requests pass through normally.
    #[default]
    Closed,
    /// Failing — requests are short-circuited to the fallback node (or return
    /// [`AgentError::CircuitOpen`]).
    Open,
    /// Recovery probe — one request is allowed through; success closes the
    /// circuit, failure re-opens it.
    HalfOpen,
}

// ============================================================================
// CircuitBreakerState — shared runtime state per node
// ============================================================================

/// Runtime state for a single node's circuit breaker.
///
/// Wrap in `Arc` so it can be shared across async tasks without cloning the
/// lock; use [`CircuitBreakerState::new`] via [`NodePolicy`].
///
/// All methods are `async` because they acquire the internal `RwLock`.
#[derive(Debug, Clone)]
pub struct CircuitBreakerState {
    inner: Arc<RwLock<CbInner>>,
}

#[derive(Debug)]
struct CbInner {
    state: CircuitState,
    consecutive_failures: u32,
    opened_at: Option<Instant>,
    reset_after: Duration,
    open_after: u32,
}

impl CircuitBreakerState {
    /// Create a new circuit breaker that opens after `open_after` consecutive
    /// failures and resets after `reset_after` has elapsed.
    pub fn new(open_after: u32, reset_after: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(CbInner {
                state: CircuitState::Closed,
                consecutive_failures: 0,
                opened_at: None,
                reset_after,
                open_after,
            })),
        }
    }

    /// Record a successful node execution.
    ///
    /// - Resets the consecutive-failure counter.
    /// - Closes the circuit if it was `HalfOpen`.
    pub async fn record_success(&self) {
        let mut inner = self.inner.write().await;
        inner.consecutive_failures = 0;
        if inner.state == CircuitState::HalfOpen {
            inner.state = CircuitState::Closed;
            inner.opened_at = None;
        }
    }

    /// Record a failed node execution.
    ///
    /// Increments the failure counter; opens the circuit when it reaches the
    /// threshold.  If the circuit is `HalfOpen`, the probe failed so it
    /// immediately re-opens.
    pub async fn record_failure(&self) {
        let mut inner = self.inner.write().await;
        inner.consecutive_failures += 1;

        let should_open = inner.state == CircuitState::HalfOpen
            || (inner.state == CircuitState::Closed
                && inner.consecutive_failures >= inner.open_after);

        if should_open {
            inner.state = CircuitState::Open;
            inner.opened_at = Some(Instant::now());
        }
    }

    /// Returns the current [`CircuitState`], automatically transitioning
    /// `Open → HalfOpen` when `reset_after` has elapsed.
    pub async fn state(&self) -> CircuitState {
        // Fast read path
        {
            let inner = self.inner.read().await;
            match inner.state {
                CircuitState::Closed => return CircuitState::Closed,
                CircuitState::HalfOpen => return CircuitState::HalfOpen,
                CircuitState::Open => {
                    // Check if we should transition to HalfOpen
                    if let Some(opened_at) = inner.opened_at
                        && opened_at.elapsed() < inner.reset_after
                    {
                        return CircuitState::Open;
                    }
                    // Fall through to write path
                }
            }
        }

        // Write path: Open → HalfOpen transition
        let mut inner = self.inner.write().await;
        if inner.state == CircuitState::Open
            && let Some(opened_at) = inner.opened_at
            && opened_at.elapsed() >= inner.reset_after
        {
            inner.state = CircuitState::HalfOpen;
        }
        inner.state
    }

    /// Returns `true` if the circuit is open (blocking requests).
    pub async fn is_open(&self) -> bool {
        self.state().await == CircuitState::Open
    }

    /// Force-close the circuit (useful for testing or operator override).
    pub async fn force_close(&self) {
        let mut inner = self.inner.write().await;
        inner.state = CircuitState::Closed;
        inner.consecutive_failures = 0;
        inner.opened_at = None;
    }

    /// Returns the current consecutive failure count.
    pub async fn consecutive_failures(&self) -> u32 {
        self.inner.read().await.consecutive_failures
    }
}

// ============================================================================
// NodePolicy — per-node fault-tolerance configuration
// ============================================================================

/// Per-node fault-tolerance policy for a [`StateGraph`] node.
///
/// Attach a policy to a node via [`StateGraph::with_node_policy`].
/// The default value is a **no-op**: all errors propagate immediately with no
/// retry or circuit-breaker logic, preserving full backward compatibility.
///
/// # Fields
///
/// | Field | Default | Description |
/// |-------|---------|-------------|
/// | `max_retries` | `0` | Maximum retry attempts before giving up |
/// | `retry_backoff_ms` | `100` | Base delay between retries (ms); doubles each attempt |
/// | `retry_condition` | `Never` | Which errors trigger a retry |
/// | `fallback_node` | `None` | Node to route to when all retries are exhausted |
/// | `circuit_open_after` | `u32::MAX` | Consecutive failures before circuit opens |
/// | `circuit_reset_after` | `60s` | Duration until `Open → HalfOpen` transition |
///
/// # Example
///
/// ```rust
/// use mofa_kernel::workflow::policy::{NodePolicy, RetryCondition};
/// use std::time::Duration;
///
/// let policy = NodePolicy {
///     max_retries: 3,
///     retry_backoff_ms: 200,
///     retry_condition: RetryCondition::OnTransient(vec!["timeout".to_string()]),
///     fallback_node: Some("fallback_llm".to_string()),
///     circuit_open_after: 5,
///     circuit_reset_after: Duration::from_secs(30),
/// };
///
/// assert!(policy.is_retryable("connection timeout"));
/// assert!(!policy.is_retryable("invalid schema"));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePolicy {
    /// Maximum number of retry attempts (0 = no retry).
    pub max_retries: u32,

    /// Base back-off delay in milliseconds between retry attempts.
    /// The actual delay is `retry_backoff_ms * 2^(attempt - 1)` (exponential).
    pub retry_backoff_ms: u64,

    /// Condition that determines which errors are eligible for retry.
    pub retry_condition: RetryCondition,

    /// Optional node ID to route execution to when all retries are exhausted
    /// or when the circuit is open.  If `None`, the error is propagated to the
    /// caller.
    pub fallback_node: Option<String>,

    /// Number of consecutive failures that cause the circuit breaker to open.
    /// Set to `u32::MAX` (default) to effectively disable the circuit breaker.
    pub circuit_open_after: u32,

    /// Duration after which an `Open` circuit transitions to `HalfOpen` to
    /// probe for recovery.
    #[serde(with = "duration_serde")]
    pub circuit_reset_after: Duration,
}

impl Default for NodePolicy {
    fn default() -> Self {
        Self {
            max_retries: 0,
            retry_backoff_ms: 100,
            retry_condition: RetryCondition::Never,
            fallback_node: None,
            circuit_open_after: u32::MAX,
            circuit_reset_after: Duration::from_secs(60),
        }
    }
}

impl NodePolicy {
    /// Returns `true` if the error should be retried according to
    /// [`RetryCondition`] and `max_retries`.
    pub fn is_retryable(&self, error_message: &str) -> bool {
        self.max_retries > 0 && self.retry_condition.matches(error_message)
    }

    /// Compute the back-off delay for a given attempt number (0-indexed).
    ///
    /// Uses capped exponential back-off: `min(backoff_ms * 2^attempt, 30_000)` ms.
    pub fn backoff_for_attempt(&self, attempt: u32) -> Duration {
        let shift = attempt.min(14); // cap shift to avoid overflow (2^14 = 16384)
        let multiplier: u64 = 1u64 << shift;
        let ms = self.retry_backoff_ms.saturating_mul(multiplier).min(30_000);
        Duration::from_millis(ms)
    }

    /// Build a fresh [`CircuitBreakerState`] configured from this policy.
    pub fn build_circuit_breaker(&self) -> CircuitBreakerState {
        CircuitBreakerState::new(self.circuit_open_after, self.circuit_reset_after)
    }
}

// ============================================================================
// Serde helper for Duration (seconds as u64)
// ============================================================================

mod duration_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_secs())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let secs = u64::deserialize(d)?;
        Ok(Duration::from_secs(secs))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time;

    // --- RetryCondition ---

    #[test]
    fn test_retry_condition_always() {
        let cond = RetryCondition::Always;
        assert!(cond.matches("anything"));
        assert!(cond.matches(""));
    }

    #[test]
    fn test_retry_condition_never() {
        let cond = RetryCondition::Never;
        assert!(!cond.matches("anything"));
        assert!(!cond.matches("timeout"));
    }

    #[test]
    fn test_retry_condition_on_transient_match() {
        let cond =
            RetryCondition::OnTransient(vec!["timeout".to_string(), "rate limit".to_string()]);
        assert!(cond.matches("connection timeout exceeded"));
        assert!(cond.matches("Rate Limit hit")); // case-insensitive
        assert!(!cond.matches("permission denied"));
        assert!(!cond.matches("invalid schema"));
    }

    // --- NodePolicy ---

    #[test]
    fn test_default_policy_is_noop() {
        let policy = NodePolicy::default();
        assert_eq!(policy.max_retries, 0);
        assert!(!policy.is_retryable("timeout"));
        assert!(policy.fallback_node.is_none());
        assert_eq!(policy.circuit_open_after, u32::MAX);
    }

    #[test]
    fn test_policy_is_retryable() {
        let policy = NodePolicy {
            max_retries: 3,
            retry_condition: RetryCondition::OnTransient(vec!["timeout".to_string()]),
            ..Default::default()
        };
        assert!(policy.is_retryable("request timeout"));
        assert!(!policy.is_retryable("auth error")); // not in patterns
    }

    #[test]
    fn test_policy_no_retry_when_max_zero() {
        let policy = NodePolicy {
            max_retries: 0,
            retry_condition: RetryCondition::Always,
            ..Default::default()
        };
        assert!(!policy.is_retryable("timeout")); // max_retries = 0 blocks it
    }

    #[test]
    fn test_backoff_for_attempt() {
        let policy = NodePolicy {
            retry_backoff_ms: 100,
            ..Default::default()
        };
        assert_eq!(policy.backoff_for_attempt(0), Duration::from_millis(100));
        assert_eq!(policy.backoff_for_attempt(1), Duration::from_millis(200));
        assert_eq!(policy.backoff_for_attempt(2), Duration::from_millis(400));
        // Capped at 30 000ms
        assert_eq!(
            policy.backoff_for_attempt(20),
            Duration::from_millis(30_000)
        );
    }

    #[test]
    fn test_policy_serialization() {
        let policy = NodePolicy {
            max_retries: 2,
            retry_backoff_ms: 250,
            retry_condition: RetryCondition::Always,
            fallback_node: Some("fallback".to_string()),
            circuit_open_after: 5,
            circuit_reset_after: Duration::from_secs(45),
        };
        let json = serde_json::to_string(&policy).unwrap();
        let restored: NodePolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.max_retries, 2);
        assert_eq!(restored.fallback_node.as_deref(), Some("fallback"));
        assert_eq!(restored.circuit_reset_after, Duration::from_secs(45));
    }

    // --- CircuitBreakerState ---

    #[tokio::test]
    async fn test_circuit_starts_closed() {
        let cb = CircuitBreakerState::new(3, Duration::from_secs(60));
        assert_eq!(cb.state().await, CircuitState::Closed);
        assert!(!cb.is_open().await);
        assert_eq!(cb.consecutive_failures().await, 0);
    }

    #[tokio::test]
    async fn test_circuit_opens_after_n_failures() {
        let cb = CircuitBreakerState::new(3, Duration::from_secs(60));
        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Closed);
        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Closed);
        cb.record_failure().await; // 3rd failure → open
        assert_eq!(cb.state().await, CircuitState::Open);
        assert!(cb.is_open().await);
    }

    #[tokio::test]
    async fn test_success_resets_failure_counter() {
        let cb = CircuitBreakerState::new(3, Duration::from_secs(60));
        cb.record_failure().await;
        cb.record_failure().await;
        cb.record_success().await;
        assert_eq!(cb.consecutive_failures().await, 0);
        assert_eq!(cb.state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_transitions_to_half_open() {
        // Use a very short reset window so we don't wait 60 s in tests
        let cb = CircuitBreakerState::new(1, Duration::from_millis(50));
        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Open);

        // Wait for reset window
        time::sleep(Duration::from_millis(60)).await;
        assert_eq!(cb.state().await, CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn test_circuit_closes_after_half_open_success() {
        let cb = CircuitBreakerState::new(1, Duration::from_millis(50));
        cb.record_failure().await;
        time::sleep(Duration::from_millis(60)).await;
        assert_eq!(cb.state().await, CircuitState::HalfOpen);

        cb.record_success().await;
        assert_eq!(cb.state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_reopens_after_half_open_failure() {
        let cb = CircuitBreakerState::new(1, Duration::from_millis(50));
        cb.record_failure().await;
        time::sleep(Duration::from_millis(60)).await;
        assert_eq!(cb.state().await, CircuitState::HalfOpen);

        cb.record_failure().await; // probe failed → re-open
        assert_eq!(cb.state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn test_force_close() {
        let cb = CircuitBreakerState::new(1, Duration::from_secs(60));
        cb.record_failure().await;
        assert!(cb.is_open().await);
        cb.force_close().await;
        assert_eq!(cb.state().await, CircuitState::Closed);
        assert_eq!(cb.consecutive_failures().await, 0);
    }
}
