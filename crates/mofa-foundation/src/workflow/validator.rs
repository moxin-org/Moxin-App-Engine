use super::graph::{EdgeConfig, EdgeType, WorkflowGraph};
use super::node::{NodeType, WorkflowNode};
use std::collections::{HashMap, HashSet};

/// Validation error severity levels
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
}

/// A specific validation issue found in the workflow graph
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub severity: Severity,
    pub node_id: Option<String>,
    pub message: String,
}

impl ValidationIssue {
    pub fn error(node_id: Option<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            node_id,
            message: message.into(),
        }
    }

    pub fn warning(node_id: Option<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            node_id,
            message: message.into(),
        }
    }
}

/// Statistics about the validated graph
#[derive(Debug, Clone, Default)]
pub struct GraphStats {
    pub total_nodes: usize,
    pub total_edges: usize,
    pub start_nodes: usize,
    pub end_nodes: usize,
}

/// The final report produced by examining a workflow graph
#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub issues: Vec<ValidationIssue>,
    pub stats: GraphStats,
}

impl ValidationReport {
    pub fn new() -> Self {
        Self {
            issues: Vec::new(),
            stats: GraphStats::default(),
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.issues.iter().any(|i| i.severity == Severity::Error)
    }

    pub fn errors(&self) -> impl Iterator<Item = &ValidationIssue> {
        self.issues.iter().filter(|i| i.severity == Severity::Error)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &ValidationIssue> {
        self.issues.iter().filter(|i| i.severity == Severity::Warning)
    }
}

/// Engine to validate a `WorkflowGraph` statically
pub struct WorkflowValidator;

impl WorkflowValidator {
    /// Validate the given graph without executing it
    pub fn validate(graph: &WorkflowGraph) -> ValidationReport {
        let mut report = ValidationReport::new();

        report.stats.total_nodes = graph.node_count();
        report.stats.total_edges = graph.edge_count();

        // 1. Structural Validation
        Self::validate_structure(graph, &mut report);

        // 2. Connectivity Validation  
        Self::validate_connectivity(graph, &mut report);

        // 3. Cycle Detection in Normal loops
        // The graph inherently checks for cycles via topological_sort internally,
        // but we want to map those specifically if they aren't explicit `Loop` nodes.
        Self::validate_cycles(graph, &mut report);

        report
    }

    fn validate_structure(graph: &WorkflowGraph, report: &mut ValidationReport) {
        let start_node = graph.start_node();
        let end_nodes = graph.end_nodes();

        report.stats.start_nodes = if start_node.is_some() { 1 } else { 0 };
        report.stats.end_nodes = end_nodes.len();

        if start_node.is_none() {
            report.issues.push(ValidationIssue::error(
                None,
                "Workflow has no Start node.",
            ));
        }

        if end_nodes.is_empty() {
            report.issues.push(ValidationIssue::error(
                None,
                "Workflow has no End node.",
            ));
        }
        
        // Ensure starting point has no incoming edges
        if let Some(start) = start_node {
            if !graph.get_incoming_edges(start).is_empty() {
                report.issues.push(ValidationIssue::warning(
                    Some(start.to_string()),
                    "Start node should typically not have incoming edges.",
                ));
            }
        }
    }

    fn validate_connectivity(graph: &WorkflowGraph, report: &mut ValidationReport) {
        let node_ids = graph.node_ids();

        for node_id in &node_ids {
            let outgoing = graph.get_outgoing_edges(node_id);
            let incoming = graph.get_incoming_edges(node_id);
            let node = graph.get_node(node_id).unwrap();

            // All edges must point to existing nodes (The core graph enforces this partially, but good to assert)
            for edge in outgoing {
                if graph.get_node(&edge.to).is_none() {
                    report.issues.push(ValidationIssue::error(
                        Some(node_id.to_string()),
                        format!("Dangling edge points to non-existent node '{}'", edge.to),
                    ));
                }
            }

            // Check if node is unreachable
            if *node_id != graph.start_node().unwrap_or("") && incoming.is_empty() {
                report.issues.push(ValidationIssue::warning(
                    Some(node_id.to_string()),
                    format!("Node '{}' is unreachable (no incoming edges).", node_id),
                ));
            }

            // End nodes shouldn't have outgoing references typically
            if matches!(node.node_type(), NodeType::End) && !outgoing.is_empty() {
                report.issues.push(ValidationIssue::warning(
                    Some(node_id.to_string()),
                    "End node has outgoing edges.",
                ));
            }
        }
    }

    fn validate_cycles(graph: &WorkflowGraph, report: &mut ValidationReport) {
        if graph.has_cycle() {
            report.issues.push(ValidationIssue::error(
                None,
                "Unintentional cycle detected in graph. For explicit loops, use a Loop node.",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::node::NodeType;

    #[test]
    fn test_empty_graph_validation() {
        let graph = WorkflowGraph::new("g1", "Empty Graph");
        let report = WorkflowValidator::validate(&graph);
        assert!(!report.is_valid());
        assert_eq!(report.errors().count(), 2); // No start, No end
    }

    #[test]
    fn test_valid_basic_graph() {
        let mut graph = WorkflowGraph::new("g1", "Valid Graph");
        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::end("end"));
        graph.connect("start", "end");

        let report = WorkflowValidator::validate(&graph);
        assert!(report.is_valid());
        assert_eq!(report.stats.total_nodes, 2);
        assert_eq!(report.stats.total_edges, 1);
    }

    #[test]
    fn test_unreachable_node_warning() {
        let mut graph = WorkflowGraph::new("g1", "Unreachable Node");
        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::end("end"));
        graph.add_node(WorkflowNode::task("task", "Task", |_ctx, input| async move { Ok(input) }));
        
        graph.connect("start", "end");
        // "task" is left floating

        let report = WorkflowValidator::validate(&graph);
        assert!(report.is_valid()); // Warnings don't invalidate
        assert_eq!(report.warnings().count(), 1);
        let warning = report.warnings().next().unwrap();
        assert!(warning.message.contains("unreachable"));
    }
}
