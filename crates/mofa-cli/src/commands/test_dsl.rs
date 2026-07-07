//! `mofa test-dsl` command implementation

use crate::CliError;
use crate::cli::TestDslReportFormat;
use crate::output::OutputFormat;
use mofa_testing::{
    AgentRunArtifact, DslError, JsonFormatter, ReportFormatter, TestCaseResult, TestReport,
    TestStatus, TextFormatter, TestCaseDsl, assertion_error_from_outcomes,
    collect_assertion_outcomes, execute_test_case,
};
use serde::Serialize;
use serde_json::json;
use std::path::Path;

#[derive(Debug, Serialize)]
struct TestDslSummary {
    name: String,
    success: bool,
    output_text: Option<String>,
    duration_ms: u128,
    tool_calls: Vec<String>,
    workspace_root: String,
}

/// Execute one TOML DSL test case through the testing runner.
pub async fn run(
    path: &Path,
    format: OutputFormat,
    artifact_out: Option<&Path>,
    report_out: Option<&Path>,
    report_format: TestDslReportFormat,
) -> Result<(), CliError> {
    let case = TestCaseDsl::from_toml_file(path).map_err(map_dsl_error)?;
    let result = execute_test_case(&case).await.map_err(map_dsl_error)?;
    let assertions = collect_assertion_outcomes(&case, &result);
    let artifact = AgentRunArtifact::from_run_result(&case, &result, assertions.clone());
    let report = build_report(&artifact);

    if let Some(artifact_out) = artifact_out {
        write_artifact(artifact_out, &artifact)?;
    }

    if let Some(report_out) = report_out {
        write_report(report_out, report_format, &report)?;
    }

    let summary = TestDslSummary {
        name: case.name,
        success: result.is_success(),
        output_text: result.output_text(),
        duration_ms: result.duration.as_millis(),
        tool_calls: result
            .metadata
            .tool_calls
            .iter()
            .map(|record| record.tool_name.clone())
            .collect(),
        workspace_root: result.metadata.workspace_root.display().to_string(),
    };

    match format {
        OutputFormat::Json => {
            let output = json!({
                "success": true,
                "case": summary,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        _ => {
            println!("case: {}", summary.name);
            println!("status: {}", if summary.success { "passed" } else { "failed" });
            if let Some(output_text) = &summary.output_text {
                println!("output: {}", output_text);
            }
            if !summary.tool_calls.is_empty() {
                println!("tool_calls: {}", summary.tool_calls.join(", "));
            }
            println!("duration_ms: {}", summary.duration_ms);
        }
    }

    if let Some(error) = assertion_error_from_outcomes(&assertions) {
        return Err(map_dsl_error(error));
    }

    Ok(())
}

fn build_report(artifact: &AgentRunArtifact) -> TestReport {
    let status = if artifact.status == "passed" {
        TestStatus::Passed
    } else {
        TestStatus::Failed
    };
    let error = artifact
        .runner_error
        .clone()
        .or_else(|| {
            artifact
                .assertions
                .iter()
                .find(|item| !item.passed)
                .map(|item| format!("assertion failed: {}", item.kind))
        });
    let metadata = vec![
        (
            "execution_id".to_string(),
            artifact.execution_id.clone(),
        ),
        (
            "workspace_root".to_string(),
            artifact.workspace_root.clone(),
        ),
        (
            "tool_calls".to_string(),
            artifact.tool_calls.len().to_string(),
        ),
    ];

    TestReport {
        suite_name: "dsl".to_string(),
        results: vec![TestCaseResult {
            name: artifact.case_name.clone(),
            status,
            duration: std::time::Duration::from_millis(artifact.duration_ms),
            error,
            metadata,
        }],
        total_duration: std::time::Duration::from_millis(artifact.duration_ms),
        timestamp: artifact.started_at_ms,
    }
}

fn write_artifact(path: &Path, artifact: &AgentRunArtifact) -> Result<(), CliError> {
    let body = serde_json::to_string_pretty(artifact)?;
    std::fs::write(path, body)?;
    Ok(())
}

fn write_report(path: &Path, format: TestDslReportFormat, report: &TestReport) -> Result<(), CliError> {
    let body = match format {
        TestDslReportFormat::Json => JsonFormatter.format(report),
        TestDslReportFormat::Text => TextFormatter.format(report),
    };
    std::fs::write(path, body)?;
    Ok(())
}

fn map_dsl_error(error: DslError) -> CliError {
    CliError::Other(format!("DSL test failed: {error}"))
}
