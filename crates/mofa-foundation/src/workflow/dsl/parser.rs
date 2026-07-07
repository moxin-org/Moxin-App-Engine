//! Workflow DSL Parser
//!
//! Parses YAML/TOML workflow definitions and builds executable workflows.

use super::env::substitute_env_recursive;
use super::schema::*;
use super::{DslError, DslResult};
use crate::llm::LLMAgent;
use crate::workflow::builder::WorkflowBuilder;
use crate::workflow::node::RetryPolicy as NodeRetryPolicy;
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
        let json_value: serde_json::Value = serde_json::to_value(&value)
            .map_err(|e| DslError::Validation(format!("TOML to JSON conversion error: {}", e)))?;
        let substituted = substitute_env_recursive(&json_value);
        let def: WorkflowDefinition = serde_json::from_value(substituted)?;
        Ok(def)
    }

    /// Parse workflow definition from file (auto-detect format)
    pub fn from_file(path: impl AsRef<Path>) -> DslResult<WorkflowDefinition> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)?;

        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| DslError::Validation("No file extension".to_string()))?;

        match extension.to_lowercase().as_str() {
            "yaml" | "yml" => Self::from_yaml(&content),
            "toml" => Self::from_toml(&content),
            _ => Err(DslError::Validation(format!(
                "Unsupported file extension: {}",
                extension
            ))),
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

        // Keep node-level config so it can be applied after graph construction.
        // Builder APIs don't currently expose full NodeConfig customization.
        let node_configs = Self::collect_node_configs(&definition.nodes);

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

        let mut graph = builder.build();
        Self::apply_workflow_and_node_config(&mut graph, &definition.config, &node_configs);
        Ok(graph)
    }

    fn collect_node_configs(nodes: &[NodeDefinition]) -> HashMap<String, NodeConfigDef> {
        let mut configs = HashMap::new();
        for node in nodes {
            let (id, config) = match node {
                NodeDefinition::Task { id, config, .. }
                | NodeDefinition::LlmAgent { id, config, .. }
                | NodeDefinition::Condition { id, config, .. }
                | NodeDefinition::Parallel { id, config, .. }
                | NodeDefinition::Join { id, config, .. }
                | NodeDefinition::Loop { id, config, .. }
                | NodeDefinition::Transform { id, config, .. }
                | NodeDefinition::SubWorkflow { id, config, .. }
                | NodeDefinition::Wait { id, config, .. } => (id, config),
                NodeDefinition::Start { .. } | NodeDefinition::End { .. } => continue,
            };
            configs.insert(id.clone(), config.clone());
        }
        configs
    }

    fn apply_workflow_and_node_config(
        graph: &mut crate::workflow::WorkflowGraph,
        workflow_config: &WorkflowConfig,
        node_configs: &HashMap<String, NodeConfigDef>,
    ) {
        let node_ids: Vec<String> = graph
            .node_ids()
            .into_iter()
            .map(|id| id.to_string())
            .collect();
        for node_id in node_ids {
            let Some(node) = graph.get_node_mut(&node_id) else {
                continue;
            };

            if let Some(retry_policy) = &workflow_config.retry_policy {
                node.config.retry_policy = Self::convert_retry_policy(retry_policy);
            }
            node.config.timeout.execution_timeout_ms = workflow_config.default_timeout_ms;

            if let Some(node_cfg) = node_configs.get(&node_id) {
                if let Some(retry_policy) = &node_cfg.retry_policy {
                    node.config.retry_policy = Self::convert_retry_policy(retry_policy);
                }
                if let Some(timeout_ms) = node_cfg.timeout_ms {
                    node.config.timeout.execution_timeout_ms = timeout_ms;
                }
                for (key, value) in &node_cfg.metadata {
                    node.config.metadata.insert(key.clone(), value.clone());
                }
            }
        }
    }

    fn convert_retry_policy(policy: &RetryPolicy) -> NodeRetryPolicy {
        NodeRetryPolicy {
            max_retries: policy.max_retries,
            retry_delay_ms: policy.retry_delay_ms,
            exponential_backoff: policy.exponential_backoff,
            max_delay_ms: policy.max_delay_ms,
        }
    }

    /// Validate workflow definition
    fn validate(definition: &WorkflowDefinition) -> DslResult<()> {
        // Check for required nodes
        let node_ids: Vec<&str> = definition.nodes.iter().map(|n| n.id()).collect();

        // Verify start node exists
        if !node_ids.iter().any(|&id| {
            definition
                .nodes
                .iter()
                .any(|n| matches!(n, NodeDefinition::Start { id: start_id, .. } if start_id == id))
        }) {
            return Err(DslError::Validation(
                "Workflow must have a start node".to_string(),
            ));
        }

        // Verify end node exists
        if !node_ids.iter().any(|&id| {
            definition
                .nodes
                .iter()
                .any(|n| matches!(n, NodeDefinition::End { id: end_id, .. } if end_id == id))
        }) {
            return Err(DslError::Validation(
                "Workflow must have an end node".to_string(),
            ));
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
                        return Err(DslError::Validation(
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
                    return Err(DslError::Validation(
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

#[cfg(test)]
mod tests {
    use super::WorkflowDslParser;
    use crate::llm::LLMAgent;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test]
    async fn workflow_retry_and_timeout_defaults_propagate() {
        let yaml = r#"
metadata:
  id: wf_defaults
  name: Workflow Defaults
config:
  default_timeout_ms: 42000
  retry_policy:
    max_retries: 7
    retry_delay_ms: 250
    exponential_backoff: false
    max_delay_ms: 1000
nodes:
  - type: start
    id: start
  - type: task
    id: task_a
    name: Task A
    executor_type: none
  - type: end
    id: end
edges:
  - from: start
    to: task_a
  - from: task_a
    to: end
"#;

        let def = WorkflowDslParser::from_yaml(yaml).expect("DSL should parse");
        let registry: HashMap<String, Arc<LLMAgent>> = HashMap::new();
        let graph = WorkflowDslParser::build_with_agents(def, &registry)
            .await
            .expect("workflow should build");

        let task = graph.get_node("task_a").expect("task node should exist");
        assert_eq!(task.config.retry_policy.max_retries, 7);
        assert_eq!(task.config.retry_policy.retry_delay_ms, 250);
        assert!(!task.config.retry_policy.exponential_backoff);
        assert_eq!(task.config.retry_policy.max_delay_ms, 1000);
        assert_eq!(task.config.timeout.execution_timeout_ms, 42000);
    }

    #[tokio::test]
    async fn node_retry_and_timeout_override_workflow_defaults() {
        let yaml = r#"
metadata:
  id: wf_override
  name: Workflow Override
config:
  default_timeout_ms: 60000
  retry_policy:
    max_retries: 9
    retry_delay_ms: 500
    exponential_backoff: true
    max_delay_ms: 30000
nodes:
  - type: start
    id: start
  - type: task
    id: task_b
    name: Task B
    executor_type: none
    config:
      timeout_ms: 1500
      retry_policy:
        max_retries: 1
        retry_delay_ms: 10
        exponential_backoff: false
        max_delay_ms: 10
      metadata:
        owner: workflow-team
  - type: end
    id: end
edges:
  - from: start
    to: task_b
  - from: task_b
    to: end
"#;

        let def = WorkflowDslParser::from_yaml(yaml).expect("DSL should parse");
        let registry: HashMap<String, Arc<LLMAgent>> = HashMap::new();
        let graph = WorkflowDslParser::build_with_agents(def, &registry)
            .await
            .expect("workflow should build");

        let task = graph.get_node("task_b").expect("task node should exist");
        assert_eq!(task.config.retry_policy.max_retries, 1);
        assert_eq!(task.config.retry_policy.retry_delay_ms, 10);
        assert!(!task.config.retry_policy.exponential_backoff);
        assert_eq!(task.config.retry_policy.max_delay_ms, 10);
        assert_eq!(task.config.timeout.execution_timeout_ms, 1500);
        assert_eq!(
            task.config.metadata.get("owner"),
            Some(&"workflow-team".to_string())
        );
    }
}
