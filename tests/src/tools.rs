//! Mock tools for testing agent tool-selection logic.

use async_trait::async_trait;
use mofa_foundation::agent::components::tool::{SimpleTool, ToolCategory};
use mofa_kernel::agent::components::tool::{ToolInput, ToolResult};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A mock tool that records calls and returns a configurable result.
/// Supports failure injection via [`fail_next`](Self::fail_next),
/// input-pattern failures via [`fail_on_input`](Self::fail_on_input),
/// and sequenced results via [`add_result_sequence`](Self::add_result_sequence).
#[derive(Clone)]
pub struct MockTool {
    name: String,
    description: String,
    schema: Value,
    category: ToolCategory,
    pub stubbed_result: Arc<RwLock<ToolResult>>,
    pub call_history: Arc<RwLock<Vec<ToolInput>>>,
    pub result_history: Arc<RwLock<Vec<ToolResult>>>,
    failure_queue: Arc<RwLock<VecDeque<String>>>,
    failure_patterns: Arc<RwLock<Vec<(Value, String)>>>,
    result_sequence: Arc<RwLock<VecDeque<ToolResult>>>,
}

impl MockTool {
    /// Create a mock tool with the given identity and a default "success" result.
    pub fn new(name: &str, description: &str, schema: Value) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            schema,
            category: ToolCategory::Custom,
            stubbed_result: Arc::new(RwLock::new(ToolResult::success_text(
                "Mock execution default",
            ))),
            call_history: Arc::new(RwLock::new(Vec::new())),
            result_history: Arc::new(RwLock::new(Vec::new())),
            failure_queue: Arc::new(RwLock::new(VecDeque::new())),
            failure_patterns: Arc::new(RwLock::new(Vec::new())),
            result_sequence: Arc::new(RwLock::new(VecDeque::new())),
        }
    }

    /// Replace the result that will be returned on subsequent calls.
    pub async fn set_result(&self, result: ToolResult) {
        *self.stubbed_result.write().await = result;
    }

    /// Retrieve a clone of the full call history.
    pub async fn history(&self) -> Vec<ToolInput> {
        self.call_history.read().await.clone()
    }

    /// Number of times this tool has been executed.
    pub async fn call_count(&self) -> usize {
        self.call_history.read().await.len()
    }

    /// Retrieve a clone of the full result history.
    pub async fn results(&self) -> Vec<ToolResult> {
        self.result_history.read().await.clone()
    }

    /// Returns the most recent result, or `None` if never executed.
    pub async fn last_result(&self) -> Option<ToolResult> {
        self.result_history.read().await.last().cloned()
    }

    /// Queue failures for the next N calls.
    pub async fn fail_next(&self, count: usize, error_msg: &str) {
        let mut queue = self.failure_queue.write().await;
        for _ in 0..count {
            queue.push_back(error_msg.to_string());
        }
    }

    /// Fail when input arguments match the given JSON value.
    pub async fn fail_on_input(&self, input_pattern: Value, error_msg: &str) {
        self.failure_patterns
            .write()
            .await
            .push((input_pattern, error_msg.to_string()));
    }

    /// Returns the most recent call's input, or `None` if never called.
    pub async fn last_call(&self) -> Option<ToolInput> {
        self.call_history.read().await.last().cloned()
    }

    /// Returns the Nth call (0-indexed), or `None` if out of bounds.
    pub async fn nth_call(&self, n: usize) -> Option<ToolInput> {
        self.call_history.read().await.get(n).cloned()
    }

    /// Add a sequence of results. Each call consumes the next entry;
    /// when exhausted, falls back to `stubbed_result`.
    pub async fn add_result_sequence(&self, results: Vec<ToolResult>) {
        let mut seq = self.result_sequence.write().await;
        for r in results {
            seq.push_back(r);
        }
    }
}

#[async_trait]
impl SimpleTool for MockTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        self.call_history.write().await.push(input.clone());
        let start = std::time::Instant::now();

        // 1. Drain failure queue
        {
            let mut queue = self.failure_queue.write().await;
            if let Some(err) = queue.pop_front() {
                let mut result = ToolResult::failure(err);
                result
                    .metadata
                    .insert("duration_ms".to_string(), start.elapsed().as_millis().to_string());
                self.result_history.write().await.push(result.clone());
                return result;
            }
        }

        // 2. Check input-pattern failures
        {
            let patterns = self.failure_patterns.read().await;
            for (pattern, err) in patterns.iter() {
                if input.arguments == *pattern {
                    let mut result = ToolResult::failure(err);
                    result
                        .metadata
                        .insert("duration_ms".to_string(), start.elapsed().as_millis().to_string());
                    self.result_history.write().await.push(result.clone());
                    return result;
                }
            }
        }

        // 3. Drain result sequence
        {
            let mut seq = self.result_sequence.write().await;
            if let Some(result) = seq.pop_front() {
                let mut result = result;
                result
                    .metadata
                    .insert("duration_ms".to_string(), start.elapsed().as_millis().to_string());
                self.result_history.write().await.push(result.clone());
                return result;
            }
        }

        let mut result = self.stubbed_result.read().await.clone();
        result
            .metadata
            .insert("duration_ms".to_string(), start.elapsed().as_millis().to_string());
        self.result_history.write().await.push(result.clone());
        result
    }

    fn category(&self) -> ToolCategory {
        self.category
    }
}

/// Assert that a [`MockTool`] was called exactly `$expected` times.
#[macro_export]
macro_rules! assert_tool_call_count {
    ($tool:expr, $expected_count:expr) => {{
        use mofa_foundation::agent::components::tool::SimpleTool as _;
        let count = $tool.call_count().await;
        assert_eq!(
            count,
            $expected_count,
            "Expected tool '{}' to be called {} time(s), but was called {} time(s)",
            $tool.name(),
            $expected_count,
            count
        );
    }};
}
