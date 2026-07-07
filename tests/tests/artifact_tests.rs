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

#[tokio::test]
async fn artifact_compare_to_returns_match_for_identical_runs() {
    let case = TestCaseDsl::from_toml_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/simple_agent.toml"
    ))
    .expect("DSL example should parse");

    let result = execute_test_case(&case)
        .await
        .expect("DSL case should execute");
    let assertions = collect_assertion_outcomes(&case, &result);
    let artifact = AgentRunArtifact::from_run_result(&case, &result, assertions);

    let diff = artifact.compare_to(&artifact);

    assert!(diff.matches);
    assert!(diff.differences.is_empty());
}

#[tokio::test]
async fn artifact_compare_to_detects_output_and_tool_changes() {
    let baseline = TestCaseDsl::from_toml_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/tool_agent.toml"
    ))
    .expect("tool DSL should parse");
    let baseline_result = execute_test_case(&baseline)
        .await
        .expect("baseline DSL should execute");
    let baseline_assertions = collect_assertion_outcomes(&baseline, &baseline_result);
    let baseline_artifact =
        AgentRunArtifact::from_run_result(&baseline, &baseline_result, baseline_assertions);

    let candidate = TestCaseDsl::from_toml_str(
        r#"
name = "tool_agent_run"
input = "Use the echo tool and summarize the result."

[[tools]]
name = "lookup_tool"
description = "Lookup the provided input."
schema = { type = "object", properties = { input = { type = "string" } }, required = ["input"] }
result = "lookup result"

[assert]
contains = "Different output"
tool_called = "lookup_tool"

[llm]

[[llm.steps]]
type = "tool_call"
tool = "lookup_tool"
arguments = { input = "ping" }

[[llm.steps]]
type = "text"
content = "Different output"
"#,
    )
    .expect("candidate DSL should parse");
    let candidate_result = execute_test_case(&candidate)
        .await
        .expect("candidate DSL should execute");
    let candidate_assertions = collect_assertion_outcomes(&candidate, &candidate_result);
    let candidate_artifact =
        AgentRunArtifact::from_run_result(&candidate, &candidate_result, candidate_assertions);

    let diff = candidate_artifact.compare_to(&baseline_artifact);

    assert!(!diff.matches);
    assert!(diff.differences.iter().any(|item| item.field == "output_text"));
    assert!(diff.differences.iter().any(|item| item.field == "tool_calls"));
}
