//! Result types for LLM-as-Judge evaluations.
//!
//! This module provides [`JudgmentResult`] for single evaluations,
//! [`ComparisonResult`] for pairwise comparisons, and [`JudgmentReport`]
//! for aggregating results across a test suite.

use serde::{Deserialize, Serialize};

/// Result of an LLM-as-Judge evaluation.
///
/// Contains the score, pass/fail status, reasoning, and metadata
/// about the evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgmentResult {
    /// Score between 0.0 and 1.0
    pub score: f64,
    /// Whether the evaluation passed (score >= threshold)
    pub passed: bool,
    /// The LLM's reasoning for the score
    pub reasoning: String,
    /// The criteria that was evaluated
    pub criteria: String,
    /// Evaluation latency in milliseconds
    pub latency_ms: u64,
    /// The threshold used for pass/fail determination
    pub threshold: f64,
}

impl JudgmentResult {
    /// Create a new judgment result.
    ///
    /// The score is clamped to the range [0.0, 1.0].
    #[must_use]
    pub fn new(
        score: f64,
        reasoning: impl Into<String>,
        criteria: impl Into<String>,
        threshold: f64,
        latency_ms: u64,
    ) -> Self {
        let score = score.clamp(0.0, 1.0);
        Self {
            score,
            passed: score >= threshold,
            reasoning: reasoning.into(),
            criteria: criteria.into(),
            latency_ms,
            threshold,
        }
    }

    /// Create a passing judgment with score 1.0.
    #[must_use]
    pub fn pass(criteria: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self::new(1.0, reasoning, criteria, 0.7, 0)
    }

    /// Create a failing judgment with score 0.0.
    #[must_use]
    pub fn fail(criteria: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self::new(0.0, reasoning, criteria, 0.7, 0)
    }

    /// Check if the judgment would pass with a different threshold.
    #[must_use]
    pub fn passed_with_threshold(&self, threshold: f64) -> bool {
        self.score >= threshold
    }

    /// Create a new result with updated latency.
    #[must_use]
    pub fn with_latency(mut self, latency_ms: u64) -> Self {
        self.latency_ms = latency_ms;
        self
    }
}

/// Preference in a pairwise comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Preference {
    /// Response A is preferred
    A,
    /// Response B is preferred
    B,
    /// Both responses are equally good (tie)
    Tie,
}

impl Preference {
    /// Returns true if A is preferred.
    #[must_use]
    pub fn is_a(&self) -> bool {
        matches!(self, Self::A)
    }

    /// Returns true if B is preferred.
    #[must_use]
    pub fn is_b(&self) -> bool {
        matches!(self, Self::B)
    }

    /// Returns true if it's a tie.
    #[must_use]
    pub fn is_tie(&self) -> bool {
        matches!(self, Self::Tie)
    }

    /// Get the preference as a string label.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::Tie => "Tie",
        }
    }
}

/// Result of a pairwise comparison between two responses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComparisonResult {
    /// Which response was preferred
    pub preference: Preference,
    /// Score for response A (0.0 - 1.0)
    pub score_a: f64,
    /// Score for response B (0.0 - 1.0)
    pub score_b: f64,
    /// The LLM's reasoning for the preference
    pub reasoning: String,
    /// The criteria used for comparison
    pub criteria: String,
    /// Comparison latency in milliseconds
    pub latency_ms: u64,
}

impl ComparisonResult {
    /// Create a new comparison result.
    ///
    /// Preference is determined automatically based on score difference:
    /// - Tie if scores differ by less than 0.1
    /// - A if score_a > score_b
    /// - B if score_b > score_a
    #[must_use]
    pub fn new(
        score_a: f64,
        score_b: f64,
        reasoning: impl Into<String>,
        criteria: impl Into<String>,
        latency_ms: u64,
    ) -> Self {
        let score_a = score_a.clamp(0.0, 1.0);
        let score_b = score_b.clamp(0.0, 1.0);

        let preference = if (score_a - score_b).abs() < 0.1 {
            Preference::Tie
        } else if score_a > score_b {
            Preference::A
        } else {
            Preference::B
        };

        Self {
            preference,
            score_a,
            score_b,
            reasoning: reasoning.into(),
            criteria: criteria.into(),
            latency_ms,
        }
    }

    /// Create a result with explicit preference.
    #[must_use]
    pub fn with_preference(
        preference: Preference,
        reasoning: impl Into<String>,
        criteria: impl Into<String>,
        latency_ms: u64,
    ) -> Self {
        let (score_a, score_b) = match preference {
            Preference::A => (0.9, 0.4),
            Preference::B => (0.4, 0.9),
            Preference::Tie => (0.7, 0.7),
        };

        Self {
            preference,
            score_a,
            score_b,
            reasoning: reasoning.into(),
            criteria: criteria.into(),
            latency_ms,
        }
    }

    /// Create a result preferring A.
    #[must_use]
    pub fn prefer_a(criteria: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self::with_preference(Preference::A, reasoning, criteria, 0)
    }

    /// Create a result preferring B.
    #[must_use]
    pub fn prefer_b(criteria: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self::with_preference(Preference::B, reasoning, criteria, 0)
    }

    /// Create a tie result.
    #[must_use]
    pub fn tie(criteria: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self::with_preference(Preference::Tie, reasoning, criteria, 0)
    }

    /// Get the winning response label, or None for a tie.
    #[must_use]
    pub fn winner(&self) -> Option<&'static str> {
        match self.preference {
            Preference::A => Some("A"),
            Preference::B => Some("B"),
            Preference::Tie => None,
        }
    }

    /// Get the score difference (A - B).
    #[must_use]
    pub fn score_difference(&self) -> f64 {
        self.score_a - self.score_b
    }
}

/// Aggregated judgment report for a test suite.
///
/// Collects results from multiple evaluations and provides
/// aggregate statistics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct JudgmentReport {
    /// Total number of evaluations
    pub total: usize,
    /// Number of passing evaluations
    pub passed: usize,
    /// Number of failing evaluations
    pub failed: usize,
    /// Average score across all evaluations
    pub average_score: f64,
    /// Average latency in milliseconds
    pub average_latency_ms: u64,
    /// Individual judgment results
    pub results: Vec<JudgmentResult>,
}

impl JudgmentReport {
    /// Create a new empty report.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a judgment result to the report.
    pub fn add(&mut self, result: JudgmentResult) {
        self.total += 1;
        if result.passed {
            self.passed += 1;
        } else {
            self.failed += 1;
        }

        // Update running averages
        let n = self.total as f64;
        self.average_score = ((n - 1.0) * self.average_score + result.score) / n;

        let prev_total = (self.total - 1) as u64;
        self.average_latency_ms = if self.total == 1 {
            result.latency_ms
        } else {
            (prev_total * self.average_latency_ms + result.latency_ms) / self.total as u64
        };

        self.results.push(result);
    }

    /// Get the pass rate as a percentage (0.0 - 100.0).
    #[must_use]
    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.passed as f64 / self.total as f64) * 100.0
        }
    }

    /// Check if all evaluations passed.
    #[must_use]
    pub fn all_passed(&self) -> bool {
        self.failed == 0 && self.total > 0
    }

    /// Check if any evaluations failed.
    #[must_use]
    pub fn has_failures(&self) -> bool {
        self.failed > 0
    }

    /// Get results filtered by criteria name.
    #[must_use]
    pub fn by_criteria(&self, criteria: &str) -> Vec<&JudgmentResult> {
        self.results
            .iter()
            .filter(|r| r.criteria == criteria)
            .collect()
    }

    /// Get only failing results.
    #[must_use]
    pub fn failures(&self) -> Vec<&JudgmentResult> {
        self.results.iter().filter(|r| !r.passed).collect()
    }

    /// Clear all results.
    pub fn clear(&mut self) {
        self.total = 0;
        self.passed = 0;
        self.failed = 0;
        self.average_score = 0.0;
        self.average_latency_ms = 0;
        self.results.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn judgment_result_pass_fail() {
        let pass = JudgmentResult::pass("test", "good");
        assert!(pass.passed);
        assert!((pass.score - 1.0).abs() < f64::EPSILON);

        let fail = JudgmentResult::fail("test", "bad");
        assert!(!fail.passed);
        assert!(fail.score.abs() < f64::EPSILON);
    }

    #[test]
    fn judgment_result_threshold() {
        let result = JudgmentResult::new(0.6, "ok", "test", 0.5, 100);
        assert!(result.passed); // 0.6 >= 0.5
        assert!(!result.passed_with_threshold(0.7)); // 0.6 < 0.7
        assert!(result.passed_with_threshold(0.6)); // 0.6 >= 0.6
    }

    #[test]
    fn judgment_result_score_clamped() {
        let high = JudgmentResult::new(1.5, "high", "test", 0.7, 0);
        assert!((high.score - 1.0).abs() < f64::EPSILON);

        let low = JudgmentResult::new(-0.5, "low", "test", 0.7, 0);
        assert!(low.score.abs() < f64::EPSILON);
    }

    #[test]
    fn preference_helpers() {
        assert!(Preference::A.is_a());
        assert!(!Preference::A.is_b());
        assert!(Preference::Tie.is_tie());
        assert_eq!(Preference::A.as_str(), "A");
    }

    #[test]
    fn comparison_result_auto_preference() {
        // A wins clearly
        let a_wins = ComparisonResult::new(0.9, 0.3, "reason", "test", 0);
        assert_eq!(a_wins.preference, Preference::A);

        // B wins clearly
        let b_wins = ComparisonResult::new(0.3, 0.9, "reason", "test", 0);
        assert_eq!(b_wins.preference, Preference::B);

        // Tie (within 0.1)
        let tie = ComparisonResult::new(0.7, 0.75, "reason", "test", 0);
        assert_eq!(tie.preference, Preference::Tie);
    }

    #[test]
    fn comparison_result_winner() {
        let a_wins = ComparisonResult::prefer_a("test", "A is better");
        assert_eq!(a_wins.winner(), Some("A"));

        let tie = ComparisonResult::tie("test", "equal");
        assert_eq!(tie.winner(), None);
    }

    #[test]
    fn judgment_report_aggregation() {
        let mut report = JudgmentReport::new();

        report.add(JudgmentResult::new(0.8, "good", "helpfulness", 0.7, 100));
        report.add(JudgmentResult::new(0.5, "ok", "helpfulness", 0.7, 150));
        report.add(JudgmentResult::new(0.9, "great", "safety", 0.7, 50));

        assert_eq!(report.total, 3);
        assert_eq!(report.passed, 2);
        assert_eq!(report.failed, 1);
        assert!((report.average_score - 0.733).abs() < 0.01);
        assert_eq!(report.average_latency_ms, 100);
        assert!((report.pass_rate() - 66.67).abs() < 1.0);
    }

    #[test]
    fn judgment_report_filtering() {
        let mut report = JudgmentReport::new();
        report.add(JudgmentResult::pass("helpfulness", "good"));
        report.add(JudgmentResult::fail("safety", "bad"));
        report.add(JudgmentResult::pass("helpfulness", "great"));

        let helpfulness = report.by_criteria("helpfulness");
        assert_eq!(helpfulness.len(), 2);

        let failures = report.failures();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].criteria, "safety");
    }

    #[test]
    fn judgment_report_all_passed() {
        let mut report = JudgmentReport::new();
        assert!(!report.all_passed()); // Empty report

        report.add(JudgmentResult::pass("test", "good"));
        assert!(report.all_passed());

        report.add(JudgmentResult::fail("test", "bad"));
        assert!(!report.all_passed());
    }
}
