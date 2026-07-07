//! Tests for canonical DSL run artifact generation.

use mofa_testing::{
    AgentRunArtifact, TestCaseDsl, assertion_error_from_outcomes, collect_assertion_outcomes,
    execute_test_case,
};

#[tokio::test]
async fn artifact_contains_core_run_data() {
    let case = TestCaseDsl::from_toml_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/tool_agent.toml"
    ))
    .expect("tool DSL example should parse");

    let result = execute_test_case(&case)
        .await
        .expect("DSL case should execute");
    let assertions = collect_assertion_outcomes(&case, &result);
    let artifact = AgentRunArtifact::from_run_result(&case, &result, assertions);

    assert_eq!(artifact.case_name, "tool_agent_run");
    assert_eq!(artifact.status, "passed");
    assert_eq!(artifact.output_text.as_deref(), Some("Tool execution complete"));
    assert_eq!(artifact.tool_calls.len(), 1);
    assert_eq!(artifact.tool_calls[0].tool_name, "echo_tool");
    assert!(!artifact.agent.name.is_empty());
    assert!(artifact.llm_request.is_some());
    assert!(artifact.workspace_after.files.iter().any(|file| {
        file.relative_path.ends_with(".jsonl")
    }));
}

#[tokio::test]
async fn artifact_captures_failed_assertions() {
    let case = TestCaseDsl::from_toml_str(
        r#"
name = "failing_case"
prompt = "Say hello"

[llm]
responses = ["wrong output"]

[assert]
contains = "expected text"
"#,
    )
    .expect("inline DSL should parse");

    let result = execute_test_case(&case)
        .await
        .expect("DSL case should execute");
    let assertions = collect_assertion_outcomes(&case, &result);
    let artifact = AgentRunArtifact::from_run_result(&case, &result, assertions.clone());

    assert_eq!(artifact.status, "failed");
    assert_eq!(artifact.assertions.len(), 1);
    assert!(!artifact.assertions[0].passed);
    assert!(assertion_error_from_outcomes(&assertions).is_some());
}
