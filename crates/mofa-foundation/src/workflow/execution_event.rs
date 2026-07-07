//! Execution Event Schema Definition
//!
//! This module defines the canonical ExecutionEvent enum and envelope wrapper
//! for versioned execution tracing, replay, and monitoring.
//!
//! # Schema Version
//!
//! The schema version is used to ensure compatibility between trace recording
//! and replay systems. Versions are integers starting from 1.

use serde::{Deserialize, Serialize};

/// Current schema version for execution events
pub const SCHEMA_VERSION: u32 = 1;

/// Lightweight structured trace emitted for each node execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionSpan {
    pub node_id: String,
    pub start_time: u64,
    pub end_time: u64,
    pub duration_ms: u64,
    pub input_state_hash: Option<String>,
    pub output_state_hash: Option<String>,
    pub command_type: String,
    pub error: Option<String>,
}

/// Canonical execution event types for workflow execution tracing
///
/// This enum defines the core events that can occur during workflow execution.
/// Each variant represents a distinct phase or action in the workflow lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum ExecutionEvent {
    /// Workflow execution started
    WorkflowStarted {
        workflow_id: String,
        workflow_name: String,
        started_at: u64, // Unix timestamp in milliseconds
    },

    /// Node execution started
    NodeStarted {
        node_id: String,
        node_name: String,
        parent_span_id: Option<String>,
    },

    /// Node execution completed
    NodeCompleted {
        node_id: String,
        output: Option<serde_json::Value>,
        duration_ms: u64,
    },

    /// Node execution failed
    NodeFailed {
        node_id: String,
        error: String,
        duration_ms: u64,
    },

    /// Tool invocation started
    ToolInvoked {
        tool_name: String,
        input: serde_json::Value,
        invocation_id: String,
    },

    /// Tool invocation completed
    ToolCompleted {
        invocation_id: String,
        output: serde_json::Value,
        duration_ms: u64,
    },

    /// Tool invocation failed
    ToolFailed {
        invocation_id: String,
        error: String,
        duration_ms: u64,
    },

    /// State update occurred
    StateUpdated {
        key: String,
        old_value: Option<serde_json::Value>,
        new_value: serde_json::Value,
    },

    /// Workflow execution completed
    WorkflowCompleted {
        workflow_id: String,
        final_output: Option<serde_json::Value>,
        total_duration_ms: u64,
    },

    /// Workflow execution failed
    WorkflowFailed {
        workflow_id: String,
        error: String,
        total_duration_ms: u64,
    },

    /// Checkpoint created during workflow execution
    CheckpointCreated { label: String },

    /// Retry attempt for a node
    NodeRetrying {
        node_id: String,
        attempt: u32,
        max_attempts: u32,
        last_error: Option<String>,
    },

    /// Branch decision made (conditional nodes)
    BranchDecision {
        node_id: String,
        branch_taken: String,
        condition_result: bool,
    },

    /// Parallel execution group started
    ParallelGroupStarted {
        group_id: String,
        node_ids: Vec<String>,
    },

    /// Parallel execution group completed
    ParallelGroupCompleted {
        group_id: String,
        completed_count: u32,
    },
}

/// Envelope wrapper for execution events with schema version
///
/// All execution events should be wrapped in this envelope to ensure
/// proper schema versioning and future compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionEventEnvelope {
    /// Schema version for this event
    pub schema_version: u32,

    /// The wrapped execution event
    pub event: ExecutionEvent,
}

impl ExecutionEventEnvelope {
    /// Create a new envelope with the current schema version
    pub fn new(event: ExecutionEvent) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            event,
        }
    }

    /// Validate that the schema version is compatible
    ///
    /// Returns true if the version matches the current schema version.
    /// In the future, this may support forward compatibility.
    pub fn is_compatible(&self) -> bool {
        self.schema_version == SCHEMA_VERSION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_consistency() {
        let event = ExecutionEvent::WorkflowStarted {
            workflow_id: "wf-123".to_string(),
            workflow_name: "test_workflow".to_string(),
            started_at: 1700000000000,
        };

        let envelope = ExecutionEventEnvelope::new(event.clone());

        let serialized = serde_json::to_string(&envelope).unwrap();
        let deserialized: ExecutionEventEnvelope = serde_json::from_str(&serialized).unwrap();

        assert_eq!(envelope, deserialized);
    }

    #[test]
    fn test_schema_version_stored_correctly() {
        let event = ExecutionEvent::NodeCompleted {
            node_id: "node-1".to_string(),
            output: Some(serde_json::json!({"result": "ok"})),
            duration_ms: 150,
        };

        let envelope = ExecutionEventEnvelope::new(event);

        let serialized = serde_json::to_string(&envelope).unwrap();
        let json: serde_json::Value = serde_json::from_str(&serialized).unwrap();

        assert_eq!(json["schema_version"], SCHEMA_VERSION);
    }

    #[test]
    fn test_is_compatible_returns_true_for_current_version() {
        let event = ExecutionEvent::ToolCompleted {
            invocation_id: "inv-123".to_string(),
            output: serde_json::json!({"status": "success"}),
            duration_ms: 50,
        };

        let envelope = ExecutionEventEnvelope::new(event);
        assert!(envelope.is_compatible());
    }

    #[test]
    fn test_event_variants_serialize_correctly() {
        let events = vec![
            ExecutionEvent::WorkflowStarted {
                workflow_id: "wf-1".to_string(),
                workflow_name: "test".to_string(),
                started_at: 1000,
            },
            ExecutionEvent::NodeStarted {
                node_id: "node-1".to_string(),
                node_name: "process".to_string(),
                parent_span_id: Some("span-1".to_string()),
            },
            ExecutionEvent::ToolInvoked {
                tool_name: "calculator".to_string(),
                input: serde_json::json!({"operation": "add", "a": 1, "b": 2}),
                invocation_id: "inv-1".to_string(),
            },
            ExecutionEvent::StateUpdated {
                key: "counter".to_string(),
                old_value: Some(serde_json::json!(5)),
                new_value: serde_json::json!(6),
            },
        ];

        for event in events {
            let envelope = ExecutionEventEnvelope::new(event);
            let serialized = serde_json::to_string(&envelope).unwrap();
            let deserialized: ExecutionEventEnvelope = serde_json::from_str(&serialized).unwrap();
            assert_eq!(envelope, deserialized);
        }
    }

    #[test]
    fn test_workflow_completed_serialization() {
        let event = ExecutionEvent::WorkflowCompleted {
            workflow_id: "wf-123".to_string(),
            final_output: Some(serde_json::json!({"status": "completed"})),
            total_duration_ms: 5000,
        };

        let envelope = ExecutionEventEnvelope::new(event);
        let serialized = serde_json::to_string_pretty(&envelope).unwrap();

        assert!(serialized.contains("WorkflowCompleted"));
        assert!(serialized.contains("schema_version"));
    }

    #[test]
    fn test_node_retrying_serialization() {
        let event = ExecutionEvent::NodeRetrying {
            node_id: "node-1".to_string(),
            attempt: 2,
            max_attempts: 3,
            last_error: Some("Connection timeout".to_string()),
        };

        let envelope = ExecutionEventEnvelope::new(event);
        let serialized = serde_json::to_string(&envelope).unwrap();
        let deserialized: ExecutionEventEnvelope = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.schema_version, SCHEMA_VERSION);
    }
}
