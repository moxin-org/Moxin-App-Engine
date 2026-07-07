//! MoFA Testing Framework
//!
//! Provides mock implementations, failure injection, and deterministic time
//! control for testing MoFA agents.
//!
//! Contract fixtures live under `tests/fixtures/`. Prefer them when you need
//! deterministic, portable coverage for scenario and contract tests.

pub mod adversarial;
pub mod assertions;
pub mod backend;
pub mod bus;
pub mod clock;
pub mod fixtures;
pub mod report;
pub mod scenario;
pub mod tools;

pub use backend::MockLLMBackend;
pub use bus::MockAgentBus;
pub use clock::{Clock, MockClock, SystemClock};
pub use fixtures::{fixture_path, fixtures_root, load_fixture};
pub use report::{
    JUnitFormatter, JsonFormatter, ReportFormatter, TestCaseResult, TestReport, TestReportBuilder,
    TestStatus, TextFormatter,
};
pub use scenario::{
    BusExpectation, BusScenarioSpec, ClockScenarioSpec, LlmFailureSpec, LlmResponseRule,
    LlmResponseSequence, LlmScenarioSpec, PromptCountExpectation, ScenarioContext, ScenarioRunner,
    ScenarioSpec, ToolCallExpectation, ToolFailureSpec, ToolInputFailureSpec, ToolResultSpec,
    ToolScenarioSpec,
};
pub use tools::MockTool;
