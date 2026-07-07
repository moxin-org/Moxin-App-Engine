//! Circuit Breaker Metrics
//!
//! Provides metrics collection and tracking for circuit breaker state transitions
//! and request statistics.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::state::State;

/// State transition event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    /// Previous state
    pub from_state: State,
    /// New state
    pub to_state: State,
    /// Timestamp of transition (milliseconds since Unix epoch)
    pub timestamp_ms: u64,
}

impl StateTransition {
    /// Create a new state transition
    pub fn new(from_state: State, to_state: State) -> Self {
        Self {
            from_state,
            to_state,
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }

    /// Get the duration since the transition
    pub fn duration_since(&self) -> Duration {
        Duration::from_millis(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
                - self.timestamp_ms,
        )
    }
}

/// Circuit breaker metrics
#[derive(Debug)]
pub struct CircuitBreakerMetrics {
    /// Total number of successful requests
    total_successes: AtomicU64,
    /// Total number of failed requests
    total_failures: AtomicU64,
    /// Total number of rejected requests (when circuit is open)
    total_rejected: AtomicU64,
    /// Number of state transitions
    total_transitions: AtomicU64,
    /// Current consecutive successes
    current_consecutive_successes: AtomicU64,
    /// Current consecutive failures
    current_consecutive_failures: AtomicU64,
    /// Timestamp when circuit was last opened
    last_opened_at: AtomicU64,
    /// Timestamp when circuit was last closed
    last_closed_at: AtomicU64,
    /// Total time spent in open state (nanoseconds)
    total_open_duration_ns: AtomicU64,
    /// Timestamp when current open period started (for calculating open duration)
    current_open_started_at: AtomicU64,
    /// State transition history (we keep last N transitions)
    #[cfg(feature = "tracing")]
    transitions: parking_lot::RwLock<Vec<StateTransition>>,
}

impl CircuitBreakerMetrics {
    /// Create new metrics
    pub fn new() -> Self {
        Self {
            total_successes: AtomicU64::new(0),
            total_failures: AtomicU64::new(0),
            total_rejected: AtomicU64::new(0),
            total_transitions: AtomicU64::new(0),
            current_consecutive_successes: AtomicU64::new(0),
            current_consecutive_failures: AtomicU64::new(0),
            last_opened_at: AtomicU64::new(0),
            last_closed_at: AtomicU64::new(0),
            total_open_duration_ns: AtomicU64::new(0),
            current_open_started_at: AtomicU64::new(0),
            #[cfg(feature = "tracing")]
            transitions: parking_lot::RwLock::new(Vec::new()),
        }
    }

    /// Record a successful request
    pub fn record_success(&self) {
        self.total_successes.fetch_add(1, Ordering::SeqCst);
        self.current_consecutive_successes
            .fetch_add(1, Ordering::SeqCst);
        self.current_consecutive_failures.store(0, Ordering::SeqCst);
    }

    /// Record a failed request
    pub fn record_failure(&self) {
        self.total_failures.fetch_add(1, Ordering::SeqCst);
        self.current_consecutive_failures
            .fetch_add(1, Ordering::SeqCst);
        self.current_consecutive_successes
            .store(0, Ordering::SeqCst);
    }

    /// Record a rejected request (when circuit is open)
    pub fn record_rejected(&self) {
        self.total_rejected.fetch_add(1, Ordering::SeqCst);
    }

    /// Record a state transition
    pub fn record_transition(&self, transition: StateTransition) {
        self.total_transitions.fetch_add(1, Ordering::SeqCst);

        // Track last opened/closed times
        match transition.to_state {
            State::Open => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);
                self.last_opened_at.store(now, Ordering::SeqCst);
                self.current_open_started_at
                    .store(transition.timestamp_ms, Ordering::SeqCst);
            }
            State::Closed => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);
                self.last_closed_at.store(now, Ordering::SeqCst);

                // Calculate open duration
                let open_started = self.current_open_started_at.load(Ordering::SeqCst);
                if open_started > 0 {
                    let duration = transition.timestamp_ms.saturating_sub(open_started) * 1_000_000; // Convert ms to ns
                    self.total_open_duration_ns
                        .fetch_add(duration, Ordering::SeqCst);
                    self.current_open_started_at.store(0, Ordering::SeqCst);
                }
            }
            _ => {}
        }

        #[cfg(feature = "tracing")]
        {
            let mut transitions = self.transitions.write();
            transitions.push(transition);
            // Keep last 100 transitions
            if transitions.len() > 100 {
                transitions.remove(0);
            }
        }
    }

    /// Get total successes
    pub fn total_successes(&self) -> u64 {
        self.total_successes.load(Ordering::SeqCst)
    }

    /// Get total failures
    pub fn total_failures(&self) -> u64 {
        self.total_failures.load(Ordering::SeqCst)
    }

    /// Get total rejected
    pub fn total_rejected(&self) -> u64 {
        self.total_rejected.load(Ordering::SeqCst)
    }

    /// Get total transitions
    pub fn total_transitions(&self) -> u64 {
        self.total_transitions.load(Ordering::SeqCst)
    }

    /// Get current consecutive successes
    pub fn current_consecutive_successes(&self) -> u64 {
        self.current_consecutive_successes.load(Ordering::SeqCst)
    }

    /// Get current consecutive failures
    pub fn current_consecutive_failures(&self) -> u64 {
        self.current_consecutive_failures.load(Ordering::SeqCst)
    }

    /// Get total requests (success + failure)
    pub fn total_requests(&self) -> u64 {
        self.total_successes() + self.total_failures()
    }

    /// Get failure rate as a percentage (0-100)
    pub fn failure_rate(&self) -> f64 {
        let total = self.total_requests();
        if total == 0 {
            return 0.0;
        }
        (self.total_failures() as f64 / total as f64) * 100.0
    }

    /// Get success rate as a percentage (0-100)
    pub fn success_rate(&self) -> f64 {
        100.0 - self.failure_rate()
    }

    /// Get total time spent in open state
    pub fn total_open_duration(&self) -> Duration {
        let ns = self.total_open_duration_ns.load(Ordering::SeqCst);
        Duration::from_nanos(ns)
    }

    /// Get last opened timestamp (as duration since epoch)
    pub fn last_opened_at(&self) -> Option<Duration> {
        let val = self.last_opened_at.load(Ordering::SeqCst);
        if val == 0 {
            None
        } else {
            Some(Duration::from_nanos(val))
        }
    }

    /// Get last closed timestamp (as duration since epoch)
    pub fn last_closed_at(&self) -> Option<Duration> {
        let val = self.last_closed_at.load(Ordering::SeqCst);
        if val == 0 {
            None
        } else {
            Some(Duration::from_nanos(val))
        }
    }

    /// Get all state transitions
    #[cfg(feature = "tracing")]
    pub fn transitions(&self) -> Vec<StateTransition> {
        self.transitions.read().clone()
    }

    /// Get recent state transitions (last N)
    #[cfg(feature = "tracing")]
    pub fn recent_transitions(&self, n: usize) -> Vec<StateTransition> {
        let transitions = self.transitions.read();
        let len = transitions.len();
        if n >= len {
            transitions.clone()
        } else {
            transitions[len - n..].to_vec()
        }
    }

    /// Reset all metrics
    pub fn reset(&self) {
        self.total_successes.store(0, Ordering::SeqCst);
        self.total_failures.store(0, Ordering::SeqCst);
        self.total_rejected.store(0, Ordering::SeqCst);
        self.total_transitions.store(0, Ordering::SeqCst);
        self.current_consecutive_successes
            .store(0, Ordering::SeqCst);
        self.current_consecutive_failures.store(0, Ordering::SeqCst);
        self.last_opened_at.store(0, Ordering::SeqCst);
        self.last_closed_at.store(0, Ordering::SeqCst);
        self.total_open_duration_ns.store(0, Ordering::SeqCst);
        self.current_open_started_at.store(0, Ordering::SeqCst);

        #[cfg(feature = "tracing")]
        {
            self.transitions.write().clear();
        }
    }
}

impl Default for CircuitBreakerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializable metrics for monitoring/display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerMetricsSnapshot {
    /// Total successful requests
    pub total_successes: u64,
    /// Total failed requests
    pub total_failures: u64,
    /// Total rejected requests
    pub total_rejected: u64,
    /// Total requests
    pub total_requests: u64,
    /// Failure rate percentage
    pub failure_rate: f64,
    /// Success rate percentage
    pub success_rate: f64,
    /// Number of state transitions
    pub total_transitions: u64,
    /// Current consecutive successes
    pub consecutive_successes: u64,
    /// Current consecutive failures
    pub consecutive_failures: u64,
    /// Total time spent in open state
    pub total_open_duration_ms: u64,
    /// Last opened timestamp (ms since epoch, 0 if never)
    pub last_opened_ms: u64,
    /// Last closed timestamp (ms since epoch, 0 if never)
    pub last_closed_ms: u64,
}

impl CircuitBreakerMetrics {
    /// Take a snapshot of current metrics
    pub fn snapshot(&self) -> CircuitBreakerMetricsSnapshot {
        CircuitBreakerMetricsSnapshot {
            total_successes: self.total_successes(),
            total_failures: self.total_failures(),
            total_rejected: self.total_rejected(),
            total_requests: self.total_requests(),
            failure_rate: self.failure_rate(),
            success_rate: self.success_rate(),
            total_transitions: self.total_transitions(),
            consecutive_successes: self.current_consecutive_successes(),
            consecutive_failures: self.current_consecutive_failures(),
            total_open_duration_ms: self.total_open_duration().as_millis() as u64,
            last_opened_ms: self
                .last_opened_at()
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            last_closed_ms: self
                .last_closed_at()
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }
}

impl std::fmt::Display for CircuitBreakerMetricsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Circuit Breaker Metrics:")?;
        writeln!(f, "  Total Requests: {}", self.total_requests)?;
        writeln!(f, "  Successes: {}", self.total_successes)?;
        writeln!(f, "  Failures: {}", self.total_failures)?;
        writeln!(f, "  Rejected: {}", self.total_rejected)?;
        writeln!(f, "  Success Rate: {:.2}%", self.success_rate)?;
        writeln!(f, "  Failure Rate: {:.2}%", self.failure_rate)?;
        writeln!(f, "  Consecutive Successes: {}", self.consecutive_successes)?;
        writeln!(f, "  Consecutive Failures: {}", self.consecutive_failures)?;
        writeln!(f, "  Total State Transitions: {}", self.total_transitions)?;
        writeln!(
            f,
            "  Total Open Duration: {}ms",
            self.total_open_duration_ms
        )?;
        if self.last_opened_ms > 0 {
            writeln!(f, "  Last Opened: {}ms ago", self.last_opened_ms)?;
        }
        if self.last_closed_ms > 0 {
            writeln!(f, "  Last Closed: {}ms ago", self.last_closed_ms)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_initialization() {
        let metrics = CircuitBreakerMetrics::new();
        assert_eq!(metrics.total_requests(), 0);
        assert_eq!(metrics.failure_rate(), 0.0);
    }

    #[test]
    fn test_record_success() {
        let metrics = CircuitBreakerMetrics::new();
        metrics.record_success();
        metrics.record_success();

        assert_eq!(metrics.total_successes(), 2);
        assert_eq!(metrics.total_requests(), 2);
        assert_eq!(metrics.success_rate(), 100.0);
    }

    #[test]
    fn test_record_failure() {
        let metrics = CircuitBreakerMetrics::new();
        metrics.record_failure();
        metrics.record_failure();
        metrics.record_success();

        assert_eq!(metrics.total_failures(), 2);
        assert_eq!(metrics.total_successes(), 1);
        assert_eq!(metrics.total_requests(), 3);
        assert!((metrics.failure_rate() - 66.67).abs() < 0.1);
    }

    #[test]
    fn test_record_rejected() {
        let metrics = CircuitBreakerMetrics::new();
        metrics.record_rejected();

        assert_eq!(metrics.total_rejected(), 1);
    }

    #[test]
    fn test_consecutive_counts() {
        let metrics = CircuitBreakerMetrics::new();

        metrics.record_success();
        metrics.record_success();
        assert_eq!(metrics.current_consecutive_successes(), 2);

        metrics.record_failure();
        assert_eq!(metrics.current_consecutive_failures(), 1);
        assert_eq!(metrics.current_consecutive_successes(), 0);
    }

    #[test]
    fn test_snapshot() {
        let metrics = CircuitBreakerMetrics::new();
        metrics.record_success();
        metrics.record_failure();

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.total_requests, 2);
        assert_eq!(snapshot.total_successes, 1);
        assert_eq!(snapshot.total_failures, 1);
    }

    #[test]
    fn test_reset() {
        let metrics = CircuitBreakerMetrics::new();
        metrics.record_success();
        metrics.record_failure();
        metrics.record_rejected();

        metrics.reset();

        assert_eq!(metrics.total_requests(), 0);
        assert_eq!(metrics.total_rejected(), 0);
    }
}
