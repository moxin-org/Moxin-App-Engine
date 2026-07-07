//! Convenience assertion macros for mock verification.
//!
//! Reduces boilerplate when checking that mocks were called as expected.

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

/// Assert the most recent tool result matches the expected JSON output.
///
/// # Example
/// ```ignore
/// assert_tool_last_result!(tool, json!("done"));
/// ```
#[macro_export]
macro_rules! assert_tool_last_result {
    ($tool:expr, $expected:expr) => {{
        let result = $tool
            .last_result()
            .await
            .expect("Expected tool to have a result, but it was never executed");
        let expected = $expected;
        assert_eq!(
            result.output, expected,
            "Expected latest tool result {:?}, got {:?}",
            expected, result.output
        );
    }};
}

/// Assert the agent run produced the expected output text.
///
/// # Example
/// ```ignore
/// assert_agent_output_text!(result, "hello");
/// ```
#[macro_export]
macro_rules! assert_agent_output_text {
    ($result:expr, $expected:expr) => {{
        let expected = $expected;
        let actual = $result.output_text();
        assert_eq!(
            actual.as_deref(),
            Some(expected),
            "Expected agent output {:?}, got {:?}",
            expected,
            actual
        );
    }};
}

/// Assert the agent run failed with an error containing the given substring.
///
/// # Example
/// ```ignore
/// assert_run_failed_with!(result, "timeout");
/// ```
#[macro_export]
macro_rules! assert_run_failed_with {
    ($result:expr, $pattern:expr) => {{
        let pattern = $pattern;
        let error = $result
            .error
            .as_ref()
            .expect("Expected run to fail, but it succeeded");
        let message = error.to_string();
        assert!(
            message.contains(pattern),
            "Expected error containing {:?}, got {:?}",
            pattern,
            message
        );
    }};
}

/// Assert the workspace snapshot contains a file with the given relative path.
///
/// # Example
/// ```ignore
/// assert_workspace_contains_file!(snapshot, "sessions/demo.jsonl");
/// ```
#[macro_export]
macro_rules! assert_workspace_contains_file {
    ($snapshot:expr, $relative_path:expr) => {{
        let relative_path = $relative_path;
        let found = $snapshot
            .files
            .iter()
            .any(|file| file.relative_path == relative_path);
        assert!(
            found,
            "Expected workspace snapshot to contain {:?}, found paths: {:?}",
            relative_path,
            $snapshot
                .files
                .iter()
                .map(|file| file.relative_path.as_str())
                .collect::<Vec<_>>()
        );
    }};
}

/// Assert the run metadata captured a tool call with the given tool name.
///
/// # Example
/// ```ignore
/// assert_run_recorded_tool_call!(result, "echo_tool");
/// ```
#[macro_export]
macro_rules! assert_run_recorded_tool_call {
    ($result:expr, $tool_name:expr) => {{
        let tool_name = $tool_name;
        let found = $result
            .metadata
            .tool_calls
            .iter()
            .any(|record| record.tool_name == tool_name);
        assert!(
            found,
            "Expected run metadata to contain tool call {:?}, found tool calls: {:?}",
            tool_name,
            $result
                .metadata
                .tool_calls
                .iter()
                .map(|record| record.tool_name.as_str())
                .collect::<Vec<_>>()
        );
    }};
}

/// Assert the runner total execution count after a run matches the expected value.
///
/// # Example
/// ```ignore
/// assert_runner_total_executions!(result, 1);
/// ```
#[macro_export]
macro_rules! assert_runner_total_executions {
    ($result:expr, $expected:expr) => {{
        let expected = $expected;
        let actual = $result.metadata.runner_stats_after.total_executions;
        assert_eq!(
            actual, expected,
            "Expected runner total executions {}, got {}",
            expected, actual
        );
    }};
}
