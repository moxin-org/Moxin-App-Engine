//! Fallback Strategies
//!
//! Provides various fallback strategies to use when the circuit breaker is open.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Fallback strategy types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FallbackStrategy {
    /// Return an error message
    ReturnError(String),
    /// Return a cached response
    ReturnCachedResponse,
    /// Return a default value
    ReturnDefaultValue(String),
    /// Call an alternative service
    CallAlternativeService(String),
    /// Queue the request for later retry
    QueueForRetry,
}

/// Fallback handler trait
#[async_trait]
pub trait FallbackHandler<T, R>: Send + Sync {
    /// Handle the fallback when circuit is open
    async fn handle(&self, input: &T, context: &FallbackContext) -> R;
}

/// Context for fallback handling
#[derive(Debug, Clone)]
pub struct FallbackContext {
    /// Circuit breaker name
    pub circuit_name: String,
    /// Current state
    pub state: super::state::State,
    /// Number of times circuit has been open
    pub open_count: u64,
    /// Last error message
    pub last_error: Option<String>,
    /// Request timestamp
    pub request_time: std::time::Instant,
}

impl FallbackContext {
    /// Create a new fallback context
    pub fn new(circuit_name: impl Into<String>, state: super::state::State) -> Self {
        Self {
            circuit_name: circuit_name.into(),
            state,
            open_count: 0,
            last_error: None,
            request_time: std::time::Instant::now(),
        }
    }

    /// Set the open count
    pub fn with_open_count(mut self, count: u64) -> Self {
        self.open_count = count;
        self
    }

    /// Set the last error
    pub fn with_last_error(mut self, error: impl Into<String>) -> Self {
        self.last_error = Some(error.into());
        self
    }
}

/// Simple fallback handler that returns a fixed value
pub struct SimpleFallbackHandler<R: Clone + Default + Send + Sync + 'static> {
    value: R,
}

impl<R: Clone + Default + Send + Sync + 'static> SimpleFallbackHandler<R> {
    /// Create a new simple fallback handler
    pub fn new(value: R) -> Arc<Self> {
        Arc::new(Self { value })
    }
}

#[async_trait]
impl<R: Clone + Default + Send + Sync + 'static> FallbackHandler<String, R>
    for SimpleFallbackHandler<R>
{
    async fn handle(&self, _input: &String, _context: &FallbackContext) -> R {
        self.value.clone()
    }
}

/// Cached response fallback handler
pub struct CachedResponseFallbackHandler<T: Clone + Send + Sync + 'static> {
    cache: Arc<RwLock<Option<T>>>,
}

impl<T: Clone + Send + Sync + 'static> CachedResponseFallbackHandler<T> {
    /// Create a new cached response fallback handler
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            cache: Arc::new(RwLock::new(None)),
        })
    }

    /// Set the cached response
    pub async fn set_cached(&self, response: T) {
        let mut cache = self.cache.write().await;
        *cache = Some(response);
    }

    /// Get the cached response
    pub async fn get_cached(&self) -> Option<T> {
        let cache = self.cache.read().await;
        cache.clone()
    }
}

impl<T: Clone + Send + Sync + 'static> Default for CachedResponseFallbackHandler<T> {
    fn default() -> Self {
        Self {
            cache: Arc::new(RwLock::new(None)),
        }
    }
}

#[async_trait]
impl<T: Clone + Send + Sync + 'static> FallbackHandler<String, Option<T>>
    for CachedResponseFallbackHandler<T>
{
    async fn handle(&self, _input: &String, _context: &FallbackContext) -> Option<T> {
        let cache = self.cache.read().await;
        cache.clone()
    }
}

/// Chain of fallback handlers
pub struct FallbackChain<T, R> {
    handlers: Vec<Box<dyn FallbackHandler<T, R> + Send + Sync>>,
}

impl<T, R> FallbackChain<T, R> {
    /// Create a new fallback chain
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Add a handler to the chain
    pub fn with_handler<H>(mut self, handler: H) -> Self
    where
        H: FallbackHandler<T, R> + Send + Sync + 'static,
    {
        self.handlers.push(Box::new(handler));
        self
    }

    /// Execute the chain
    pub async fn execute(&self, input: &T, context: &FallbackContext) -> Option<R> {
        for handler in &self.handlers {
            let result = handler.handle(input, context).await;
            // If we get a Some value, return it
            // For Option<R>, we check if it's Some
            if let Some(value) = self.to_option(result) {
                return Some(value);
            }
        }
        None
    }

    /// Convert result to Option
    fn to_option(&self, _result: R) -> Option<R> {
        // This is a placeholder - the actual implementation depends on R
        None
    }
}

impl<T, R> Default for FallbackChain<T, R> {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating fallback strategies
pub struct FallbackBuilder {
    strategy: Option<FallbackStrategy>,
}

impl FallbackBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self { strategy: None }
    }

    /// Set return error strategy
    pub fn return_error(mut self, message: impl Into<String>) -> Self {
        self.strategy = Some(FallbackStrategy::ReturnError(message.into()));
        self
    }

    /// Set return cached response strategy
    pub fn return_cached_response(mut self) -> Self {
        self.strategy = Some(FallbackStrategy::ReturnCachedResponse);
        self
    }

    /// Set return default value strategy
    pub fn return_default(mut self, value: impl Into<String>) -> Self {
        self.strategy = Some(FallbackStrategy::ReturnDefaultValue(value.into()));
        self
    }

    /// Set call alternative service strategy
    pub fn call_alternative(mut self, service: impl Into<String>) -> Self {
        self.strategy = Some(FallbackStrategy::CallAlternativeService(service.into()));
        self
    }

    /// Set queue for retry strategy
    pub fn queue_for_retry(mut self) -> Self {
        self.strategy = Some(FallbackStrategy::QueueForRetry);
        self
    }

    /// Build the fallback strategy
    pub fn build(self) -> FallbackStrategy {
        self.strategy.unwrap_or(FallbackStrategy::ReturnError(
            "Circuit breaker is open".to_string(),
        ))
    }
}

impl Default for FallbackBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Execute a fallback strategy and return the result
pub async fn execute_fallback(
    strategy: &FallbackStrategy,
    input: &str,
    context: &FallbackContext,
) -> Result<String, FallbackError> {
    match strategy {
        FallbackStrategy::ReturnError(message) => Err(FallbackError::CircuitOpen {
            message: message.clone(),
            context: context.clone(),
        }),
        FallbackStrategy::ReturnCachedResponse => {
            // This would need a cache implementation
            Err(FallbackError::NoCache {
                message: "No cached response available".to_string(),
            })
        }
        FallbackStrategy::ReturnDefaultValue(value) => Ok(value.clone()),
        FallbackStrategy::CallAlternativeService(service) => {
            // This would need an alternative service implementation
            Err(FallbackError::AlternativeServiceNotConfigured {
                service: service.clone(),
            })
        }
        FallbackStrategy::QueueForRetry => Err(FallbackError::QueuedForRetry {
            message: "Request queued for retry".to_string(),
        }),
    }
}

/// Fallback errors
#[derive(Debug)]
pub enum FallbackError {
    /// Circuit is open
    CircuitOpen {
        message: String,
        context: FallbackContext,
    },
    /// No cached response available
    NoCache { message: String },
    /// Alternative service not configured
    AlternativeServiceNotConfigured { service: String },
    /// Request queued for retry
    QueuedForRetry { message: String },
}

impl std::fmt::Display for FallbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CircuitOpen { message, .. } => write!(f, "Circuit open: {}", message),
            Self::NoCache { message } => write!(f, "No cache: {}", message),
            Self::AlternativeServiceNotConfigured { service } => {
                write!(f, "Alternative service not configured: {}", service)
            }
            Self::QueuedForRetry { message } => write!(f, "Queued for retry: {}", message),
        }
    }
}

impl std::error::Error for FallbackError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_builder_error() {
        let strategy = FallbackBuilder::new()
            .return_error("Service unavailable")
            .build();

        assert!(matches!(strategy, FallbackStrategy::ReturnError(_)));
    }

    #[test]
    fn test_fallback_builder_default() {
        let strategy = FallbackBuilder::new()
            .return_default("default response")
            .build();

        assert!(matches!(strategy, FallbackStrategy::ReturnDefaultValue(_)));
    }

    #[tokio::test]
    async fn test_execute_fallback_error() {
        let strategy = FallbackStrategy::ReturnError("Test error".to_string());
        let context = FallbackContext::new("test", super::super::state::State::Open);

        let result = execute_fallback(&strategy, "input", &context).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_fallback_default() {
        let strategy = FallbackStrategy::ReturnDefaultValue("default".to_string());
        let context = FallbackContext::new("test", super::super::state::State::Open);

        let result = execute_fallback(&strategy, "input", &context).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "default");
    }

    #[test]
    fn test_fallback_context() {
        let context = FallbackContext::new("test-circuit", super::super::state::State::Open)
            .with_open_count(5)
            .with_last_error("Connection timeout");

        assert_eq!(context.circuit_name, "test-circuit");
        assert_eq!(context.open_count, 5);
        assert!(context.last_error.is_some());
    }
}
