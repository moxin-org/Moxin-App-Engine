use std::panic::{AssertUnwindSafe, catch_unwind};

use mofa_foundation::react::{ReActResult, ReActStep};
use mofa_testing::assert::{
    assert_response_contains, assert_response_not_contains, assert_tool_never_before,
    assert_tool_not_used, assert_tool_order, assert_tool_used, assert_tool_used_n_times,
};

fn sample_result() -> ReActResult {
    ReActResult::success(
        "task-1",
        "Summarize Rust async traits",
        "Summary ready with cited docs.",
        vec![
            ReActStep::thought("Need to gather relevant material", 1),
            ReActStep::action("search", "Rust async traits", 2),
            ReActStep::observation("Collected two official references", 3),
            ReActStep::action("summarize", "official references", 4),
            ReActStep::final_answer("Summary ready with cited docs.", 5),
        ],
        2,
        84,
    )
}

fn out_of_order_result() -> ReActResult {
    ReActResult::success(
        "task-2",
        "Check ordering",
        "Done",
        vec![
            ReActStep::action("summarize", "draft", 1),
            ReActStep::action("search", "docs", 2),
            ReActStep::final_answer("Done", 3),
        ],
        2,
        21,
    )
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(message) => (*message).to_string(),
            Err(_) => "<non-string panic payload>".to_string(),
        },
    }
}

#[test]
fn tool_usage_assertions_pass_for_expected_trace() {
    let result = sample_result();

    assert_tool_used(&result, "search");
    assert_tool_used_n_times(&result, "search", 1);
    assert_tool_not_used(&result, "calculator");
}

#[test]
fn tool_order_assertions_pass_for_expected_trace() {
    let result = sample_result();

    assert_tool_order(&result, "search", "summarize");
    assert_tool_never_before(&result, "summarize", "search");
}

#[test]
fn response_assertions_pass_for_expected_answer() {
    let result = sample_result();

    assert_response_contains(&result, "cited docs");
    assert_response_not_contains(&result, "unsafe shell command");
}

#[test]
fn tool_order_failure_message_is_human_readable() {
    let result = out_of_order_result();
    let panic = catch_unwind(AssertUnwindSafe(|| {
        assert_tool_order(&result, "search", "summarize");
    }))
    .expect_err("assertion should fail for reversed tool order");

    let message = panic_message(panic);
    assert!(message.contains("search"));
    assert!(message.contains("summarize"));
    assert!(message.contains("Trace:"));
}

#[test]
fn response_failure_message_is_human_readable() {
    let result = sample_result();
    let panic = catch_unwind(AssertUnwindSafe(|| {
        assert_response_not_contains(&result, "cited docs");
    }))
    .expect_err("assertion should fail when forbidden content is present");

    let message = panic_message(panic);
    assert!(message.contains("cited docs"));
    assert!(message.contains("Trace:"));
}