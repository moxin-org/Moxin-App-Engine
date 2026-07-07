//! Deterministic Workflow Replay Module
//!
//! This module provides deterministic replay capabilities for workflow execution.
//! Phase 1: WorkflowTrace structure for recording execution
//! Phase 2: ExecutionMode for replay with recorded traces
//!
//! Usage:
//! ```rust,ignore
//! use mofa_foundation::workflow::{ExecutionMode, WorkflowTrace, ReplayError};
//!
//! // Record mode
//! let trace = WorkflowTrace::new("workflow-1".to_string());
//! let mode = ExecutionMode::Normal(trace);
//!
//! // Replay mode  
//! let replay_mode = ExecutionMode::Replay(trace);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Execution mode for workflow execution.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionMode {
    /// Normal execution - runs workflow normally
    Normal,
    /// Replay mode - returns recorded outputs from trace
    Replay(WorkflowTrace),
}

impl Default for ExecutionMode {
    fn default() -> Self {
        ExecutionMode::Normal
    }
}

/// Replay error types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReplayError {
    /// Node execution order doesn't match trace
    NodeOrderMismatch {
        expected: String,
        actual: String,
    },
    /// Required tool output not found in trace
    ToolOutputMissing {
        node_id: String,
        tool_name: String,
    },
    /// Tool output doesn't match recorded output
    ToolOutputMismatch {
        node_id: String,
        tool_name: String,
        expected: String,
        actual: String,
    },
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReplayError::NodeOrderMismatch { expected, actual } => {
                write!(f, "Node order mismatch: expected '{}', got '{}'", expected, actual)
            }
            ReplayError::ToolOutputMissing { node_id, tool_name } => {
                write!(f, "Tool output missing: node='{}', tool='{}'", node_id, tool_name)
            }
            ReplayError::ToolOutputMismatch { node_id, tool_name, expected, actual } => {
                write!(f, "Tool output mismatch: node='{}', tool='{}', expected='{}', got='{}'", 
                    node_id, tool_name, expected, actual)
            }
        }
    }
}

impl std::error::Error for ReplayError {}

/// A recorded tool invocation from trace
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInvocation {
    /// Node ID where tool was invoked
    pub node_id: String,
    /// Tool name
    pub tool_name: String,
    /// Tool input (JSON string)
    pub input: String,
    /// Tool output (JSON string)
    pub output: String,
}

/// A recorded node execution from trace
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeExecution {
    /// Node ID
    pub node_id: String,
    /// Node name
    pub node_name: String,
    /// Input to node
    pub input: String,
    /// Output from node
    pub output: String,
    /// Tool invocations within this node
    pub tool_invocations: Vec<ToolInvocation>,
}

/// Workflow trace - records workflow execution for replay
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WorkflowTrace {
    /// Workflow identifier
    pub workflow_id: String,
    /// Recorded node executions in order
    pub node_executions: Vec<NodeExecution>,
    /// Current position in replay mode
    #[serde(skip)]
    replay_position: usize,
}

impl WorkflowTrace {
    /// Creates a new WorkflowTrace
    pub fn new(workflow_id: String) -> Self {
        Self {
            workflow_id,
            node_executions: Vec::new(),
            replay_position: 0,
        }
    }

    /// Records a node execution
    pub fn record_node_execution(&mut self, execution: NodeExecution) {
        self.node_executions.push(execution);
    }

    /// Records a tool invocation within current node
    pub fn record_tool_invocation(&mut self, invocation: ToolInvocation) {
        if let Some(last_node) = self.node_executions.last_mut() {
            last_node.tool_invocations.push(invocation);
        }
    }

    /// Gets the next node execution for replay
    pub fn next_node(&mut self) -> Option<NodeExecution> {
        if self.replay_position < self.node_executions.len() {
            let node = self.node_executions[self.replay_position].clone();
            self.replay_position += 1;
            Some(node)
        } else {
            None
        }
    }

    /// Validates node order matches expected
    pub fn validate_node_order(&self, expected_order: &[String]) -> Result<(), ReplayError> {
        let recorded: Vec<String> = self.node_executions
            .iter()
            .map(|n| n.node_id.clone())
            .collect();
        
        if recorded.len() != expected_order.len() {
            return Err(ReplayError::NodeOrderMismatch {
                expected: expected_order.join(" -> "),
                actual: recorded.join(" -> "),
            });
        }
        
        for (i, (exp, act)) in expected_order.iter().zip(recorded.iter()).enumerate() {
            if exp != act {
                return Err(ReplayError::NodeOrderMismatch {
                    expected: expected_order.join(" -> "),
                    actual: recorded.join(" -> "),
                });
            }
        }
        
        Ok(())
    }

    /// Gets recorded tool output for a specific node and tool
    pub fn get_tool_output(&self, node_id: &str, tool_name: &str) -> Option<String> {
        for node in &self.node_executions {
            if node.node_id == node_id {
                for tool in &node.tool_invocations {
                    if tool.tool_name == tool_name {
                        return Some(tool.output.clone());
                    }
                }
            }
        }
        None
    }

    /// Resets replay position to beginning
    pub fn reset_replay(&mut self) {
        self.replay_position = 0;
    }

    /// Returns true if replay is complete
    pub fn is_replay_complete(&self) -> bool {
        self.replay_position >= self.node_executions.len()
    }

    /// Gets all recorded node IDs in order
    pub fn node_order(&self) -> Vec<String> {
        self.node_executions
            .iter()
            .map(|n| n.node_id.clone())
            .collect()
    }
}

/// Replay helper for intercepting and returning recorded outputs
#[derive(Debug, Clone)]
pub struct ReplayHelper {
    trace: WorkflowTrace,
    current_node_index: usize,
}

impl ReplayHelper {
    /// Creates a new ReplayHelper
    pub fn new(trace: WorkflowTrace) -> Self {
        Self {
            trace,
            current_node_index: 0,
        }
    }

    /// Gets the next node execution for replay
    pub fn next_node(&mut self) -> Option<NodeExecution> {
        if self.current_node_index < self.trace.node_executions.len() {
            let node = self.trace.node_executions[self.current_node_index].clone();
            self.current_node_index += 1;
            Some(node)
        } else {
            None
        }
    }

    /// Validates node execution order
    pub fn validate_order(&self, actual_node_id: &str) -> Result<(), ReplayError> {
        if self.current_node_index >= self.trace.node_executions.len() {
            return Err(ReplayError::NodeOrderMismatch {
                expected: "end of trace".to_string(),
                actual: actual_node_id.to_string(),
            });
        }

        let expected = &self.trace.node_executions[self.current_node_index].node_id;
        if expected != actual_node_id {
            return Err(ReplayError::NodeOrderMismatch {
                expected: expected.clone(),
                actual: actual_node_id.to_string(),
            });
        }

        Ok(())
    }

    /// Gets tool output from trace
    pub fn get_tool_output(&self, node_id: &str, tool_name: &str) -> Result<String, ReplayError> {
        self.trace
            .get_tool_output(node_id, tool_name)
            .ok_or_else(|| ReplayError::ToolOutputMissing {
                node_id: node_id.to_string(),
                tool_name: tool_name.to_string(),
            })
    }

    /// Returns true if replay is complete
    pub fn is_complete(&self) -> bool {
        self.current_node_index >= self.trace.node_executions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_mode_default() {
        let mode = ExecutionMode::default();
        assert_eq!(mode, ExecutionMode::Normal);
    }

    #[test]
    fn test_workflow_trace_record() {
        let mut trace = WorkflowTrace::new("workflow-1".to_string());
        
        trace.record_node_execution(NodeExecution {
            node_id: "node-1".to_string(),
            node_name: "Task 1".to_string(),
            input: r#"{"value": 1}"#.to_string(),
            output: r#"{"result": "done"}"#.to_string(),
            tool_invocations: vec![],
        });
        
        assert_eq!(trace.node_executions.len(), 1);
        assert_eq!(trace.node_order(), vec!["node-1"]);
    }

    #[test]
    fn test_replay_detects_node_order_mismatch() {
        let mut trace = WorkflowTrace::new("workflow-1".to_string());
        
        trace.record_node_execution(NodeExecution {
            node_id: "node-1".to_string(),
            node_name: "Task 1".to_string(),
            input: "{}".to_string(),
            output: "{}".to_string(),
            tool_invocations: vec![],
        });
        
        trace.record_node_execution(NodeExecution {
            node_id: "node-2".to_string(),
            node_name: "Task 2".to_string(),
            input: "{}".to_string(),
            output: "{}".to_string(),
            tool_invocations: vec![],
        });
        
        let result = trace.validate_node_order(&["node-1".to_string(), "node-3".to_string()]);
        assert!(result.is_err());
        
        match result.unwrap_err() {
            ReplayError::NodeOrderMismatch { .. } => {}
            _ => panic!("Expected NodeOrderMismatch"),
        }
    }

    #[test]
    fn test_replay_detects_missing_tool_output() {
        let trace = WorkflowTrace::new("workflow-1".to_string());
        
        let helper = ReplayHelper::new(trace);
        let result = helper.get_tool_output("node-1", "nonexistent-tool");
        
        assert!(result.is_err());
        
        match result.unwrap_err() {
            ReplayError::ToolOutputMissing { .. } => {}
            _ => panic!("Expected ToolOutputMissing"),
        }
    }

    #[test]
    fn test_replay_does_not_execute_real_tool() {
        // This test verifies the replay logic doesn't call real tools
        let mut trace = WorkflowTrace::new("workflow-1".to_string());
        
        // Pre-record a tool output
        trace.record_node_execution(NodeExecution {
            node_id: "node-1".to_string(),
            node_name: "Task 1".to_string(),
            input: r#"{"query": "test"}"#.to_string(),
            output: r#"{"result": "recorded"}"#.to_string(),
            tool_invocations: vec![ToolInvocation {
                node_id: "node-1".to_string(),
                tool_name: "search".to_string(),
                input: r#"{"query": "test"}"#.to_string(),
                output: r#"{"result": "recorded"}"#.to_string(),
            }],
        });
        
        // In replay mode, we get the recorded output without calling real tool
        let mut helper = ReplayHelper::new(trace);
        
        // Get next node
        let node = helper.next_node();
        assert!(node.is_some());
        assert_eq!(node.unwrap().node_id, "node-1");
        
        // Get tool output from trace - no real tool called
        let tool_result = helper.get_tool_output("node-1", "search");
        assert!(tool_result.is_ok());
        assert_eq!(tool_result.unwrap(), r#"{"result": "recorded"}"#);
    }

    #[test]
    fn test_record_replay_identical_result() {
        // Test: Record -> Replay -> identical result
        let mut trace = WorkflowTrace::new("workflow-1".to_string());
        
        // Phase 1: Record
        trace.record_node_execution(NodeExecution {
            node_id: "node-1".to_string(),
            node_name: "Task 1".to_string(),
            input: r#"{"data": "test"}"#.to_string(),
            output: r#"{"processed": true}"#.to_string(),
            tool_invocations: vec![],
        });
        
        trace.record_node_execution(NodeExecution {
            node_id: "node-2".to_string(),
            node_name: "Task 2".to_string(),
            input: r#"{"processed": true}"#.to_string(),
            output: r#"{"final": "result"}"#.to_string(),
            tool_invocations: vec![],
        });
        
        // Phase 2: Replay
        let mut replay = ReplayHelper::new(trace);
        
        // Get first node output
        let node1 = replay.next_node().unwrap();
        assert_eq!(node1.output, r#"{"processed": true}"#);
        
        // Get second node output
        let node2 = replay.next_node().unwrap();
        assert_eq!(node2.output, r#"{"final": "result"}"#);
        
        // Verify replay is complete
        assert!(replay.is_complete());
    }

    #[test]
    fn test_replay_error_display() {
        let error = ReplayError::NodeOrderMismatch {
            expected: "node-1 -> node-2".to_string(),
            actual: "node-1 -> node-3".to_string(),
        };
        
        let display = format!("{}", error);
        assert!(display.contains("Node order mismatch"));
    }
}
