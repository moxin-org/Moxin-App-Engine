//! LLM-as-Judge evaluator trait and configuration.
//!
//! This module defines the [`LLMJudge`] trait that all judge implementations
//! must satisfy, along with [`JudgeConfig`] for configuring judge behavior.

use crate::judge::{ComparisonResult, EvaluationCriteria, JudgmentResult};

/// Trait for LLM-based evaluation of agent responses.
///
/// Implementations can use a real LLM backend or provide mock responses
/// for deterministic testing. The trait is designed to be object-safe
/// for dynamic dispatch.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_testing::judge::{LLMJudge, MockLLMJudge, EvaluationCriteria};
///
/// async fn evaluate_response(judge: &impl LLMJudge) {
///     let result = judge.evaluate(
///         "What is Rust?",
///         "Rust is a systems programming language.",
///         &EvaluationCriteria::Helpfulness,
///     ).await;
///
///     println!("Score: {}, Passed: {}", result.score, result.passed);
/// }
/// ```
pub trait LLMJudge: Send + Sync {
    /// Evaluate a response against the given criteria.
    ///
    /// # Arguments
    /// * `input` - The original user input/query
    /// * `output` - The agent's response to evaluate
    /// * `criteria` - The evaluation criteria to use
    ///
    /// # Returns
    /// A `JudgmentResult` containing the score, reasoning, and pass/fail status.
    fn evaluate(
        &self,
        input: &str,
        output: &str,
        criteria: &EvaluationCriteria,
    ) -> impl std::future::Future<Output = JudgmentResult> + Send;

    /// Compare two responses and determine which is better.
    ///
    /// # Arguments
    /// * `input` - The original user input/query
    /// * `output_a` - The first response to compare
    /// * `output_b` - The second response to compare
    /// * `criteria` - The evaluation criteria to use
    ///
    /// # Returns
    /// A `ComparisonResult` indicating preference and scores.
    fn compare(
        &self,
        input: &str,
        output_a: &str,
        output_b: &str,
        criteria: &EvaluationCriteria,
    ) -> impl std::future::Future<Output = ComparisonResult> + Send;

    /// Get the default pass threshold for this judge.
    ///
    /// Returns 0.7 by default, meaning scores >= 0.7 are considered passing.
    fn default_threshold(&self) -> f64 {
        0.7
    }

    /// Get the name of this judge implementation.
    fn name(&self) -> &str;
}

/// Configuration for an LLM judge.
///
/// Allows customizing judge behavior such as the pass threshold,
/// retry policy, and output verbosity.
#[derive(Debug, Clone, PartialEq)]
pub struct JudgeConfig {
    /// Default threshold for pass/fail determination (0.0 - 1.0)
    pub threshold: f64,
    /// Maximum retries for evaluation failures
    pub max_retries: usize,
    /// Whether to include detailed reasoning in results
    pub include_reasoning: bool,
    /// Timeout for evaluation in milliseconds (0 = no timeout)
    pub timeout_ms: u64,
}

impl Default for JudgeConfig {
    fn default() -> Self {
        Self {
            threshold: 0.7,
            max_retries: 3,
            include_reasoning: true,
            timeout_ms: 30_000, // 30 seconds
        }
    }
}

impl JudgeConfig {
    /// Create a new judge configuration with defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the pass threshold (clamped to 0.0 - 1.0).
    #[must_use]
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Set maximum retries for failed evaluations.
    #[must_use]
    pub fn with_max_retries(mut self, retries: usize) -> Self {
        self.max_retries = retries;
        self
    }

    /// Enable or disable detailed reasoning in results.
    #[must_use]
    pub fn with_reasoning(mut self, include: bool) -> Self {
        self.include_reasoning = include;
        self
    }

    /// Set evaluation timeout in milliseconds.
    #[must_use]
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Create a strict configuration (high threshold, no retries).
    #[must_use]
    pub fn strict() -> Self {
        Self {
            threshold: 0.9,
            max_retries: 0,
            include_reasoning: true,
            timeout_ms: 10_000,
        }
    }

    /// Create a lenient configuration (low threshold, more retries).
    #[must_use]
    pub fn lenient() -> Self {
        Self {
            threshold: 0.5,
            max_retries: 5,
            include_reasoning: true,
            timeout_ms: 60_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let config = JudgeConfig::default();
        assert!((config.threshold - 0.7).abs() < f64::EPSILON);
        assert_eq!(config.max_retries, 3);
        assert!(config.include_reasoning);
        assert_eq!(config.timeout_ms, 30_000);
    }

    #[test]
    fn config_builder() {
        let config = JudgeConfig::new()
            .with_threshold(0.8)
            .with_max_retries(5)
            .with_reasoning(false)
            .with_timeout(10_000);

        assert!((config.threshold - 0.8).abs() < f64::EPSILON);
        assert_eq!(config.max_retries, 5);
        assert!(!config.include_reasoning);
        assert_eq!(config.timeout_ms, 10_000);
    }

    #[test]
    fn config_threshold_clamped() {
        let high = JudgeConfig::new().with_threshold(1.5);
        assert!((high.threshold - 1.0).abs() < f64::EPSILON);

        let low = JudgeConfig::new().with_threshold(-0.5);
        assert!(low.threshold.abs() < f64::EPSILON);
    }

    #[test]
    fn config_presets() {
        let strict = JudgeConfig::strict();
        assert!((strict.threshold - 0.9).abs() < f64::EPSILON);
        assert_eq!(strict.max_retries, 0);

        let lenient = JudgeConfig::lenient();
        assert!((lenient.threshold - 0.5).abs() < f64::EPSILON);
        assert_eq!(lenient.max_retries, 5);
    }
}
