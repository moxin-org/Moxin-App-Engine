//! MoFA Testing Framework
//!
//! Provides mock implementations, failure injection, and deterministic time
//! control for testing MoFA agents.

pub mod adversarial;
pub mod assertions;
pub mod backend;
pub mod bus;
pub mod clock;
pub mod dsl;
pub mod golden;
pub mod parameterized;
pub mod report;
pub mod tools;

pub use backend::MockLLMBackend;
pub use bus::MockAgentBus;
pub use clock::{Clock, MockClock, SystemClock};
pub use dsl::{
    AgentTest, AgentTestScenario, MockScenarioAgent, ScenarioAgent, ScenarioBuildError,
    ScenarioLoadError, ScenarioTurn, ScenarioTurnOutput, ToolCallRecord, ToolInvocationRule,
    TurnExpectation,
};
pub use parameterized::{
    ParameterExpansionError, ParameterMatrix, ParameterSet, ParameterizedScenario,
    ParameterizedScenarioFile,
};
pub use golden::{
    GoldenCompareMode, GoldenCompareResult, GoldenDiff, GoldenError, GoldenSnapshot, GoldenStore,
    GoldenTestConfig, GoldenTurnSnapshot, NormalizerChain, RegexNormalizer, WhitespaceNormalizer,
    compare_golden, run_golden_test,
};
pub use report::{
    JUnitFormatter, JsonFormatter, ReportFormatter, TestCaseResult, TestReport, TestReportBuilder,
    TestStatus, TextFormatter,
};
pub use tools::MockTool;
