#![allow(missing_docs)]

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("resolve workspace root")
}

#[test]
fn workflow_dsl_runs_offline() {
    let root = workspace_root();
    let manifest = root.join("examples").join("workflow_dsl").join("Cargo.toml");
    let example_dir = root.join("examples").join("workflow_dsl");

    let out = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(&manifest)
        .current_dir(&example_dir)
        .env_remove("OPENAI_API_KEY")
        .output()
        .expect("run workflow_dsl example");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    assert!(
        out.status.success(),
        "workflow_dsl run failed\nstdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
    assert!(
        combined.contains("Workflow DSL Example"),
        "workflow_dsl output missing expected marker\noutput:\n{}",
        combined
    );
    assert!(
        combined.contains("Built workflow with"),
        "workflow_dsl output missing build marker\noutput:\n{}",
        combined
    );
}
