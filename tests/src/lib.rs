//! MoFA Testing Framework
//!
//! Provides mock implementations, failure injection, and deterministic time
//! control for testing MoFA agents.

pub mod adversarial;
pub mod assert;
pub mod assertions;
pub mod backend;
pub mod bus;
pub mod clock;
pub mod report;
pub mod tools;

pub use assert::{
    assert_response_contains, assert_response_not_contains, assert_tool_never_before,
    assert_tool_not_used, assert_tool_order, assert_tool_used, assert_tool_used_n_times,
};
pub use backend::MockLLMBackend;
pub use bus::MockAgentBus;
pub use clock::{Clock, MockClock, SystemClock};
pub use report::{
    JUnitFormatter, JsonFormatter, ReportFormatter, TestCaseResult, TestReport, TestReportBuilder,
    TestStatus, TextFormatter,
};
pub use tools::MockTool;
