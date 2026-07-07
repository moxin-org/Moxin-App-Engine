//! Workflow DSL Parser
//!
//! Parses YAML/TOML workflow definitions and builds executable workflows.

use super::env::substitute_env_recursive;
use super::schema::*;
use super::{DslError, DslResult};
use crate::llm::LLMAgent;
use crate::workflow::graph::EdgeConfig;
use crate::workflow::node::{RetryPolicy as NodeRetryPolicy, WorkflowNode};
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
        let json_value: serde_json::Value = serde_json::to_value(&value)?;
        let substituted = substitute_env_recursive(&json_value);
        let def: WorkflowDefinition = serde_json::from_value(substituted)?;
        Ok(def)
    }

    /// Parse workflow definition from TOML string
    pub fn from_toml(content: &str) -> DslResult<WorkflowDefinition> {
        let value: toml::Value = toml::from_str(content)?;
        let json_value: serde_json::Value = serde_json::to_value(&value)
            .map_err(|e| DslError::Validation(format!("TOML to JSON conversion error: {e}")))?;
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
                "Unsupported file extension: {extension}"
            ))),
        }
    }

    /// Build a workflow graph from definition.
    ///
    /// This path builds nodes directly into the graph to avoid implicit auto-wiring
    /// side effects from fluent builder methods.
    pub async fn build_with_agents(
        definition: WorkflowDefinition,
        agent_registry: &HashMap<String, Arc<LLMAgent>>,
    ) -> DslResult<crate::workflow::WorkflowGraph> {
        Self::validate(&definition)?;

        let mut graph = crate::workflow::WorkflowGraph::new(
            &definition.metadata.id,
            &definition.metadata.name,
        )
        .with_description(&definition.metadata.description);

        for node_def in definition.nodes {
            Self::add_node(&mut graph, node_def, agent_registry).await?;
        }

        for edge in definition.edges {
            let mut cfg = if let Some(condition) = &edge.condition {
                EdgeConfig::conditional(&edge.from, &edge.to, condition)
            } else {
                EdgeConfig::new(&edge.from, &edge.to)
            };

            if let Some(label) = edge.label.as_deref() {
                cfg = cfg.with_label(label);
            }

            graph.add_edge(cfg);
        }

        graph
            .validate()
            .map_err(|errs| DslError::Validation(errs.join("; ")))?;

        Ok(graph)
    }

    /// Validate workflow definition
    fn validate(definition: &WorkflowDefinition) -> DslResult<()> {
        if let Some(version) = definition.metadata.version.as_deref() {
            if version != "1.0.0" {
                return Err(DslError::Validation(format!(
                    "Unsupported workflow DSL version: {version}. Supported versions: [1.0.0]"
                )));
            }
        }

        let node_ids: Vec<&str> = definition.nodes.iter().map(|n| n.id()).collect();

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

        for node in &definition.nodes {
            if let NodeDefinition::LlmAgent { agent, .. } = node {
                match agent {
                    AgentRef::Registry { agent_id } => {
                        if !definition.agents.contains_key(agent_id) {
                            return Err(DslError::AgentNotFound(agent_id.clone()));
                        }
                    }
                    AgentRef::Inline(_) => {}
                }
            }
        }

        Ok(())
    }

    async fn add_node(
        graph: &mut crate::workflow::WorkflowGraph,
        node_def: NodeDefinition,
        agent_registry: &HashMap<String, Arc<LLMAgent>>,
    ) -> DslResult<()> {
        match node_def {
            NodeDefinition::Start { id, name } => {
                let mut node = WorkflowNode::start(&id);
                if let Some(custom_name) = name {
                    node.config.name = custom_name;
                }
                graph.add_node(node);
            }
            NodeDefinition::End { id, name } => {
                let mut node = WorkflowNode::end(&id);
                if let Some(custom_name) = name {
                    node.config.name = custom_name;
                }
                graph.add_node(node);
            }
            NodeDefinition::Task {
                id,
                name,
                executor,
                config,
            } => {
                let mut node = match executor {
                    TaskExecutorDef::None => {
                        WorkflowNode::task(&id, &name, |_ctx, input| async move { Ok(input) })
                    }
                    _ => {
                        return Err(DslError::Validation(
                            "Only 'none' executor type is currently supported for task nodes"
                                .to_string(),
                        ));
                    }
                };
                Self::apply_node_config(&mut node, &config);
                graph.add_node(node);
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
                        return Err(DslError::Build(
                            "Inline agent configuration requires a provider. Use agent registry instead."
                                .to_string(),
                        ));
                    }
                };

                let mut node = if let Some(template) = prompt_template {
                    WorkflowNode::llm_agent_with_template(&id, &name, llm_agent, template)
                } else {
                    WorkflowNode::llm_agent(&id, &name, llm_agent)
                };
                Self::apply_node_config(&mut node, &config);
                graph.add_node(node);
            }
            NodeDefinition::Condition {
                id,
                name,
                condition,
                config,
            } => {
                let mut node = match condition {
                    ConditionDef::Expression { expr } => {
                        let expr_lc = expr.to_lowercase();
                        WorkflowNode::condition(&id, &name, move |_ctx, input| {
                            let expr_lc = expr_lc.clone();
                            async move {
                                let input_text = input
                                    .as_str()
                                    .map(|s| s.to_lowercase())
                                    .unwrap_or_else(|| serde_json::to_string(&input).unwrap_or_default());
                                input_text.contains(&expr_lc)
                            }
                        })
                    }
                    ConditionDef::Value {
                        field,
                        operator,
                        value,
                    } => WorkflowNode::condition(&id, &name, move |_ctx, input| {
                        let field = field.clone();
                        let operator = operator.clone();
                        let value = value.clone();
                        async move {
                            let json_value = serde_json::to_value(&input).unwrap_or(serde_json::Value::Null);
                            let candidate = json_value
                                .get(&field)
                                .cloned()
                                .unwrap_or(serde_json::Value::Null);
                            match operator.as_str() {
                                "==" => candidate == value,
                                "!=" => candidate != value,
                                _ => false,
                            }
                        }
                    }),
                };
                Self::apply_node_config(&mut node, &config);
                graph.add_node(node);
            }
            NodeDefinition::Parallel { id, name, config } => {
                let mut node = WorkflowNode::parallel(&id, &name, vec![]);
                Self::apply_node_config(&mut node, &config);
                graph.add_node(node);
            }
            NodeDefinition::Join {
                id,
                name,
                wait_for,
                config,
            } => {
                let wait_refs: Vec<&str> = wait_for.iter().map(|s| s.as_str()).collect();
                let mut node = WorkflowNode::join(&id, &name, wait_refs);
                Self::apply_node_config(&mut node, &config);
                graph.add_node(node);
            }
            NodeDefinition::Loop {
                id,
                name,
                body,
                condition,
                max_iterations,
                config,
            } => {
                match body {
                    TaskExecutorDef::None => {
                        let mut node = WorkflowNode::loop_node(
                            &id,
                            &name,
                            |_ctx, input| async move { Ok(input) },
                            move |_ctx, input| {
                                let condition = condition.clone();
                                async move {
                                    match condition {
                                        LoopConditionDef::Count { max } => {
                                            let current = input.as_i64().unwrap_or(0);
                                            current < i64::from(max)
                                        }
                                        LoopConditionDef::While { .. } => true,
                                        LoopConditionDef::Until { .. } => false,
                                    }
                                }
                            },
                            if max_iterations == 0 { 1 } else { max_iterations },
                        );
                        Self::apply_node_config(&mut node, &config);
                        graph.add_node(node);
                    }
                    _ => {
                        return Err(DslError::Validation(
                            "Only 'none' executor type is currently supported for loop nodes"
                                .to_string(),
                        ));
                    }
                }
            }
            NodeDefinition::Transform {
                id,
                name,
                transform,
                config,
            } => {
                let mut node = match transform {
                    TransformDef::Template { template } => WorkflowNode::transform(
                        &id,
                        &name,
                        move |inputs| {
                            let template = template.clone();
                            async move {
                                let mut rendered = template.clone();
                                for (key, value) in inputs {
                                    let replacement = value
                                        .as_str()
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| serde_json::to_string(&value).unwrap_or_default());
                                    rendered = rendered
                                        .replace(&format!("{{{{ {} }}}}", key), &replacement)
                                        .replace(&format!("{{{{{}}}}}", key), &replacement);
                                }
                                WorkflowValue::String(rendered)
                            }
                        },
                    ),
                    TransformDef::Expression { expr } => WorkflowNode::transform(
                        &id,
                        &name,
                        move |_inputs| {
                            let expr = expr.clone();
                            async move { WorkflowValue::String(expr) }
                        },
                    ),
                    TransformDef::MapReduce { .. } => WorkflowNode::transform(
                        &id,
                        &name,
                        move |inputs| async move { WorkflowValue::Map(inputs) },
                    ),
                };
                Self::apply_node_config(&mut node, &config);
                graph.add_node(node);
            }
            NodeDefinition::SubWorkflow {
                id,
                name,
                workflow_id,
                config,
            } => {
                let mut node = WorkflowNode::sub_workflow(&id, &name, &workflow_id);
                Self::apply_node_config(&mut node, &config);
                graph.add_node(node);
            }
            NodeDefinition::Wait {
                id,
                name,
                event_type,
                config,
            } => {
                let mut node = WorkflowNode::wait(&id, &name, &event_type);
                Self::apply_node_config(&mut node, &config);
                graph.add_node(node);
            }
        }

        Ok(())
    }

    fn apply_node_config(node: &mut WorkflowNode, config: &NodeConfigDef) {
        if let Some(timeout_ms) = config.timeout_ms {
            *node = node.clone().with_timeout(timeout_ms);
        }

        if let Some(retry) = &config.retry_policy {
            let policy = NodeRetryPolicy {
                max_retries: retry.max_retries,
                retry_delay_ms: retry.retry_delay_ms,
                exponential_backoff: retry.exponential_backoff,
                max_delay_ms: retry.max_delay_ms,
            };
            *node = node.clone().with_retry(policy);
        }

        for (k, v) in &config.metadata {
            node.config.metadata.insert(k.clone(), v.clone());
        }
    }
}