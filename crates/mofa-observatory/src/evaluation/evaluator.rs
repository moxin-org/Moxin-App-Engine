use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result from a single evaluator run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    /// Name of the evaluator that produced this result.
    pub evaluator: String,
    /// Overall score in [0.0, 1.0].
    pub score: f64,
    /// Whether the score meets the passing threshold (typically >= 0.7).
    pub passed: bool,
    /// Human-readable explanation of the score.
    pub reason: String,
    /// Per-criterion scores for rubric-based evaluators (e.g. LLM judge).
    pub per_criterion: HashMap<String, f64>,
}

/// Trait for pluggable evaluators.
#[async_trait]
pub trait Evaluator: Send + Sync {
    /// Unique name identifying this evaluator (used in reports).
    fn name(&self) -> &str;

    /// Score an `output` given an `input` and optional `context`.
    async fn evaluate(
        &self,
        input: &str,
        output: &str,
        context: Option<&str>,
    ) -> Result<EvaluationResult>;
}
