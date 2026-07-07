//! Integration tests for `mofa test-dsl`.

use assert_cmd::Command;
use predicates::prelude::*;
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
