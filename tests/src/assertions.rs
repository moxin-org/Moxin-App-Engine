//! Assertion helpers for mock verification and structured agent-run results.
//!
//! Keeps tests focused on behavior instead of hand-parsing run metadata.

use std::time::Duration;

use mofa_kernel::agent::AgentState;
use mofa_runtime::runner::RunnerState;
use regex::Regex;
use serde_json::Value;

use crate::agent_runner::{AgentRunResult, ToolCallRecord, WorkspaceFileSnapshot, WorkspaceSnapshot};

/// Assert the named tool was called at least once.
///
/// # Example
/// ```ignore
/// assert_tool_called!(tool, "search");
/// ```
#[macro_export]
macro_rules! assert_tool_called {
    ($tool:expr, $name:expr) => {{
        use mofa_foundation::agent::components::tool::SimpleTool as _;
        assert_eq!(
            $tool.name(),
            $name,
            "Tool name mismatch: expected '{}', got '{}'",
            $name,
            $tool.name()
        );
        let count = $tool.call_count().await;
        assert!(
            count > 0,
            "Expected tool '{}' to be called at least once, but it was never called",
            $name
        );
    }};
}

/// Assert the tool received a call with the given JSON arguments.
///
/// # Example
/// ```ignore
/// assert_tool_called_with!(tool, json!({"query": "rust"}));
/// ```
#[macro_export]
macro_rules! assert_tool_called_with {
    ($tool:expr, $args:expr) => {{
        let history = $tool.history().await;
        let expected = $args;
        assert!(
            history.iter().any(|h| h.arguments == expected),
            "Expected tool to be called with arguments {:?}, but no matching call found in history: {:?}",
            expected,
            history.iter().map(|h| &h.arguments).collect::<Vec<_>>()
        );
    }};
}

/// Assert the LLM backend's `infer()` was called at least once.
///
/// # Example
/// ```ignore
/// assert_infer_called!(backend);
/// ```
#[macro_export]
macro_rules! assert_infer_called {
    ($backend:expr) => {{
        let count = $backend.call_count();
        assert!(
            count > 0,
            "Expected LLM backend infer() to be called at least once, but call_count is 0"
        );
    }};
}

/// Assert a bus message was sent by the given sender.
///
/// # Example
/// ```ignore
/// assert_bus_message_sent!(bus, "agent-1");
/// ```
#[macro_export]
macro_rules! assert_bus_message_sent {
    ($bus:expr, $sender_id:expr) => {{
        let messages = $bus.captured_messages.read().await;
        let found = messages.iter().any(|(sid, _, _)| sid == $sender_id);
        assert!(
            found,
            "Expected a message from sender '{}', but none found among {} captured message(s)",
            $sender_id,
            messages.len()
        );
    }};
}

/// Assert a session's messages match the expected (role, content) pairs.
pub fn assert_session_messages(
    session: &mofa_foundation::agent::session::Session,
    expected: &[(&str, &str)],
) {
    assert_eq!(
        session.messages.len(),
        expected.len(),
        "Expected {} session messages, got {}",
        expected.len(),
        session.messages.len()
    );

    for (idx, (role, content)) in expected.iter().enumerate() {
        let msg = &session.messages[idx];
        assert_eq!(
            msg.role, *role,
            "Expected role '{}' at index {}, got '{}'",
            role, idx, msg.role
        );
        assert_eq!(
            msg.content, *content,
            "Expected content '{}' at index {}, got '{}'",
            content, idx, msg.content
        );
    }
}

/// Assert that a structured run completed without an execution error.
pub fn assert_run_success(result: &AgentRunResult) {
    assert!(
        result.error.is_none(),
        "Expected run to succeed, but got error: {:?}",
        result.error
    );
}

/// Assert that a structured run failed with an error containing the expected text.
pub fn assert_run_failure_contains(result: &AgentRunResult, expected: &str) {
    let error = result
        .error
        .as_ref()
        .unwrap_or_else(|| panic!("Expected run to fail with '{expected}', but it succeeded"));
    let actual = error.to_string();
    assert!(
        actual.contains(expected),
        "Expected error containing '{}', got '{}'",
        expected,
        actual
    );
}

/// Assert that the run metadata includes a non-empty execution id.
pub fn assert_execution_id_present(result: &AgentRunResult) {
    assert!(
        !result.metadata.execution_id.trim().is_empty(),
        "Expected non-empty execution id"
    );
}

/// Assert that the captured session id matches the expected value.
pub fn assert_session_id_equals(result: &AgentRunResult, expected: &str) {
    let actual = result
        .metadata
        .session_id
        .as_deref()
        .unwrap_or_else(|| panic!("Expected session id '{}', but none was captured", expected));
    assert_eq!(
        actual, expected,
        "Expected session id '{}', got '{}'",
        expected, actual
    );
}

/// Assert that the runner state before execution matches the expected state.
pub fn assert_runner_state_before(result: &AgentRunResult, expected: RunnerState) {
    assert_eq!(
        result.metadata.runner_state_before, expected,
        "Expected runner state before {:?}, got {:?}",
        expected, result.metadata.runner_state_before
    );
}

/// Assert that the runner state after execution matches the expected state.
pub fn assert_runner_state_after(result: &AgentRunResult, expected: RunnerState) {
    assert_eq!(
        result.metadata.runner_state_after, expected,
        "Expected runner state after {:?}, got {:?}",
        expected, result.metadata.runner_state_after
    );
}

/// Assert that total executions changed by the expected delta.
pub fn assert_total_executions_delta(result: &AgentRunResult, expected_delta: u64) {
    let before = result.metadata.runner_stats_before.total_executions;
    let after = result.metadata.runner_stats_after.total_executions;
    assert_eq!(
        after.saturating_sub(before),
        expected_delta,
        "Expected total executions delta {}, got {} (before {}, after {})",
        expected_delta,
        after.saturating_sub(before),
        before,
        after
    );
}

/// Assert that successful executions changed by the expected delta.
pub fn assert_successful_executions_delta(result: &AgentRunResult, expected_delta: u64) {
    let before = result.metadata.runner_stats_before.successful_executions;
    let after = result.metadata.runner_stats_after.successful_executions;
    assert_eq!(
        after.saturating_sub(before),
        expected_delta,
        "Expected successful executions delta {}, got {} (before {}, after {})",
        expected_delta,
        after.saturating_sub(before),
        before,
        after
    );
}

/// Assert that failed executions changed by the expected delta.
pub fn assert_failed_executions_delta(result: &AgentRunResult, expected_delta: u64) {
    let before = result.metadata.runner_stats_before.failed_executions;
    let after = result.metadata.runner_stats_after.failed_executions;
    assert_eq!(
        after.saturating_sub(before),
        expected_delta,
        "Expected failed executions delta {}, got {} (before {}, after {})",
        expected_delta,
        after.saturating_sub(before),
        before,
        after
    );
}

/// Assert that the runner recorded a last execution time.
pub fn assert_last_execution_time_recorded(result: &AgentRunResult) {
    assert!(
        result.metadata.runner_stats_after.last_execution_time_ms.is_some(),
        "Expected runner stats to record last execution time, got {:?}",
        result.metadata.runner_stats_after
    );
}

/// Assert that the agent state before execution matches the expected state.
pub fn assert_agent_state_before(result: &AgentRunResult, expected: AgentState) {
    assert_eq!(
        result.metadata.agent_state_before, expected,
        "Expected agent state before {:?}, got {:?}",
        expected, result.metadata.agent_state_before
    );
}

/// Assert that the agent state after execution matches the expected state.
pub fn assert_agent_state_after(result: &AgentRunResult, expected: AgentState) {
    assert_eq!(
        result.metadata.agent_state_after, expected,
        "Expected agent state after {:?}, got {:?}",
        expected, result.metadata.agent_state_after
    );
}

/// Assert that the final text output contains the expected substring.
pub fn assert_output_contains(result: &AgentRunResult, expected: &str) {
    let output = result
        .output_text()
        .unwrap_or_else(|| panic!("Expected output containing '{expected}', but run had no output"));
    assert!(
        output.contains(expected),
        "Expected output containing '{}', got '{}'",
        expected,
        output
    );
}

/// Assert that the final text output matches the given regex.
pub fn assert_output_matches_regex(result: &AgentRunResult, pattern: &str) {
    let output = result
        .output_text()
        .unwrap_or_else(|| panic!("Expected output matching '{pattern}', but run had no output"));
    let regex = Regex::new(pattern)
        .unwrap_or_else(|err| panic!("Invalid regex pattern '{}': {}", pattern, err));
    assert!(
        regex.is_match(&output),
        "Expected output matching '{}', got '{}'",
        pattern,
        output
    );
}

/// Assert that the run duration is less than or equal to the threshold.
pub fn assert_duration_under(result: &AgentRunResult, max: Duration) {
    assert!(
        result.duration <= max,
        "Expected run duration <= {:?}, got {:?}",
        max,
        result.duration
    );
}

/// Assert that the last captured LLM request contains the expected text.
pub fn assert_llm_request_contains(result: &AgentRunResult, expected: &str) {
    let request = result.metadata.llm_last_request.as_ref().unwrap_or_else(|| {
        panic!("Expected LLM request containing '{}', but none was captured", expected)
    });
    let messages = request
        .messages
        .iter()
        .filter_map(|msg| msg.content.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        messages.contains(expected),
        "Expected LLM request containing '{}', got '{}'",
        expected,
        messages
    );
}

/// Assert that the last captured LLM request has the expected number of messages.
pub fn assert_llm_request_message_count(result: &AgentRunResult, expected: usize) {
    let request = result.metadata.llm_last_request.as_ref().unwrap_or_else(|| {
        panic!(
            "Expected LLM request with {} messages, but none was captured",
            expected
        )
    });
    assert_eq!(
        request.messages.len(),
        expected,
        "Expected LLM request to have {} messages, got {}",
        expected,
        request.messages.len()
    );
}

/// Assert that the last captured LLM request includes a system message.
pub fn assert_llm_request_has_system_message(result: &AgentRunResult) {
    let request = result
        .metadata
        .llm_last_request
        .as_ref()
        .unwrap_or_else(|| panic!("Expected LLM request with system message, but none was captured"));
    let has_system = request.messages.iter().any(|msg| msg.role == "system");
    assert!(
        has_system,
        "Expected LLM request to include a system message, got roles {:?}",
        request
            .messages
            .iter()
            .map(|msg| msg.role.as_str())
            .collect::<Vec<_>>()
    );
}

/// Assert that the last captured LLM response contains the expected text.
pub fn assert_llm_response_contains(result: &AgentRunResult, expected: &str) {
    let response = result.metadata.llm_last_response.as_ref().unwrap_or_else(|| {
        panic!(
            "Expected LLM response containing '{}', but none was captured",
            expected
        )
    });
    let content = response.content.as_deref().unwrap_or("");
    assert!(
        content.contains(expected),
        "Expected LLM response containing '{}', got '{}'",
        expected,
        content
    );
}

/// Assert that the last captured LLM response content equals the expected text.
pub fn assert_llm_response_equals(result: &AgentRunResult, expected: &str) {
    let response = result.metadata.llm_last_response.as_ref().unwrap_or_else(|| {
        panic!(
            "Expected LLM response equal to '{}', but none was captured",
            expected
        )
    });
    let content = response.content.as_deref().unwrap_or("");
    assert_eq!(
        content, expected,
        "Expected LLM response content '{}', got '{}'",
        expected, content
    );
}

/// Assert that the last captured LLM response includes at least one tool call.
pub fn assert_llm_response_has_tool_calls(result: &AgentRunResult) {
    let response = result
        .metadata
        .llm_last_response
        .as_ref()
        .unwrap_or_else(|| panic!("Expected LLM response with tool calls, but none was captured"));
    let tool_calls = response.tool_calls.as_ref().map(|calls| calls.len()).unwrap_or(0);
    assert!(
        tool_calls > 0,
        "Expected LLM response to include tool calls, got {:?}",
        response.tool_calls
    );
}

/// Assert that the last captured LLM response does not include any tool calls.
pub fn assert_llm_response_has_no_tool_calls(result: &AgentRunResult) {
    let response = result.metadata.llm_last_response.as_ref().unwrap_or_else(|| {
        panic!("Expected LLM response without tool calls, but none was captured")
    });
    let tool_calls = response.tool_calls.as_ref().map(|calls| calls.len()).unwrap_or(0);
    assert_eq!(
        tool_calls, 0,
        "Expected LLM response to have no tool calls, got {:?}",
        response.tool_calls
    );
}

/// Assert that the captured session snapshot has the expected length.
pub fn assert_session_len(result: &AgentRunResult, expected: usize) {
    let session = result.metadata.session_snapshot.as_ref().unwrap_or_else(|| {
        panic!(
            "Expected session snapshot with {} messages, but none was captured",
            expected
        )
    });
    assert_eq!(
        session.len(),
        expected,
        "Expected {} session messages, got {}",
        expected,
        session.len()
    );
}

/// Assert that the captured session snapshot has at least the expected number of messages.
pub fn assert_session_len_at_least(result: &AgentRunResult, minimum: usize) {
    let session = result.metadata.session_snapshot.as_ref().unwrap_or_else(|| {
        panic!(
            "Expected session snapshot with at least {} messages, but none was captured",
            minimum
        )
    });
    assert!(
        session.len() >= minimum,
        "Expected at least {} session messages, got {}",
        minimum,
        session.len()
    );
}

/// Assert that the captured session contains a message with the expected role/content.
pub fn assert_session_contains(
    result: &AgentRunResult,
    role: &str,
    expected_content: &str,
) {
    let session = result.metadata.session_snapshot.as_ref().unwrap_or_else(|| {
        panic!(
            "Expected session message '{}:{}', but no snapshot was captured",
            role, expected_content
        )
    });
    let found = session
        .messages
        .iter()
        .any(|msg| msg.role == role && msg.content == expected_content);
    assert!(
        found,
        "Expected session to contain role '{}' with content '{}', got {:?}",
        role,
        expected_content,
        session
            .messages
            .iter()
            .map(|msg| (&msg.role, &msg.content))
            .collect::<Vec<_>>()
    );
}

/// Assert that the captured session contains the expected messages in order.
pub fn assert_session_contains_in_order(
    result: &AgentRunResult,
    expected: &[(&str, &str)],
) {
    let session = result.metadata.session_snapshot.as_ref().unwrap_or_else(|| {
        panic!(
            "Expected session to contain {:?} in order, but no snapshot was captured",
            expected
        )
    });

    let mut expected_idx = 0usize;
    for message in &session.messages {
        if expected_idx >= expected.len() {
            break;
        }

        let (role, content) = expected[expected_idx];
        if message.role == role && message.content == content {
            expected_idx += 1;
        }
    }

    assert_eq!(
        expected_idx,
        expected.len(),
        "Expected session to contain {:?} in order, got {:?}",
        expected,
        session
            .messages
            .iter()
            .map(|msg| (&msg.role, &msg.content))
            .collect::<Vec<_>>()
    );
}

fn find_tool_call<'a>(result: &'a AgentRunResult, tool_name: &str) -> &'a ToolCallRecord {
    result
        .metadata
        .tool_calls
        .iter()
        .find(|call| call.tool_name == tool_name)
        .unwrap_or_else(|| {
            panic!(
                "Expected tool '{}' in run metadata, got {:?}",
                tool_name,
                result
                    .metadata
                    .tool_calls
                    .iter()
                    .map(|call| &call.tool_name)
                    .collect::<Vec<_>>()
            )
        })
}

/// Assert that a given tool was called during the run.
pub fn assert_run_tool_called(result: &AgentRunResult, tool_name: &str) {
    let _ = find_tool_call(result, tool_name);
}

/// Assert that a given tool was not called during the run.
pub fn assert_run_tool_not_called(result: &AgentRunResult, tool_name: &str) {
    let found = result
        .metadata
        .tool_calls
        .iter()
        .any(|call| call.tool_name == tool_name);
    assert!(
        !found,
        "Expected tool '{}' not to be called, but it was. Calls: {:?}",
        tool_name,
        result
            .metadata
            .tool_calls
            .iter()
            .map(|call| &call.tool_name)
            .collect::<Vec<_>>()
    );
}

/// Assert that a given tool was called the expected number of times.
pub fn assert_run_tool_call_count(result: &AgentRunResult, tool_name: &str, expected: usize) {
    let count = result
        .metadata
        .tool_calls
        .iter()
        .filter(|call| call.tool_name == tool_name)
        .count();
    assert_eq!(
        count, expected,
        "Expected tool '{}' to be called {} time(s), got {}",
        tool_name, expected, count
    );
}

/// Assert that a tool call used the expected input JSON.
pub fn assert_run_tool_input(result: &AgentRunResult, tool_name: &str, expected: &Value) {
    let call = find_tool_call(result, tool_name);
    assert_eq!(
        &call.input, expected,
        "Expected tool '{}' input {:?}, got {:?}",
        tool_name, expected, call.input
    );
}

/// Assert that a tool call completed successfully.
pub fn assert_run_tool_succeeded(result: &AgentRunResult, tool_name: &str) {
    let call = find_tool_call(result, tool_name);
    assert!(
        call.success,
        "Expected tool '{}' to succeed, but metadata was {:?}",
        tool_name,
        call
    );
}

/// Assert that a tool call failed.
pub fn assert_run_tool_failed(result: &AgentRunResult, tool_name: &str) {
    let call = find_tool_call(result, tool_name);
    assert!(
        !call.success,
        "Expected tool '{}' to fail, but metadata was {:?}",
        tool_name,
        call
    );
}

/// Assert that a tool call output contains the expected substring.
pub fn assert_run_tool_output_contains(
    result: &AgentRunResult,
    tool_name: &str,
    expected: &str,
) {
    let call = find_tool_call(result, tool_name);
    let output = call
        .output
        .as_ref()
        .unwrap_or_else(|| panic!("Expected tool '{}' output, but it was None", tool_name))
        .to_string();
    assert!(
        output.contains(expected),
        "Expected tool '{}' output containing '{}', got '{}'",
        tool_name,
        expected,
        output
    );
}

/// Assert that a tool call output matches the expected JSON value.
pub fn assert_run_tool_output_equals_json(
    result: &AgentRunResult,
    tool_name: &str,
    expected: &Value,
) {
    let call = find_tool_call(result, tool_name);
    let output = call
        .output
        .as_ref()
        .unwrap_or_else(|| panic!("Expected tool '{}' output, but it was None", tool_name));
    assert_eq!(
        output, expected,
        "Expected tool '{}' output {:?}, got {:?}",
        tool_name, expected, output
    );
}

/// Assert that a tool call recorded a duration.
pub fn assert_run_tool_duration_recorded(result: &AgentRunResult, tool_name: &str) {
    let call = find_tool_call(result, tool_name);
    assert!(
        call.duration_ms.is_some(),
        "Expected tool '{}' to record duration metadata, got {:?}",
        tool_name,
        call
    );
}

/// Assert that a tool call duration is less than or equal to the threshold.
pub fn assert_run_tool_duration_under(
    result: &AgentRunResult,
    tool_name: &str,
    max_duration_ms: u64,
) {
    let call = find_tool_call(result, tool_name);
    let duration_ms = call.duration_ms.unwrap_or_else(|| {
        panic!(
            "Expected tool '{}' to record duration metadata, got {:?}",
            tool_name, call
        )
    });
    assert!(
        duration_ms <= max_duration_ms,
        "Expected tool '{}' duration <= {} ms, got {} ms",
        tool_name,
        max_duration_ms,
        duration_ms
    );
}

/// Assert that a tool call was marked as timed out.
pub fn assert_run_tool_timed_out(result: &AgentRunResult, tool_name: &str) {
    let call = find_tool_call(result, tool_name);
    assert!(
        call.timed_out,
        "Expected tool '{}' to time out, but metadata was {:?}",
        tool_name,
        call
    );
}

fn find_file<'a>(snapshot: &'a WorkspaceSnapshot, relative_path: &str) -> &'a WorkspaceFileSnapshot {
    snapshot
        .files
        .iter()
        .find(|file| file.relative_path == relative_path)
        .unwrap_or_else(|| {
            panic!(
                "Expected workspace file '{}', got {:?}",
                relative_path,
                snapshot
                    .files
                    .iter()
                    .map(|file| &file.relative_path)
                    .collect::<Vec<_>>()
            )
        })
}

/// Assert that the workspace snapshot contains the given file.
pub fn assert_workspace_has_file(snapshot: &WorkspaceSnapshot, relative_path: &str) {
    let _ = find_file(snapshot, relative_path);
}

/// Assert that the workspace snapshot before execution contains the given file.
pub fn assert_workspace_file_exists_before(result: &AgentRunResult, relative_path: &str) {
    assert_workspace_has_file(&result.metadata.workspace_snapshot_before, relative_path);
}

/// Assert that the workspace snapshot after execution contains the given file.
pub fn assert_workspace_file_exists_after(result: &AgentRunResult, relative_path: &str) {
    assert_workspace_has_file(&result.metadata.workspace_snapshot_after, relative_path);
}

/// Assert that the workspace snapshot does not contain the given file.
pub fn assert_workspace_missing_file(snapshot: &WorkspaceSnapshot, relative_path: &str) {
    let found = snapshot
        .files
        .iter()
        .any(|file| file.relative_path == relative_path);
    assert!(
        !found,
        "Expected workspace file '{}' to be absent, but snapshot contained it",
        relative_path
    );
}

/// Assert that the workspace file count delta matches the expected change.
pub fn assert_workspace_file_count_delta(result: &AgentRunResult, expected_delta: isize) {
    let before = result.metadata.workspace_snapshot_before.files.len() as isize;
    let after = result.metadata.workspace_snapshot_after.files.len() as isize;
    let actual_delta = after - before;
    assert_eq!(
        actual_delta, expected_delta,
        "Expected workspace file count delta {}, got {} (before {}, after {})",
        expected_delta, actual_delta, before, after
    );
}

/// Assert that a workspace file checksum changed across the run.
pub fn assert_workspace_file_checksum_changed(result: &AgentRunResult, relative_path: &str) {
    let before = find_file(&result.metadata.workspace_snapshot_before, relative_path);
    let after = find_file(&result.metadata.workspace_snapshot_after, relative_path);
    assert_ne!(
        before.checksum, after.checksum,
        "Expected workspace file '{}' checksum to change, but it stayed {}",
        relative_path, before.checksum
    );
}

/// Assert that the run created or changed the given workspace file.
pub fn assert_workspace_file_changed(result: &AgentRunResult, relative_path: &str) {
    let before = result
        .metadata
        .workspace_snapshot_before
        .files
        .iter()
        .find(|file| file.relative_path == relative_path);
    let after = result
        .metadata
        .workspace_snapshot_after
        .files
        .iter()
        .find(|file| file.relative_path == relative_path);

    match (before, after) {
        (None, Some(_)) => {}
        (Some(before), Some(after)) => {
            assert!(
                before.size_bytes != after.size_bytes
                    || before.modified_ms != after.modified_ms
                    || before.checksum != after.checksum,
                "Expected workspace file '{}' to change, but snapshots were unchanged",
                relative_path
            );
        }
        (Some(_), None) => panic!(
            "Expected workspace file '{}' to change, but it disappeared after the run",
            relative_path
        ),
        (None, None) => panic!(
            "Expected workspace file '{}' to change, but it never existed in snapshots",
            relative_path
        ),
    }
}
