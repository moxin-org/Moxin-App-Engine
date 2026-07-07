mod evaluator;
mod keyword;
mod latency;
pub mod llm_judge;

pub use evaluator::{EvaluationResult, Evaluator};
pub use keyword::KeywordEvaluator;
pub use latency::LatencyEvaluator;
pub use llm_judge::LlmJudgeEvaluator;

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct DatasetEntry {
    pub input: String,
    pub output: String,
    pub context: Option<String>,
}

/// Run all evaluators on every entry in the dataset and collect results.
pub async fn run_dataset(
    evaluators: &[Box<dyn Evaluator>],
    entries: &[DatasetEntry],
) -> Vec<Vec<EvaluationResult>> {
    let mut all = Vec::new();
    for entry in entries {
        let mut row = Vec::new();
        for eval in evaluators {
            if let Ok(r) = eval
                .evaluate(&entry.input, &entry.output, entry.context.as_deref())
                .await
            {
                row.push(r);
            }
        }
        all.push(row);
    }
    all
}
