//! `mofa test-dsl` command implementation

use crate::CliError;
use crate::cli::TestDslReportFormat;
use crate::output::OutputFormat;
use mofa_testing::{
    DslError, JsonFormatter, ReportFormatter, TestCaseResult, TestReport, TestStatus,
    TextFormatter, run_test_case, TestCaseDsl,
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
    report_out: Option<&Path>,
    report_format: TestDslReportFormat,
) -> Result<(), CliError> {
    let case = TestCaseDsl::from_toml_file(path).map_err(map_dsl_error)?;
    let result = run_test_case(&case).await.map_err(map_dsl_error)?;
    let report = build_report(&case.name, &result);

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

    Ok(())
}

fn build_report(case_name: &str, result: &mofa_testing::AgentRunResult) -> TestReport {
    let status = if result.is_success() {
        TestStatus::Passed
    } else {
        TestStatus::Failed
    };
    let error = result.error.as_ref().map(ToString::to_string);
    let metadata = vec![
        (
            "execution_id".to_string(),
            result.metadata.execution_id.clone(),
        ),
        (
            "workspace_root".to_string(),
            result.metadata.workspace_root.display().to_string(),
        ),
        (
            "tool_calls".to_string(),
            result.metadata.tool_calls.len().to_string(),
        ),
    ];

    TestReport {
        suite_name: "dsl".to_string(),
        results: vec![TestCaseResult {
            name: case_name.to_string(),
            status,
            duration: result.duration,
            error,
            metadata,
        }],
        total_duration: result.duration,
        timestamp: result.metadata.started_at.timestamp_millis() as u64,
    }
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
