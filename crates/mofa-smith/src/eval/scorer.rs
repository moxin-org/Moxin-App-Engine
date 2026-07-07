//! Pluggable scoring for evaluation cases.
//!
//! A [`Scorer`] receives the case, the actual output produced by the swarm,
//! and the [`SchedulerSummary`] and returns a score in `[0.0, 1.0]`.

use mofa_foundation::swarm::SchedulerSummary;

use crate::eval::dataset::EvalCase;

/// Pluggable scorer: returns a score in `[0.0, 1.0]` for one eval case.
pub trait Scorer: Send + Sync {
    /// Short label shown in the report (e.g. `"keyword"`, `"exact"`, `"latency"`).
    fn name(&self) -> &str;

    /// Score the result.
    ///
    /// - `case`   — the original test case (includes `expected_output`)
    /// - `output` — actual output returned by the swarm executor
    /// - `summary` — full scheduler summary (wall time, success counts, etc.)
    fn score(&self, case: &EvalCase, output: &str, summary: &SchedulerSummary) -> f64;
}

/// Passes (1.0) if the actual output exactly equals `expected_output`
/// (case-insensitive, trimmed). Scores 0.0 if no expected output is set.
#[derive(Debug, Default)]
pub struct ExactMatchScorer;

impl Scorer for ExactMatchScorer {
    fn name(&self) -> &str {
        "exact"
    }

    fn score(&self, case: &EvalCase, output: &str, _summary: &SchedulerSummary) -> f64 {
        match &case.expected_output {
            Some(expected) => {
                if output.trim().to_lowercase() == expected.trim().to_lowercase() {
                    1.0
                } else {
                    0.0
                }
            }
            None => 0.0,
        }
    }
}

/// Scores by the fraction of expected keywords found in the actual output.
///
/// `expected_output` is split on whitespace and commas. Each token is
/// searched (case-insensitive substring match) in `output`. The score is
/// `found / total`.
#[derive(Debug)]
pub struct KeywordScorer;

impl Scorer for KeywordScorer {
    fn name(&self) -> &str {
        "keyword"
    }

    fn score(&self, case: &EvalCase, output: &str, _summary: &SchedulerSummary) -> f64 {
        let expected = match &case.expected_output {
            Some(e) => e,
            None => return 0.0,
        };

        let keywords: Vec<&str> = expected
            .split([',', ' ', ';'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();

        if keywords.is_empty() {
            return 0.0;
        }

        let output_lower = output.to_lowercase();
        let found = keywords
            .iter()
            .filter(|kw| output_lower.contains(&kw.to_lowercase()))
            .count();

        found as f64 / keywords.len() as f64
    }
}

/// Scores based on wall-time latency relative to a target.
///
/// - At or under `target_secs` → 1.0
/// - At 2x `target_secs` → 0.5
/// - Scales linearly to 0.0 at infinity (capped at 0.0)
#[derive(Debug)]
pub struct LatencyScorer {
    /// Target wall time in seconds. Tasks completing within this score 1.0.
    pub target_secs: f64,
}

impl LatencyScorer {
    pub fn new(target_secs: f64) -> Self {
        Self { target_secs }
    }
}

impl Scorer for LatencyScorer {
    fn name(&self) -> &str {
        "latency"
    }

    fn score(&self, _case: &EvalCase, _output: &str, summary: &SchedulerSummary) -> f64 {
        let actual = summary.total_wall_time.as_secs_f64();
        if actual <= self.target_secs {
            return 1.0;
        }
        let ratio = self.target_secs / actual;
        ratio.max(0.0_f64)
    }
}

/// Combines multiple scorers with weights, returning a weighted average.
pub struct CompositeScorer {
    scorers: Vec<(Box<dyn Scorer>, f64)>,
}

impl CompositeScorer {
    pub fn new() -> Self {
        Self {
            scorers: Vec::new(),
        }
    }

    pub fn add(mut self, scorer: Box<dyn Scorer>, weight: f64) -> Self {
        self.scorers.push((scorer, weight));
        self
    }
}

impl Default for CompositeScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl Scorer for CompositeScorer {
    fn name(&self) -> &str {
        "composite"
    }

    fn score(&self, case: &EvalCase, output: &str, summary: &SchedulerSummary) -> f64 {
        if self.scorers.is_empty() {
            return 0.0;
        }
        let total_weight: f64 = self.scorers.iter().map(|(_, w)| w).sum();
        if total_weight == 0.0 {
            return 0.0;
        }
        let weighted_sum: f64 = self
            .scorers
            .iter()
            .map(|(s, w)| s.score(case, output, summary) * w)
            .sum();
        weighted_sum / total_weight
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofa_foundation::swarm::{CoordinationPattern, SchedulerSummary};
    use std::time::Duration;

    fn make_summary(wall_secs: f64) -> SchedulerSummary {
        SchedulerSummary {
            pattern: CoordinationPattern::Sequential,
            total_tasks: 1,
            succeeded: 1,
            failed: 0,
            skipped: 0,
            total_wall_time: Duration::from_secs_f64(wall_secs),
            results: vec![],
        }
    }

    #[test]
    fn exact_match_passes_on_equal_trimmed_output() {
        let case = EvalCase::new("c1", "test").with_expected("hello");
        let summary = make_summary(0.1);
        let scorer = ExactMatchScorer;
        assert_eq!(scorer.score(&case, "  HELLO  ", &summary), 1.0);
    }

    #[test]
    fn exact_match_fails_on_mismatch() {
        let case = EvalCase::new("c1", "test").with_expected("hello");
        let summary = make_summary(0.1);
        let scorer = ExactMatchScorer;
        assert_eq!(scorer.score(&case, "world", &summary), 0.0);
    }

    #[test]
    fn exact_match_scores_zero_when_no_expected() {
        let case = EvalCase::new("c1", "test");
        let summary = make_summary(0.1);
        assert_eq!(ExactMatchScorer.score(&case, "anything", &summary), 0.0);
    }

    #[test]
    fn keyword_scorer_partial_match() {
        let case = EvalCase::new("c1", "test").with_expected("revenue profit");
        let summary = make_summary(0.1);
        let score = KeywordScorer.score(&case, "revenue was up this quarter", &summary);
        assert!((score - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn keyword_scorer_full_match() {
        let case = EvalCase::new("c1", "test").with_expected("revenue profit");
        let summary = make_summary(0.1);
        let score = KeywordScorer.score(&case, "revenue and profit both rose", &summary);
        assert_eq!(score, 1.0);
    }

    #[test]
    fn latency_scorer_at_target_is_one() {
        let case = EvalCase::new("c1", "test");
        let summary = make_summary(1.0);
        let scorer = LatencyScorer::new(2.0);
        assert_eq!(scorer.score(&case, "", &summary), 1.0);
    }

    #[test]
    fn latency_scorer_over_target_scales_down() {
        let case = EvalCase::new("c1", "test");
        let summary = make_summary(4.0);
        let scorer = LatencyScorer::new(2.0);
        let score = scorer.score(&case, "", &summary);
        assert!((score - 0.5).abs() < 1e-9);
    }
}
