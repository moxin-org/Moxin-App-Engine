//! Retry policies and async retry helper.

use std::future::Future;
use std::time::Duration;

use crate::agent::error::{AgentError, AgentResult};
use rand::Rng;

/// Global safety cap for linear retry delay to avoid pathological sleeps.
const MAX_LINEAR_BACKOFF_MS: u64 = 60_000;

/// Delay strategy between retry attempts.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RetryPolicy {
    /// Same delay every attempt.
    Fixed { delay_ms: u64 },
    /// Delay increases linearly: `base_ms * attempt`, capped by
    /// [`MAX_LINEAR_BACKOFF_MS`].
    Linear { base_ms: u64 },
    /// Exponential backoff capped at `max_ms`, with optional ±12.5% jitter.
    ExponentialBackoff {
        base_ms: u64,
        max_ms: u64,
        jitter: bool,
    },
}

impl RetryPolicy {
    /// Returns the sleep duration before the given retry attempt (0-indexed).
    pub fn delay_for(&self, attempt: usize) -> Duration {
        let ms = match self {
            RetryPolicy::Fixed { delay_ms } => *delay_ms,
            RetryPolicy::Linear { base_ms } => {
                let factor = attempt
                    .checked_add(1)
                    .and_then(|v| u64::try_from(v).ok())
                    .unwrap_or(u64::MAX);
                base_ms.saturating_mul(factor).min(MAX_LINEAR_BACKOFF_MS)
            }
            RetryPolicy::ExponentialBackoff {
                base_ms,
                max_ms,
                jitter,
            } => {
                let exp = 1u64
                    .checked_shl(attempt as u32)
                    .and_then(|s| base_ms.checked_mul(s))
                    .unwrap_or(*max_ms);
                let capped = exp.min(*max_ms);
                if *jitter {
                    let eighth = capped / 8;
                    let lower = capped.saturating_sub(eighth);
                    let upper = capped.saturating_add(eighth).min(*max_ms);
                    rand::thread_rng().gen_range(lower..=upper)
                } else {
                    capped
                }
            }
        };
        Duration::from_millis(ms)
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy::Fixed { delay_ms: 1_000 }
    }
}

/// How many attempts to make and which [`RetryPolicy`] to use.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RetryConfig {
    /// Total attempts (1 = no retry).
    pub max_attempts: usize,
    pub policy: RetryPolicy,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 1,
            policy: RetryPolicy::default(),
        }
    }
}

impl RetryConfig {
    /// Exponential backoff with jitter — a sensible production default.
    pub fn exponential(max_attempts: usize, base_ms: u64, max_ms: u64) -> Self {
        Self {
            max_attempts,
            policy: RetryPolicy::ExponentialBackoff {
                base_ms,
                max_ms,
                jitter: true,
            },
        }
    }
}

/// Retry `f` up to `config.max_attempts` times
pub async fn retry_with_policy<F, Fut, T>(
    config: &RetryConfig,
    is_retryable: impl Fn(&AgentError) -> bool,
    mut f: F,
) -> AgentResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = AgentResult<T>>,
{
    let max_attempts = config.max_attempts.max(1);
    let mut last_err = None;

    for attempt in 0..max_attempts {
        if attempt > 0 {
            tokio::time::sleep(config.policy.delay_for(attempt - 1)).await;
        }
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                if !is_retryable(&e) {
                    return Err(e);
                }
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| AgentError::ExecutionFailed("No attempts made".into())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_fixed_policy_delay() {
        let p = RetryPolicy::Fixed { delay_ms: 500 };
        assert_eq!(p.delay_for(0), Duration::from_millis(500));
        assert_eq!(p.delay_for(5), Duration::from_millis(500));
    }

    #[test]
    fn test_linear_policy_delay() {
        let p = RetryPolicy::Linear { base_ms: 200 };
        assert_eq!(p.delay_for(0), Duration::from_millis(200));
        assert_eq!(p.delay_for(2), Duration::from_millis(600));
    }

    #[test]
    fn test_linear_policy_delay_is_capped() {
        let p = RetryPolicy::Linear { base_ms: 10_000 };
        assert_eq!(
            p.delay_for(100),
            Duration::from_millis(MAX_LINEAR_BACKOFF_MS)
        );
    }

    #[test]
    fn test_linear_policy_large_attempt_stays_bounded() {
        let p = RetryPolicy::Linear { base_ms: 1 };
        assert_eq!(
            p.delay_for(usize::MAX),
            Duration::from_millis(MAX_LINEAR_BACKOFF_MS)
        );
    }

    #[test]
    fn test_exponential_policy_delay() {
        let p = RetryPolicy::ExponentialBackoff {
            base_ms: 100,
            max_ms: 800,
            jitter: false,
        };
        assert_eq!(p.delay_for(0), Duration::from_millis(100));
        assert_eq!(p.delay_for(1), Duration::from_millis(200));
        assert_eq!(p.delay_for(3), Duration::from_millis(800));
    }

    #[test]
    fn test_jitter_does_not_exceed_cap() {
        let p = RetryPolicy::ExponentialBackoff {
            base_ms: 500,
            max_ms: 1_000,
            jitter: true,
        };
        for attempt in 0..10 {
            assert!(p.delay_for(attempt).as_millis() <= 1_000);
        }
    }

    #[tokio::test]
    async fn test_retry_helper_succeeds_on_second_attempt() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = call_count.clone();
        let config = RetryConfig {
            max_attempts: 3,
            policy: RetryPolicy::Fixed { delay_ms: 0 },
        };

        let result = retry_with_policy(
            &config,
            |e| e.is_retryable(),
            || {
                let cc = cc.clone();
                async move {
                    let n = cc.fetch_add(1, Ordering::SeqCst);
                    if n == 0 {
                        Err(AgentError::ResourceUnavailable("busy".into()))
                    } else {
                        Ok(42u32)
                    }
                }
            },
        )
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_retry_helper_fails_on_non_retryable() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = call_count.clone();
        let config = RetryConfig {
            max_attempts: 5,
            policy: RetryPolicy::Fixed { delay_ms: 0 },
        };

        let result: AgentResult<u32> = retry_with_policy(
            &config,
            |e| e.is_retryable(),
            || {
                let cc = cc.clone();
                async move {
                    cc.fetch_add(1, Ordering::SeqCst);
                    Err(AgentError::ConfigError("bad config".into()))
                }
            },
        )
        .await;

        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 1); // aborted after 1, not 5
    }

    // ── Bounded-iteration invariant tests (#780) ──────────────────────

    #[test]
    fn test_backoff_delay_never_exceeds_max() {
        let base_ms = 100;
        let max_ms = 5_000;

        // Without jitter
        let p = RetryPolicy::ExponentialBackoff {
            base_ms,
            max_ms,
            jitter: false,
        };
        for attempt in 0..20 {
            let delay = p.delay_for(attempt).as_millis() as u64;
            assert!(
                delay <= max_ms,
                "attempt {attempt}: delay {delay} ms exceeded max {max_ms} ms (no jitter)",
            );
        }

        // With jitter
        let p = RetryPolicy::ExponentialBackoff {
            base_ms,
            max_ms,
            jitter: true,
        };
        for attempt in 0..20 {
            let delay = p.delay_for(attempt).as_millis() as u64;
            assert!(
                delay <= max_ms,
                "attempt {attempt}: delay {delay} ms exceeded max {max_ms} ms (jitter)",
            );
        }
    }

    #[test]
    fn test_jitter_stays_within_bounds() {
        let base_ms = 200;
        let max_ms = 10_000;
        let p = RetryPolicy::ExponentialBackoff {
            base_ms,
            max_ms,
            jitter: true,
        };

        for attempt in 0..20 {
            let delay = p.delay_for(attempt).as_millis() as u64;

            // Recompute the non-jittered capped value to derive bounds.
            let exp = 1u64
                .checked_shl(attempt as u32)
                .and_then(|s| base_ms.checked_mul(s))
                .unwrap_or(max_ms);
            let capped = exp.min(max_ms);
            let eighth = capped / 8;
            let lower_bound = capped.saturating_sub(eighth);

            assert!(
                delay >= lower_bound,
                "attempt {attempt}: delay {delay} ms below lower bound {lower_bound} ms",
            );
            assert!(
                delay <= max_ms,
                "attempt {attempt}: delay {delay} ms exceeded max {max_ms} ms",
            );
        }
    }

    #[test]
    fn test_jitter_is_not_deterministic_by_attempt_parity() {
        let p = RetryPolicy::ExponentialBackoff {
            base_ms: 400,
            max_ms: 10_000,
            jitter: true,
        };

        let mut observed = std::collections::BTreeSet::new();
        for _ in 0..32 {
            observed.insert(p.delay_for(3).as_millis() as u64);
        }

        assert!(
            observed.len() > 1,
            "expected randomized jitter to produce varied delay values"
        );
    }

    #[test]
    fn test_monotonic_growth_before_saturation_no_jitter() {
        let base_ms = 50;
        let max_ms = 3_200;
        let p = RetryPolicy::ExponentialBackoff {
            base_ms,
            max_ms,
            jitter: false,
        };

        let mut prev_delay = 0u64;
        for attempt in 0..20 {
            let delay = p.delay_for(attempt).as_millis() as u64;
            assert!(
                delay >= prev_delay,
                "attempt {attempt}: delay {delay} ms decreased from previous {prev_delay} ms",
            );
            prev_delay = delay;
        }
    }
}
