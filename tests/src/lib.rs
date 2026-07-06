//! MoFA Testing Framework
//!
//! Provides mock implementations, failure injection, deterministic time
//! control, and LLM-as-Judge evaluation for testing MoFA agents.
//!
//! # Modules
//!
//! - [`judge`] - LLM-as-Judge evaluation framework for semantic quality assessment
//! - [`adversarial`] - Adversarial testing and security evaluation
//! - [`backend`] - Mock LLM backend for deterministic responses
//! - [`tools`] - Mock tools for testing tool selection
//! - [`bus`] - Mock agent bus for inter-agent communication testing
//! - [`clock`] - Deterministic time control
//! - [`report`] - Test report generation

pub mod adversarial;
pub mod assertions;
pub mod backend;
pub mod bus;
pub mod clock;
pub mod judge;
pub mod report;
pub mod tools;

pub use backend::MockLLMBackend;
pub use bus::MockAgentBus;
pub use clock::{Clock, MockClock, SystemClock};
pub use judge::{
    ComparisonResult, EvaluationCriteria, JudgeConfig, JudgmentReport, JudgmentResult, LLMJudge,
    MockLLMJudge, Preference, ScoringRubric,
};
pub use report::{
    JUnitFormatter, JsonFormatter, ReportFormatter, TestCaseResult, TestReport, TestReportBuilder,
    TestStatus, TextFormatter,
};
pub use tools::MockTool;
