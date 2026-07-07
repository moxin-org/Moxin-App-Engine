//! Tool-level assertions for [`mofa_foundation::react::ReActResult`].

use mofa_foundation::react::ReActResult;

use super::{observed_tools, tool_positions, trace_summary};

/// Assert that a tool appears at least once in the ReAct trace.
#[track_caller]
pub fn assert_tool_used(result: &ReActResult, tool_name: &str) {
    let positions = tool_positions(result, tool_name);
    if positions.is_empty() {
        panic!(
            "Expected tool '{tool_name}' to be used at least once, but it was never used. \
Observed tools: {:?}. Trace: {}",
            observed_tools(result),
            trace_summary(result)
        );
    }
}

/// Assert that a tool never appears in the ReAct trace.
#[track_caller]
pub fn assert_tool_not_used(result: &ReActResult, tool_name: &str) {
    let positions = tool_positions(result, tool_name);
    if !positions.is_empty() {
        panic!(
            "Expected tool '{tool_name}' to never be used, but it appeared at steps {:?}. \
Observed tools: {:?}. Trace: {}",
            positions,
            observed_tools(result),
            trace_summary(result)
        );
    }
}

/// Assert that a tool appears exactly `expected_count` times in the ReAct trace.
#[track_caller]
pub fn assert_tool_used_n_times(result: &ReActResult, tool_name: &str, expected_count: usize) {
    let positions = tool_positions(result, tool_name);
    let actual_count = positions.len();

    if actual_count != expected_count {
        panic!(
            "Expected tool '{tool_name}' to be used {expected_count} time(s), but it was used \
{actual_count} time(s) at steps {:?}. Observed tools: {:?}. Trace: {}",
            positions,
            observed_tools(result),
            trace_summary(result)
        );
    }
}

/// Assert that at least one call to `first` occurs before a call to `second`.
#[track_caller]
pub fn assert_tool_order(result: &ReActResult, first: &str, second: &str) {
    let first_positions = tool_positions(result, first);
    let second_positions = tool_positions(result, second);

    if first_positions.is_empty() {
        panic!(
            "Expected tool '{first}' to appear before '{second}', but '{first}' was never used. \
Observed tools: {:?}. Trace: {}",
            observed_tools(result),
            trace_summary(result)
        );
    }

    if second_positions.is_empty() {
        panic!(
            "Expected tool '{first}' to appear before '{second}', but '{second}' was never used. \
Observed tools: {:?}. Trace: {}",
            observed_tools(result),
            trace_summary(result)
        );
    }

    let ordered = first_positions
        .iter()
        .any(|first_step| second_positions.iter().any(|second_step| first_step < second_step));

    if !ordered {
        panic!(
            "Expected tool '{first}' to be called before '{second}', but observed positions were \
{first:?} -> {:?} and {second:?} -> {:?}. Trace: {}",
            first_positions,
            second_positions,
            trace_summary(result)
        );
    }
}

/// Assert that `forbidden_before` never appears before the first call to `reference`.
#[track_caller]
pub fn assert_tool_never_before(
    result: &ReActResult,
    forbidden_before: &str,
    reference: &str,
) {
    let forbidden_positions = tool_positions(result, forbidden_before);
    let reference_positions = tool_positions(result, reference);

    if reference_positions.is_empty() {
        panic!(
            "Expected reference tool '{reference}' to appear in the trace, but it was never used. \
Observed tools: {:?}. Trace: {}",
            observed_tools(result),
            trace_summary(result)
        );
    }

    let first_reference = reference_positions[0];
    let violating_positions: Vec<usize> = forbidden_positions
        .into_iter()
        .filter(|position| *position < first_reference)
        .collect();

    if !violating_positions.is_empty() {
        panic!(
            "Expected tool '{forbidden_before}' to never appear before '{reference}', but it \
appeared earlier at steps {:?} while '{reference}' first appeared at step {}. Trace: {}",
            violating_positions,
            first_reference,
            trace_summary(result)
        );
    }
}