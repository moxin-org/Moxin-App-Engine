//! Integration tests for `mofa test-dsl`.

use axum::{Json, Router, routing::post};
use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn test_dsl_command_runs_example_case() {
    let case_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/examples/simple_agent.toml"
    );

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .args(["test-dsl", case_path])
        .assert()
        .success()
        .stdout(predicate::str::contains("status: passed"))
        .stdout(predicate::str::contains("output: hello from DSL"));
}

#[test]
fn test_dsl_command_emits_json() {
    let case_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/examples/tool_agent.toml"
    );

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .args(["--output-format", "json", "test-dsl", case_path])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"success\": true"))
        .stdout(predicate::str::contains("\"tool_calls\""))
        .stdout(predicate::str::contains("\"echo_tool\""));
}

#[test]
fn test_dsl_command_runs_tape_backed_case() {
    let case_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/examples/simple_agent_tape.toml"
    );

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .args(["test-dsl", case_path])
        .assert()
        .success()
        .stdout(predicate::str::contains("status: passed"))
        .stdout(predicate::str::contains("output: hello from tape"));
}

#[test]
fn test_dsl_command_writes_json_report_file() {
    let case_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/examples/simple_agent.toml"
    );
    let temp = tempdir().expect("temp dir");
    let report_path = temp.path().join("dsl-report.json");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .args([
            "test-dsl",
            case_path,
            "--report-out",
            report_path.to_str().expect("utf8 report path"),
            "--report-format",
            "json",
        ])
        .assert()
        .success();

    let report = std::fs::read_to_string(&report_path).expect("report file exists");
    assert!(report.contains("\"suite\": \"dsl\""));
    assert!(report.contains("\"name\": \"simple_agent_run\""));
    assert!(report.contains("\"status\": \"passed\""));
}

#[test]
fn test_dsl_command_writes_text_report_file() {
    let case_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/examples/tool_agent.toml"
    );
    let temp = tempdir().expect("temp dir");
    let report_path = temp.path().join("dsl-report.txt");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .args([
            "test-dsl",
            case_path,
            "--report-out",
            report_path.to_str().expect("utf8 report path"),
            "--report-format",
            "text",
        ])
        .assert()
        .success();

    let report = std::fs::read_to_string(&report_path).expect("report file exists");
    assert!(report.contains("=== dsl ==="));
    assert!(report.contains("tool_agent_run"));
    assert!(report.contains("[+]"));
}

#[test]
fn test_dsl_command_writes_canonical_artifact_file() {
    let case_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/examples/tool_agent.toml"
    );
    let temp = tempdir().expect("temp dir");
    let artifact_path = temp.path().join("dsl-artifact.json");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .args([
            "test-dsl",
            case_path,
            "--artifact-out",
            artifact_path.to_str().expect("utf8 artifact path"),
        ])
        .assert()
        .success();

    let artifact = std::fs::read_to_string(&artifact_path).expect("artifact file exists");
    assert!(artifact.contains("\"case_name\": \"tool_agent_run\""));
    assert!(artifact.contains("\"status\": \"passed\""));
    assert!(artifact.contains("\"assertions\""));
    assert!(artifact.contains("\"tool_calls\""));
}

#[test]
fn test_dsl_command_writes_baseline_file() {
    let case_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/examples/simple_agent.toml"
    );
    let temp = tempdir().expect("temp dir");
    let baseline_path = temp.path().join("dsl-baseline.json");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .args([
            "test-dsl",
            case_path,
            "--baseline-out",
            baseline_path.to_str().expect("utf8 baseline path"),
        ])
        .assert()
        .success();

    let baseline = std::fs::read_to_string(&baseline_path).expect("baseline file exists");
    assert!(baseline.contains("\"case_name\": \"simple_agent_run\""));
    assert!(baseline.contains("\"status\": \"passed\""));
}

#[test]
fn test_dsl_command_reports_baseline_mismatch() {
    let case_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/examples/tool_agent.toml"
    );
    let temp = tempdir().expect("temp dir");
    let baseline_path = temp.path().join("dsl-baseline.json");

    std::fs::write(
        &baseline_path,
        r#"{
  "case_name": "tool_agent_run",
  "status": "passed",
  "output_text": "Baseline output",
  "runner_error": null,
  "duration_ms": 0,
  "started_at_ms": 0,
  "execution_id": "baseline-exec",
  "session_id": "baseline-session",
  "workspace_root": "/tmp/baseline",
  "agent": { "id": "baseline-agent", "name": "baseline" },
  "assertions": [{ "kind": "contains", "expected": "Baseline output", "actual": "Baseline output", "passed": true }],
  "tool_calls": [],
  "llm_request": null,
  "llm_response": null,
  "session_snapshot": null,
  "workspace_before": { "files": [] },
  "workspace_after": { "files": [] }
}"#,
    )
    .expect("baseline fixture written");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .args([
            "test-dsl",
            case_path,
            "--baseline-in",
            baseline_path.to_str().expect("utf8 baseline path"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("baseline: mismatch"))
        .stdout(predicate::str::contains("difference: output_text"));
}

#[test]
fn test_dsl_command_writes_comparison_file() {
    let case_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/examples/simple_agent.toml"
    );
    let temp = tempdir().expect("temp dir");
    let baseline_path = temp.path().join("dsl-baseline.json");
    let comparison_path = temp.path().join("dsl-comparison.json");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .args([
            "test-dsl",
            case_path,
            "--baseline-out",
            baseline_path.to_str().expect("utf8 baseline path"),
        ])
        .assert()
        .success();

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .args([
            "test-dsl",
            case_path,
            "--baseline-in",
            baseline_path.to_str().expect("utf8 baseline path"),
            "--comparison-out",
            comparison_path.to_str().expect("utf8 comparison path"),
        ])
        .assert()
        .success();

    let comparison = std::fs::read_to_string(&comparison_path).expect("comparison file exists");
    assert!(comparison.contains("\"case_name\": \"simple_agent_run\""));
    assert!(comparison.contains("\"matches\": true"));
}

#[test]
fn test_dsl_command_fails_on_baseline_mismatch_when_flag_set() {
    let case_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/examples/tool_agent.toml"
    );
    let temp = tempdir().expect("temp dir");
    let baseline_path = temp.path().join("dsl-baseline.json");

    std::fs::write(
        &baseline_path,
        r#"{
  "case_name": "tool_agent_run",
  "status": "passed",
  "output_text": "Baseline output",
  "runner_error": null,
  "duration_ms": 0,
  "started_at_ms": 0,
  "execution_id": "baseline-exec",
  "session_id": "baseline-session",
  "workspace_root": "/tmp/baseline",
  "agent": { "id": "baseline-agent", "name": "baseline" },
  "assertions": [{ "kind": "contains", "expected": "Baseline output", "actual": "Baseline output", "passed": true }],
  "tool_calls": [],
  "llm_request": null,
  "llm_response": null,
  "session_snapshot": null,
  "workspace_before": { "files": [] },
  "workspace_after": { "files": [] }
}"#,
    )
    .expect("baseline fixture written");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .args([
            "test-dsl",
            case_path,
            "--baseline-in",
            baseline_path.to_str().expect("utf8 baseline path"),
            "--fail-on-diff",
        ])
        .assert()
        .failure();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_dsl_command_records_tape_from_live_provider() {
    fn toml_string(path: &std::path::Path) -> String {
        path.display().to_string().replace('\\', "\\\\")
    }

    async fn completions(Json(_request): Json<serde_json::Value>) -> Json<serde_json::Value> {
        Json(json!({
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "hello from live provider"
                    }
                }
            ],
            "usage": {
                "prompt_tokens": 3,
                "completion_tokens": 4,
                "total_tokens": 7
            }
        }))
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server");
    let address = listener.local_addr().expect("local addr");
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/v1/chat/completions", post(completions)))
            .await
            .expect("mock server should run");
    });

    let temp = tempdir().expect("temp dir");
    let tape_path = temp.path().join("recorded.tape.json");
    let case_path = temp.path().join("record_case.toml");
    std::fs::write(
        &case_path,
        format!(
            "name = \"record_case\"\nprompt = \"hello\"\n\n[llm]\nrecord_tape = \"{}\"\n\n[llm.provider]\nkind = \"open_ai_compatible\"\nbase_url = \"http://{}/v1\"\nmodel = \"mock-model\"\n",
            toml_string(&tape_path),
            address
        ),
    )
    .expect("record case written");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .args(["test-dsl", case_path.to_str().expect("utf8 case path")])
        .assert()
        .success()
        .stdout(predicate::str::contains("output: hello from live provider"));

    let tape = std::fs::read_to_string(&tape_path).expect("tape file exists");
    assert!(tape.contains("\"case_name\": \"record_case\""));
    assert!(tape.contains("\"response\": \"hello from live provider\""));

    server.abort();
}
