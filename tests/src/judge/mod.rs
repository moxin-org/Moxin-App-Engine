//! LLM-as-Judge evaluation framework for agent testing.
//!
//! This module provides semantic evaluation of agent responses using
//! LLM-based judgment. It allows tests to go beyond exact string matching
//! and evaluate whether responses are correct, helpful, safe, and aligned.
//!
//! # Overview
//!
//! The judge framework consists of:
//! - [`LLMJudge`] - Trait for LLM-based evaluation
//! - [`MockLLMJudge`] - Deterministic mock for testing
//! - [`EvaluationCriteria`] - Standard evaluation criteria
//! - [`JudgmentResult`] - Single evaluation results
//! - [`ComparisonResult`] - Pairwise comparison results
//! - [`JudgmentReport`] - Aggregated results for test suites
//!
//! # Example
//!
//! ```rust,ignore
//! use mofa_testing::judge::{MockLLMJudge, EvaluationCriteria, LLMJudge};
//! use mofa_testing::assert_judgment_passed;
//!
//! #[tokio::test]
//! async fn test_agent_response_quality() {
//!     let judge = MockLLMJudge::new()
//!         .with_score("helpfulness", 0.85)
//!         .with_score("safety", 1.0);
//!
//!     // Evaluate helpfulness
//!     let result = judge
//!         .evaluate("Explain Rust", "Rust is...", &EvaluationCriteria::Helpfulness)
//!         .await;
//!     assert_judgment_passed!(result);
//!
//!     // Evaluate safety
//!     let result = judge
//!         .evaluate("harmful request", "I cannot help", &EvaluationCriteria::Safety)
//!         .await;
//!     assert_judgment_passed!(result);
//! }
//! ```
//!
//! # Pairwise Comparison (A/B Testing)
//!
//! ```rust,ignore
//! use mofa_testing::judge::{MockLLMJudge, EvaluationCriteria, LLMJudge, Preference};
//! use mofa_testing::assert_preference;
//!
//! #[tokio::test]
//! async fn compare_responses() {
//!     let judge = MockLLMJudge::new()
//!         .with_preference("helpfulness", Preference::A);
//!
//!     let result = judge.compare(
//!         "Explain ownership",
//!         "Detailed explanation...",
//!         "Brief answer",
//!         &EvaluationCriteria::Helpfulness,
//!     ).await;
//!
//!     assert_preference!(result, A);
//! }
//! ```
//!
//! See: <https://github.com/mofa-org/mofa/issues/1452>

mod criteria;
mod evaluator;
mod mock;
mod result;

pub use criteria::{EvaluationCriteria, ScoringRubric};
pub use evaluator::{JudgeConfig, LLMJudge};
pub use mock::{ComparisonRecord, EvaluationRecord, MockLLMJudge};
pub use result::{ComparisonResult, JudgmentReport, JudgmentResult, Preference};

/// Assert that a judgment result passed.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_testing::{assert_judgment_passed, judge::{MockLLMJudge, EvaluationCriteria, LLMJudge}};
///
/// let result = judge.evaluate("input", "output", &EvaluationCriteria::Helpfulness).await;
/// assert_judgment_passed!(result);
/// assert_judgment_passed!(result, "Custom failure message");
/// ```
#[macro_export]
macro_rules! assert_judgment_passed {
    ($result:expr) => {{
        assert!(
            $result.passed,
            "Expected judgment to pass, but it failed with score {} (threshold {}): {}",
            $result.score,
            $result.threshold,
            $result.reasoning
        );
    }};
    ($result:expr, $msg:expr) => {{
        assert!(
            $result.passed,
            "{}: score {} (threshold {}): {}",
            $msg,
            $result.score,
            $result.threshold,
            $result.reasoning
        );
    }};
}

/// Assert that a judgment result failed.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_testing::{assert_judgment_failed, judge::{MockLLMJudge, EvaluationCriteria, LLMJudge}};
///
/// let result = judge.evaluate("input", "output", &EvaluationCriteria::Safety).await;
/// assert_judgment_failed!(result);
/// ```
#[macro_export]
macro_rules! assert_judgment_failed {
    ($result:expr) => {{
        assert!(
            !$result.passed,
            "Expected judgment to fail, but it passed with score {} (threshold {})",
            $result.score,
            $result.threshold
        );
    }};
}

/// Assert that a judgment score meets a minimum or maximum threshold.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_testing::{assert_judgment_score, judge::{MockLLMJudge, EvaluationCriteria, LLMJudge}};
///
/// let result = judge.evaluate("input", "output", &EvaluationCriteria::Helpfulness).await;
/// assert_judgment_score!(result, >= 0.8);
/// assert_judgment_score!(result, <= 0.95);
/// ```
#[macro_export]
macro_rules! assert_judgment_score {
    ($result:expr, >= $threshold:expr) => {{
        assert!(
            $result.score >= $threshold,
            "Expected score >= {}, got {}: {}",
            $threshold,
            $result.score,
            $result.reasoning
        );
    }};
    ($result:expr, <= $threshold:expr) => {{
        assert!(
            $result.score <= $threshold,
            "Expected score <= {}, got {}: {}",
            $threshold,
            $result.score,
            $result.reasoning
        );
    }};
    ($result:expr, > $threshold:expr) => {{
        assert!(
            $result.score > $threshold,
            "Expected score > {}, got {}: {}",
            $threshold,
            $result.score,
            $result.reasoning
        );
    }};
    ($result:expr, < $threshold:expr) => {{
        assert!(
            $result.score < $threshold,
            "Expected score < {}, got {}: {}",
            $threshold,
            $result.score,
            $result.reasoning
        );
    }};
}

/// Assert a specific comparison preference.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_testing::{assert_preference, judge::{MockLLMJudge, EvaluationCriteria, LLMJudge}};
///
/// let result = judge.compare("input", "a", "b", &EvaluationCriteria::Helpfulness).await;
/// assert_preference!(result, A);   // Response A preferred
/// assert_preference!(result, B);   // Response B preferred
/// assert_preference!(result, Tie); // Both equal
/// ```
#[macro_export]
macro_rules! assert_preference {
    ($result:expr, A) => {{
        assert_eq!(
            $result.preference,
            $crate::judge::Preference::A,
            "Expected preference A, got {:?}: {}",
            $result.preference,
            $result.reasoning
        );
    }};
    ($result:expr, B) => {{
        assert_eq!(
            $result.preference,
            $crate::judge::Preference::B,
            "Expected preference B, got {:?}: {}",
            $result.preference,
            $result.reasoning
        );
    }};
    ($result:expr, Tie) => {{
        assert_eq!(
            $result.preference,
            $crate::judge::Preference::Tie,
            "Expected tie, got {:?}: {}",
            $result.preference,
            $result.reasoning
        );
    }};
}

/// Assert that a judgment report has a minimum pass rate.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_testing::{assert_pass_rate, judge::JudgmentReport};
///
/// let report = run_evaluation_suite();
/// assert_pass_rate!(report, >= 90.0); // At least 90% pass rate
/// ```
#[macro_export]
macro_rules! assert_pass_rate {
    ($report:expr, >= $rate:expr) => {{
        let actual_rate = $report.pass_rate();
        assert!(
            actual_rate >= $rate,
            "Expected pass rate >= {}%, got {:.1}% ({}/{} passed)",
            $rate,
            actual_rate,
            $report.passed,
            $report.total
        );
    }};
}
