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
fn examples_compile_from_root_wrapper() {
    let root = workspace_root();
    let manifest = root.join("examples").join("cli_production_smoke").join("Cargo.toml");

    let out = Command::new("cargo")
        .arg("test")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--test")
        .arg("examples_compile")
        .current_dir(&root)
        .output()
        .expect("run examples_compile smoke test");

    assert!(
        out.status.success(),
        "examples_compile wrapper failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
