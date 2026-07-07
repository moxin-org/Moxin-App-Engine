//! EvalReport rendering — terminal table and JSON export.

use std::path::Path;

use anyhow::Result;
use comfy_table::{Attribute, Cell, Color, Table, presets::UTF8_FULL};

use crate::eval::runner::EvalReport;

/// Render an [`EvalReport`] as a formatted terminal table.
pub fn print_report(report: &EvalReport) {
    println!();
    println!("=== EvalReport: {} ===", report.dataset_name);
    println!("    ran at:  {}", report.ran_at.format("%Y-%m-%d %H:%M:%S UTC"));
    println!(
        "    cases:   {}   passed: {}   failed: {}   ({})",
        report.total_cases,
        report.passed,
        report.failed,
        report.pass_rate_pct(),
    );
    println!("    score:   {:.3}", report.overall_score);
    println!();

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        Cell::new("case id").add_attribute(Attribute::Bold),
        Cell::new("result").add_attribute(Attribute::Bold),
        Cell::new("score").add_attribute(Attribute::Bold),
        Cell::new("wall time").add_attribute(Attribute::Bold),
        Cell::new("actual output").add_attribute(Attribute::Bold),
    ]);

    for r in &report.results {
        let (result_cell, score_cell) = if r.passed {
            (
                Cell::new("PASS").fg(Color::Green).add_attribute(Attribute::Bold),
                Cell::new(format!("{:.3}", r.score)).fg(Color::Green),
            )
        } else {
            (
                Cell::new("FAIL").fg(Color::Red).add_attribute(Attribute::Bold),
                Cell::new(format!("{:.3}", r.score)).fg(Color::Red),
            )
        };

        let output_preview = r
            .actual_output
            .as_deref()
            .unwrap_or("-")
            .chars()
            .take(60)
            .collect::<String>();

        table.add_row(vec![
            Cell::new(&r.case_id),
            result_cell,
            score_cell,
            Cell::new(format!("{}ms", r.wall_time.as_millis())),
            Cell::new(output_preview),
        ]);
    }

    println!("{table}");
    println!();
}

/// Write an [`EvalReport`] as JSON to the given path.
pub fn write_json_report(report: &EvalReport, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json)?;
    println!("report written to: {}", path.display());
    Ok(())
}
