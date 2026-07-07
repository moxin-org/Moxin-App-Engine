//! Assertion helpers for behavior-oriented ReAct tests.
//!
//! These assertions operate on [`mofa_foundation::react::ReActResult`] so test
//! cases can verify tool usage and final responses without hand-writing trace
//! traversal logic in every test.

mod agent;
mod tool;

use mofa_foundation::react::{ReActResult, ReActStep, ReActStepType};

pub use agent::{assert_response_contains, assert_response_not_contains};
pub use tool::{
    assert_tool_never_before, assert_tool_not_used, assert_tool_order, assert_tool_used,
    assert_tool_used_n_times,
};

fn action_steps(result: &ReActResult) -> impl Iterator<Item = &ReActStep> {
    result
        .steps
        .iter()
        .filter(|step| matches!(step.step_type, ReActStepType::Action))
}

fn observed_tools(result: &ReActResult) -> Vec<&str> {
    action_steps(result)
        .filter_map(|step| step.tool_name.as_deref())
        .collect()
}

fn tool_positions(result: &ReActResult, tool_name: &str) -> Vec<usize> {
    action_steps(result)
        .filter_map(|step| {
            step.tool_name
                .as_deref()
                .filter(|name| *name == tool_name)
                .map(|_| step.step_number)
        })
        .collect()
}

fn trace_summary(result: &ReActResult) -> String {
    if result.steps.is_empty() {
        return "<no ReAct steps recorded>".to_string();
    }

    result
        .steps
        .iter()
        .map(format_step)
        .collect::<Vec<_>>()
        .join(" -> ")
}

fn format_step(step: &ReActStep) -> String {
    match step.step_type {
        ReActStepType::Thought => {
            format!("#{} Thought({})", step.step_number, preview(&step.content))
        }
        ReActStepType::Action => format!(
            "#{} Action({})",
            step.step_number,
            step.tool_name.as_deref().unwrap_or("<unknown>")
        ),
        ReActStepType::Observation => {
            format!("#{} Observation({})", step.step_number, preview(&step.content))
        }
        ReActStepType::FinalAnswer => format!(
            "#{} FinalAnswer({})",
            step.step_number,
            preview(&step.content)
        ),
    }
}

fn preview(value: &str) -> String {
    let trimmed = value.trim();
    let char_count = trimmed.chars().count();

    if char_count <= 40 {
        trimmed.to_string()
    } else {
        format!("{}…", trimmed.chars().take(40).collect::<String>())
    }
}