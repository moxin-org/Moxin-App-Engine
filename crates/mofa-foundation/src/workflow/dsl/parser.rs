//! Workflow DSL Parser
//!
//! Parses YAML/TOML workflow definitions and builds executable workflows.

use super::env::substitute_env_recursive;
use super::schema::*;
use super::{DslError, DslResult};
use crate::llm::LLMAgent;
use crate::workflow::builder::WorkflowBuilder;
use crate::workflow::node::WorkflowNode;
use crate::workflow::state::WorkflowValue;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// Workflow DSL parser
pub struct WorkflowDslParser;

impl WorkflowDslParser {
    const SUPPORTED_OPERATORS: [&str; 6] = ["==", "!=", ">=", "<=", ">", "<"];

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
            NodeDefinition::Condition {
                id,
                name,
                condition,
                ..
            } => {
                if let ConditionDef::Value { operator, .. } = &condition
                    && !Self::SUPPORTED_OPERATORS.contains(&operator.as_str())
                {
                    return Err(DslError::Validation(format!(
                        "Unsupported condition operator: {}. Supported operators: {}",
                        operator,
                        Self::SUPPORTED_OPERATORS.join(", ")
                    )));
                }

                builder = builder.node(WorkflowNode::condition(&id, &name, move |_ctx, input| {
                    let condition = condition.clone();
                    async move { Self::evaluate_condition_def(&condition, &input) }
                }));
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

    fn evaluate_condition_def(condition: &ConditionDef, input: &WorkflowValue) -> bool {
        match condition {
            ConditionDef::Expression { expr } => Self::evaluate_expression(expr, input),
            ConditionDef::Value {
                field,
                operator,
                value,
            } => {
                let Some(left) = Self::extract_field_value(input, field) else {
                    return false;
                };
                Self::compare_values(&left, operator, value)
            }
        }
    }

    fn evaluate_expression(expr: &str, input: &WorkflowValue) -> bool {
        let expr = expr.trim();
        if expr.eq_ignore_ascii_case("true") {
            return true;
        }
        if expr.eq_ignore_ascii_case("false") {
            return false;
        }

        for operator in Self::SUPPORTED_OPERATORS {
            if let Some((left, right)) = expr.split_once(operator) {
                let lhs = left.trim();
                let rhs = right.trim();
                let Some(left_val) = Self::extract_field_value(input, lhs) else {
                    return false;
                };
                let right_val = Self::parse_literal_value(rhs);
                return Self::compare_values(&left_val, operator, &right_val);
            }
        }

        Self::extract_field_value(input, expr)
            .as_ref()
            .is_some_and(Self::truthy)
    }

    fn extract_field_value(input: &WorkflowValue, field: &str) -> Option<serde_json::Value> {
        let mut current = serde_json::to_value(input).ok()?;
        let normalized = field.trim();
        if normalized.is_empty() {
            return None;
        }

        if normalized == "input" {
            return Some(current);
        }

        let path = normalized.strip_prefix("input.").unwrap_or(normalized);
        for segment in path.split('.') {
            let key = segment.trim();
            if key.is_empty() {
                return None;
            }
            current = current.get(key)?.clone();
        }
        Some(current)
    }

    fn parse_literal_value(raw: &str) -> serde_json::Value {
        let value = raw.trim();
        if value.is_empty() {
            return serde_json::Value::Null;
        }

        if (value.starts_with('\'') && value.ends_with('\''))
            || (value.starts_with('"') && value.ends_with('"'))
        {
            return serde_json::Value::String(value[1..value.len() - 1].to_string());
        }

        serde_json::from_str(value).unwrap_or_else(|_| serde_json::Value::String(value.to_string()))
    }

    fn compare_values(left: &serde_json::Value, operator: &str, right: &serde_json::Value) -> bool {
        match operator {
            "==" => left == right,
            "!=" => left != right,
            ">" | ">=" | "<" | "<=" => {
                if let (Some(l), Some(r)) = (left.as_f64(), right.as_f64()) {
                    return match operator {
                        ">" => l > r,
                        ">=" => l >= r,
                        "<" => l < r,
                        "<=" => l <= r,
                        _ => false,
                    };
                }

                if let (Some(l), Some(r)) = (left.as_str(), right.as_str()) {
                    return match operator {
                        ">" => l > r,
                        ">=" => l >= r,
                        "<" => l < r,
                        "<=" => l <= r,
                        _ => false,
                    };
                }

                false
            }
            _ => false,
        }
    }

    fn truthy(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Null => false,
            serde_json::Value::Bool(b) => *b,
            serde_json::Value::Number(n) => n.as_f64().is_some_and(|v| v != 0.0),
            serde_json::Value::String(s) => !s.is_empty(),
            serde_json::Value::Array(v) => !v.is_empty(),
            serde_json::Value::Object(m) => !m.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{ExecutorConfig, WorkflowExecutor, WorkflowStatus};

    #[tokio::test]
    async fn test_condition_expression_routes_to_true_branch() {
        let yaml = r#"
metadata:
  id: expression_routing
  name: Expression Routing

nodes:
  - type: start
    id: start
  - type: condition
    id: route
    name: Route
    condition:
      condition_type: expression
      expr: "input.score >= 10"
  - type: task
    id: low
    name: Low Branch
    executor_type: none
  - type: task
    id: high
    name: High Branch
    executor_type: none
  - type: end
    id: end

edges:
  - from: start
    to: route
  - from: route
    to: high
    condition: "true"
  - from: route
    to: low
    condition: "false"
  - from: high
    to: end
  - from: low
    to: end
"#;

        let definition = WorkflowDslParser::from_yaml(yaml).expect("yaml should parse");
        let graph = WorkflowDslParser::build_with_agents(definition, &HashMap::new())
            .await
            .expect("graph should build");

        let executor = WorkflowExecutor::new(ExecutorConfig::default());
        let mut input = HashMap::new();
        input.insert("score".to_string(), WorkflowValue::Int(20));

        let record = executor
            .execute(&graph, WorkflowValue::Map(input))
            .await
            .expect("execution should succeed");

        assert!(matches!(record.status, WorkflowStatus::Completed));
        assert_eq!(
            record.outputs.get("route").and_then(WorkflowValue::as_str),
            Some("true")
        );
        assert!(record.outputs.contains_key("high"));
        assert!(!record.outputs.contains_key("low"));
    }

    #[tokio::test]
    async fn test_condition_value_routes_to_false_branch() {
        let yaml = r#"
metadata:
  id: value_routing
  name: Value Routing

nodes:
  - type: start
    id: start
  - type: condition
    id: route
    name: Route
    condition:
      condition_type: value
      field: category
      operator: "=="
      value: "billing"
  - type: task
    id: billing
    name: Billing Branch
    executor_type: none
  - type: task
    id: general
    name: General Branch
    executor_type: none
  - type: end
    id: end

edges:
  - from: start
    to: route
  - from: route
    to: billing
    condition: "true"
  - from: route
    to: general
    condition: "false"
  - from: billing
    to: end
  - from: general
    to: end
"#;

        let definition = WorkflowDslParser::from_yaml(yaml).expect("yaml should parse");
        let graph = WorkflowDslParser::build_with_agents(definition, &HashMap::new())
            .await
            .expect("graph should build");

        let executor = WorkflowExecutor::new(ExecutorConfig::default());
        let mut input = HashMap::new();
        input.insert(
            "category".to_string(),
            WorkflowValue::String("general".to_string()),
        );

        let record = executor
            .execute(&graph, WorkflowValue::Map(input))
            .await
            .expect("execution should succeed");

        assert!(matches!(record.status, WorkflowStatus::Completed));
        assert!(record.outputs.contains_key("general"));
        assert!(!record.outputs.contains_key("billing"));
    }

    #[tokio::test]
    async fn test_condition_value_rejects_unsupported_operator() {
        let yaml = r#"
metadata:
  id: bad_operator
  name: Bad Operator

nodes:
  - type: start
    id: start
  - type: condition
    id: route
    name: Route
    condition:
      condition_type: value
      field: score
      operator: "contains"
      value: 10
  - type: end
    id: end

edges:
  - from: start
    to: route
  - from: route
    to: end
"#;

        let definition = WorkflowDslParser::from_yaml(yaml).expect("yaml should parse");
        let err = match WorkflowDslParser::build_with_agents(definition, &HashMap::new()).await {
            Ok(_) => panic!("unsupported operator should be rejected"),
            Err(err) => err,
        };

        match err {
            DslError::Validation(msg) => assert!(msg.contains("Unsupported condition operator")),
            other => panic!("expected validation error, got: {:?}", other),
        }
    }
}
