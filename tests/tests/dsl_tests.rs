//! Integration tests for the minimal TOML DSL adapter.

use mofa_testing::{configure_runner_from_test_case, run_test_case, AgentTestRunner, TestCaseDsl};

#[tokio::test]
async fn toml_dsl_runs_through_agent_runner() {
    // Load the example DSL from the crate so the test exercises parsing and
    // adapter execution together.
    let case = TestCaseDsl::from_toml_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/simple_agent.toml"
    ))
        .expect("DSL example should parse");

    assert_eq!(case.name, "simple_agent_run");

    let result = run_test_case(&case)
        .await
        .expect("DSL case should run successfully");

    assert!(result.is_success());
    assert_eq!(result.output_text().as_deref(), Some("hello from DSL"));
}

#[tokio::test]
async fn toml_dsl_supports_bootstrap_files() {
    let case = TestCaseDsl::from_toml_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/bootstrap_agent.toml"
    ))
    .expect("bootstrap DSL example should parse");

    let mut runner = AgentTestRunner::new()
        .await
        .expect("runner should initialize");

    configure_runner_from_test_case(&case, &mut runner)
        .await
        .expect("DSL bootstrap config should apply");

    let _ = runner
        .run_text(case.prompt.as_deref().expect("prompt should be present"))
        .await
        .expect("bootstrap run should succeed");

    let request = runner
        .mock_llm()
        .last_request()
        .await
        .expect("request should be captured");
    let system_message = request
        .messages
        .first()
        .and_then(|msg| msg.content.as_deref())
        .expect("system message content");

    assert!(system_message.contains("AGENTS.md"));
    assert!(system_message.contains("Bootstrapped instructions for the DSL test."));

    runner.shutdown().await.expect("shutdown succeeds");
}

#[tokio::test]
async fn toml_dsl_supports_tool_backed_runs() {
    let case = TestCaseDsl::from_toml_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/tool_agent.toml"
    ))
    .expect("tool DSL example should parse");

    let result = run_test_case(&case)
        .await
        .expect("tool-backed DSL case should run successfully");

    assert!(result.is_success());
    assert_eq!(result.output_text().as_deref(), Some("Tool execution complete"));
    assert_eq!(result.metadata.tool_calls.len(), 1);
    assert_eq!(result.metadata.tool_calls[0].tool_name, "echo_tool");

    let request = result
        .metadata
        .llm_last_request
        .as_ref()
        .expect("request should be captured");
    let system_message = request
        .messages
        .first()
        .and_then(|msg| msg.content.as_deref())
        .expect("system message content");
    assert!(system_message.contains("ToolAgent"));
    assert!(system_message.contains("Agent used to validate tool-aware DSL execution."));
}
