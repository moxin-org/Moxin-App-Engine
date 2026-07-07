#![allow(missing_docs)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("resolve workspace root")
}

fn parse_example_members() -> Vec<String> {
    let root = workspace_root();
    let cargo_toml = root.join("examples").join("Cargo.toml");
    let content = fs::read_to_string(&cargo_toml).expect("read examples/Cargo.toml");

    let mut members = Vec::new();
    let mut in_members = false;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.starts_with("members") && line.ends_with('[') {
            in_members = true;
            continue;
        }

        if in_members {
            if line == "]" {
                break;
            }

            if let Some(stripped) = line.strip_prefix('"') {
                if let Some(end_quote) = stripped.find('"') {
                    members.push(stripped[..end_quote].to_string());
                }
            }
        }
    }

    members
}

fn cargo_check_examples_workspace(selected_packages: &[String], excluded_packages: &[String]) {
    let root = workspace_root();
    let manifest = root.join("examples").join("Cargo.toml");

    let out = Command::new("cargo")
        .arg("check")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--workspace")
        .args(
            excluded_packages
                .iter()
                .flat_map(|pkg| ["--exclude", pkg.as_str()]),
        )
        .current_dir(&root)
        .output()
        .expect("run cargo check");

    assert!(
        out.status.success(),
        "cargo check failed for selected examples\nselected: {:?}\nexcluded: {:?}\nstdout:\n{}\nstderr:\n{}",
        selected_packages,
        excluded_packages,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn examples_compile_manifest_driven_with_skip_list() {
    // Keep this list narrow and explicit for heavyweight or environment-sensitive examples.
    let skip_packages = vec!["integrations_demo".to_string(), "llm_tts_streaming".to_string()];

    let members = parse_example_members();
    assert!(
        members.len() >= 10,
        "expected broad examples workspace membership, got {}",
        members.len()
    );

    let selected: Vec<String> = members
        .iter()
        .filter(|m| !skip_packages.contains(*m))
        .cloned()
        .collect();

    assert!(
        selected.len() > 10,
        "selected examples are too narrow: {}",
        selected.len()
    );

    cargo_check_examples_workspace(&selected, &skip_packages);
}
