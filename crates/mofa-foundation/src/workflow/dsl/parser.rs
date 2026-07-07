//! Workflow DSL Parser
//!
//! Parses YAML/TOML workflow definitions and builds executable workflows.

use super::env::substitute_env_recursive;
use super::schema::*;
use super::{DslError, DslResult};
use crate::llm::LLMAgent;
use crate::workflow::state::WorkflowValue;
use crate::workflow::{WorkflowGraph, WorkflowNode};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tracing::debug;

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
    ) -> DslResult<WorkflowGraph> {
        // Validate definition
        Self::validate(&definition)?;

        // Build workflow directly so the DSL only creates declared edges.
        let mut graph = WorkflowGraph::new(&definition.metadata.id, &definition.metadata.name)
            .with_description(&definition.metadata.description);

        // Add nodes
        for node_def in definition.nodes {
            let node = Self::build_node(node_def, agent_registry)?;
            graph.add_node(node);
        }

        // Add edges
        for edge in definition.edges {
            if let Some(condition) = &edge.condition {
                graph.connect_conditional(&edge.from, &edge.to, condition);
            } else {
                graph.connect(&edge.from, &edge.to);
            }
        }

        graph
            .validate()
            .map_err(|errors| DslError::Build(errors.join("; ")))?;

        Ok(graph)
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

    /// Build a workflow node from the DSL definition.
    fn build_node(
        node_def: NodeDefinition,
        agent_registry: &HashMap<String, Arc<LLMAgent>>,
    ) -> DslResult<WorkflowNode> {
        let mut node = match node_def {
            NodeDefinition::Start { id, .. } => WorkflowNode::start(&id),
            NodeDefinition::End { id, .. } => WorkflowNode::end(&id),
            NodeDefinition::Task {
                id, name, executor, config,
            } => {
                let mut n = match executor {
                    TaskExecutorDef::None => {
                        WorkflowNode::task(&id, &name, |_ctx, input| async move { Ok(input) })
                    }
                    TaskExecutorDef::Script { script } => {
                        let node_id = id.clone();
                        WorkflowNode::task(&id, &name, move |_ctx, input| {
                            let s = script.clone();
                            let n_id = node_id.clone();
                            async move {
                                debug!("Executing script task {}: {}", n_id, s);
                                Ok(input)
                            }
                        })
                    }
                    _ => {
                        return Err(DslError::Validation(format!(
                            "Task executor {:?} not supported yet",
                            executor
                        )));
                    }
                };
                Self::apply_node_config(&mut n, &config);
                n
            }
            NodeDefinition::LlmAgent {
                id,
                name,
                agent,
                prompt_template,
                config,
            } => {
                let llm_agent = match agent {
                    AgentRef::Registry { agent_id } => agent_registry
                        .get(agent_id.as_str())
                        .ok_or_else(|| DslError::AgentNotFound(agent_id.clone()))?
                        .clone(),
                    AgentRef::Inline(_) => {
                        // Inline agents are self-contained
                        return Err(DslError::Build(
                            "Inline agent configuration requires a provider. Use agent registry instead.".to_string(),
                        ));
                    }
                };

                let mut n = if let Some(template) = prompt_template {
                    WorkflowNode::llm_agent_with_template(&id, &name, llm_agent, template)
                } else {
                    WorkflowNode::llm_agent(&id, &name, llm_agent)
                };
                Self::apply_node_config(&mut n, &config);
                n
            }
            NodeDefinition::Condition {
                id,
                name,
                condition,
                config,
            } => {
                // Condition evaluation is still a placeholder until the DSL grows
                // a real expression runtime.
                let mut n = match condition {
                    ConditionDef::Expression { expr } => {
                        WorkflowNode::condition(&id, &name, move |_ctx, _input| {
                            let e = expr.clone();
                            async move {
                                debug!("Evaluating condition expression: {}", e);
                                true
                            }
                        })
                    }
                    _ => WorkflowNode::condition(&id, &name, |_ctx, _input| async move { true }),
                };
                Self::apply_node_config(&mut n, &config);
                n
            }
            NodeDefinition::Parallel { id, name, config } => {
                let mut n = WorkflowNode::parallel(&id, &name, vec![]);
                Self::apply_node_config(&mut n, &config);
                n
            }
            NodeDefinition::Join {
                id,
                name,
                wait_for,
                config,
            } => {
                let mut n =
                    WorkflowNode::join(&id, &name, wait_for.iter().map(String::as_str).collect());
                Self::apply_node_config(&mut n, &config);
                n
            }
            NodeDefinition::Loop {
                id,
                name,
                body,
                condition,
                max_iterations,
                config,
            } => {
                let mut n = match body {
                    TaskExecutorDef::None => WorkflowNode::loop_node(
                        &id,
                        &name,
                        |_ctx, input| async move { Ok(input) },
                        |_ctx, _input| async move { false },
                        max_iterations,
                    ),
                    _ => {
                        return Err(DslError::Validation(
                            "Loop body executor not supported yet".to_string(),
                        ));
                    }
                };
                Self::apply_node_config(&mut n, &config);
                n
            }
            NodeDefinition::Transform {
                id,
                name,
                transform,
                config,
            } => {
                let mut n = WorkflowNode::transform(&id, &name, |inputs| async move {
                    inputs.get("input").cloned().unwrap_or(WorkflowValue::Null)
                });
                Self::apply_node_config(&mut n, &config);
                n
            }
            NodeDefinition::SubWorkflow {
                id,
                name,
                workflow_id,
                config,
            } => {
                let mut n = WorkflowNode::sub_workflow(&id, &name, &workflow_id);
                Self::apply_node_config(&mut n, &config);
                n
            }
            NodeDefinition::Wait {
                id,
                name,
                event_type,
                config,
            } => {
                let mut n = WorkflowNode::wait(&id, &name, &event_type);
                Self::apply_node_config(&mut n, &config);
                n
            }
        };

        Ok(node)
    }

    /// Apply node configuration from DSL to the workflow node.
    fn apply_node_config(node: &mut WorkflowNode, config: &NodeConfigDef) {
        if let Some(retry) = &config.retry_policy {
            node.config.retry_policy = crate::workflow::node::RetryPolicy {
                max_retries: retry.max_retries,
                retry_delay_ms: retry.retry_delay_ms,
                exponential_backoff: retry.exponential_backoff,
                max_delay_ms: retry.max_delay_ms,
            };
        }

        if let Some(timeout_ms) = config.timeout_ms {
            node.config.timeout.execution_timeout_ms = timeout_ms;
        }

        for (k, v) in &config.metadata {
            node.config.metadata.insert(k.clone(), v.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WorkflowDslParser;
    use crate::workflow::NodeType;
    use std::collections::HashMap;

    #[tokio::test]
    async fn build_with_agents_uses_only_declared_edges() {
        let yaml = r#"
metadata:
  id: ordered-probe
  name: Ordered Probe

nodes:
  - type: start
    id: start

  - type: task
    id: second
    name: Second
    executor_type: none

  - type: task
    id: first
    name: First
    executor_type: none

  - type: end
    id: end

edges:
  - from: start
    to: first
  - from: first
    to: second
  - from: second
    to: end
"#;

        let definition = WorkflowDslParser::from_yaml(yaml).unwrap();
        let graph = WorkflowDslParser::build_with_agents(definition, &HashMap::new())
            .await
            .unwrap();

        assert_eq!(graph.get_successors("start"), vec!["first"]);
        assert_eq!(graph.get_successors("first"), vec!["second"]);
        assert_eq!(graph.get_successors("second"), vec!["end"]);
        assert_eq!(graph.get_outgoing_edges("start").len(), 1);
        assert!(graph.validate().is_ok());
    }

    #[tokio::test]
    async fn build_with_agents_preserves_parallel_and_join_nodes() {
        let yaml = r#"
metadata:
  id: parallel-probe
  name: Parallel Probe

nodes:
  - type: start
    id: start

  - type: parallel
    id: fork
    name: Fork

  - type: task
    id: left
    name: Left
    executor_type: none

  - type: task
    id: right
    name: Right
    executor_type: none

  - type: join
    id: merge
    name: Merge
    wait_for:
      - left
      - right

  - type: end
    id: end

edges:
  - from: start
    to: fork
  - from: fork
    to: left
  - from: fork
    to: right
  - from: left
    to: merge
  - from: right
    to: merge
  - from: merge
    to: end
"#;

        let definition = WorkflowDslParser::from_yaml(yaml).unwrap();
        let graph = WorkflowDslParser::build_with_agents(definition, &HashMap::new())
            .await
            .unwrap();

        let fork = graph.get_node("fork").unwrap();
        let merge = graph.get_node("merge").unwrap();

        assert_eq!(fork.node_type(), &NodeType::Parallel);
        assert_eq!(merge.node_type(), &NodeType::Join);
        assert_eq!(graph.node_count(), 6);
        assert_eq!(graph.get_successors("start"), vec!["fork"]);
        assert_eq!(graph.get_successors("fork"), vec!["left", "right"]);
        assert_eq!(graph.get_successors("left"), vec!["merge"]);
        assert_eq!(graph.get_successors("right"), vec!["merge"]);
        assert_eq!(
            merge.join_nodes(),
            &["left".to_string(), "right".to_string()]
        );
        assert!(graph.validate().is_ok());
    }

    #[tokio::test]
    async fn build_with_agents_applies_node_configs() {
        let yaml = r#"
metadata:
  id: config-probe
  name: Config Probe

nodes:
  - type: start
    id: start

  - type: task
    id: task1
    name: Task 1
    executor_type: none
    config:
      retry_policy:
        max_retries: 5
        retry_delay_ms: 2000
      timeout_ms: 5000
      metadata:
        key1: value1

  - type: end
    id: end

edges:
  - from: start
    to: task1
  - from: task1
    to: end
"#;

        let definition = WorkflowDslParser::from_yaml(yaml).unwrap();
        let graph = WorkflowDslParser::build_with_agents(definition, &HashMap::new())
            .await
            .unwrap();

        let node = graph.get_node("task1").unwrap();
        assert_eq!(node.config.retry_policy.max_retries, 5);
        assert_eq!(node.config.retry_policy.retry_delay_ms, 2000);
        assert_eq!(node.config.timeout.execution_timeout_ms, 5000);
        assert_eq!(node.config.metadata.get("key1").unwrap(), "value1");
    }
}
