//! Retry utilities for dora adapter node execution.

use mofa_kernel::core::MofaError;
use std::future::Future;
use std::time::Duration;

/// Retry policy for node execution in dora runtime paths.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 200,
            max_delay_ms: 5_000,
            jitter: false,
        }
    }
}

impl RetryPolicy {
    /// Exponential backoff with cap:
    /// min(base_delay * 2^attempt, max_delay)
    pub fn delay_for(&self, attempt: u32) -> Duration {
        let shift = attempt.min(63);
        let factor = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
        let delay_ms = self
            .base_delay_ms
            .saturating_mul(factor)
            .min(self.max_delay_ms);

        if !self.jitter {
            return Duration::from_millis(delay_ms);
        }

        let jitter_window = delay_ms / 10;
        let jittered = if attempt.is_multiple_of(2) {
            delay_ms.saturating_add(jitter_window)
        } else {
            delay_ms.saturating_sub(jitter_window)
        }
        .min(self.max_delay_ms);

        Duration::from_millis(jittered)
    }
}

/// Retry helper dedicated to [`MofaError`] classification.
pub async fn retry_with_policy<F, Fut, T>(
    policy: &RetryPolicy,
    mut operation: F,
) -> Result<T, MofaError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, MofaError>>,
{
    let max_attempts = policy.max_attempts.max(1);
    let mut attempt: u32 = 0;

    loop {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) => {
                let next_attempt = attempt.saturating_add(1);
                let can_retry = error.is_retryable() && next_attempt < max_attempts;
                if !can_retry {
                    return Err(error);
                }

                tokio::time::sleep(policy.delay_for(attempt)).await;
                attempt = next_attempt;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn delay_for_exponential_backoff_with_cap() {
        let policy = RetryPolicy {
            max_attempts: 5,
            base_delay_ms: 100,
            max_delay_ms: 450,
            jitter: false,
        };

        assert_eq!(policy.delay_for(0), Duration::from_millis(100));
        assert_eq!(policy.delay_for(1), Duration::from_millis(200));
        assert_eq!(policy.delay_for(2), Duration::from_millis(400));
        assert_eq!(policy.delay_for(3), Duration::from_millis(450));
    }

    #[tokio::test]
    async fn retries_timeout_until_success() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_ref = attempts.clone();

        let policy = RetryPolicy {
            max_attempts: 4,
            base_delay_ms: 0,
            max_delay_ms: 0,
            jitter: false,
        };

        let result = retry_with_policy(&policy, || {
            let attempts_ref = attempts_ref.clone();
            async move {
                let n = attempts_ref.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    Err(MofaError::Timeout { timeout_ms: 500 })
                } else {
                    Ok("ok")
                }
            }
        })
        .await;

        assert_eq!(result, Ok("ok"));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn does_not_retry_permanent_auth_error() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_ref = attempts.clone();

        let policy = RetryPolicy {
            max_attempts: 5,
            base_delay_ms: 0,
            max_delay_ms: 0,
            jitter: false,
        };

        let result = retry_with_policy(&policy, || {
            let attempts_ref = attempts_ref.clone();
            async move {
                attempts_ref.fetch_add(1, Ordering::SeqCst);
                Err(MofaError::AuthFailed {
                    reason: "invalid api key".to_string(),
                })
            }
        })
        .await;

        assert!(matches!(result, Err(MofaError::AuthFailed { .. })));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
}