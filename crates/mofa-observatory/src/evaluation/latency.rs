use super::evaluator::{EvaluationResult, Evaluator};
use anyhow::Result;
use async_trait::async_trait;

/// Evaluates output based on whether response latency meets a threshold.
///
/// Score decays linearly from 1.0 at 0ms to 0.0 at 2× threshold.
pub struct LatencyEvaluator {
    /// Maximum acceptable latency in milliseconds.
    pub threshold_ms: u64,
    /// Actual measured latency in milliseconds.
    pub measured_ms: u64,
}

#[async_trait]
impl Evaluator for LatencyEvaluator {
    fn name(&self) -> &str {
        "latency"
    }

    async fn evaluate(
        &self,
        _input: &str,
        _output: &str,
        _context: Option<&str>,
    ) -> Result<EvaluationResult> {
        let ratio = self.measured_ms as f64 / self.threshold_ms as f64;
        let score = (1.0 - ratio / 2.0).clamp(0.0, 1.0);

        Ok(EvaluationResult {
            evaluator: self.name().to_string(),
            score,
            passed: self.measured_ms <= self.threshold_ms,
            reason: format!(
                "{}ms measured vs {}ms threshold (ratio {:.2})",
                self.measured_ms,
                self.threshold_ms,
                ratio
            ),
            per_criterion: Default::default(),
        })
    }
}
