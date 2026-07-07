//! Tool-focused example showing assertion-driven validation over a tool-calling agent run.
//!
//! Demonstrates tool-call, LLM-capture, and workspace-side-effect assertions.

use anyhow::Result;
use mofa_testing::agent_runner::AgentTestRunner;
use mofa_testing::assertions::{
    assert_llm_request_has_system_message, assert_llm_request_message_count,
    assert_llm_response_equals, assert_run_tool_called, assert_run_tool_call_count,
    assert_run_tool_duration_recorded, assert_run_tool_input, assert_run_tool_output_contains,
    assert_run_tool_succeeded, assert_workspace_file_count_delta,
    assert_workspace_file_exists_after,
};
use mofa_testing::tools::MockTool;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    let mut runner = AgentTestRunner::new().await?;

    let tool = MockTool::new(
        "echo_tool",
        "Echo the provided input",
        json!({
            "type": "object",
            "properties": {
                "input": { "type": "string" }
            },
            "required": ["input"]
        }),
    );

    runner.register_mock_tool(tool).await?;
    runner
        .mock_llm()
        .add_tool_call_response("echo_tool", json!({ "input": "ping" }), Some("calling tool".into()))
        .await;
    runner.mock_llm().add_response("Tool response completed").await;

    let result = runner.run_text("use the tool").await?;
    let session_file = format!("sessions/{}.jsonl", runner.session_id());

    assert_llm_request_message_count(&result, 4);
    assert_llm_request_has_system_message(&result);
    assert_llm_response_equals(&result, "Tool response completed");
    assert_run_tool_called(&result, "echo_tool");
    assert_run_tool_call_count(&result, "echo_tool", 1);
    assert_run_tool_input(&result, "echo_tool", &json!({ "input": "ping" }));
    assert_run_tool_succeeded(&result, "echo_tool");
    assert_run_tool_output_contains(&result, "echo_tool", "Mock execution default");
    assert_run_tool_duration_recorded(&result, "echo_tool");
    assert_workspace_file_count_delta(&result, 1);
    assert_workspace_file_exists_after(&result, &session_file);

    println!("tool assertions example passed");

    runner.shutdown().await?;
    Ok(())
}
