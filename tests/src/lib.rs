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
pub use report::{
    JUnitFormatter, JsonFormatter, ReportFormatter, TestCaseResult, TestReport, TestReportBuilder,
    TestStatus, TextFormatter,
};
pub use tools::MockTool;
