//! Mock LLM judge for deterministic testing.
//!
//! Provides [`MockLLMJudge`] which returns configurable, deterministic
//! results for testing evaluation logic without real LLM calls.

use crate::judge::{
    ComparisonResult, EvaluationCriteria, JudgeConfig, JudgmentResult, LLMJudge, Preference,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A mock LLM judge that returns configurable, deterministic results.
///
/// Use this in tests to avoid real LLM calls while validating
/// evaluation logic and assertions.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_testing::judge::{MockLLMJudge, EvaluationCriteria, LLMJudge};
///
/// #[tokio::test]
/// async fn test_agent_is_helpful() {
///     let judge = MockLLMJudge::new()
///         .with_score("helpfulness", 0.85)
///         .with_reasoning("helpfulness", "Clear and helpful response");
///
///     let result = judge.evaluate(
///         "question",
///         "answer",
///         &EvaluationCriteria::Helpfulness,
///     ).await;
///
///     assert!(result.passed);
///     assert!(result.score >= 0.7);
/// }
/// ```
#[derive(Clone)]
pub struct MockLLMJudge {
    config: JudgeConfig,
    scores: Arc<RwLock<HashMap<String, f64>>>,
    reasoning: Arc<RwLock<HashMap<String, String>>>,
    preferences: Arc<RwLock<HashMap<String, Preference>>>,
    evaluation_history: Arc<RwLock<Vec<EvaluationRecord>>>,
    comparison_history: Arc<RwLock<Vec<ComparisonRecord>>>,
    default_score: f64,
    simulated_latency_ms: u64,
}

/// Record of an evaluation call for inspection.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationRecord {
    /// The input that was evaluated
    pub input: String,
    /// The output that was evaluated
    pub output: String,
    /// The criteria used
    pub criteria: String,
    /// The result of the evaluation
    pub result: JudgmentResult,
}

/// Record of a comparison call for inspection.
#[derive(Debug, Clone, PartialEq)]
pub struct ComparisonRecord {
    /// The input for the comparison
    pub input: String,
    /// The first output (A)
    pub output_a: String,
    /// The second output (B)
    pub output_b: String,
    /// The criteria used
    pub criteria: String,
    /// The result of the comparison
    pub result: ComparisonResult,
}

impl Default for MockLLMJudge {
    fn default() -> Self {
        Self::new()
    }
}

impl MockLLMJudge {
    /// Create a new mock judge with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: JudgeConfig::default(),
            scores: Arc::new(RwLock::new(HashMap::new())),
            reasoning: Arc::new(RwLock::new(HashMap::new())),
            preferences: Arc::new(RwLock::new(HashMap::new())),
            evaluation_history: Arc::new(RwLock::new(Vec::new())),
            comparison_history: Arc::new(RwLock::new(Vec::new())),
            default_score: 0.8,
            simulated_latency_ms: 0,
        }
    }

    /// Create a mock judge with custom configuration.
    #[must_use]
    pub fn with_config(config: JudgeConfig) -> Self {
        Self {
            config,
            ..Self::new()
        }
    }

    /// Set the score to return for a specific criteria.
    #[must_use]
    pub fn with_score(self, criteria: &str, score: f64) -> Self {
        // Use try_write to avoid blocking in builder pattern
        if let Ok(mut scores) = self.scores.try_write() {
            scores.insert(criteria.to_string(), score.clamp(0.0, 1.0));
        }
        self
    }

    /// Set the score to return for a specific criteria (async version).
    pub async fn set_score(&self, criteria: &str, score: f64) {
        let mut scores = self.scores.write().await;
        scores.insert(criteria.to_string(), score.clamp(0.0, 1.0));
    }

    /// Set the reasoning to return for a specific criteria.
    #[must_use]
    pub fn with_reasoning(self, criteria: &str, reasoning: &str) -> Self {
        if let Ok(mut reasons) = self.reasoning.try_write() {
            reasons.insert(criteria.to_string(), reasoning.to_string());
        }
        self
    }

    /// Set the reasoning to return for a specific criteria (async version).
    pub async fn set_reasoning(&self, criteria: &str, reasoning: &str) {
        let mut reasons = self.reasoning.write().await;
        reasons.insert(criteria.to_string(), reasoning.to_string());
    }

    /// Set the comparison preference for a specific criteria.
    #[must_use]
    pub fn with_preference(self, criteria: &str, preference: Preference) -> Self {
        if let Ok(mut prefs) = self.preferences.try_write() {
            prefs.insert(criteria.to_string(), preference);
        }
        self
    }

    /// Set the comparison preference for a specific criteria (async version).
    pub async fn set_preference(&self, criteria: &str, preference: Preference) {
        let mut prefs = self.preferences.write().await;
        prefs.insert(criteria.to_string(), preference);
    }

    /// Set the default score when no specific score is configured.
    #[must_use]
    pub fn with_default_score(mut self, score: f64) -> Self {
        self.default_score = score.clamp(0.0, 1.0);
        self
    }

    /// Set simulated latency for timing tests.
    #[must_use]
    pub fn with_simulated_latency(mut self, latency_ms: u64) -> Self {
        self.simulated_latency_ms = latency_ms;
        self
    }

    /// Get the evaluation history.
    pub async fn evaluation_history(&self) -> Vec<EvaluationRecord> {
        self.evaluation_history.read().await.clone()
    }

    /// Get the comparison history.
    pub async fn comparison_history(&self) -> Vec<ComparisonRecord> {
        self.comparison_history.read().await.clone()
    }

    /// Get the number of evaluations performed.
    pub async fn evaluation_count(&self) -> usize {
        self.evaluation_history.read().await.len()
    }

    /// Get the number of comparisons performed.
    pub async fn comparison_count(&self) -> usize {
        self.comparison_history.read().await.len()
    }

    /// Clear all history.
    pub async fn clear_history(&self) {
        self.evaluation_history.write().await.clear();
        self.comparison_history.write().await.clear();
    }

    /// Configure to always pass evaluations (score = 1.0).
    #[must_use]
    pub fn always_pass(self) -> Self {
        self.with_default_score(1.0)
    }

    /// Configure to always fail evaluations (score = 0.0).
    #[must_use]
    pub fn always_fail(self) -> Self {
        self.with_default_score(0.0)
    }

    async fn get_score(&self, criteria: &str) -> f64 {
        self.scores
            .read()
            .await
            .get(criteria)
            .copied()
            .unwrap_or(self.default_score)
    }

    async fn get_reasoning(&self, criteria: &str) -> String {
        self.reasoning
            .read()
            .await
            .get(criteria)
            .cloned()
            .unwrap_or_else(|| format!("Mock evaluation for {criteria}"))
    }

    async fn get_preference(&self, criteria: &str) -> Preference {
        self.preferences
            .read()
            .await
            .get(criteria)
            .copied()
            .unwrap_or(Preference::Tie)
    }
}

impl LLMJudge for MockLLMJudge {
    async fn evaluate(
        &self,
        input: &str,
        output: &str,
        criteria: &EvaluationCriteria,
    ) -> JudgmentResult {
        let criteria_name = criteria.name();
        let score = self.get_score(criteria_name).await;
        let reasoning = self.get_reasoning(criteria_name).await;

        let result = JudgmentResult::new(
            score,
            reasoning,
            criteria_name,
            self.config.threshold,
            self.simulated_latency_ms,
        );

        // Record the evaluation
        let record = EvaluationRecord {
            input: input.to_string(),
            output: output.to_string(),
            criteria: criteria_name.to_string(),
            result: result.clone(),
        };
        self.evaluation_history.write().await.push(record);

        result
    }

    async fn compare(
        &self,
        input: &str,
        output_a: &str,
        output_b: &str,
        criteria: &EvaluationCriteria,
    ) -> ComparisonResult {
        let criteria_name = criteria.name();
        let preference = self.get_preference(criteria_name).await;
        let reasoning = self.get_reasoning(criteria_name).await;

        let result = ComparisonResult::with_preference(
            preference,
            reasoning,
            criteria_name,
            self.simulated_latency_ms,
        );

        // Record the comparison
        let record = ComparisonRecord {
            input: input.to_string(),
            output_a: output_a.to_string(),
            output_b: output_b.to_string(),
            criteria: criteria_name.to_string(),
            result: result.clone(),
        };
        self.comparison_history.write().await.push(record);

        result
    }

    fn default_threshold(&self) -> f64 {
        self.config.threshold
    }

    fn name(&self) -> &str {
        "MockLLMJudge"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_judge_returns_configured_score() {
        let judge = MockLLMJudge::new().with_score("helpfulness", 0.85);

        let result = judge
            .evaluate("test input", "test output", &EvaluationCriteria::Helpfulness)
            .await;

        assert!((result.score - 0.85).abs() < 0.01);
        assert!(result.passed);
    }

    #[tokio::test]
    async fn mock_judge_returns_configured_reasoning() {
        let judge = MockLLMJudge::new().with_reasoning("safety", "Response is safe");

        let result = judge
            .evaluate("test", "test", &EvaluationCriteria::Safety)
            .await;

        assert_eq!(result.reasoning, "Response is safe");
    }

    #[tokio::test]
    async fn mock_judge_records_history() {
        let judge = MockLLMJudge::new();

        judge
            .evaluate("input1", "output1", &EvaluationCriteria::Helpfulness)
            .await;
        judge
            .evaluate("input2", "output2", &EvaluationCriteria::Safety)
            .await;

        let history = judge.evaluation_history().await;
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].input, "input1");
        assert_eq!(history[1].criteria, "safety");
    }

    #[tokio::test]
    async fn mock_judge_comparison() {
        let judge = MockLLMJudge::new().with_preference("helpfulness", Preference::A);

        let result = judge
            .compare(
                "question",
                "answer A",
                "answer B",
                &EvaluationCriteria::Helpfulness,
            )
            .await;

        assert_eq!(result.preference, Preference::A);
        assert!(result.score_a > result.score_b);
    }

    #[tokio::test]
    async fn mock_judge_always_pass() {
        let judge = MockLLMJudge::new().always_pass();

        let result = judge
            .evaluate("test", "test", &EvaluationCriteria::Safety)
            .await;

        assert!(result.passed);
        assert!((result.score - 1.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn mock_judge_always_fail() {
        let judge = MockLLMJudge::new().always_fail();

        let result = judge
            .evaluate("test", "test", &EvaluationCriteria::Safety)
            .await;

        assert!(!result.passed);
        assert!(result.score.abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn mock_judge_async_configuration() {
        let judge = MockLLMJudge::new();

        judge.set_score("coherence", 0.95).await;
        judge.set_reasoning("coherence", "Very coherent").await;

        let result = judge
            .evaluate("test", "test", &EvaluationCriteria::Coherence)
            .await;

        assert!((result.score - 0.95).abs() < 0.01);
        assert_eq!(result.reasoning, "Very coherent");
    }

    #[tokio::test]
    async fn mock_judge_clear_history() {
        let judge = MockLLMJudge::new();

        judge
            .evaluate("a", "b", &EvaluationCriteria::Helpfulness)
            .await;
        judge
            .compare("c", "d", "e", &EvaluationCriteria::Safety)
            .await;

        assert_eq!(judge.evaluation_count().await, 1);
        assert_eq!(judge.comparison_count().await, 1);

        judge.clear_history().await;

        assert_eq!(judge.evaluation_count().await, 0);
        assert_eq!(judge.comparison_count().await, 0);
    }

    #[tokio::test]
    async fn mock_judge_with_config() {
        let config = JudgeConfig::new().with_threshold(0.9);
        let judge = MockLLMJudge::with_config(config).with_default_score(0.85);

        let result = judge
            .evaluate("test", "test", &EvaluationCriteria::Helpfulness)
            .await;

        // 0.85 < 0.9, so should fail with strict threshold
        assert!(!result.passed);
        assert!((result.threshold - 0.9).abs() < f64::EPSILON);
    }
}
