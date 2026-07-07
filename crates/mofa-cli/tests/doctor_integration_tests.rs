//! Integration tests for `mofa doctor` command practical scenarios.

#![cfg(test)]
#![allow(deprecated)]

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

fn make_fixture_from_template(template_dir_name: &str) -> tempfile::TempDir {
    let root = tempdir().expect("tempdir");
    let fixture_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("doctor")
        .join(template_dir_name);

    copy_dir_recursive(&fixture_root, root.path()).expect("copy fixture");
    root
}

fn copy_dir_recursive(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;

    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = to.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            let content = fs::read(&src_path)?;
            fs::write(&dst_path, content)?;
        }
    }

    Ok(())
}

#[test]
fn doctor_json_reports_summary_and_checks() {
    let fixture = make_fixture_from_template("healthy_project");

    let output = Command::cargo_bin("mofa")
        .expect("mofa bin")
        .arg("doctor")
        .arg("--scenario")
        .arg("ci")
        .arg("--json")
        .arg("--path")
        .arg(fixture.path())
        .env_remove("OPENAI_API_KEY")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: Value = serde_json::from_slice(&output).expect("valid json output");
    let summary = value.get("summary").expect("summary field");
    let checks = value
        .get("checks")
        .and_then(Value::as_array)
        .expect("checks array");

    assert!(summary.get("passed").is_some());
    assert!(summary.get("warnings").is_some());
    assert!(summary.get("failed").is_some());
    assert!(checks.len() >= 8, "expected meaningful doctor checks");
}

#[test]
fn doctor_local_dev_strict_fails_without_openai_key() {
    let fixture = make_fixture_from_template("healthy_project");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .arg("doctor")
        .arg("--scenario")
        .arg("local-dev")
        .arg("--strict")
        .arg("--path")
        .arg(fixture.path())
        .env_remove("OPENAI_API_KEY")
        .assert()
        .failure()
        .stderr(contains("doctor strict mode failed"));
}

#[test]
fn doctor_ci_allows_missing_openai_key_non_strict() {
    let fixture = make_fixture_from_template("healthy_project");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .arg("doctor")
        .arg("--scenario")
        .arg("ci")
        .arg("--path")
        .arg(fixture.path())
        .env_remove("OPENAI_API_KEY")
        .assert()
        .success();
}

#[test]
fn doctor_flags_missing_agent_config_in_output() {
    let fixture = make_fixture_from_template("missing_agent_config");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .arg("doctor")
        .arg("--scenario")
        .arg("ci")
        .arg("--path")
        .arg(fixture.path())
        .env_remove("OPENAI_API_KEY")
        .assert()
        .success()
        .stdout(contains("No agent config file found"));
}

#[test]
fn doctor_flags_missing_env_gitignore_rules() {
    let fixture = make_fixture_from_template("missing_env_gitignore");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .arg("doctor")
        .arg("--scenario")
        .arg("ci")
        .arg("--path")
        .arg(fixture.path())
        .env_remove("OPENAI_API_KEY")
        .assert()
        .success()
        .stdout(contains(".gitignore does not include .env patterns"));
}

#[test]
fn doctor_strict_can_fail_with_empty_path_env() {
    let fixture = make_fixture_from_template("healthy_project");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .arg("doctor")
        .arg("--scenario")
        .arg("ci")
        .arg("--strict")
        .arg("--path")
        .arg(fixture.path())
        .env("PATH", "")
        .env_remove("OPENAI_API_KEY")
        .assert()
        .failure();
}

#[test]
fn doctor_fix_creates_runtime_dirs_when_available() {
    let fixture = make_fixture_from_template("healthy_project");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .arg("doctor")
        .arg("--scenario")
        .arg("ci")
        .arg("--fix")
        .arg("--path")
        .arg(fixture.path())
        .env_remove("OPENAI_API_KEY")
        .assert()
        .success();
}

#[test]
fn doctor_json_includes_scenario_name() {
    let fixture = make_fixture_from_template("healthy_project");

    let output = Command::cargo_bin("mofa")
        .expect("mofa bin")
        .arg("doctor")
        .arg("--scenario")
        .arg("release")
        .arg("--json")
        .arg("--path")
        .arg(fixture.path())
        .env("OPENAI_API_KEY", "dummy")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: Value = serde_json::from_slice(&output).expect("valid json output");
    assert_eq!(
        value.get("scenario").and_then(Value::as_str),
        Some("release")
    );
}

#[test]
fn doctor_reports_project_path_in_json() {
    let fixture = make_fixture_from_template("healthy_project");

    let output = Command::cargo_bin("mofa")
        .expect("mofa bin")
        .arg("doctor")
        .arg("--scenario")
        .arg("ci")
        .arg("--json")
        .arg("--path")
        .arg(fixture.path())
        .env_remove("OPENAI_API_KEY")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: Value = serde_json::from_slice(&output).expect("valid json output");
    let project_path = value
        .get("project_path")
        .and_then(Value::as_str)
        .expect("project path");
    assert!(project_path.contains(fixture.path().to_string_lossy().as_ref()));
}

#[test]
fn doctor_nonexistent_path_in_strict_mode_fails() {
    let nonexistent = tempdir().expect("tmp").path().join("not-here");

    Command::cargo_bin("mofa")
        .expect("mofa bin")
        .arg("doctor")
        .arg("--scenario")
        .arg("ci")
        .arg("--strict")
        .arg("--path")
        .arg(nonexistent)
        .env_remove("OPENAI_API_KEY")
        .assert()
        .failure();
}
