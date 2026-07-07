use clap::ValueEnum;
use colored::Colorize;
use serde::Serialize;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::error::{CliError, CliResult};
use crate::error::IntoCliReport as _;

#[derive(Clone, Copy, Debug, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum DoctorScenario {
    LocalDev,
    Ci,
    Docker,
    Release,
}

impl std::fmt::Display for DoctorScenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::LocalDev => "local-dev",
            Self::Ci => "ci",
            Self::Docker => "docker",
            Self::Release => "release",
        };
        write!(f, "{value}")
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DoctorSeverity {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Serialize)]
pub struct DoctorCheck {
    pub id: String,
    pub title: String,
    pub severity: DoctorSeverity,
    pub details: String,
    pub recommendation: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DoctorSummary {
    pub passed: usize,
    pub warnings: usize,
    pub failed: usize,
}

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub project_path: String,
    pub scenario: DoctorScenario,
    pub strict: bool,
    pub summary: DoctorSummary,
    pub checks: Vec<DoctorCheck>,
}

pub fn run(
    path: Option<PathBuf>,
    scenario: DoctorScenario,
    strict: bool,
    json: bool,
    fix: bool,
) -> CliResult<()> {
    let project_path = path.unwrap_or_else(|| PathBuf::from("."));
    let report = build_report(&project_path, scenario, strict, fix)?;

    if json {
        let json_str = serde_json::to_string_pretty(&report)
            .map_err(|e| CliError::SerializationError(e))?;
        println!("{}", json_str);
    } else {
        print_report(&report);
    }

    if strict && report.summary.failed > 0 {
        return Err(CliError::Other(
            format!(
                "doctor strict mode failed with {} failing checks",
                report.summary.failed
            )
        ).into());
    }

    Ok(())
}

fn build_report(
    project_path: &Path,
    scenario: DoctorScenario,
    strict: bool,
    fix: bool,
) -> CliResult<DoctorReport> {
    let mut checks = vec![
        check_project_directory(project_path),
        check_project_markers(project_path),
        check_agent_configuration(project_path),
        check_env_secret(scenario),
        check_gitignore(project_path),
        check_cargo_lock(project_path),
        check_test_layout(project_path),
    ];
    checks.extend(check_required_binaries(scenario));
    checks.extend(check_optional_binaries(scenario));
    checks.extend(check_runtime_directories(fix)?);

    let summary = summarize_checks(&checks);

    Ok(DoctorReport {
        project_path: project_path.display().to_string(),
        scenario,
        strict,
        summary,
        checks,
    })
}

fn check_project_directory(project_path: &Path) -> DoctorCheck {
    if project_path.exists() && project_path.is_dir() {
        DoctorCheck {
            id: "project-dir".to_string(),
            title: "Project directory exists".to_string(),
            severity: DoctorSeverity::Pass,
            details: format!("Found project directory: {}", project_path.display()),
            recommendation: None,
        }
    } else {
        DoctorCheck {
            id: "project-dir".to_string(),
            title: "Project directory exists".to_string(),
            severity: DoctorSeverity::Fail,
            details: format!("Project directory not found: {}", project_path.display()),
            recommendation: Some("Run from a valid project path or use --path <dir>.".to_string()),
        }
    }
}

fn check_project_markers(project_path: &Path) -> DoctorCheck {
    let markers = ["Cargo.toml", "agent.yml", "agent.yaml", "mofa.toml", ".git"];
    let found = markers
        .iter()
        .filter(|marker| project_path.join(marker).exists())
        .map(|marker| marker.to_string())
        .collect::<Vec<_>>();

    if found.is_empty() {
        DoctorCheck {
            id: "project-markers".to_string(),
            title: "Project markers".to_string(),
            severity: DoctorSeverity::Warn,
            details: "No typical MoFA project markers found in target path.".to_string(),
            recommendation: Some(
                "Expected markers include Cargo.toml, agent.yml, mofa.toml, or .git.".to_string(),
            ),
        }
    } else {
        DoctorCheck {
            id: "project-markers".to_string(),
            title: "Project markers".to_string(),
            severity: DoctorSeverity::Pass,
            details: format!("Detected markers: {}", found.join(", ")),
            recommendation: None,
        }
    }
}

fn check_agent_configuration(project_path: &Path) -> DoctorCheck {
    let candidates = ["agent.yml", "agent.yaml", "agent.toml", "agent.json"];
    let found = candidates
        .iter()
        .find_map(|name| project_path.join(name).exists().then_some(*name));

    match found {
        Some(file) => DoctorCheck {
            id: "agent-config".to_string(),
            title: "Agent configuration file".to_string(),
            severity: DoctorSeverity::Pass,
            details: format!("Found agent configuration: {file}"),
            recommendation: None,
        },
        None => DoctorCheck {
            id: "agent-config".to_string(),
            title: "Agent configuration file".to_string(),
            severity: DoctorSeverity::Warn,
            details: "No agent config file found in project root.".to_string(),
            recommendation: Some(
                "Run `mofa generate config` to create a starter config.".to_string(),
            ),
        },
    }
}

fn check_env_secret(scenario: DoctorScenario) -> DoctorCheck {
    let has_key = std::env::var("OPENAI_API_KEY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);

    let required = matches!(scenario, DoctorScenario::LocalDev | DoctorScenario::Release);
    if has_key {
        DoctorCheck {
            id: "openai-api-key".to_string(),
            title: "OPENAI_API_KEY".to_string(),
            severity: DoctorSeverity::Pass,
            details: "OPENAI_API_KEY is set.".to_string(),
            recommendation: None,
        }
    } else if required {
        DoctorCheck {
            id: "openai-api-key".to_string(),
            title: "OPENAI_API_KEY".to_string(),
            severity: DoctorSeverity::Fail,
            details: "OPENAI_API_KEY is required for local-dev/release scenarios.".to_string(),
            recommendation: Some(
                "Set OPENAI_API_KEY before running LLM-backed agents.".to_string(),
            ),
        }
    } else {
        DoctorCheck {
            id: "openai-api-key".to_string(),
            title: "OPENAI_API_KEY".to_string(),
            severity: DoctorSeverity::Warn,
            details: "OPENAI_API_KEY not set (acceptable for ci/docker checks).".to_string(),
            recommendation: Some(
                "Set OPENAI_API_KEY when running LLM execution paths.".to_string(),
            ),
        }
    }
}

fn check_gitignore(project_path: &Path) -> DoctorCheck {
    let gitignore = project_path.join(".gitignore");
    if !gitignore.exists() {
        return DoctorCheck {
            id: "gitignore-env".to_string(),
            title: ".env secret hygiene".to_string(),
            severity: DoctorSeverity::Warn,
            details: "No .gitignore found; cannot verify secret file exclusions.".to_string(),
            recommendation: Some("Add a .gitignore with `.env` entries.".to_string()),
        };
    }

    let content = std::fs::read_to_string(&gitignore).unwrap_or_default();
    let has_env_rule = content.lines().any(|line| {
        let line = line.trim();
        line == ".env" || line == ".env.*"
    });

    if has_env_rule {
        DoctorCheck {
            id: "gitignore-env".to_string(),
            title: ".env secret hygiene".to_string(),
            severity: DoctorSeverity::Pass,
            details: ".gitignore contains .env protection rules.".to_string(),
            recommendation: None,
        }
    } else {
        DoctorCheck {
            id: "gitignore-env".to_string(),
            title: ".env secret hygiene".to_string(),
            severity: DoctorSeverity::Warn,
            details: ".gitignore does not include .env patterns.".to_string(),
            recommendation: Some("Add `.env` and `.env.*` to .gitignore.".to_string()),
        }
    }
}

fn check_cargo_lock(project_path: &Path) -> DoctorCheck {
    let has_cargo = project_path.join("Cargo.toml").exists();
    if !has_cargo {
        return DoctorCheck {
            id: "cargo-lock".to_string(),
            title: "Cargo.lock consistency".to_string(),
            severity: DoctorSeverity::Pass,
            details: "No Cargo.toml in target path; Rust lockfile check skipped.".to_string(),
            recommendation: None,
        };
    }

    if project_path.join("Cargo.lock").exists() {
        DoctorCheck {
            id: "cargo-lock".to_string(),
            title: "Cargo.lock consistency".to_string(),
            severity: DoctorSeverity::Pass,
            details: "Cargo.lock present.".to_string(),
            recommendation: None,
        }
    } else {
        DoctorCheck {
            id: "cargo-lock".to_string(),
            title: "Cargo.lock consistency".to_string(),
            severity: DoctorSeverity::Warn,
            details: "Cargo.toml exists but Cargo.lock is missing.".to_string(),
            recommendation: Some(
                "Run `cargo generate-lockfile` for reproducible builds.".to_string(),
            ),
        }
    }
}

fn check_test_layout(project_path: &Path) -> DoctorCheck {
    let has_tests = project_path.join("tests").exists()
        || project_path.join("src").join("lib.rs").exists()
        || project_path.join("src").join("main.rs").exists();

    if has_tests {
        DoctorCheck {
            id: "test-layout".to_string(),
            title: "Test-ready project layout".to_string(),
            severity: DoctorSeverity::Pass,
            details: "Detected source/tests layout suitable for validation workflows.".to_string(),
            recommendation: None,
        }
    } else {
        DoctorCheck {
            id: "test-layout".to_string(),
            title: "Test-ready project layout".to_string(),
            severity: DoctorSeverity::Warn,
            details: "No common source/tests layout detected in target path.".to_string(),
            recommendation: Some(
                "Add src/ and tests/ to support practical validation flows.".to_string(),
            ),
        }
    }
}

fn check_required_binaries(scenario: DoctorScenario) -> Vec<DoctorCheck> {
    required_tool_list(scenario)
        .iter()
        .map(|binary| check_binary(binary, true))
        .collect()
}

fn check_optional_binaries(scenario: DoctorScenario) -> Vec<DoctorCheck> {
    optional_tool_list(scenario)
        .iter()
        .map(|binary| check_binary(binary, false))
        .collect()
}

fn required_tool_list(scenario: DoctorScenario) -> Vec<&'static str> {
    match scenario {
        DoctorScenario::LocalDev => vec!["cargo", "rustc"],
        DoctorScenario::Ci => vec!["cargo", "rustc", "git"],
        DoctorScenario::Docker => vec!["cargo", "rustc"],
        DoctorScenario::Release => vec!["cargo", "rustc", "git"],
    }
}

fn optional_tool_list(scenario: DoctorScenario) -> Vec<&'static str> {
    match scenario {
        DoctorScenario::LocalDev => vec!["python3", "uv", "docker"],
        DoctorScenario::Ci => vec!["python3"],
        DoctorScenario::Docker => vec!["docker", "python3"],
        DoctorScenario::Release => vec!["python3", "uv"],
    }
}

fn check_binary(binary: &str, required: bool) -> DoctorCheck {
    let exists = command_exists(binary);
    if exists {
        DoctorCheck {
            id: format!("bin-{binary}"),
            title: format!("Binary `{binary}`"),
            severity: DoctorSeverity::Pass,
            details: format!("`{binary}` is available on PATH."),
            recommendation: None,
        }
    } else if required {
        DoctorCheck {
            id: format!("bin-{binary}"),
            title: format!("Binary `{binary}`"),
            severity: DoctorSeverity::Fail,
            details: format!("Missing required binary `{binary}` on PATH."),
            recommendation: Some(format!("Install `{binary}` and ensure PATH is configured.")),
        }
    } else {
        DoctorCheck {
            id: format!("bin-{binary}"),
            title: format!("Binary `{binary}`"),
            severity: DoctorSeverity::Warn,
            details: format!("Optional binary `{binary}` not found on PATH."),
            recommendation: Some(format!(
                "Install `{binary}` if your workflow depends on it."
            )),
        }
    }
}

fn check_runtime_directories(fix: bool) -> CliResult<Vec<DoctorCheck>> {
    let mut checks = vec![];

    let config_dir = crate::utils::mofa_config_dir()?;
    let data_dir = crate::utils::mofa_data_dir()?;
    let cache_dir = crate::utils::mofa_cache_dir()?;

    let dirs = [
        ("config", config_dir),
        ("data", data_dir),
        ("cache", cache_dir),
    ];

    for (name, dir) in dirs {
        if dir.exists() {
            checks.push(check_directory_writable(name, &dir));
        } else if fix {
            std::fs::create_dir_all(&dir).map_err(CliError::from).into_report()?;
            checks.push(DoctorCheck {
                id: format!("runtime-dir-{name}"),
                title: format!("Runtime {name} directory"),
                severity: DoctorSeverity::Pass,
                details: format!("Created missing runtime directory: {}", dir.display()),
                recommendation: None,
            });
            checks.push(check_directory_writable(name, &dir));
        } else {
            checks.push(DoctorCheck {
                id: format!("runtime-dir-{name}"),
                title: format!("Runtime {name} directory"),
                severity: DoctorSeverity::Warn,
                details: format!("Runtime directory missing: {}", dir.display()),
                recommendation: Some(
                    "Use --fix to create missing runtime directories.".to_string(),
                ),
            });
        }
    }

    Ok(checks)
}

fn check_directory_writable(name: &str, dir: &Path) -> DoctorCheck {
    let probe_path = dir.join(".mofa-doctor-write-test");
    let write_result = std::fs::write(&probe_path, b"ok");
    if write_result.is_ok() {
        let _ = std::fs::remove_file(&probe_path);
        DoctorCheck {
            id: format!("runtime-dir-{name}-writable"),
            title: format!("Runtime {name} directory writable"),
            severity: DoctorSeverity::Pass,
            details: format!("Directory is writable: {}", dir.display()),
            recommendation: None,
        }
    } else {
        DoctorCheck {
            id: format!("runtime-dir-{name}-writable"),
            title: format!("Runtime {name} directory writable"),
            severity: DoctorSeverity::Fail,
            details: format!("Directory is not writable: {}", dir.display()),
            recommendation: Some("Fix permissions for MoFA runtime directory.".to_string()),
        }
    }
}

fn summarize_checks(checks: &[DoctorCheck]) -> DoctorSummary {
    let passed = checks
        .iter()
        .filter(|check| check.severity == DoctorSeverity::Pass)
        .count();
    let warnings = checks
        .iter()
        .filter(|check| check.severity == DoctorSeverity::Warn)
        .count();
    let failed = checks
        .iter()
        .filter(|check| check.severity == DoctorSeverity::Fail)
        .count();

    DoctorSummary {
        passed,
        warnings,
        failed,
    }
}

fn print_report(report: &DoctorReport) {
    println!("{} MoFA Doctor Report", "→".green());
    println!("  Project: {}", report.project_path.cyan());
    println!("  Scenario: {}", report.scenario.to_string().yellow());
    println!(
        "  Strict mode: {}",
        if report.strict { "on" } else { "off" }
    );
    println!();

    for check in &report.checks {
        let (icon, colorized_title) = match check.severity {
            DoctorSeverity::Pass => ("✓".green(), check.title.green()),
            DoctorSeverity::Warn => ("!".yellow(), check.title.yellow()),
            DoctorSeverity::Fail => ("✗".red(), check.title.red()),
        };

        println!("{} {} [{}]", icon, colorized_title, check.id);
        println!("    {}", check.details);
        if let Some(recommendation) = &check.recommendation {
            println!("    Recommendation: {}", recommendation);
        }
    }

    println!();
    println!(
        "Summary: {} passed, {} warnings, {} failed",
        report.summary.passed.to_string().green(),
        report.summary.warnings.to_string().yellow(),
        report.summary.failed.to_string().red()
    );
}

fn command_exists(binary: &str) -> bool {
    command_exists_in_path(binary, std::env::var_os("PATH").as_deref())
}

fn command_exists_in_path(binary: &str, path_var: Option<&OsStr>) -> bool {
    let Some(path_var) = path_var else {
        return false;
    };

    std::env::split_paths(path_var).any(|dir| {
        let direct = dir.join(binary);
        if direct.is_file() {
            return true;
        }

        #[cfg(windows)]
        {
            for ext in [".exe", ".cmd", ".bat"] {
                let with_ext = dir.join(format!("{binary}{ext}"));
                if with_ext.is_file() {
                    return true;
                }
            }
        }

        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn marker_check_warns_when_no_markers() {
        let dir = tempdir().unwrap();
        let check = check_project_markers(dir.path());
        assert_eq!(check.severity, DoctorSeverity::Warn);
    }

    #[test]
    fn marker_check_passes_when_cargo_toml_exists() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname='x'\n").unwrap();
        let check = check_project_markers(dir.path());
        assert_eq!(check.severity, DoctorSeverity::Pass);
    }

    #[test]
    fn agent_config_warns_when_missing() {
        let dir = tempdir().unwrap();
        let check = check_agent_configuration(dir.path());
        assert_eq!(check.severity, DoctorSeverity::Warn);
    }

    #[test]
    fn agent_config_passes_when_present() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("agent.yml"), "agent:\n  id: a\n  name: b\n").unwrap();
        let check = check_agent_configuration(dir.path());
        assert_eq!(check.severity, DoctorSeverity::Pass);
    }

    #[test]
    fn gitignore_warns_when_env_not_ignored() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "target\n").unwrap();
        let check = check_gitignore(dir.path());
        assert_eq!(check.severity, DoctorSeverity::Warn);
    }

    #[test]
    fn gitignore_passes_when_env_ignored() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "target\n.env\n.env.*\n").unwrap();
        let check = check_gitignore(dir.path());
        assert_eq!(check.severity, DoctorSeverity::Pass);
    }

    #[test]
    fn cargo_lock_warns_when_missing_lock() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname='x'\n").unwrap();
        let check = check_cargo_lock(dir.path());
        assert_eq!(check.severity, DoctorSeverity::Warn);
    }

    #[test]
    fn cargo_lock_passes_when_lock_present() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname='x'\n").unwrap();
        std::fs::write(dir.path().join("Cargo.lock"), "").unwrap();
        let check = check_cargo_lock(dir.path());
        assert_eq!(check.severity, DoctorSeverity::Pass);
    }

    #[test]
    fn required_tools_include_ci_git() {
        let tools = required_tool_list(DoctorScenario::Ci);
        assert!(tools.contains(&"git"));
        assert!(tools.contains(&"cargo"));
    }

    #[test]
    fn optional_tools_for_local_dev_include_uv() {
        let tools = optional_tool_list(DoctorScenario::LocalDev);
        assert!(tools.contains(&"uv"));
    }

    #[test]
    fn command_exists_in_path_detects_binary() {
        let dir = tempdir().unwrap();
        let bin = dir.path().join("toolx");
        std::fs::write(&bin, "#!/bin/sh\necho ok\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&bin).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&bin, perms).unwrap();
        }
        let path = OsStr::new(dir.path().as_os_str());
        assert!(command_exists_in_path("toolx", Some(path)));
    }

    #[test]
    fn command_exists_in_path_handles_missing_path() {
        assert!(!command_exists_in_path("cargo", None));
    }

    #[test]
    fn summary_counts_severity() {
        let checks = vec![
            DoctorCheck {
                id: "a".to_string(),
                title: "A".to_string(),
                severity: DoctorSeverity::Pass,
                details: String::new(),
                recommendation: None,
            },
            DoctorCheck {
                id: "b".to_string(),
                title: "B".to_string(),
                severity: DoctorSeverity::Warn,
                details: String::new(),
                recommendation: None,
            },
            DoctorCheck {
                id: "c".to_string(),
                title: "C".to_string(),
                severity: DoctorSeverity::Fail,
                details: String::new(),
                recommendation: None,
            },
        ];
        let summary = summarize_checks(&checks);
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.warnings, 1);
        assert_eq!(summary.failed, 1);
    }

    #[test]
    fn build_report_collects_checks() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname='x'\n").unwrap();
        std::fs::write(dir.path().join("Cargo.lock"), "").unwrap();
        std::fs::write(dir.path().join(".gitignore"), ".env\n.env.*\n").unwrap();
        std::fs::write(dir.path().join("agent.yml"), "agent:\n  id: a\n  name: b\n").unwrap();

        let report =
            build_report(dir.path(), DoctorScenario::Ci, false, false).expect("build report");
        assert!(report.checks.len() >= 10);
    }

    #[test]
    fn local_dev_requires_openai_key() {
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
        let check = check_env_secret(DoctorScenario::LocalDev);
        assert_eq!(check.severity, DoctorSeverity::Fail);
    }

    #[test]
    fn ci_without_openai_key_is_warning() {
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
        let check = check_env_secret(DoctorScenario::Ci);
        assert_eq!(check.severity, DoctorSeverity::Warn);
    }

    #[test]
    fn env_with_openai_key_passes() {
        unsafe { std::env::set_var("OPENAI_API_KEY", "dummy") };
        let check = check_env_secret(DoctorScenario::LocalDev);
        assert_eq!(check.severity, DoctorSeverity::Pass);
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
    }
}
