//! MoFA Testing Framework
//!
//! Provides mock implementations, failure injection, and deterministic time
//! control for testing MoFA agents.

pub mod adversarial;
pub mod agent_runner;
pub mod artifact;
pub mod assertions;
pub mod backend;
pub mod bus;
pub mod clock;
pub mod dsl;
pub mod report;
pub mod tools;

pub use backend::MockLLMBackend;
pub use bus::MockAgentBus;
pub use clock::{Clock, MockClock, SystemClock};
pub use dsl::{
    assertion_error_from_outcomes, collect_assertion_outcomes, configure_runner_from_test_case,
    execute_test_case, run_test_case, AgentDsl, AssertDsl, AssertionOutcome, BootstrapFileDsl,
    DslError, LlmDsl, LlmStepDsl, LlmStepKind, TestCaseDsl, ToolDsl,
};
pub use agent_runner::{
    AgentRunMetadata, AgentRunResult, AgentRunnerError, AgentTestRunner, MockAgentLLMProvider,
    ToolCallRecord, WorkspaceFileSnapshot, WorkspaceSnapshot,
};
pub use artifact::{
    AgentArtifact, AgentRunArtifact, AgentRunArtifactDiff, ArtifactDifference,
    LlmMessageArtifact, LlmRequestArtifact, LlmResponseArtifact, LlmToolCallArtifact,
    SessionArtifact, SessionMessageArtifact, TokenUsageArtifact, ToolCallArtifact,
    WorkspaceFileArtifact, WorkspaceSnapshotArtifact,
};
pub use report::{
    JUnitFormatter, JsonFormatter, ReportFormatter, TestCaseResult, TestReport, TestReportBuilder,
    TestStatus, TextFormatter,
};
pub use tools::MockTool;
