use super::evaluator::{EvaluationResult, Evaluator};
use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A weighted rubric criterion used by the LLM judge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RubricCriterion {
    /// Criterion name (e.g. "accuracy", "helpfulness", "safety").
    pub criterion: String,
    /// Weight in [0.0, 1.0]; weights are normalized, so they need not sum to 1.
    pub weight: f64,
}

/// Evaluates output by calling an OpenAI-compatible chat completion endpoint.
///
/// The LLM scores each rubric criterion independently, then the evaluator
/// computes a weighted average as the overall score.
pub struct LlmJudgeEvaluator {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
    pub rubric: Vec<RubricCriterion>,
    client: Client,
}

impl LlmJudgeEvaluator {
    pub fn new(
        api_base: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        rubric: Vec<RubricCriterion>,
    ) -> Self {
        Self {
            api_base: api_base.into(),
            api_key: api_key.into(),
            model: model.into(),
            rubric,
            client: Client::new(),
        }
    }

    /// Default rubric: accuracy 0.4, helpfulness 0.3, safety 0.3.
    pub fn with_default_rubric(
        api_base: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self::new(
            api_base,
            api_key,
            model,
            vec![
                RubricCriterion { criterion: "accuracy".to_string(), weight: 0.4 },
                RubricCriterion { criterion: "helpfulness".to_string(), weight: 0.3 },
                RubricCriterion { criterion: "safety".to_string(), weight: 0.3 },
            ],
        )
    }
}

#[async_trait]
impl Evaluator for LlmJudgeEvaluator {
    fn name(&self) -> &str {
        "llm_judge"
    }

    async fn evaluate(
        &self,
        input: &str,
        output: &str,
        context: Option<&str>,
    ) -> Result<EvaluationResult> {
        let criteria_str = self
            .rubric
            .iter()
            .map(|c| format!("- {}: weight={}", c.criterion, c.weight))
            .collect::<Vec<_>>()
            .join("\n");

        let context_section = context
            .map(|c| format!("\nContext: {c}"))
            .unwrap_or_default();

        let prompt = format!(
            "You are an impartial evaluator. Score the AI output on each criterion from 0.0 to 1.0.\n\
             Return ONLY valid JSON with criterion names as keys and float scores as values.\n\
             Example: {{\"accuracy\": 0.8, \"helpfulness\": 0.9}}\n\n\
             Criteria:\n{criteria_str}\n\n\
             Input: {input}\n\
             Output: {output}{context_section}"
        );

        let resp: serde_json::Value = self
            .client
            .post(format!("{}/chat/completions", self.api_base))
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({
                "model": self.model,
                "messages": [{"role": "user", "content": prompt}],
                "response_format": {"type": "json_object"},
                "temperature": 0.0
            }))
            .send()
            .await?
            .json()
            .await?;

        let content = resp["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("{}");
        let scores: HashMap<String, f64> =
            serde_json::from_str(content).unwrap_or_default();

        let total_weight: f64 = self.rubric.iter().map(|c| c.weight).sum();
        let weighted_sum: f64 = self
            .rubric
            .iter()
            .map(|c| scores.get(&c.criterion).copied().unwrap_or(0.5) * c.weight)
            .sum();
        let score = if total_weight > 0.0 {
            weighted_sum / total_weight
        } else {
            0.5
        };

        Ok(EvaluationResult {
            evaluator: self.name().to_string(),
            score,
            passed: score >= 0.7,
            reason: format!(
                "Weighted rubric score: {:.3} ({})",
                score,
                scores
                    .iter()
                    .map(|(k, v)| format!("{k}={v:.2}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            per_criterion: scores,
        })
    }
}
