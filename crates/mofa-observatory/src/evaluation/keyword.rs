use super::evaluator::{EvaluationResult, Evaluator};
use anyhow::Result;
use async_trait::async_trait;

/// Evaluates output by checking for required and forbidden keywords.
///
/// Score is computed as:
/// - `required_ratio = required_found / required_total`
/// - `forbidden_penalty = forbidden_found * 0.5`
/// - `score = clamp(required_ratio - forbidden_penalty, 0.0, 1.0)`
pub struct KeywordEvaluator {
    pub required_keywords: Vec<String>,
    pub forbidden_keywords: Vec<String>,
}

#[async_trait]
impl Evaluator for KeywordEvaluator {
    fn name(&self) -> &str {
        "keyword"
    }

    async fn evaluate(
        &self,
        _input: &str,
        output: &str,
        _context: Option<&str>,
    ) -> Result<EvaluationResult> {
        let out_lower = output.to_lowercase();
        let required_found = self
            .required_keywords
            .iter()
            .filter(|k| out_lower.contains(k.to_lowercase().as_str()))
            .count();
        let forbidden_found = self
            .forbidden_keywords
            .iter()
            .filter(|k| out_lower.contains(k.to_lowercase().as_str()))
            .count();

        let required_ratio = if self.required_keywords.is_empty() {
            1.0
        } else {
            required_found as f64 / self.required_keywords.len() as f64
        };
        let forbidden_penalty = forbidden_found as f64 * 0.5;
        let score = (required_ratio - forbidden_penalty).clamp(0.0, 1.0);

        Ok(EvaluationResult {
            evaluator: self.name().to_string(),
            score,
            passed: score >= 0.7,
            reason: format!(
                "{}/{} required keywords found; {} forbidden keywords triggered",
                required_found,
                self.required_keywords.len(),
                forbidden_found
            ),
            per_criterion: Default::default(),
        })
    }
}
