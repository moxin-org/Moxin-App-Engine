use std::time::Duration;

use mofa_kernel::agent::AgentState;
use mofa_kernel::agent::components::tool::ToolResult;
use mofa_runtime::runner::RunnerState;
use mofa_testing::agent_runner::AgentTestRunner;
use mofa_testing::assertions::{
    assert_agent_state_after, assert_agent_state_before, assert_duration_under,
    assert_execution_id_present, assert_llm_request_contains,
    assert_llm_request_has_system_message, assert_llm_request_message_count,
    assert_llm_response_contains, assert_llm_response_equals,
    assert_llm_response_has_no_tool_calls,
    assert_output_contains, assert_output_matches_regex, assert_run_failure_contains,
    assert_failed_executions_delta, assert_last_execution_time_recorded, assert_run_success,
    assert_run_tool_call_count, assert_run_tool_called, assert_run_tool_duration_recorded,
    assert_run_tool_duration_under, assert_run_tool_failed, assert_run_tool_input,
    assert_run_tool_not_called, assert_run_tool_output_contains,
    assert_run_tool_output_equals_json, assert_run_tool_succeeded,
    assert_run_tool_timed_out, assert_runner_state_after, assert_runner_state_before,
    assert_session_contains, assert_session_contains_in_order, assert_session_id_equals,
    assert_session_len, assert_session_len_at_least, assert_successful_executions_delta,
    assert_total_executions_delta, assert_workspace_file_changed,
    assert_workspace_file_checksum_changed, assert_workspace_file_count_delta,
    assert_workspace_file_exists_after, assert_workspace_file_exists_before,
    assert_workspace_has_file, assert_workspace_missing_file,
};
use mofa_testing::tools::MockTool;
use serde_json::json;

#[tokio::test]
async fn artifact_assertions_cover_success_output_and_llm_capture() {
    let mut runner = AgentTestRunner::new().await.expect("runner initializes");
    runner.mock_llm().add_response("Hello from assertions").await;

    let result = runner.run_text("hello").await.expect("run succeeds");

    assert_run_success(&result);
    assert_execution_id_present(&result);
    assert_session_id_equals(&result, runner.session_id());
    assert_runner_state_before(&result, RunnerState::Created);
    assert_runner_state_after(&result, RunnerState::Running);
    assert_agent_state_before(&result, AgentState::Ready);
    assert_agent_state_after(&result, AgentState::Ready);
    assert_total_executions_delta(&result, 1);
    assert_successful_executions_delta(&result, 1);
    assert_failed_executions_delta(&result, 0);
    assert_last_execution_time_recorded(&result);
    assert_output_contains(&result, "assertions");
    assert_output_matches_regex(&result, "Hello .* assertions");
    assert_duration_under(&result, Duration::from_secs(2));
    assert_llm_request_contains(&result, "hello");
    assert_llm_request_message_count(&result, 2);
    assert_llm_request_has_system_message(&result);
    assert_llm_response_contains(&result, "Hello from assertions");
    assert_llm_response_equals(&result, "Hello from assertions");
    assert_llm_response_has_no_tool_calls(&result);
    assert_session_len(&result, 2);
    assert_session_len_at_least(&result, 2);
    assert_session_contains(&result, "user", "hello");
    assert_session_contains(&result, "assistant", "Hello from assertions");
    assert_session_contains_in_order(
        &result,
        &[("user", "hello"), ("assistant", "Hello from assertions")],
    );

    runner.shutdown().await.expect("shutdown succeeds");
}

#[tokio::test]
async fn artifact_assertions_cover_tool_records() {
    let mut runner = AgentTestRunner::new().await.expect("runner initializes");
    let tool = MockTool::new(
        "echo_tool",
        "Echo input",
        json!({
            "type": "object",
            "properties": {
                "input": { "type": "string" }
            },
            "required": ["input"]
        }),
    );
    runner
        .register_mock_tool(tool)
        .await
        .expect("tool registered");

    runner
        .mock_llm()
        .add_tool_call_response("echo_tool", json!({ "input": "ping" }), None)
        .await;
    runner.mock_llm().add_response("tool complete").await;

    let result = runner.run_text("use a tool").await.expect("run succeeds");

    assert_run_tool_called(&result, "echo_tool");
    assert_run_tool_not_called(&result, "missing_tool");
    assert_run_tool_call_count(&result, "echo_tool", 1);
    assert_run_tool_input(&result, "echo_tool", &json!({ "input": "ping" }));
    assert_run_tool_succeeded(&result, "echo_tool");
    assert_run_tool_output_contains(&result, "echo_tool", "Mock execution default");
    assert_run_tool_duration_recorded(&result, "echo_tool");
    assert_llm_response_has_no_tool_calls(&result);

    runner.shutdown().await.expect("shutdown succeeds");
}

#[tokio::test]
async fn artifact_assertions_cover_workspace_snapshots() {
    let mut runner = AgentTestRunner::new().await.expect("runner initializes");
    runner
        .write_workspace_file("notes.txt", "before")
        .expect("workspace file written");
    runner.mock_llm().add_response("workspace ok").await;

    let result = runner.run_text("write session").await.expect("run succeeds");
    let session_file = format!("sessions/{}.jsonl", runner.session_id());

    assert_workspace_missing_file(&result.metadata.workspace_snapshot_before, &session_file);
    assert_workspace_has_file(&result.metadata.workspace_snapshot_after, &session_file);
    assert_workspace_file_exists_before(&result, "notes.txt");
    assert_workspace_file_exists_after(&result, "notes.txt");
    assert_workspace_file_count_delta(&result, 1);
    assert_workspace_file_changed(&result, &session_file);

    runner.shutdown().await.expect("shutdown succeeds");
}

#[tokio::test]
async fn artifact_assertions_cover_workspace_checksum_changes() {
    let mut runner = AgentTestRunner::new().await.expect("runner initializes");
    let session_file = format!("sessions/{}.jsonl", runner.session_id());
    runner
        .write_workspace_file(&session_file, "seed\n")
        .expect("workspace file written");
    runner.mock_llm().add_response("checksum ok").await;

    let result = runner.run_text("checksum run").await.expect("run succeeds");

    assert_workspace_file_exists_before(&result, &session_file);
    assert_workspace_file_exists_after(&result, &session_file);
    assert_workspace_file_checksum_changed(&result, &session_file);

    runner.shutdown().await.expect("shutdown succeeds");
}

#[tokio::test]
async fn artifact_assertions_cover_failed_and_timed_out_tools() {
    let mut runner = AgentTestRunner::new().await.expect("runner initializes");
    let tool = MockTool::new(
        "fragile_tool",
        "Fails on purpose",
        json!({
            "type": "object",
            "properties": {
                "input": { "type": "string" }
            },
            "required": ["input"]
        }),
    );
    tool.set_result(ToolResult::failure("tool timed out")).await;
    runner
        .register_mock_tool(tool)
        .await
        .expect("tool registered");

    runner
        .mock_llm()
        .add_tool_call_response("fragile_tool", json!({ "input": "ping" }), None)
        .await;
    runner.mock_llm().add_response("tool failed handled").await;

    let result = runner
        .run_text("use fragile tool")
        .await
        .expect("run succeeds");

    assert_run_tool_called(&result, "fragile_tool");
    assert_run_tool_failed(&result, "fragile_tool");
    assert_run_tool_timed_out(&result, "fragile_tool");
    assert_run_tool_output_equals_json(&result, "fragile_tool", &json!(null));
    assert_run_tool_duration_recorded(&result, "fragile_tool");
    assert_run_tool_duration_under(&result, "fragile_tool", 1_000);

    runner.shutdown().await.expect("shutdown succeeds");
}

#[tokio::test]
async fn artifact_assertions_cover_failures() {
    let mut runner = AgentTestRunner::new().await.expect("runner initializes");
    runner.mock_llm().add_error_response("boom failure").await;

    let result = runner.run_text("trigger failure").await.expect("run returns result");

    assert_run_failure_contains(&result, "boom failure");
    assert_execution_id_present(&result);
    assert_session_id_equals(&result, runner.session_id());
    assert_runner_state_before(&result, RunnerState::Created);
    assert_runner_state_after(&result, RunnerState::Running);
    assert_agent_state_before(&result, AgentState::Ready);
    assert_agent_state_after(&result, AgentState::Ready);
    assert_total_executions_delta(&result, 1);
    assert_successful_executions_delta(&result, 0);
    assert_failed_executions_delta(&result, 1);
    assert_last_execution_time_recorded(&result);

    runner.shutdown().await.expect("shutdown succeeds");
}

#[tokio::test]
async fn artifact_assertions_cover_llm_tool_call_capture() {
    let mut runner = AgentTestRunner::new().await.expect("runner initializes");
    let tool = MockTool::new(
        "observe_tool",
        "Observes input",
        json!({
            "type": "object",
            "properties": {
                "input": { "type": "string" }
            },
            "required": ["input"]
        }),
    );
    runner
        .register_mock_tool(tool)
        .await
        .expect("tool registered");

    runner
        .mock_llm()
        .add_tool_call_response("observe_tool", json!({ "input": "inspect" }), Some("calling tool".into()))
        .await;
    runner.mock_llm().add_response("done").await;

    let result = runner.run_text("inspect this").await.expect("run succeeds");

    assert_llm_request_message_count(&result, 4);
    assert_llm_request_has_system_message(&result);
    assert_llm_response_equals(&result, "done");
    assert_llm_response_has_no_tool_calls(&result);
    assert_run_tool_called(&result, "observe_tool");

    runner.shutdown().await.expect("shutdown succeeds");
}

#[tokio::test]
async fn artifact_assertions_cover_session_ordering() {
    let mut runner = AgentTestRunner::new().await.expect("runner initializes");
    runner.mock_llm().add_response("First reply").await;
    runner.mock_llm().add_response("Second reply").await;

    let results = runner
        .run_texts(&["turn one", "turn two"])
        .await
        .expect("multi-turn run succeeds");
    let result = results.last().expect("last result present");

    assert_session_len_at_least(result, 4);
    assert_session_contains_in_order(
        result,
        &[
            ("user", "turn one"),
            ("assistant", "First reply"),
            ("user", "turn two"),
            ("assistant", "Second reply"),
        ],
    );

    runner.shutdown().await.expect("shutdown succeeds");
}
