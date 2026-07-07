//! MoFA Testing Framework
//!
//! Provides mock implementations, failure injection, and deterministic time
//! control for testing MoFA agents.

pub mod adversarial;
pub mod assertions;
pub mod backend;
pub mod bus;
pub mod clock;
pub mod report;
pub mod runner;
pub mod tools;

pub use backend::MockLLMBackend;
pub use bus::MockAgentBus;
pub use clock::{Clock, MockClock, SystemClock};
pub use report::{
    JUnitFormatter, JsonFormatter, ReportFormatter, TestCaseResult, TestReport, TestReportBuilder,
    TestStatus, TextFormatter,
};
pub use runner::{
    AsyncTestCase, AsyncTestOutcome, AsyncTestRunner, AsyncTestRunnerConfig, TestProgressEvent,
    TestProgressState,
};
pub use tools::MockTool;
