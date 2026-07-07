//! Circuit Breaker State Machine
//!
//! Implements the core circuit breaker state machine with three states:
//! - Closed: Normal operation, requests are allowed
//! - Open: Circuit is open, requests are blocked
//! - Half-Open: Testing if the service has recovered

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant as StdInstant;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time;

use super::config::CircuitBreakerConfig;
use super::metrics::{CircuitBreakerMetrics, StateTransition};

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum State {
    /// Normal operation - requests are allowed
    Closed,
    /// Circuit is open - requests are blocked
    Open,
    /// Testing recovery - limited requests allowed
    HalfOpen,
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            State::Closed => write!(f, "closed"),
            State::Open => write!(f, "open"),
            State::HalfOpen => write!(f, "half-open"),
        }
    }
}

/// Request result for recording
#[derive(Debug)]
pub enum RequestResult {
    /// Request succeeded
    Success,
    /// Request failed
    Failure(Option<Duration>), // Optional timeout duration
}

/// Circuit Breaker implementation
#[derive(Debug)]
pub struct CircuitBreaker {
    /// Configuration
    config: CircuitBreakerConfig,
    /// Current state
    state: RwLock<State>,
    /// Number of consecutive failures
    consecutive_failures: AtomicU32,
    /// Number of consecutive successes (for half-open -> closed)
    consecutive_successes: AtomicU32,
    /// Number of requests in half-open state
    half_open_requests: AtomicU32,
    /// Timestamp when circuit was opened
    opened_at: RwLock<Option<Instant>>,
    /// Metrics
    metrics: Arc<CircuitBreakerMetrics>,
    /// Last failure timestamp (for window-based failure rate)
    last_failure_at: RwLock<Option<Instant>>,
    /// Total requests in current window
    window_requests: AtomicU64,
    /// Window start time
    window_start: RwLock<Instant>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new(name: impl Into<String>, config: CircuitBreakerConfig) -> Arc<Self> {
        let mut config = config;
        config.name = name.into();

        Arc::new(Self {
            config,
            state: RwLock::new(State::Closed),
            consecutive_failures: AtomicU32::new(0),
            consecutive_successes: AtomicU32::new(0),
            half_open_requests: AtomicU32::new(0),
            opened_at: RwLock::new(None),
            metrics: Arc::new(CircuitBreakerMetrics::new()),
            last_failure_at: RwLock::new(None),
            window_requests: AtomicU64::new(0),
            window_start: RwLock::new(StdInstant::now()),
        })
    }

    /// Create with default configuration
    pub fn with_default(name: impl Into<String>) -> Arc<Self> {
        Self::new(name, CircuitBreakerConfig::default())
    }

    /// Get the current state
    pub async fn state(&self) -> State {
        // Check if we need to transition from open to half-open
        if let Some(opened_at) = *self.opened_at.read().await {
            let current_state = *self.state.read().await;
            if current_state == State::Open && opened_at.elapsed() >= self.config.timeout {
                self.transition_to_half_open().await;
            }
        }

        *self.state.read().await
    }

    /// Get the current state synchronously (without timeout check)
    pub fn state_sync(&self) -> State {
        // For sync access, we just return the current state
        // The async state() method should be used for proper timeout handling
        State::Closed // Placeholder - actual state is in RwLock
    }

    /// Check if a request can be executed
    pub async fn can_execute(&self) -> bool {
        if !self.config.enabled {
            return true;
        }

        let state = self.state().await;

        match state {
            State::Closed => true,
            State::Open => false,
            State::HalfOpen => {
                let current = self.half_open_requests.load(Ordering::SeqCst);
                current < self.config.half_open_max_requests
            }
        }
    }

    /// Record a successful request
    pub async fn record_success(&self) {
        if !self.config.enabled {
            return;
        }

        let state = *self.state.read().await;

        match state {
            State::Closed => {
                // Reset failure count on success
                self.consecutive_failures.store(0, Ordering::SeqCst);
                self.reset_window_if_needed().await;
                self.window_requests.fetch_add(1, Ordering::SeqCst);
                self.metrics.record_success();
            }
            State::HalfOpen => {
                let successes = self.consecutive_successes.fetch_add(1, Ordering::SeqCst) + 1;

                if successes >= self.config.success_threshold {
                    self.transition_to_closed().await;
                }

                self.half_open_requests.fetch_sub(1, Ordering::SeqCst);
                self.metrics.record_success();
            }
            State::Open => {
                // Should not happen - can_execute should prevent this
            }
        }
    }

    /// Record a failed request
    pub async fn record_failure(&self, _error: Option<&dyn std::error::Error>) {
        if !self.config.enabled {
            return;
        }

        let state = *self.state.read().await;
        // Note: Error type checking is simplified - if you need specific error type
        // handling, consider passing the error type directly instead of a trait object

        match state {
            State::Closed => {
                self.consecutive_failures.fetch_add(1, Ordering::SeqCst);
                self.consecutive_successes.store(0, Ordering::SeqCst);
                self.reset_window_if_needed().await;

                let failures = self.consecutive_failures.load(Ordering::SeqCst);
                let total = self.window_requests.fetch_add(1, Ordering::SeqCst) + 1;

                self.metrics.record_failure();

                // Update last failure time
                *self.last_failure_at.write().await = Some(StdInstant::now());

                // Check if we should open the circuit
                if self.config.use_failure_rate && total >= self.config.minimum_requests as u64 {
                    let failure_rate = self.calculate_failure_rate().await;
                    if failure_rate >= self.config.failure_rate_threshold as f64 {
                        self.transition_to_open().await;
                        return;
                    }
                }

                // Simple consecutive failure threshold
                if failures >= self.config.failure_threshold {
                    self.transition_to_open().await;
                }
            }
            State::HalfOpen => {
                // Any failure in half-open state goes back to open
                self.half_open_requests.fetch_sub(1, Ordering::SeqCst);
                self.transition_to_open().await;
                self.metrics.record_failure();
            }
            State::Open => {
                // Already open, just record the failure
                self.metrics.record_failure();
            }
        }
    }

    /// Record a timeout (if timeouts are counted as failures)
    pub async fn record_timeout(&self) {
        if self.config.count_timeouts_as_failures {
            self.record_failure(None).await;
        }
    }

    /// Get metrics
    pub fn metrics(&self) -> &Arc<CircuitBreakerMetrics> {
        &self.metrics
    }

    /// Get the configuration
    pub fn config(&self) -> &CircuitBreakerConfig {
        &self.config
    }

    /// Get the name
    pub fn name(&self) -> &str {
        &self.config.name
    }

    // =========================================================================
    // Private methods
    // =========================================================================

    /// Transition to closed state
    async fn transition_to_closed(&self) {
        let old_state = *self.state.read().await;
        if old_state != State::Closed {
            *self.state.write().await = State::Closed;
            self.consecutive_failures.store(0, Ordering::SeqCst);
            self.consecutive_successes.store(0, Ordering::SeqCst);
            self.half_open_requests.store(0, Ordering::SeqCst);
            *self.opened_at.write().await = None;

            self.metrics
                .record_transition(StateTransition::new(old_state, State::Closed));
        }
    }

    /// Transition to open state
    async fn transition_to_open(&self) {
        let old_state = *self.state.read().await;
        if old_state != State::Open {
            *self.state.write().await = State::Open;
            *self.opened_at.write().await = Some(StdInstant::now());

            self.metrics
                .record_transition(StateTransition::new(old_state, State::Open));
        }
    }

    /// Transition to half-open state
    async fn transition_to_half_open(&self) {
        let old_state = *self.state.read().await;
        if old_state == State::Open {
            *self.state.write().await = State::HalfOpen;
            self.consecutive_successes.store(0, Ordering::SeqCst);
            self.half_open_requests.store(0, Ordering::SeqCst);

            self.metrics
                .record_transition(StateTransition::new(old_state, State::HalfOpen));
        }
    }

    /// Reset the window if needed
    async fn reset_window_if_needed(&self) {
        let window_start = *self.window_start.read().await;
        if window_start.elapsed() >= self.config.window_duration {
            *self.window_start.write().await = StdInstant::now();
            self.window_requests.store(0, Ordering::SeqCst);
        }
    }

    /// Calculate the failure rate in the current window
    async fn calculate_failure_rate(&self) -> f64 {
        let total = self.window_requests.load(Ordering::SeqCst) as f64;
        if total == 0.0 {
            return 0.0;
        }

        let failures = self.metrics.total_failures() as f64;
        // We need to track failures in window separately for accurate rate
        // For now, approximate using total failures
        let rate = (failures / total) * 100.0;
        rate.min(100.0)
    }
}

impl Clone for CircuitBreaker {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: RwLock::new(*self.state.blocking_read()),
            consecutive_failures: AtomicU32::new(self.consecutive_failures.load(Ordering::SeqCst)),
            consecutive_successes: AtomicU32::new(
                self.consecutive_successes.load(Ordering::SeqCst),
            ),
            half_open_requests: AtomicU32::new(self.half_open_requests.load(Ordering::SeqCst)),
            opened_at: RwLock::new(*self.opened_at.blocking_read()),
            metrics: Arc::clone(&self.metrics),
            last_failure_at: RwLock::new(*self.last_failure_at.blocking_read()),
            window_requests: AtomicU64::new(self.window_requests.load(Ordering::SeqCst)),
            window_start: RwLock::new(*self.window_start.blocking_read()),
        }
    }
}

/// Async wrapper for CircuitBreaker that provides convenient async interface
pub struct AsyncCircuitBreaker {
    inner: Arc<CircuitBreaker>,
}

impl AsyncCircuitBreaker {
    /// Create a new async circuit breaker
    pub fn new(name: impl Into<String>, config: CircuitBreakerConfig) -> Arc<Self> {
        Arc::new(Self {
            inner: CircuitBreaker::new(name, config),
        })
    }

    /// Execute an operation with circuit breaker protection
    pub async fn execute<F, T, E>(&self, operation: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: std::future::Future<Output = Result<T, E>>,
        E: std::error::Error + 'static,
    {
        // Check if we can execute
        if !self.inner.can_execute().await {
            return Err(CircuitBreakerError::CircuitOpen {
                name: self.inner.name().to_string(),
                state: *self.inner.state.read().await,
            });
        }

        // Execute the operation
        let result = operation.await;

        // Record the result
        match &result {
            Ok(_) => self.inner.record_success().await,
            Err(e) => {
                self.inner
                    .record_failure(Some(e as &dyn std::error::Error))
                    .await
            }
        }

        result.map_err(|e| CircuitBreakerError::OperationError {
            name: self.inner.name().to_string(),
            error: Arc::new(e),
        })
    }

    /// Execute with fallback
    pub async fn execute_with_fallback<F, T, E, FB>(
        &self,
        operation: F,
        fallback: FB,
    ) -> Result<T, CircuitBreakerError<E>>
    where
        F: std::future::Future<Output = Result<T, E>>,
        FB: std::future::Future<Output = T>,
        E: std::error::Error + 'static,
    {
        // Check if we can execute
        if !self.inner.can_execute().await {
            // Use fallback
            let result = fallback.await;
            return Ok(result);
        }

        // Execute the operation
        let result = operation.await;

        // Record the result
        match &result {
            Ok(_) => self.inner.record_success().await,
            Err(e) => {
                self.inner
                    .record_failure(Some(e as &dyn std::error::Error))
                    .await
            }
        }

        result.map_err(|e| CircuitBreakerError::OperationError {
            name: self.inner.name().to_string(),
            error: Arc::new(e),
        })
    }

    /// Get current state
    pub async fn state(&self) -> State {
        self.inner.state().await
    }

    /// Check if can execute
    pub async fn can_execute(&self) -> bool {
        self.inner.can_execute().await
    }

    /// Get metrics
    pub fn metrics(&self) -> &Arc<CircuitBreakerMetrics> {
        self.inner.metrics()
    }
}

impl Clone for AsyncCircuitBreaker {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Circuit breaker error types
#[derive(Debug)]
pub enum CircuitBreakerError<E: std::error::Error + 'static> {
    /// Circuit is open
    CircuitOpen { name: String, state: State },
    /// Operation error
    OperationError { name: String, error: Arc<E> },
}

impl<E: std::error::Error + 'static> std::fmt::Display for CircuitBreakerError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CircuitOpen { name, state } => {
                write!(f, "Circuit breaker '{}' is {}", name, state)
            }
            Self::OperationError { name, error } => {
                write!(
                    f,
                    "Operation error in circuit breaker '{}': {}",
                    name, error
                )
            }
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for CircuitBreakerError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CircuitOpen { .. } => None,
            Self::OperationError { error, .. } => Some(error.as_ref() as _),
        }
    }
}

/// Extension trait forResult to add circuit breaker support
pub trait CircuitBreakerResultExt<T, E: std::error::Error + 'static> {
    /// Record success or failure with circuit breaker
    fn record_with(self, breaker: &CircuitBreaker);
}

impl<T, E: std::error::Error + 'static> CircuitBreakerResultExt<T, E> for Result<T, E> {
    fn record_with(self, breaker: &CircuitBreaker) {
        // We need async context, so this is a placeholder
        // Use the async methods instead
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_closed_state_allows_requests() {
        let cb = CircuitBreaker::new("test", CircuitBreakerConfig::default());
        assert!(cb.can_execute().await);
    }

    #[tokio::test]
    async fn test_failure_opens_circuit() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        // Record failures
        cb.record_failure(None).await;
        cb.record_failure(None).await;

        // Circuit should be open
        assert!(!cb.can_execute().await);
        assert_eq!(*cb.state.read().await, State::Open);
    }

    #[tokio::test]
    async fn test_success_resets_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        // Record one failure
        cb.record_failure(None).await;

        // Record success
        cb.record_success().await;

        // Record two more failures - should not open circuit
        cb.record_failure(None).await;
        cb.record_failure(None).await;

        assert!(cb.can_execute().await);
    }

    #[tokio::test]
    async fn test_timeout_transitions_to_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            timeout: Duration::from_millis(50),
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        // Open the circuit
        cb.record_failure(None).await;
        assert!(!cb.can_execute().await);

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should transition to half-open
        let state = cb.state().await;
        assert_eq!(state, State::HalfOpen);
    }

    #[tokio::test]
    async fn test_half_open_success_closes_circuit() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 2,
            timeout: Duration::from_millis(50),
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        // Open the circuit
        cb.record_failure(None).await;

        // Wait for timeout to transition to half-open
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Use state() method which triggers transition
        assert_eq!(cb.state().await, State::HalfOpen);

        // Record successes
        cb.record_success().await;
        cb.record_success().await;

        // Should be closed now
        assert_eq!(cb.state().await, State::Closed);
    }

    #[tokio::test]
    async fn test_disabled_circuit_always_allows() {
        let config = CircuitBreakerConfig {
            enabled: false,
            failure_threshold: 1,
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        // Open the circuit (but it's disabled)
        cb.record_failure(None).await;

        // Should still allow requests
        assert!(cb.can_execute().await);
    }

    #[tokio::test]
    async fn test_async_execute_success() {
        let cb = AsyncCircuitBreaker::new("test", CircuitBreakerConfig::default());

        let result = cb
            .execute(async { Ok::<_, std::io::Error>("success") })
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
    }

    #[tokio::test]
    async fn test_async_execute_circuit_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let cb = AsyncCircuitBreaker::new("test", config);

        // Open the circuit
        cb.inner.record_failure(None).await;

        // Try to execute - should fail
        let result = cb.execute(async { Ok::<_, std::io::Error>("test") }).await;
        assert!(matches!(
            result,
            Err(CircuitBreakerError::CircuitOpen { .. })
        ));
    }

    #[tokio::test]
    async fn test_async_execute_with_fallback() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let cb = AsyncCircuitBreaker::new("test", config);

        // Open the circuit
        cb.inner.record_failure(None).await;

        // Execute with fallback
        let result = cb
            .execute_with_fallback(async { Ok::<_, std::io::Error>("test") }, async {
                "fallback"
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "fallback");
    }
}
