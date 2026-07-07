#![allow(missing_docs)]

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("resolve workspace root")
}

#[test]
fn examples_dsl_regression_from_root_wrapper() {
    let root = workspace_root();
    let manifest = root.join("examples").join("cli_production_smoke").join("Cargo.toml");

    let out = Command::new("cargo")
        .arg("test")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--test")
        .arg("examples_dsl_regression")
        .current_dir(&root)
        .output()
        .expect("run examples_dsl_regression smoke test");

    assert!(
        out.status.success(),
        "examples_dsl_regression wrapper failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
