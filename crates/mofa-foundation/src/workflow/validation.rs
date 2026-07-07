//! Deterministic Workflow Validation Module
//!
//! This module provides validation utilities for workflow traces.
//! Phase 3 - Deterministic Validation
//!
//! Provides comparison, fingerprinting, and snapshot validation for workflow traces.

use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Execution status of a node
#[derive(Debug, Clone, PartialEq, Hash, Serialize, Deserialize)]
pub enum ExecutionStatus {
    /// Node completed successfully
    Success,
    /// Node failed
    Failed,
    /// Node is pending
    Pending,
    /// Node is running
    Running,
}

impl Default for ExecutionStatus {
    fn default() -> Self {
        ExecutionStatus::Pending
    }
}

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
    /// Execution status
    pub status: ExecutionStatus,
    /// Tool invocations within this node
    pub tool_invocations: Vec<ToolInvocation>,
}

impl NodeExecution {
    /// Creates a new NodeExecution with default status
    pub fn new(node_id: String, node_name: String, input: String, output: String) -> Self {
        Self {
            node_id,
            node_name,
            input,
            output,
            status: ExecutionStatus::Success,
            tool_invocations: Vec::new(),
        }
    }
}

/// Workflow trace - records workflow execution for validation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorkflowTrace {
    /// Workflow identifier
    pub workflow_id: String,
    /// Recorded node executions in order
    pub node_executions: Vec<NodeExecution>,
}

impl WorkflowTrace {
    /// Creates a new WorkflowTrace
    pub fn new(workflow_id: String) -> Self {
        Self {
            workflow_id,
            node_executions: Vec::new(),
        }
    }

    /// Records a node execution
    pub fn record_node_execution(&mut self, execution: NodeExecution) {
        self.node_executions.push(execution);
    }

    /// Gets all recorded node IDs in order
    pub fn node_order(&self) -> Vec<String> {
        self.node_executions
            .iter()
            .map(|n| n.node_id.clone())
            .collect()
    }
}

/// Output difference between expected and actual
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputDifference {
    /// Node ID where difference was found
    pub node_id: String,
    /// Expected output (JSON)
    pub expected: serde_json::Value,
    /// Actual output (JSON)
    pub actual: serde_json::Value,
}

/// Status difference between expected and actual
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusDifference {
    /// Node ID where difference was found
    pub node_id: String,
    /// Expected status
    pub expected: ExecutionStatus,
    /// Actual status
    pub actual: ExecutionStatus,
}

/// Result of comparing two workflow traces
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceComparisonResult {
    /// Whether traces are identical
    pub identical: bool,
    /// Output differences found
    pub output_differences: Vec<OutputDifference>,
    /// Status differences found
    pub status_differences: Vec<StatusDifference>,
    /// Whether node order mismatch was detected
    pub order_mismatch: bool,
}

impl TraceComparisonResult {
    /// Creates a new TraceComparisonResult with all identical
    pub fn identical() -> Self {
        Self {
            identical: true,
            output_differences: Vec::new(),
            status_differences: Vec::new(),
            order_mismatch: false,
        }
    }
}

impl WorkflowTrace {
    /// Compares this trace with another trace
    /// 
    /// Returns structured differences between the two traces
    pub fn compare(&self, other: &WorkflowTrace) -> TraceComparisonResult {
        // Compare node order
        let self_order = self.node_order();
        let other_order = other.node_order();
        
        let order_mismatch = self_order != other_order;
        
        // Collect differences
        let mut output_differences = Vec::new();
        let mut status_differences = Vec::new();
        
        // Compare by position (only if same length)
        let min_len = self.node_executions.len().min(other.node_executions.len());
        
        for i in 0..min_len {
            let self_node = &self.node_executions[i];
            let other_node = &other.node_executions[i];
            
            // Compare outputs (try to parse as JSON)
            if self_node.output != other_node.output {
                let expected = serde_json::from_str(&self_node.output)
                    .unwrap_or(serde_json::Value::String(self_node.output.clone()));
                let actual = serde_json::from_str(&other_node.output)
                    .unwrap_or(serde_json::Value::String(other_node.output.clone()));
                
                if expected != actual {
                    output_differences.push(OutputDifference {
                        node_id: self_node.node_id.clone(),
                        expected,
                        actual,
                    });
                }
            }
            
            // Compare statuses
            if self_node.status != other_node.status {
                status_differences.push(StatusDifference {
                    node_id: self_node.node_id.clone(),
                    expected: self_node.status.clone(),
                    actual: other_node.status.clone(),
                });
            }
        }
        
        let identical = !order_mismatch 
            && output_differences.is_empty() 
            && status_differences.is_empty();
        
        TraceComparisonResult {
            identical,
            output_differences,
            status_differences,
            order_mismatch,
        }
    }
    
    /// Generates a deterministic fingerprint (hash) of the trace
    /// 
    /// Uses node order, execution status, and tool outputs for fingerprinting
    pub fn fingerprint(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        
        // Hash workflow ID
        self.workflow_id.hash(&mut hasher);
        
        // Hash each node in order
        for node in &self.node_executions {
            node.node_id.hash(&mut hasher);
            node.status.hash(&mut hasher);
            
            // Hash node output
            node.output.hash(&mut hasher);
            
            // Hash tool invocations
            for tool in &node.tool_invocations {
                tool.tool_name.hash(&mut hasher);
                tool.output.hash(&mut hasher);
            }
        }
        
        hasher.finish()
    }
    
    /// Validates this trace against an expected snapshot
    /// 
    /// Returns true only if traces are identical
    pub fn validate_snapshot(&self, expected: &WorkflowTrace) -> bool {
        self.compare(expected).identical
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn create_trace(workflow_id: &str, node_ids: &[&str]) -> WorkflowTrace {
        let mut trace = WorkflowTrace::new(workflow_id.to_string());
        for node_id in node_ids {
            trace.record_node_execution(NodeExecution::new(
                node_id.to_string(),
                format!("Node {}", node_id),
                "{}".to_string(),
                r#"{"result": "ok"}"#.to_string(),
            ));
        }
        trace
    }
    
    #[test]
    fn test_identical_traces_compare_identical() {
        let trace1 = create_trace("workflow-1", &["node-1", "node-2"]);
        let trace2 = create_trace("workflow-1", &["node-1", "node-2"]);
        
        let result = trace1.compare(&trace2);
        
        assert!(result.identical);
        assert!(!result.order_mismatch);
        assert!(result.output_differences.is_empty());
    }
    
    #[test]
    fn test_different_output_detects_difference() {
        let mut trace1 = WorkflowTrace::new("workflow-1".to_string());
        trace1.record_node_execution(NodeExecution::new(
            "node-1".to_string(),
            "Node 1".to_string(),
            "{}".to_string(),
            r#"{"value": 1}"#.to_string(),
        ));
        
        let mut trace2 = WorkflowTrace::new("workflow-1".to_string());
        trace2.record_node_execution(NodeExecution::new(
            "node-1".to_string(),
            "Node 1".to_string(),
            "{}".to_string(),
            r#"{"value": 2}"#.to_string(),
        ));
        
        let result = trace1.compare(&trace2);
        
        assert!(!result.identical);
        assert_eq!(result.output_differences.len(), 1);
    }
    
    #[test]
    fn test_different_order_detects_mismatch() {
        let trace1 = create_trace("workflow-1", &["node-1", "node-2"]);
        let trace2 = create_trace("workflow-1", &["node-2", "node-1"]);
        
        let result = trace1.compare(&trace2);
        
        assert!(!result.identical);
        assert!(result.order_mismatch);
    }
    
    #[test]
    fn test_different_status_detects_difference() {
        let mut trace1 = WorkflowTrace::new("workflow-1".to_string());
        trace1.record_node_execution(NodeExecution {
            node_id: "node-1".to_string(),
            node_name: "Node 1".to_string(),
            input: "{}".to_string(),
            output: "{}".to_string(),
            status: ExecutionStatus::Success,
            tool_invocations: vec![],
        });
        
        let mut trace2 = WorkflowTrace::new("workflow-1".to_string());
        trace2.record_node_execution(NodeExecution {
            node_id: "node-1".to_string(),
            node_name: "Node 1".to_string(),
            input: "{}".to_string(),
            output: "{}".to_string(),
            status: ExecutionStatus::Failed,
            tool_invocations: vec![],
        });
        
        let result = trace1.compare(&trace2);
        
        assert!(!result.identical);
        assert_eq!(result.status_differences.len(), 1);
    }
    
    #[test]
    fn test_fingerprint_equality_identical_traces() {
        let trace1 = create_trace("workflow-1", &["node-1", "node-2"]);
        let trace2 = create_trace("workflow-1", &["node-1", "node-2"]);
        
        assert_eq!(trace1.fingerprint(), trace2.fingerprint());
    }
    
    #[test]
    fn test_fingerprint_inequality_different_traces() {
        let trace1 = create_trace("workflow-1", &["node-1"]);
        let trace2 = create_trace("workflow-1", &["node-2"]);
        
        assert_ne!(trace1.fingerprint(), trace2.fingerprint());
    }
    
    #[test]
    fn test_validate_snapshot_identical() {
        let trace1 = create_trace("workflow-1", &["node-1", "node-2"]);
        let trace2 = create_trace("workflow-1", &["node-1", "node-2"]);
        
        assert!(trace1.validate_snapshot(&trace2));
    }
    
    #[test]
    fn test_validate_snapshot_different() {
        let trace1 = create_trace("workflow-1", &["node-1", "node-2"]);
        let trace2 = create_trace("workflow-1", &["node-2", "node-1"]);
        
        assert!(!trace1.validate_snapshot(&trace2));
    }
}
