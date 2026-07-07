//! Deterministic Workflow Trace Engine
//!
//! This module provides the trace capture infrastructure for deterministic workflow replay.
//! It enables capturing execution traces that can later be used for deterministic replay,
//! debugging, and regression testing.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │                    Trace Capture Pipeline                    │
//! ├──────────────────────────────────────────────────────────────┤
//! │                                                              │
//! │  WorkflowExecutor ──trace──▶ WorkflowTrace                  │
//! │         │                      │                             │
//! │         │                      ▼                             │
//! │         │              TraceEvent (serializable)             │
//! │         │                      │                             │
//! │         │                      ▼                             │
//! │         │              JSON serialization                    │
//! │         │                      │                             │
//! │         │                      ▼                             │
//! │         │              File / Storage                        │
//! │                                                              │
//! └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use mofa_foundation::workflow::trace::{WorkflowTrace, TraceMode};
//!
//! // Enable trace recording
//! let trace = WorkflowTrace::new("workflow-1", "exec-1");
//!
//! // Configure executor to use trace
//! let executor = WorkflowExecutor::new(config)
//!     .with_trace_mode(TraceMode::Record(trace));
//! ```

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Trace mode configuration
#[derive(Debug, Clone)]
pub enum TraceMode {
    /// No tracing - execution proceeds normally without any overhead
    Disabled,
    /// Record mode - captures all execution events for later replay
    Record(Arc<WorkflowTraceHandle>),
}

impl TraceMode {
    /// Check if tracing is enabled
    pub fn is_enabled(&self) -> bool {
        matches!(self, TraceMode::Record(_))
    }

    /// Get the trace handle if in record mode
    pub fn trace(&self) -> Option<&WorkflowTraceHandle> {
        match self {
            TraceMode::Record(handle) => Some(handle),
            TraceMode::Disabled => None,
        }
    }
}

/// Tool invocation record - captures input and output of tool calls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    /// Unique identifier for this tool call
    pub id: String,
    /// Tool name or identifier
    pub tool_name: String,
    /// Input parameters passed to the tool
    pub input: serde_json::Value,
    /// Output returned by the tool
    pub output: Option<serde_json::Value>,
    /// Error message if tool call failed
    pub error: Option<String>,
    /// Whether the tool call was successful
    pub success: bool,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
}

/// Retry attempt record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryAttempt {
    /// Attempt number (1-indexed)
    pub attempt: u32,
    /// Timestamp when retry was initiated
    pub timestamp_ms: u64,
    /// Reason for retry (error message)
    pub reason: String,
}

/// State update record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceStateUpdate {
    /// Key that was updated
    pub key: String,
    /// Previous value (None if new key)
    pub old_value: Option<serde_json::Value>,
    /// New value
    pub new_value: serde_json::Value,
    /// Timestamp of update
    pub timestamp_ms: u64,
}

/// Execution status transition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionStatus {
    /// Workflow/Node has started
    Started,
    /// Running currently
    Running,
    /// Completed successfully
    Completed,
    /// Failed with error
    Failed(String),
    /// Was cancelled
    Cancelled,
}

/// Node execution record for trace capture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeExecutionRecord {
    /// Node identifier
    pub node_id: String,
    /// Node type
    pub node_type: String,
    /// Order of execution in the workflow
    pub execution_order: usize,
    /// When node started executing
    pub started_at_ms: u64,
    /// When node finished executing
    pub ended_at_ms: Option<u64>,
    /// Duration in milliseconds
    pub duration_ms: Option<u64>,
    /// Current status
    pub status: ExecutionStatus,
    /// Input to the node
    pub input: Option<serde_json::Value>,
    /// Output from the node
    pub output: Option<serde_json::Value>,
    /// Tool invocations made by this node
    #[serde(default)]
    pub tool_invocations: Vec<ToolInvocation>,
    /// Retry attempts for this node
    #[serde(default)]
    pub retry_attempts: Vec<RetryAttempt>,
    /// State updates performed by this node
    #[serde(default)]
    pub state_updates: Vec<TraceStateUpdate>,
}

/// Workflow trace - captures complete execution for deterministic replay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTrace {
    /// Unique trace identifier
    pub id: String,
    /// Workflow graph identifier
    pub workflow_id: String,
    /// Execution identifier
    pub execution_id: String,
    /// When trace was started
    pub started_at_ms: u64,
    /// When trace ended (None if still running)
    pub ended_at_ms: Option<u64>,
    /// Final execution status
    pub status: ExecutionStatus,
    /// Node execution records in order
    #[serde(default)]
    pub node_executions: Vec<NodeExecutionRecord>,
    /// Total tool invocations across all nodes
    #[serde(default)]
    pub total_tool_invocations: usize,
    /// Total retry attempts across all nodes
    #[serde(default)]
    pub total_retry_attempts: usize,
}

impl WorkflowTrace {
    /// Create a new workflow trace
    pub fn new(workflow_id: impl Into<String>, execution_id: impl Into<String>) -> Self {
        let exec_id = execution_id.into();
        let id = format!("trace-{}-{}", exec_id, uuid_v4());
        Self {
            id,
            workflow_id: workflow_id.into(),
            execution_id: exec_id,
            started_at_ms: current_time_ms(),
            ended_at_ms: None,
            status: ExecutionStatus::Started,
            node_executions: Vec::new(),
            total_tool_invocations: 0,
            total_retry_attempts: 0,
        }
    }

    /// Create a node execution record for a starting node
    pub fn start_node(
        &mut self,
        node_id: String,
        node_type: String,
        input: Option<serde_json::Value>,
    ) -> usize {
        let execution_order = self.node_executions.len();
        let record = NodeExecutionRecord {
            node_id,
            node_type,
            execution_order,
            started_at_ms: current_time_ms(),
            ended_at_ms: None,
            duration_ms: None,
            status: ExecutionStatus::Running,
            input,
            output: None,
            tool_invocations: Vec::new(),
            retry_attempts: Vec::new(),
            state_updates: Vec::new(),
        };
        self.node_executions.push(record);
        execution_order
    }

    /// Complete a node execution
    pub fn complete_node(
        &mut self,
        execution_order: usize,
        output: Option<serde_json::Value>,
        status: ExecutionStatus,
    ) {
        if let Some(record) = self.node_executions.get_mut(execution_order) {
            let ended_at = current_time_ms();
            record.ended_at_ms = Some(ended_at);
            record.duration_ms = Some(ended_at.saturating_sub(record.started_at_ms));
            record.output = output;
            record.status = status;

            // Update totals
            self.total_tool_invocations += record.tool_invocations.len();
            self.total_retry_attempts += record.retry_attempts.len();
        }
    }

    /// Record a tool invocation for a node
    pub fn record_tool_invocation(
        &mut self,
        execution_order: usize,
        invocation: ToolInvocation,
    ) {
        if let Some(record) = self.node_executions.get_mut(execution_order) {
            record.tool_invocations.push(invocation);
        }
    }

    /// Record a retry attempt for a node
    pub fn record_retry_attempt(
        &mut self,
        execution_order: usize,
        attempt: RetryAttempt,
    ) {
        if let Some(record) = self.node_executions.get_mut(execution_order) {
            record.retry_attempts.push(attempt);
        }
    }

    /// Record a state update for a node
    pub fn record_state_update(
        &mut self,
        execution_order: usize,
        update: TraceStateUpdate,
    ) {
        if let Some(record) = self.node_executions.get_mut(execution_order) {
            record.state_updates.push(update);
        }
    }

    /// Mark the trace as complete
    pub fn finish(&mut self, status: ExecutionStatus) {
        self.ended_at_ms = Some(current_time_ms());
        self.status = status;
    }

    /// Serialize the trace to JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize a trace from JSON
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Thread-safe wrapper for WorkflowTrace
#[derive(Debug)]
pub struct WorkflowTraceHandle {
    trace: Arc<RwLock<WorkflowTrace>>,
}

impl WorkflowTraceHandle {
    /// Create a new trace handle
    pub fn new(workflow_id: impl Into<String>, execution_id: impl Into<String>) -> Self {
        Self {
            trace: Arc::new(RwLock::new(WorkflowTrace::new(workflow_id, execution_id))),
        }
    }

    /// Get an owned clone of the trace
    pub async fn get_trace(&self) -> WorkflowTrace {
        self.trace.read().await.clone()
    }

    /// Start a node execution and return the execution order
    pub async fn start_node(
        &self,
        node_id: String,
        node_type: String,
        input: Option<serde_json::Value>,
    ) -> usize {
        self.trace.write().await.start_node(node_id, node_type, input)
    }

    /// Complete a node execution
    pub async fn complete_node(
        &self,
        execution_order: usize,
        output: Option<serde_json::Value>,
        status: ExecutionStatus,
    ) {
        self.trace
            .write()
            .await
            .complete_node(execution_order, output, status);
    }

    /// Record a tool invocation
    pub async fn record_tool_invocation(
        &self,
        execution_order: usize,
        invocation: ToolInvocation,
    ) {
        self.trace
            .write()
            .await
            .record_tool_invocation(execution_order, invocation);
    }

    /// Record a retry attempt
    pub async fn record_retry_attempt(&self, execution_order: usize, attempt: RetryAttempt) {
        self.trace
            .write()
            .await
            .record_retry_attempt(execution_order, attempt);
    }

    /// Record a state update
    pub async fn record_state_update(&self, execution_order: usize, update: TraceStateUpdate) {
        self.trace
            .write()
            .await
            .record_state_update(execution_order, update);
    }

    /// Finish the trace
    pub async fn finish(&self, status: ExecutionStatus) {
        self.trace.write().await.finish(status);
    }

    /// Serialize to JSON
    pub async fn to_json(&self) -> Result<String, serde_json::Error> {
        self.trace.read().await.to_json()
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Get current time in milliseconds
fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Generate a simple UUID v4-like string
fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        now.as_secs() as u32,
        (now.as_nanos() >> 16) as u16 & 0x0FFF,
        (now.as_nanos() >> 12) as u16 & 0x0FFF,
        (rand_u16() & 0x3FFF) | 0x8000,
        now.as_nanos() as u64 & 0xFFFFFFFFFFFF
    )
}

/// Generate a random u16
fn rand_u16() -> u16 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    RandomState::new().build_hasher().finish() as u16
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_creation() {
        let trace = WorkflowTrace::new("workflow-1", "exec-1");
        assert_eq!(trace.workflow_id, "workflow-1");
        assert_eq!(trace.execution_id, "exec-1");
        assert!(trace.ended_at_ms.is_none());
        assert!(matches!(trace.status, ExecutionStatus::Started));
    }

    #[test]
    fn test_trace_node_lifecycle() {
        let mut trace = WorkflowTrace::new("workflow-1", "exec-1");

        let order = trace.start_node(
            "node-1".to_string(),
            "task".to_string(),
            Some(serde_json::json!({"input": "test"})),
        );
        assert_eq!(order, 0);

        trace.complete_node(
            order,
            Some(serde_json::json!({"result": "success"})),
            ExecutionStatus::Completed,
        );

        assert_eq!(trace.node_executions.len(), 1);
        let node = &trace.node_executions[0];
        assert_eq!(node.node_id, "node-1");
        assert!(node.ended_at_ms.is_some());
        assert!(node.duration_ms.is_some());
    }

    #[test]
    fn test_trace_tool_invocation() {
        let mut trace = WorkflowTrace::new("workflow-1", "exec-1");

        let order = trace.start_node("node-1".to_string(), "task".to_string(), None);

        trace.record_tool_invocation(
            order,
            ToolInvocation {
                id: "tool-1".to_string(),
                tool_name: "search".to_string(),
                input: serde_json::json!({"query": "test"}),
                output: Some(serde_json::json!({"results": []})),
                error: None,
                success: true,
                duration_ms: 100,
            },
        );

        assert_eq!(trace.node_executions[0].tool_invocations.len(), 1);
    }

    #[test]
    fn test_trace_retry_attempts() {
        let mut trace = WorkflowTrace::new("workflow-1", "exec-1");

        let order = trace.start_node("node-1".to_string(), "task".to_string(), None);

        trace.record_retry_attempt(
            order,
            RetryAttempt {
                attempt: 1,
                timestamp_ms: 1000,
                reason: "Connection timeout".to_string(),
            },
        );

        assert_eq!(trace.node_executions[0].retry_attempts.len(), 1);
    }

    #[test]
    fn test_trace_serialization() {
        let mut trace = WorkflowTrace::new("workflow-1", "exec-1");
        trace.start_node("node-1".to_string(), "task".to_string(), None);
        trace.complete_node(0, None, ExecutionStatus::Completed);
        trace.finish(ExecutionStatus::Completed);

        let json = trace.to_json().unwrap();
        let round_trip: WorkflowTrace = WorkflowTrace::from_json(&json).unwrap();

        assert_eq!(round_trip.workflow_id, "workflow-1");
        assert_eq!(round_trip.node_executions.len(), 1);
    }

    #[test]
    fn test_trace_mode() {
        let handle = Arc::new(WorkflowTraceHandle::new("wf", "exec"));
        let mode = TraceMode::Record(handle.clone());

        assert!(mode.is_enabled());
        assert!(mode.trace().is_some());

        let disabled = TraceMode::Disabled;
        assert!(!disabled.is_enabled());
        assert!(disabled.trace().is_none());
    }

    #[test]
    fn test_trace_handle() {
        let handle = WorkflowTraceHandle::new("workflow-1", "exec-1");

        let order = futures::executor::block_on(async {
            handle
                .start_node(
                    "node-1".to_string(),
                    "task".to_string(),
                    Some(serde_json::json!({"key": "value"})),
                )
                .await
        });

        assert_eq!(order, 0);

        futures::executor::block_on(async {
            handle
                .complete_node(
                    order,
                    Some(serde_json::json!({"result": "ok"})),
                    ExecutionStatus::Completed,
                )
                .await;

            handle.finish(ExecutionStatus::Completed).await;

            let trace = handle.get_trace().await;
            assert_eq!(trace.node_executions.len(), 1);
        });
    }
}
