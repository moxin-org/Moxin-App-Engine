//! EvalRunner — executes a dataset through the swarm scheduler and scores results.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use mofa_foundation::swarm::{
    CoordinationPattern, ParallelScheduler, SchedulerSummary, SequentialScheduler, SubtaskDAG,
    SubtaskExecutorFn, SwarmScheduler, SwarmSubtask,
};
use serde::{Deserialize, Serialize};

use crate::eval::dataset::{EvalCase, EvalDataset};
use crate::eval::scorer::Scorer;

/// Result of running a single [`EvalCase`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseResult {
    /// ID of the case that was run.
    pub case_id: String,
    /// Score returned by the scorer (`0.0..=1.0`).
    pub score: f64,
    /// True if `score >= pass_threshold`.
    pub passed: bool,
    /// Actual output returned by the executor, if any.
    pub actual_output: Option<String>,
    /// Wall time for this case's scheduler run.
    pub wall_time: Duration,
    /// Name of the scorer used.
    pub scorer_name: String,
}

/// Full report produced after running all cases in a dataset.
#[derive(Debug, Clone, Serialize)]
pub struct EvalReport {
    /// Name of the dataset that was evaluated.
    pub dataset_name: String,
    /// Total cases run.
    pub total_cases: usize,
    /// Cases that passed.
    pub passed: usize,
    /// Cases that failed.
    pub failed: usize,
    /// Overall score: mean of per-case scores.
    pub overall_score: f64,
    /// Per-case results in run order.
    pub results: Vec<CaseResult>,
    /// UTC timestamp when the run started.
    pub ran_at: DateTime<Utc>,
}

impl EvalReport {
    /// Pass rate as a percentage string (e.g. `"80.0%"`).
    pub fn pass_rate_pct(&self) -> String {
        if self.total_cases == 0 {
            return "n/a".into();
        }
        format!("{:.1}%", self.passed as f64 / self.total_cases as f64 * 100.0)
    }
}

/// Runs an [`EvalDataset`] through the swarm scheduler and produces an [`EvalReport`].
pub struct EvalRunner {
    dataset: EvalDataset,
    scorer: Box<dyn Scorer>,
    pattern: CoordinationPattern,
    timeout_secs: u64,
    pass_threshold: f64,
}

impl EvalRunner {
    /// Create a runner with a dataset and scorer.
    pub fn new(dataset: EvalDataset, scorer: Box<dyn Scorer>) -> Self {
        Self {
            dataset,
            scorer,
            pattern: CoordinationPattern::Sequential,
            timeout_secs: 120,
            pass_threshold: 0.5,
        }
    }

    /// Override the coordination pattern used for each case.
    pub fn with_pattern(mut self, pattern: CoordinationPattern) -> Self {
        self.pattern = pattern;
        self
    }

    /// Override the per-task timeout in seconds.
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Minimum score to count as a pass (default `0.5`).
    pub fn with_pass_threshold(mut self, threshold: f64) -> Self {
        self.pass_threshold = threshold;
        self
    }

    /// Run all cases using a default per-case executor that returns the case's
    /// input values as the output text.
    ///
    /// This simulates a pass-through agent so that keyword and exact scorers
    /// check against the actual input documents rather than synthetic strings.
    /// Swap in a real executor via [`EvalRunner::run_with_executor`] to test
    /// a live agent.
    pub async fn run(self) -> EvalReport {
        self.run_internal(None).await
    }

    /// Run all cases with a custom executor injected by the caller.
    pub async fn run_with_executor(self, executor: SubtaskExecutorFn) -> EvalReport {
        self.run_internal(Some(executor)).await
    }

    async fn run_internal(self, executor: Option<SubtaskExecutorFn>) -> EvalReport {
        let ran_at = Utc::now();
        let dataset_name = self.dataset.name.clone();
        let scorer_name = self.scorer.name().to_string();
        let pass_threshold = self.pass_threshold;

        let mut results = Vec::with_capacity(self.dataset.cases.len());

        for case in &self.dataset.cases {
            let case_result = run_case(
                case,
                self.pattern.clone(),
                self.timeout_secs,
                executor.clone(),
                self.scorer.as_ref(),
                pass_threshold,
                &scorer_name,
            )
            .await;
            results.push(case_result);
        }

        let total_cases = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        let failed = total_cases - passed;
        let overall_score = if total_cases == 0 {
            0.0
        } else {
            results.iter().map(|r| r.score).sum::<f64>() / total_cases as f64
        };

        EvalReport {
            dataset_name,
            total_cases,
            passed,
            failed,
            overall_score,
            results,
            ran_at,
        }
    }
}

async fn run_case(
    case: &EvalCase,
    pattern: CoordinationPattern,
    timeout_secs: u64,
    executor: Option<SubtaskExecutorFn>,
    scorer: &dyn Scorer,
    pass_threshold: f64,
    scorer_name: &str,
) -> CaseResult {
    // When no executor is injected, build a per-case pass-through that returns
    // the actual input document text so scorers evaluate real content.
    let actual_executor: SubtaskExecutorFn = executor.unwrap_or_else(|| {
        let inputs_text = case.inputs_as_text();
        Arc::new(move |_idx, _task| {
            let text = inputs_text.clone();
            Box::pin(async move { Ok(text) })
        })
    });

    let mut dag = SubtaskDAG::new(format!("eval-{}", case.id));
    dag.add_task(
        SwarmSubtask::new(case.id.clone(), case.description.clone()).with_complexity(0.5),
    );

    let config = mofa_foundation::swarm::SwarmSchedulerConfig {
        task_timeout: Duration::from_secs(timeout_secs),
        ..Default::default()
    };

    let start = Instant::now();
    let summary: SchedulerSummary = match pattern {
        CoordinationPattern::Parallel => {
            let scheduler = ParallelScheduler::with_config(config);
            match scheduler.execute(&mut dag, actual_executor).await {
                Ok(s) => s,
                Err(e) => return failed_case(case, scorer_name, e.to_string(), start.elapsed()),
            }
        }
        _ => {
            let scheduler = SequentialScheduler::with_config(config);
            match scheduler.execute(&mut dag, actual_executor).await {
                Ok(s) => s,
                Err(e) => return failed_case(case, scorer_name, e.to_string(), start.elapsed()),
            }
        }
    };

    let actual_output = summary.successful_outputs().first().map(|s| s.to_string());
    let output_str = actual_output.as_deref().unwrap_or("");
    let score = scorer.score(case, output_str, &summary);

    CaseResult {
        case_id: case.id.clone(),
        score,
        passed: score >= pass_threshold,
        actual_output,
        wall_time: summary.total_wall_time,
        scorer_name: scorer_name.to_string(),
    }
}

fn failed_case(
    case: &EvalCase,
    scorer_name: &str,
    error: String,
    elapsed: Duration,
) -> CaseResult {
    CaseResult {
        case_id: case.id.clone(),
        score: 0.0,
        passed: false,
        actual_output: Some(format!("error: {error}")),
        wall_time: elapsed,
        scorer_name: scorer_name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::dataset::EvalCase;
    use crate::eval::report::write_json_report;
    use crate::eval::scorer::KeywordScorer;

    #[tokio::test]
    async fn test_default_executor_returns_input_document_text() {
        // The default run() executor must return actual input content, not the
        // task description — otherwise keyword scoring is circular.
        let dataset = EvalDataset::new("test-ds").with_case(
            EvalCase::new("c1", "extract revenue figures")
                .with_input("document", "Total revenue reached 1.2 million dollars.")
                .with_expected("revenue"),
        );

        let report = EvalRunner::new(dataset, Box::new(KeywordScorer))
            .run()
            .await;

        assert_eq!(report.passed, 1);
        let output = report.results[0].actual_output.as_deref().unwrap_or("");
        assert!(output.contains("1.2 million"), "executor must return document text, got: {output}");
    }

    #[tokio::test]
    async fn test_default_executor_fails_when_keyword_not_in_document() {
        // Keyword not in the input document -> genuine failure.
        let dataset = EvalDataset::new("test-ds").with_case(
            EvalCase::new("c1", "detect churn signal")
                .with_input("document", "Great product, very happy with the purchase.")
                .with_expected("churn"),
        );

        let report = EvalRunner::new(dataset, Box::new(KeywordScorer))
            .run()
            .await;

        assert_eq!(report.failed, 1);
        assert_eq!(report.overall_score, 0.0);
    }

    #[tokio::test]
    async fn test_runner_all_pass_with_injected_executor() {
        // The injected executor returns a fixed string containing the keywords.
        // This verifies that run_with_executor correctly routes through to the
        // scorer and that pass/fail aggregation is correct.
        let dataset = EvalDataset::new("test-ds")
            .with_case(
                EvalCase::new("c1", "revenue extraction task").with_expected("revenue"),
            )
            .with_case(
                EvalCase::new("c2", "latency analysis task").with_expected("latency"),
            );

        // Executor returns the task description, which contains the keywords.
        let executor: SubtaskExecutorFn = Arc::new(|_idx, task: SwarmSubtask| {
            Box::pin(async move { Ok(task.description.clone()) })
        });

        let report = EvalRunner::new(dataset, Box::new(KeywordScorer))
            .run_with_executor(executor)
            .await;

        assert_eq!(report.total_cases, 2);
        assert_eq!(report.passed, 2);
        assert_eq!(report.failed, 0);
    }

    #[tokio::test]
    async fn test_runner_fail_when_keyword_missing() {
        let dataset = EvalDataset::new("test-ds").with_case(
            EvalCase::new("c1", "hello world").with_expected("missing-keyword"),
        );

        let executor: SubtaskExecutorFn = Arc::new(|_idx, task: SwarmSubtask| {
            Box::pin(async move { Ok(task.description.clone()) })
        });

        let report = EvalRunner::new(dataset, Box::new(KeywordScorer))
            .run_with_executor(executor)
            .await;

        assert_eq!(report.passed, 0);
        assert_eq!(report.failed, 1);
        assert_eq!(report.overall_score, 0.0);
    }

    #[tokio::test]
    async fn test_empty_dataset_produces_empty_report() {
        let dataset = EvalDataset::new("empty");
        let report = EvalRunner::new(dataset, Box::new(KeywordScorer))
            .run()
            .await;

        assert_eq!(report.total_cases, 0);
        assert_eq!(report.overall_score, 0.0);
    }

    #[tokio::test]
    async fn test_pass_threshold_controls_pass_fail() {
        let dataset = EvalDataset::new("test-ds").with_case(
            EvalCase::new("c1", "task")
                .with_input("doc", "revenue report Q3")
                .with_expected("revenue"),
        );

        let report = EvalRunner::new(dataset, Box::new(KeywordScorer))
            .with_pass_threshold(0.99)
            .run()
            .await;

        assert_eq!(report.passed, 1);
    }

    #[tokio::test]
    async fn test_parallel_pattern_runs_correctly() {
        let dataset = EvalDataset::new("test-ds").with_case(
            EvalCase::new("c1", "parallel task")
                .with_input("doc", "revenue and profit both up")
                .with_expected("revenue"),
        );

        let report = EvalRunner::new(dataset, Box::new(KeywordScorer))
            .with_pattern(CoordinationPattern::Parallel)
            .run()
            .await;

        assert_eq!(report.total_cases, 1);
        assert_eq!(report.passed, 1);
    }

    #[tokio::test]
    async fn test_json_report_roundtrip() {
        let dataset = EvalDataset::new("json-test").with_case(
            EvalCase::new("c1", "task")
                .with_input("doc", "latency spike detected in prod")
                .with_expected("latency"),
        );

        let report = EvalRunner::new(dataset, Box::new(KeywordScorer))
            .run()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.json");
        write_json_report(&report, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert_eq!(parsed["dataset_name"], "json-test");
        assert_eq!(parsed["total_cases"], 1);
        assert_eq!(parsed["passed"], 1);
        assert!(parsed["overall_score"].as_f64().unwrap() > 0.0);
        assert!(parsed["results"].is_array());
    }
}
