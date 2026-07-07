//! Integration tests for `mofa swarm run`.

#![cfg(test)]

use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn write_swarm_config(contents: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("swarm.yaml");
    fs::write(&path, contents).expect("write swarm config");
    (dir, path)
}

fn example_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(name)
}

#[test]
fn swarm_run_executes_pipeline_and_prints_stage_markers_and_audit() {
    let (_dir, path) = write_swarm_config(
        r#"
name: document review pipeline
pattern: sequential
agents:
  - id: reader-a
    capabilities: [extract]
  - id: reviewer-a
    capabilities: [review]
tasks:
  - id: extract
    description: extract key facts from the document
    capabilities: [extract]
    complexity: 0.3
  - id: review
    description: review extracted content
    capabilities: [review]
    complexity: 0.4
    depends_on: [extract]
"#,
    );

    // Assert only the stable stage flow and audit markers, not table formatting.
    assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .assert()
        .success()
        .stdout(predicate::str::contains("[1/5] loading config"))
        .stdout(predicate::str::contains("[2/5] coverage check"))
        .stdout(predicate::str::contains("[3/5] admission check"))
        .stdout(predicate::str::contains("[4/5] executing"))
        .stdout(predicate::str::contains("[5/5] results"))
        .stdout(predicate::str::contains("audit trail:"))
        .stdout(predicate::str::contains("[swarmstarted]"))
        .stdout(predicate::str::contains("[swarmcompleted]"))
        .stdout(predicate::str::contains("pattern"))
        .stdout(predicate::str::contains("tasks"));
}

#[test]
fn swarm_run_dry_run_blocks_uncovered_tasks_before_execution() {
    let (_dir, path) = write_swarm_config(
        r#"
name: uncovered capability demo
pattern: sequential
agents:
  - id: reader-a
    capabilities: [extract]
tasks:
  - id: write
    description: write a final report
    capabilities: [write]
"#,
    );

    // Dry-run should stop at validation when no capable agent exists.
    assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains("[1/3] loading config"))
        .stdout(predicate::str::contains("[2/3] coverage check"))
        .stdout(predicate::str::contains("blocked: 1 task(s) have no capable agent"))
        .stdout(predicate::str::contains("[3/3] admission check").not())
        .stdout(predicate::str::contains("dry-run complete").not())
        .stdout(predicate::str::contains("[4/5] executing").not());
}

#[test]
fn swarm_run_dry_run_blocks_sla_overrun_before_execution() {
    let (_dir, path) = write_swarm_config(
        r#"
name: sla gate demo
pattern: sequential
agents:
  - id: reader-a
    capabilities: [extract]
sla:
  max_duration_secs: 5
tasks:
  - id: extract
    description: extract key facts from the document
    capabilities: [extract]
    complexity: 1.0
"#,
    );

    // Admission checks should block execution in dry-run mode when the SLA is exceeded.
    assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains("[1/3] loading config"))
        .stdout(predicate::str::contains("[2/3] coverage check"))
        .stdout(predicate::str::contains("[3/3] admission check"))
        .stdout(predicate::str::contains("blocked: estimated duration (30s) exceeds sla limit"))
        .stdout(predicate::str::contains("dry-run complete").not())
        .stdout(predicate::str::contains("[4/5] executing").not());
}

#[test]
fn swarm_run_warns_on_uncovered_tasks_during_execution() {
    let (_dir, path) = write_swarm_config(
        r#"
name: uncovered warning demo
pattern: sequential
agents:
  - id: reader-a
    capabilities: [extract]
tasks:
  - id: write
    description: write a final report
    capabilities: [write]
"#,
    );

    // Non-dry runs currently warn and continue so downstream behavior remains visible.
    assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .assert()
        .success()
        .stdout(predicate::str::contains("[4/5] executing"))
        .stderr(predicate::str::contains(
            "warning: proceeding with 1 uncovered task(s)",
        ));
}

#[test]
fn swarm_run_emits_metrics_when_requested() {
    let (_dir, path) = write_swarm_config(
        r#"
name: metrics demo
pattern: sequential
agents:
  - id: reader-a
    capabilities: [extract]
tasks:
  - id: extract
    description: extract key facts from the document
    capabilities: [extract]
"#,
    );

    // Check section headers and metric names rather than the full Prometheus payload.
    assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .arg("--metrics")
        .assert()
        .success()
        .stdout(predicate::str::contains("# HELP mofa_swarm_scheduler_runs_total"))
        .stdout(predicate::str::contains(
            "# TYPE mofa_swarm_scheduler_duration_seconds gauge",
        ))
        .stdout(predicate::str::contains("mofa_swarm_tasks_total"))
        .stdout(predicate::str::contains("pattern=\"Sequential\""))
        .stdout(predicate::str::contains("status=\"succeeded\""));
}

#[test]
fn swarm_run_reports_invalid_yaml() {
    let (_dir, path) = write_swarm_config("name: [broken");

    assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .assert()
        .failure()
        .stdout(predicate::str::contains("[1/5] loading config"))
        .stderr(predicate::str::contains(
            "invalid type: sequence, expected a string",
        ));
}

#[test]
fn swarm_run_reports_unknown_dependency() {
    let (_dir, path) = write_swarm_config(
        r#"
name: bad dag demo
pattern: sequential
agents:
  - id: reviewer-a
    capabilities: [review]
tasks:
  - id: review
    description: review extracted content
    capabilities: [review]
    depends_on: [extract]
"#,
    );

    assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .assert()
        .failure()
        .stdout(predicate::str::contains("[4/5] executing"))
        .stderr(predicate::str::contains("unknown depends_on: extract"));
}

#[test]
fn swarm_run_warns_on_partial_coverage_and_preserves_task_order_in_lists() {
    let path = example_path("swarm_demo.yaml");

    // Read stdout once so we can check both the warning and the stable list order.
    let output = assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).expect("utf8 stdout");
    assert!(stdout.contains("covered  (2 tasks):  extract-1, extract-2"));
    assert!(stdout.contains("partial  (2 tasks):  translate, review"));
    assert!(stdout.contains("warning: partial tasks have single-agent coverage (spof risk)"));
}

#[test]
fn swarm_run_honors_pattern_override_flag() {
    let path = example_path("swarm_pipeline_demo.yaml");

    // The CLI flag should win over the config pattern.
    assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .arg("--pattern")
        .arg("parallel")
        .assert()
        .success()
        .stdout(predicate::str::contains("[4/5] executing"))
        .stdout(predicate::str::contains("pattern: Parallel"));
}

#[test]
fn swarm_run_auto_upgrades_to_parallel_for_independent_tasks() {
    let (_dir, path) = write_swarm_config(
        r#"
name: independent tasks demo
pattern: sequential
agents:
  - id: worker-a
    capabilities: [extract]
  - id: worker-b
    capabilities: [review]
tasks:
  - id: extract
    description: extract key facts
    capabilities: [extract]
  - id: review
    description: review extracted facts
    capabilities: [review]
"#,
    );

    // Independent tasks should trigger the throughput-oriented pattern upgrade note.
    assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "all tasks are independent - switching to Parallel for better throughput",
        ).or(predicate::str::contains(
            "all tasks are independent — switching to Parallel for better throughput",
        )))
        .stdout(predicate::str::contains("pattern: Parallel"));
}

#[test]
fn swarm_run_keeps_sequential_when_dependencies_exist() {
    let path = example_path("swarm_pipeline_demo.yaml");

    // A dependency chain should suppress the auto-upgrade path.
    assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .assert()
        .success()
        .stdout(predicate::str::contains("pattern: Sequential"))
        .stdout(predicate::str::contains("all tasks are independent").not());
}

#[test]
fn swarm_run_accepts_timeout_flag() {
    let (_dir, path) = write_swarm_config(
        r#"
name: timeout demo
pattern: sequential
agents:
  - id: worker-a
    capabilities: [extract]
tasks:
  - id: extract
    description: extract key facts
    capabilities: [extract]
"#,
    );

    // This locks down CLI parsing and scheduler config wiring for the timeout flag.
    assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .arg("--timeout")
        .arg("1")
        .assert()
        .success()
        .stdout(predicate::str::contains("[5/5] results"));
}

#[test]
fn swarm_run_reports_missing_file() {
    let path = PathBuf::from("/tmp/definitely-missing-swarm-config.yaml");

    // Missing-file failures should surface as a clear CLI error before any stage advances.
    assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .assert()
        .failure()
        .stdout(predicate::str::contains("[1/5] loading config"))
        .stderr(predicate::str::contains("I/O error:"))
        .stderr(
            predicate::str::contains("No such file or directory").or(predicate::str::contains(
                "The system cannot find the path specified.",
            )),
        );
}

#[test]
fn swarm_run_examples_swarm_demo_regression() {
    let path = example_path("swarm_demo.yaml");

    // Keep the shipped example runnable as a contributor-facing regression check.
    assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .assert()
        .success()
        .stdout(predicate::str::contains("name:    document review pipeline"))
        .stdout(predicate::str::contains("covered  (2 tasks):  extract-1, extract-2"))
        .stdout(predicate::str::contains("partial  (2 tasks):  translate, review"))
        .stdout(predicate::str::contains("pattern: Sequential"))
        .stdout(predicate::str::contains("audit trail:"));
}

#[test]
fn swarm_run_examples_swarm_pipeline_demo_regression() {
    let path = example_path("swarm_pipeline_demo.yaml");

    // The sequential example should keep its current stage flow and successful completion markers.
    assert_cmd::cargo::cargo_bin_cmd!("mofa")
        .arg("swarm")
        .arg("run")
        .arg(&path)
        .assert()
        .success()
        .stdout(predicate::str::contains("name:    research synthesis pipeline"))
        .stdout(predicate::str::contains("[2/5] coverage check"))
        .stdout(predicate::str::contains("[3/5] admission check"))
        .stdout(predicate::str::contains("pattern: Sequential"))
        .stdout(predicate::str::contains("[swarmcompleted] 4/4 tasks succeeded"));
}
