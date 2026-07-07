//! MoFA Testing Framework
//!
//! Provides mock implementations, failure injection, and deterministic time
//! control for testing MoFA agents.

pub mod adversarial;
pub mod agent_runner;
pub mod assertions;
pub mod backend;
pub mod bus;
pub mod clock;
pub mod report;
pub mod tools;

pub use backend::MockLLMBackend;
pub use bus::MockAgentBus;
pub use clock::{Clock, MockClock, SystemClock};
pub use agent_runner::{
    AgentRunMetadata, AgentRunResult, AgentRunnerError, AgentTestRunner, MockAgentLLMProvider,
    ToolCallRecord, WorkspaceFileSnapshot, WorkspaceSnapshot,
};
pub use report::{
    JUnitFormatter, JsonFormatter, ReportFormatter, TestCaseResult, TestReport, TestReportBuilder,
    TestStatus, TextFormatter,
};
pub use tools::MockTool;
