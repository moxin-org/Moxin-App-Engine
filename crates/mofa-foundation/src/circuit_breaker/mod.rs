//! Circuit Breaker Pattern Implementation
//!
//! This module provides a configurable circuit breaker pattern for resilient agent execution.
//! It includes:
//! - Circuit breaker state machine (closed, open, half-open)
//! - Exponential backoff with jitter (integrated with existing LLM retry)
//! - Fallback strategies when circuit is open
//! - Per-agent and global circuit breaker configurations
//! - Metrics for circuit breaker state transitions
//!
//! # Architecture
//!
//! ```text
//! +-------------------------------------------------------------------+
//! |                     Circuit Breaker                               |
//! +-------------------------------------------------------------------+
//! |                                                                   |
//! |    +---------+   failure threshold   +--------+                  |
//! |    | CLOSED  | --------------------> |  OPEN  |                  |
//! |    +---------+                       +--------+                  |
//! |         ^                                 |                      |
//! |         |    success threshold           | timeout              |
//! |         |                                 |                      |
//! |         +----------------------------- ----                      |
//! |                           |                                        |
//! |                           v                                        |
//! |                    +-------------+                                 |
//! |                    | HALF-OPEN   |                                 |
//! |                    +-------------+                                 |
//! |                          |                                         |
//! |                          | success                                 |
//! |                          v                                         |
//! |                    +-------------+                                 |
//! |                    |   CLOSED    |                                 |
//! |                    +-------------+                                 |
//! |                                                                   |
//! +-------------------------------------------------------------------+
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use mofa_foundation::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitBreakerState};
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! // Create a circuit breaker with default configuration
//! let config = CircuitBreakerConfig::default();
//! let circuit_breaker = Arc::new(CircuitBreaker::new("agent-1", config));
//!
//! // Check if requests can proceed
//! if circuit_breaker.can_execute() {
//!     // Execute the operation
//!     let result = execute_operation().await;
//!
//!     // Record the result
//!     match result {
//!         Ok(_) => circuit_breaker.record_success(),
//!         Err(e) => circuit_breaker.record_failure(&e),
//!     }
//! } else {
//!     // Circuit is open - use fallback
//!     let fallback_result = circuit_breaker.execute_fallback().await;
//! }
//! ```

pub mod config;
pub mod fallback;
pub mod metrics;
pub mod state;

pub use config::{AgentCircuitBreakerConfig, CircuitBreakerConfig, GlobalCircuitBreakerConfig};
pub use fallback::{
    FallbackBuilder, FallbackContext, FallbackError, FallbackHandler, FallbackStrategy,
    execute_fallback,
};
pub use metrics::{CircuitBreakerMetrics, CircuitBreakerMetricsSnapshot, StateTransition};
pub use state::{AsyncCircuitBreaker, CircuitBreaker, CircuitBreakerError, State};

// Re-export for convenience
pub use crate::llm::types::BackoffStrategy;
