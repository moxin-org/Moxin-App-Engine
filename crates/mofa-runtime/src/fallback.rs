//! Graceful degradation strategies invoked after retries are exhausted.

use async_trait::async_trait;
use mofa_kernel::agent::{error::AgentError, types::AgentOutput};

/// Called when all retry attempts fail. Return `Some(output)` to substitute a
/// degraded response, or `None` to propagate the error.
#[async_trait]
pub trait FallbackStrategy: Send + Sync {
    async fn on_failure(
        &self,
        agent_id: &str,
        error: &AgentError,
        attempt_count: usize,
    ) -> Option<AgentOutput>;
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
    use super::{FallbackStrategy, NoFallback, StaticFallback};
    use mofa_kernel::agent::{error::AgentError, types::AgentOutput};

    #[tokio::test]
    async fn test_static_fallback_returns_some_output() {
        let strategy = StaticFallback {
            output: AgentOutput::text("service unavailable"),
        };

        let result = strategy
            .on_failure("agent-a", &AgentError::ExecutionFailed("boom".to_string()), 1)
            .await;

        assert!(result.is_some());
        assert_eq!(result.unwrap().to_text(), "service unavailable");
    }

    #[tokio::test]
    async fn test_static_fallback_ignores_agent_id_and_attempt_count() {
        let strategy = StaticFallback {
            output: AgentOutput::text("fallback"),
        };
        let error = AgentError::Timeout { duration_ms: 5000 };

        let first = strategy.on_failure("agent-a", &error, 1).await;
        let second = strategy.on_failure("agent-b", &error, 999).await;

        assert_eq!(first.unwrap().to_text(), "fallback");
        assert_eq!(second.unwrap().to_text(), "fallback");
    }

    #[tokio::test]
    async fn test_no_fallback_returns_none() {
        let strategy = NoFallback;

        let result = strategy
            .on_failure(
                "agent-a",
                &AgentError::ResourceUnavailable("db".to_string()),
                3,
            )
            .await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_no_fallback_always_none_for_any_attempt_count() {
        let strategy = NoFallback;
        let error = AgentError::ExecutionFailed("boom".to_string());

        let first = strategy.on_failure("agent", &error, 1).await;
        let second = strategy.on_failure("agent", &error, 1000).await;

        assert!(first.is_none());
        assert!(second.is_none());
    }
}
