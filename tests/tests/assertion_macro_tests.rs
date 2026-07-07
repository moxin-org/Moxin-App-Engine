//! Tests for assertion macros and assertion helpers used by the testing crate.

use mofa_foundation::agent::components::tool::SimpleTool;
use mofa_foundation::orchestrator::{ModelOrchestrator, ModelProviderConfig, ModelType};
use mofa_kernel::agent::components::tool::ToolInput;
use mofa_kernel::bus::CommunicationMode;
use mofa_kernel::message::AgentMessage;
use mofa_testing::agent_runner::AgentTestRunner;
use mofa_testing::backend::MockLLMBackend;
use mofa_testing::bus::MockAgentBus;
use mofa_testing::tools::MockTool;
use serde_json::json;
use std::collections::HashMap;

fn make_config(name: &str) -> ModelProviderConfig {
    ModelProviderConfig {
        model_name: name.into(),
        model_path: "/mock".into(),
        device: "cpu".into(),
        model_type: ModelType::Llm,
        max_context_length: None,
        quantization: None,
        extra_config: HashMap::new(),
    }
}

// ===================================================================
// assert_tool_called!
// ===================================================================

#[tokio::test]
async fn assert_tool_called_passes_when_tool_was_called() {
    let tool = MockTool::new("search", "Search tool", json!({"type": "object"}));
    tool.execute(ToolInput::from_json(json!({"q": "rust"})))
        .await;

    mofa_testing::assert_tool_called!(tool, "search");
}

#[tokio::test]
#[should_panic(expected = "to be called at least once")]
async fn assert_tool_called_panics_when_tool_never_called() {
    let tool = MockTool::new("search", "Search tool", json!({"type": "object"}));
    mofa_testing::assert_tool_called!(tool, "search");
}

// ===================================================================
// assert_tool_called_with!
// ===================================================================

#[tokio::test]
async fn assert_tool_called_with_passes_on_matching_arguments() {
    let tool = MockTool::new("search", "Search tool", json!({"type": "object"}));
    tool.execute(ToolInput::from_json(json!({"query": "rust"})))
        .await;

    mofa_testing::assert_tool_called_with!(tool, json!({"query": "rust"}));
}

#[tokio::test]
#[should_panic(expected = "no matching call found")]
async fn assert_tool_called_with_panics_on_mismatched_arguments() {
    let tool = MockTool::new("search", "Search tool", json!({"type": "object"}));
    tool.execute(ToolInput::from_json(json!({"query": "python"})))
        .await;

    mofa_testing::assert_tool_called_with!(tool, json!({"query": "rust"}));
}

// ===================================================================
// assert_infer_called!
// ===================================================================

#[tokio::test]
async fn assert_infer_called_passes_after_infer_call() {
    let backend = MockLLMBackend::new();
    backend.add_response("hi", "hello");
    backend.register_model(make_config("m")).await.unwrap();
    backend.load_model("m").await.unwrap();
    backend.infer("m", "hi").await.unwrap();

    mofa_testing::assert_infer_called!(backend);
}

#[tokio::test]
#[should_panic(expected = "call_count is 0")]
async fn assert_infer_called_panics_when_call_count_is_zero() {
    let backend = MockLLMBackend::new();
    mofa_testing::assert_infer_called!(backend);
}

// ===================================================================
// assert_bus_message_sent!
// ===================================================================

#[tokio::test]
async fn assert_bus_message_sent_passes_when_sender_matches() {
    let bus = MockAgentBus::new();
    let msg = AgentMessage::TaskRequest {
        task_id: "t-1".into(),
        content: "ping".into(),
    };
    let _ = bus
        .send_and_capture("agent-1", CommunicationMode::Broadcast, msg)
        .await;

    mofa_testing::assert_bus_message_sent!(bus, "agent-1");
}

#[tokio::test]
#[should_panic(expected = "Expected a message from sender")]
async fn assert_bus_message_sent_panics_when_no_message_from_sender() {
    let bus = MockAgentBus::new();
    let msg = AgentMessage::TaskRequest {
        task_id: "t-1".into(),
        content: "ping".into(),
    };
    let _ = bus
        .send_and_capture("agent-1", CommunicationMode::Broadcast, msg)
        .await;

    mofa_testing::assert_bus_message_sent!(bus, "agent-2");
}

// ===================================================================
// New assertion helpers
// ===================================================================

#[tokio::test]
async fn assert_tool_last_result_passes_on_matching_output() {
    let tool = MockTool::new("search", "Search tool", json!({"type": "object"}));
    tool.execute(ToolInput::from_json(json!({"query": "rust"})))
        .await;

    mofa_testing::assert_tool_last_result!(tool, json!("Mock execution default"));
}

#[tokio::test]
async fn assert_agent_output_text_passes_on_matching_output() {
    let mut runner = AgentTestRunner::new().await.expect("runner initializes");
    runner.mock_llm().add_response("hello from runner").await;

    let result = runner.run_text("hello").await.expect("run succeeds");

    mofa_testing::assert_agent_output_text!(result, "hello from runner");
    runner.shutdown().await.expect("shutdown succeeds");
}

#[tokio::test]
async fn assert_run_failed_with_passes_on_matching_error() {
    let mut runner = AgentTestRunner::new().await.expect("runner initializes");
    runner.mock_llm().add_error_response("mock failure").await;

    let result = runner.run_text("hello").await.expect("run completes");

    mofa_testing::assert_run_failed_with!(result, "mock failure");
    runner.shutdown().await.expect("shutdown succeeds");
}

#[tokio::test]
async fn assert_workspace_contains_file_passes_when_snapshot_has_file() {
    let mut runner = AgentTestRunner::new().await.expect("runner initializes");
    runner.mock_llm().add_response("workspace ready").await;

    let result = runner.run_text("hello").await.expect("run succeeds");
    let expected = format!("sessions/{}.jsonl", runner.session_id());

    mofa_testing::assert_workspace_contains_file!(result.metadata.workspace_snapshot_after, expected);
    runner.shutdown().await.expect("shutdown succeeds");
}

#[tokio::test]
async fn assert_run_recorded_tool_call_passes_when_tool_metadata_exists() {
    let mut runner = AgentTestRunner::new().await.expect("runner initializes");
    let tool = MockTool::new(
        "echo_tool",
        "Echo tool",
        json!({
            "type": "object",
            "properties": { "input": { "type": "string" } },
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
    runner.mock_llm().add_response("done").await;

    let result = runner.run_text("use tool").await.expect("run succeeds");

    mofa_testing::assert_run_recorded_tool_call!(result, "echo_tool");
    runner.shutdown().await.expect("shutdown succeeds");
}

#[tokio::test]
async fn assert_runner_total_executions_passes_on_first_run() {
    let mut runner = AgentTestRunner::new().await.expect("runner initializes");
    runner.mock_llm().add_response("counted").await;

    let result = runner.run_text("hello").await.expect("run succeeds");

    mofa_testing::assert_runner_total_executions!(result, 1);
    runner.shutdown().await.expect("shutdown succeeds");
}
