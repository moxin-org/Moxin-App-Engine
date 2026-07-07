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
    use super::*;

    fn test_error() -> AgentError {
        AgentError::NotFound("test-agent".into())
    }

    #[tokio::test]
    async fn static_fallback_returns_output() {
        let fallback = StaticFallback {
            output: AgentOutput::text("service unavailable"),
        };
        let result = fallback.on_failure("agent-1", &test_error(), 3).await;
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn static_fallback_ignores_agent_id_and_attempts() {
        let fallback = StaticFallback {
            output: AgentOutput::text("down"),
        };
        // Different agent_id and attempt_count should still return the same output
        let r1 = fallback.on_failure("a", &test_error(), 1).await;
        let r2 = fallback.on_failure("b", &test_error(), 100).await;
        assert!(r1.is_some());
        assert!(r2.is_some());
    }

    #[tokio::test]
    async fn no_fallback_returns_none() {
        let fallback = NoFallback;
        let result = fallback.on_failure("agent-1", &test_error(), 5).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn no_fallback_always_none_regardless_of_inputs() {
        let fallback = NoFallback;
        for attempts in [0, 1, 10, 100] {
            let result = fallback.on_failure("any", &test_error(), attempts).await;
            assert!(result.is_none());
        }
    }
}
