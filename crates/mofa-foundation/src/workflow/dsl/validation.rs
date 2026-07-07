use super::schema::*;
use std::collections::HashSet;
use thiserror::Error;
#[derive(Debug, Error)]
pub enum ValidationError {
        #[error("Missing workflow metadata id")]
    MissingWorkflowId,
        #[error("Duplicate node ID: '{node_id}'")]
    DuplicateNodeId { node_id: String },
        #[error("Edge references unknown node: '{node_id}'")]
    UnknownNodeReference { node_id: String },
        #[error("No start node found")]
    NoStartNode,
    #[error("Missing workflow metadata name")]
MissingWorkflowName,
#[error("Unreachable node detected: '{node_id}'")]
UnreachableNode { node_id: String },
}
pub fn validate_workflow(
    workflow: &WorkflowDefinition,
) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    check_metadata(workflow, &mut errors);
check_duplicate_ids(workflow, &mut errors);
check_node_references(workflow, &mut errors);
check_entry_point(workflow, &mut errors);
check_unreachable_nodes(workflow, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
fn check_metadata(workflow: &WorkflowDefinition, errors: &mut Vec<ValidationError>) {
    if workflow.metadata.id.trim().is_empty() {
        errors.push(ValidationError::MissingWorkflowId);
    }
}
fn check_duplicate_ids(workflow: &WorkflowDefinition, errors: &mut Vec<ValidationError>) {
    let mut seen = HashSet::new();

    for node in &workflow.nodes {
        let id = node.id().to_string();

        if !seen.insert(id.clone()) {
            errors.push(ValidationError::DuplicateNodeId { node_id: id });
        }
    }
}
fn check_node_references(workflow: &WorkflowDefinition, errors: &mut Vec<ValidationError>) {
    let node_ids: HashSet<_> = workflow.nodes.iter().map(|n| n.id()).collect();

    for edge in &workflow.edges {
        if !node_ids.contains(edge.from.as_str()) {
            errors.push(ValidationError::UnknownNodeReference {
                node_id: edge.from.clone(),
            });
        }

        if !node_ids.contains(edge.to.as_str()) {
            errors.push(ValidationError::UnknownNodeReference {
                node_id: edge.to.clone(),
            });
        }
    }
}
fn check_entry_point(workflow: &WorkflowDefinition, errors: &mut Vec<ValidationError>) {
    let has_start = workflow.nodes.iter().any(|n| {
        matches!(n, NodeDefinition::Start { .. })
    });

    if !has_start {
        errors.push(ValidationError::NoStartNode);
    }
}

    if workflow.metadata.name.trim().is_empty() {
        errors.push(ValidationError::MissingWorkflowName);
    }

fn check_unreachable_nodes(
    workflow: &WorkflowDefinition,
    errors: &mut Vec<ValidationError>,
) {
    use std::collections::{HashMap, HashSet, VecDeque};

    // Build adjacency list
    let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &workflow.edges {
        graph.entry(edge.from.as_str()).or_default().push(edge.to.as_str());
    }

    // Find start node
    let start = workflow.nodes.iter().find_map(|n| {
        if let NodeDefinition::Start { id, .. } = n {
            Some(id.as_str())
        } else {
            None
        }
    });

    let Some(start_id) = start else { return };

    // BFS traversal
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();

    queue.push_back(start_id);
    visited.insert(start_id);

    while let Some(current) = queue.pop_front() {
        if let Some(neighbors) = graph.get(current) {
            for &next in neighbors {
                if visited.insert(next) {
                    queue.push_back(next);
                }
            }
        }
    }

    // Find unreachable nodes
    for node in &workflow.nodes {
        let id = node.id();
        if !visited.contains(id) {
            errors.push(ValidationError::UnreachableNode {
                node_id: id.to_string(),
            });
        }
    }
}