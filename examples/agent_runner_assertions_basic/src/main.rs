//! Basic example showing assertion-driven validation over a successful agent run.
//!
//! Demonstrates output, session, LLM-capture, runner-state, and stats assertions.

use anyhow::Result;
use mofa_runtime::runner::RunnerState;
use mofa_testing::agent_runner::AgentTestRunner;
use mofa_testing::assertions::{
    assert_duration_under, assert_execution_id_present, assert_llm_request_has_system_message,
    assert_llm_request_message_count, assert_llm_response_equals, assert_output_contains,
    assert_run_success, assert_runner_state_after, assert_runner_state_before,
    assert_session_contains_in_order, assert_session_id_equals, assert_session_len_at_least,
    assert_successful_executions_delta, assert_total_executions_delta,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    let mut runner = AgentTestRunner::new().await?;
    runner.mock_llm().add_response("Hello from assertions").await;

    let result = runner.run_text("hello").await?;

    assert_run_success(&result);
    assert_execution_id_present(&result);
    assert_session_id_equals(&result, runner.session_id());
    assert_runner_state_before(&result, RunnerState::Created);
    assert_runner_state_after(&result, RunnerState::Running);
    assert_total_executions_delta(&result, 1);
    assert_successful_executions_delta(&result, 1);
    assert_output_contains(&result, "assertions");
    assert_duration_under(&result, Duration::from_secs(2));
    assert_llm_request_message_count(&result, 2);
    assert_llm_request_has_system_message(&result);
    assert_llm_response_equals(&result, "Hello from assertions");
    assert_session_len_at_least(&result, 2);
    assert_session_contains_in_order(
        &result,
        &[("user", "hello"), ("assistant", "Hello from assertions")],
    );

    println!("basic assertions example passed");

    runner.shutdown().await?;
    Ok(())
}
