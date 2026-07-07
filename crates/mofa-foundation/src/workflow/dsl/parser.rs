//! Workflow DSL Parser
//!
//! Parses YAML/TOML workflow definitions and builds executable workflows.

use super::env::substitute_env_recursive;
use super::schema::*;
use super::{DslError, DslResult};
use crate::llm::LLMAgent;
use crate::workflow::builder::WorkflowBuilder;
use crate::workflow::state::WorkflowValue;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// Workflow DSL parser
pub struct WorkflowDslParser;

impl WorkflowDslParser {
    /// Parse workflow definition from YAML string
    pub fn from_yaml(content: &str) -> DslResult<WorkflowDefinition> {
        let value: serde_yaml::Value = serde_yaml::from_str(content)?;
        // Apply environment variable substitution
        let json_value: serde_json::Value = serde_json::to_value(&value)?;
        let substituted = substitute_env_recursive(&json_value);
        let def: WorkflowDefinition = serde_json::from_value(substituted)?;
        Ok(def)
    }

    /// Parse workflow definition from TOML string
    pub fn from_toml(content: &str) -> DslResult<WorkflowDefinition> {
        let value: toml::Value = toml::from_str(content)?;
        // Convert to JSON for env substitution
        let json_value: serde_json::Value =
            serde_json::to_value(&value).map_err(|_| DslError::TomlToJsonConversion)?;
        let substituted = substitute_env_recursive(&json_value);
        let def: WorkflowDefinition = serde_json::from_value(substituted)?;
        Ok(def)
    }

    /// Parse workflow definition from file (auto-detect format)
    pub fn from_file(path: impl AsRef<Path>) -> DslResult<WorkflowDefinition> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)?;

        let extension = path.extension().ok_or(DslError::MissingFileExtension)?;

        match extension.to_string_lossy().to_lowercase().as_str() {
            "yaml" | "yml" => Self::from_yaml(&content),
            "toml" => Self::from_toml(&content),
            _ => Err(DslError::UnsupportedFileExtension(
                extension.to_string_lossy().into_owned(),
            )),
        }
    }

    /// Build a workflow graph from definition
    ///
    /// This method requires a registry of pre-built LLMAgent instances.
    /// Agents referenced in the workflow definition must be available in the registry.
    pub async fn build_with_agents(
        definition: WorkflowDefinition,
        agent_registry: &HashMap<String, Arc<LLMAgent>>,
    ) -> DslResult<crate::workflow::WorkflowGraph> {
        // Validate definition
        Self::validate(&definition)?;

        // Build workflow
        let mut builder = WorkflowBuilder::new(&definition.metadata.id, &definition.metadata.name)
            .description(&definition.metadata.description);

        // Add nodes
        for node_def in definition.nodes {
            builder = Self::add_node(builder, node_def, agent_registry).await?;
        }

        // Add edges
        for edge in definition.edges {
            if let Some(condition) = &edge.condition {
                builder = builder.conditional_edge(&edge.from, &edge.to, condition);
            } else {
                builder = builder.edge(&edge.from, &edge.to);
            }
        }

        Ok(builder.build())
    }

    /// Validate workflow definition
    fn validate(definition: &WorkflowDefinition) -> DslResult<()> {
        // Check for required nodes
        let node_ids: Vec<&str> = definition.nodes.iter().map(|n| n.id()).collect();

        // Verify start node exists
        if !definition
            .nodes
            .iter()
            .any(|n| matches!(n, NodeDefinition::Start { .. }))
        {
            return Err(DslError::MissingStartNode);
        }

        // Verify end node exists
        if !definition
            .nodes
            .iter()
            .any(|n| matches!(n, NodeDefinition::End { .. }))
        {
            return Err(DslError::MissingEndNode);
        }

        // Verify all edge references are valid
        for edge in &definition.edges {
            if !node_ids.contains(&edge.from.as_str()) {
                return Err(DslError::InvalidEdge {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                });
            }
            if !node_ids.contains(&edge.to.as_str()) {
                return Err(DslError::InvalidEdge {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                });
            }
        }

        // Verify agent references
        for node in &definition.nodes {
            if let NodeDefinition::LlmAgent { agent, .. } = node {
                match agent {
                    AgentRef::Registry { agent_id } => {
                        if !definition.agents.contains_key(agent_id) {
                            return Err(DslError::AgentNotFound(agent_id.clone()));
                        }
                    }
                    AgentRef::Inline(_) => {
                        // Inline agents are self-contained
                    }
                }
            }
        }

        Ok(())
    }

    /// Add a node to the workflow builder
    async fn add_node(
        mut builder: WorkflowBuilder,
        node_def: NodeDefinition,
        agent_registry: &HashMap<String, Arc<LLMAgent>>,
    ) -> DslResult<WorkflowBuilder> {
        match node_def {
            NodeDefinition::Start { id, .. } => {
                builder = builder.start_with_id(&id);
            }
            NodeDefinition::End { id, .. } => {
                builder = builder.end_with_id(&id);
            }
            NodeDefinition::Task {
                id, name, executor, ..
            } => {
                // For now, tasks are limited to simple operations
                // More complex task execution will be added later
                match executor {
                    TaskExecutorDef::None => {
                        builder = builder.task(&id, &name, |_ctx, input| async move { Ok(input) });
                    }
                    _ => {
                        return Err(DslError::InvalidNodeType(
                            "Only 'none' executor type is currently supported for task nodes"
                                .to_string(),
                        ));
                    }
                }
            }
            NodeDefinition::LlmAgent {
                id,
                name,
                agent,
                prompt_template,
                ..
            } => {
                let llm_agent = match agent {
                    AgentRef::Registry { agent_id } => agent_registry
                        .get(agent_id.as_str())
                        .ok_or_else(|| DslError::AgentNotFound(agent_id.clone()))?
                        .clone(),
                    AgentRef::Inline(_) => {
                        // Build agent from inline config
                        // Note: This requires a provider to be available
                        // For now, we'll return an error
                        return Err(DslError::Build(
                            "Inline agent configuration requires a provider. Use agent registry instead.".to_string(),
                        ));
                    }
                };

                if let Some(template) = prompt_template {
                    builder = builder.llm_agent_with_template(&id, &name, llm_agent, template);
                } else {
                    builder = builder.llm_agent(&id, &name, llm_agent);
                }
            }
            NodeDefinition::Condition { id, name, .. } => {
                // Condition nodes need special handling - use the agent node type
                // with a custom executor that evaluates to true/false
                builder = builder.task(&id, &name, |_ctx, _input| async move {
                    Ok(WorkflowValue::Bool(true))
                });
            }
            NodeDefinition::Parallel { id, name, .. } => {
                // Parallel node - just mark it, actual parallelism handled by edges
                builder = builder.task(&id, &name, |_ctx, input| async move { Ok(input) });
            }
            NodeDefinition::Join {
                id, name, wait_for, ..
            } => {
                let wait_for_refs: Vec<&str> = wait_for.iter().map(|s| s.as_str()).collect();
                builder = builder.goto(&id);
                // Note: The join node will be connected later
                let _ = (id, name, wait_for_refs);
            }
            NodeDefinition::Loop { id, name, body, .. } => match body {
                TaskExecutorDef::None => {
                    builder = builder.loop_node(
                        &id,
                        &name,
                        |_ctx, input| async move { Ok(input) },
                        |_ctx, _input| async move { false },
                        10,
                    );
                }
                _ => {
                    return Err(DslError::InvalidNodeType(
                        "Loop body executor not supported yet".to_string(),
                    ));
                }
            },
            NodeDefinition::Transform { id, name, .. } => {
                builder = builder.transform(&id, &name, |inputs| async move {
                    inputs.get("input").cloned().unwrap_or(WorkflowValue::Null)
                });
            }
            NodeDefinition::SubWorkflow {
                id,
                name,
                workflow_id,
                ..
            } => {
                builder = builder.sub_workflow(&id, &name, &workflow_id);
            }
            NodeDefinition::Wait {
                id,
                name,
                event_type,
                ..
            } => {
                builder = builder.wait(&id, &name, &event_type);
            }
        }

        Ok(builder)
    }
}
