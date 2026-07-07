//! Agent-level assertions for [`mofa_foundation::react::ReActResult`].

use mofa_foundation::react::ReActResult;

use super::trace_summary;

/// Assert that the final answer contains the expected text.
#[track_caller]
pub fn assert_response_contains(result: &ReActResult, needle: &str) {
    if !result.answer.contains(needle) {
        panic!(
            "Expected final answer to contain {needle:?}, but actual answer was {:?}. Trace: {}",
            result.answer,
            trace_summary(result)
        );
    }
}

/// Assert that the final answer does not contain the given text.
#[track_caller]
pub fn assert_response_not_contains(result: &ReActResult, needle: &str) {
    if result.answer.contains(needle) {
        panic!(
            "Expected final answer to not contain {needle:?}, but actual answer was {:?}. Trace: {}",
            result.answer,
            trace_summary(result)
        );
    }
}