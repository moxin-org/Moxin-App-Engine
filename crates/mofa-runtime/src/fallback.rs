//! Graceful degradation strategies invoked after retries are exhausted.

use async_trait::async_trait;
use mofa_kernel::agent::{error::AgentError, types::AgentOutput};
use std::time::Duration;

/// Rich execution context passed to [`FallbackStrategy::on_failure_with_context`].
///
/// Provides the fallback implementation with enough information to make
/// intelligent degradation decisions (e.g., "return cached response when the
/// circuit was open" vs "try a cheaper model on timeout").
///
/// # Example
///
/// ```rust,ignore
/// use mofa_runtime::fallback::{FallbackStrategy, FallbackContext};
///
/// struct SmartFallback;
///
/// #[async_trait]
/// impl FallbackStrategy for SmartFallback {
///     async fn on_failure(
///         &self,
///         _agent_id: &str,
///         _error: &AgentError,
///         _attempt_count: usize,
///     ) -> Option<AgentOutput> {
///         None // unused — we override on_failure_with_context
///     }
///
///     async fn on_failure_with_context(
///         &self,
///         ctx: &FallbackContext<'_>,
///     ) -> Option<AgentOutput> {
///         if ctx.total_attempts > 3 {
///             // Return a degraded placeholder after many failures
///             Some(AgentOutput::text("Service temporarily degraded"))
///         } else {
///             None
///         }
///     }
/// }
/// ```
#[derive(Debug)]
pub struct FallbackContext<'a> {
    /// Identifier of the agent that failed.
    pub agent_id: &'a str,
    /// The final error after all retries were exhausted.
    pub error: &'a AgentError,
    /// Number of attempts that were actually made (always ≥ 1).
    pub attempt_count: usize,
    /// Maximum attempts that were configured (from `RetryConfig`).
    pub total_attempts: usize,
    /// Cumulative wall-clock duration across all retry attempts,
    /// or `None` if not tracked by the caller.
    pub elapsed: Option<Duration>,
}

/// Called when all retry attempts fail. Return `Some(output)` to substitute a
/// degraded response, or `None` to propagate the error.
///
/// Implementors can override either [`on_failure`](FallbackStrategy::on_failure)
/// for simple cases, or [`on_failure_with_context`](FallbackStrategy::on_failure_with_context)
/// for access to richer execution metadata.  The default
/// `on_failure_with_context` delegates to `on_failure`, so existing
/// implementations remain backward-compatible.
#[async_trait]
pub trait FallbackStrategy: Send + Sync {
    /// Simple fallback hook. Override this for basic strategies.
    async fn on_failure(
        &self,
        agent_id: &str,
        error: &AgentError,
        attempt_count: usize,
    ) -> Option<AgentOutput>;

    /// Context-aware fallback hook. Override this for intelligent degradation.
    ///
    /// The default implementation delegates to [`on_failure`](FallbackStrategy::on_failure),
    /// so existing implementations do not need to change.
    async fn on_failure_with_context(
        &self,
        ctx: &FallbackContext<'_>,
    ) -> Option<AgentOutput> {
        self.on_failure(ctx.agent_id, ctx.error, ctx.attempt_count)
            .await
    }
}

/// Returns a fixed static output useful for "service unavailable" placeholders.
pub struct StaticFallback {
    pub output: AgentOutput,
}

#[async_trait]
impl FallbackStrategy for StaticFallback {
    async fn on_failure(&self, _: &str, _: &AgentError, _: usize) -> Option<AgentOutput> {
        Some(self.output.clone())
    }
}

/// Returns `None` error propagates unchanged. Default behaviour.
pub struct NoFallback;

#[async_trait]
impl FallbackStrategy for NoFallback {
    async fn on_failure(&self, _: &str, _: &AgentError, _: usize) -> Option<AgentOutput> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_context_debug_output() {
        let err = AgentError::ExecutionFailed("timeout".into());
        let ctx = FallbackContext {
            agent_id: "agent-1",
            error: &err,
            attempt_count: 3,
            total_attempts: 5,
            elapsed: Some(Duration::from_secs(10)),
        };
        let debug = format!("{ctx:?}");
        assert!(debug.contains("agent-1"));
        assert!(debug.contains("attempt_count: 3"));
        assert!(debug.contains("total_attempts: 5"));
    }

    #[tokio::test]
    async fn default_on_failure_with_context_delegates() {
        let fb = NoFallback;
        let err = AgentError::ExecutionFailed("test".into());
        let ctx = FallbackContext {
            agent_id: "a",
            error: &err,
            attempt_count: 1,
            total_attempts: 1,
            elapsed: None,
        };
        // NoFallback.on_failure returns None, so delegation should too
        assert!(fb.on_failure_with_context(&ctx).await.is_none());
    }

    #[tokio::test]
    async fn static_fallback_with_context_returns_output() {
        let output = AgentOutput::text("degraded");
        let fb = StaticFallback {
            output: output.clone(),
        };
        let err = AgentError::ExecutionFailed("test".into());
        let ctx = FallbackContext {
            agent_id: "a",
            error: &err,
            attempt_count: 3,
            total_attempts: 3,
            elapsed: Some(Duration::from_millis(500)),
        };
        let result = fb.on_failure_with_context(&ctx).await;
        assert!(result.is_some());
    }
}
