//! Evaluation framework — dataset, scoring, running, and reporting.

pub mod dataset;
pub mod report;
pub mod runner;
pub mod scorer;

pub use dataset::{EvalCase, EvalDataset};
pub use runner::{CaseResult, EvalReport, EvalRunner};
pub use scorer::{CompositeScorer, ExactMatchScorer, KeywordScorer, LatencyScorer, Scorer};
