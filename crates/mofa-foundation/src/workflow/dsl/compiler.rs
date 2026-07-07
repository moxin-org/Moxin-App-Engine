//! DSL → StateGraph Compiler
//!
//! Bridges the declarative DSL workflow definitions to the StateGraph execution engine.
//! Transforms a parsed `WorkflowDefinition` into a `CompiledGraphImpl<JsonState>` that
//! can be invoked, streamed, or stepped through.
//!
//! # Example
//!
//! ```rust,ignore
//! use mofa_foundation::workflow::dsl::{WorkflowDslParser, DslCompiler};
//!
//! let yaml = std::fs::read_to_string("workflow.yaml")?;
//! let def = WorkflowDslParser::from_yaml(&yaml)?;
//! let compiled = DslCompiler::compile(def)?;
//! let result = compiled.invoke(JsonState::default(), None).await?;
//! ```

use super::schema::*;
use super::{DslError, DslResult};
use crate::llm::LLMAgent;
use crate::workflow::state_graph::{CompiledGraphImpl, StateGraphImpl};
use async_trait::async_trait;
use mofa_kernel::agent::error::AgentResult;
use mofa_kernel::workflow::{
    Command, END, GraphState, JsonState, NodeFunc, RuntimeContext, START, StateGraph,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

// Node Adapters — DSL NodeDefinition → Box<dyn NodeFunc<JsonState>>
/// A pass-through node that forwards state unchanged.
///
/// Used for Start, End, and Parallel placeholder nodes.
struct PassthroughNode {
    node_name: String,
}

impl PassthroughNode {
    fn new(name: impl Into<String>) -> Self {
        Self {
            node_name: name.into(),
        }
    }
}

#[async_trait]
impl NodeFunc<JsonState, Value> for PassthroughNode {
    async fn call(
        &self,
        _state: &mut JsonState,
        _ctx: &RuntimeContext<Value>,
    ) -> AgentResult<Command<Value>> {
        Ok(Command::new().continue_())
    }
    fn name(&self) -> &str {
        &self.node_name
    }
    fn description(&self) -> Option<&str> {
        Some("Pass-through node (no-op)")
    }
}

/// A task node that executes a no-op (placeholder for future executor support).
///
/// Currently only supports `TaskExecutorDef::None`. Other executor types
/// (Function, Http, Script) can be added in future PRs.
struct DslTaskNode {
    node_name: String,
    executor: TaskExecutorDef,
}

impl DslTaskNode {
    fn new(name: impl Into<String>, executor: TaskExecutorDef) -> Self {
        Self {
            node_name: name.into(),
            executor,
        }
    }
}

#[async_trait]
impl NodeFunc<JsonState, Value> for DslTaskNode {
    async fn call(
        &self,
        _state: &mut JsonState,
        _ctx: &RuntimeContext<Value>,
    ) -> AgentResult<Command<Value>> {
        match &self.executor {
            TaskExecutorDef::None => Ok(Command::new().continue_()),
            TaskExecutorDef::Function { function } => Ok(Command::new()
                .update(
                    &self.node_name,
                    serde_json::json!({
                        "type": "function",
                        "function": function,
                        "status": "placeholder"
                    }),
                )
                .continue_()),
            TaskExecutorDef::Http { url, method } => Ok(Command::new()
                .update(
                    &self.node_name,
                    serde_json::json!({
                        "type": "http",
                        "url": url,
                        "method": method.as_deref().unwrap_or("GET"),
                        "status": "placeholder"
                    }),
                )
                .continue_()),
            TaskExecutorDef::Script { script } => Ok(Command::new()
                .update(
                    &self.node_name,
                    serde_json::json!({
                        "type": "script",
                        "script_length": script.len(),
                        "status": "placeholder"
                    }),
                )
                .continue_()),
        }
    }
    fn name(&self) -> &str {
        &self.node_name
    }
    fn description(&self) -> Option<&str> {
        Some("DSL task node")
    }
}

/// A condition node that evaluates a condition and sets the route in state.
///
/// The condition result is stored in state under the node's ID key, which can
/// then be used by conditional edges to route to the appropriate next node.
struct DslConditionNode {
    node_name: String,
    condition: ConditionDef,
}

impl DslConditionNode {
    fn new(name: impl Into<String>, condition: ConditionDef) -> Self {
        Self {
            node_name: name.into(),
            condition,
        }
    }
}

#[async_trait]
impl NodeFunc<JsonState, Value> for DslConditionNode {
    async fn call(
        &self,
        state: &mut JsonState,
        _ctx: &RuntimeContext<Value>,
    ) -> AgentResult<Command<Value>> {
        let result = match &self.condition {
            ConditionDef::Expression { expr } => {
                // Currently returns the expression as the route key for conditional edges.
                // A full expression evaluator (e.g. Rhai) can be integrated in a future PR.
                serde_json::json!(expr)
            }
            ConditionDef::Value {
                field,
                operator,
                value,
            } => {
                let field_value: Option<Value> = state.get_value(field);
                let matches = match field_value {
                    Some(actual) => evaluate_condition(&actual, operator, value),
                    None => false,
                };
                serde_json::json!(if matches { "true" } else { "false" })
            }
        };
        Ok(Command::new().update(&self.node_name, result).continue_())
    }
    fn name(&self) -> &str {
        &self.node_name
    }
    fn description(&self) -> Option<&str> {
        Some("DSL condition node")
    }
}

/// A join node that waits for specified upstream nodes.
///
/// In the StateGraph model, join semantics are handled by the graph's edge
/// topology. This node stores the join metadata in state for observability.
struct DslJoinNode {
    node_name: String,
    wait_for: Vec<String>,
}

impl DslJoinNode {
    fn new(name: impl Into<String>, wait_for: Vec<String>) -> Self {
        Self {
            node_name: name.into(),
            wait_for,
        }
    }
}

#[async_trait]
impl NodeFunc<JsonState, Value> for DslJoinNode {
    async fn call(
        &self,
        _state: &mut JsonState,
        _ctx: &RuntimeContext<Value>,
    ) -> AgentResult<Command<Value>> {
        Ok(Command::new()
            .update(
                &self.node_name,
                serde_json::json!({
                    "type": "join",
                    "waited_for": self.wait_for,
                }),
            )
            .continue_())
    }
    fn name(&self) -> &str {
        &self.node_name
    }
    fn description(&self) -> Option<&str> {
        Some("DSL join node")
    }
}

/// A transform node that records the transform definition in state.
struct DslTransformNode {
    node_name: String,
    transform: TransformDef,
}

impl DslTransformNode {
    fn new(name: impl Into<String>, transform: TransformDef) -> Self {
        Self {
            node_name: name.into(),
            transform,
        }
    }
}

#[async_trait]
impl NodeFunc<JsonState, Value> for DslTransformNode {
    async fn call(
        &self,
        _state: &mut JsonState,
        _ctx: &RuntimeContext<Value>,
    ) -> AgentResult<Command<Value>> {
        let info = match &self.transform {
            TransformDef::Template { template } => serde_json::json!({
                "type": "template",
                "template": template,
            }),
            TransformDef::Expression { expr } => serde_json::json!({
                "type": "expression",
                "expr": expr,
            }),
            TransformDef::MapReduce { map, reduce } => serde_json::json!({
                "type": "map_reduce",
                "map": map,
                "reduce": reduce,
            }),
        };
        Ok(Command::new().update(&self.node_name, info).continue_())
    }
    fn name(&self) -> &str {
        &self.node_name
    }
    fn description(&self) -> Option<&str> {
        Some("DSL transform node")
    }
}

/// An agent node that routes execution to an LLM.
///
/// Sends the current `input` from state to the agent and stores the response.
struct DslAgentNode {
    node_name: String,
    agent: Arc<LLMAgent>,
    _config: AgentRef,
}

impl DslAgentNode {
    fn new(name: impl Into<String>, agent: Arc<LLMAgent>, config: AgentRef) -> Self {
        Self {
            node_name: name.into(),
            agent,
            _config: config,
        }
    }
}

#[async_trait]
impl NodeFunc<JsonState, Value> for DslAgentNode {
    async fn call(
        &self,
        state: &mut JsonState,
        _ctx: &RuntimeContext<Value>,
    ) -> AgentResult<Command<Value>> {
        // Find input argument depending on upstream context
        // Try `input` first, then error if missing
        let input_value: Option<Value> = state.get_value("input");
        let input_text = match input_value {
            Some(Value::String(s)) => s,
            Some(other) => other.to_string(),
            None => {
                let mut keys = state.keys();
                keys.sort();
                return Err(mofa_kernel::agent::error::AgentError::ExecutionFailed(
                    format!(
                        "Agent node '{}' requires 'input' key in state, but none found. Available keys: {:?}",
                        self.node_name, keys
                    ),
                ));
            }
        };

        // Use simple Q&A ask() for stateless workflow processing
        let response = self.agent.ask(input_text).await.map_err(|e| {
            mofa_kernel::agent::error::AgentError::ExecutionFailed(format!(
                "LLM Agent failed: {}",
                e
            ))
        })?;

        Ok(Command::new()
            .update(&self.node_name, serde_json::json!(response))
            .continue_())
    }
    fn name(&self) -> &str {
        &self.node_name
    }
    fn description(&self) -> Option<&str> {
        Some("DSL LLM Agent node")
    }
}

// Condition Evaluation Helper

/// Evaluate a simple condition: `actual <operator> expected`
fn evaluate_condition(actual: &Value, operator: &str, expected: &Value) -> bool {
    match operator {
        "==" | "eq" => actual == expected,
        "!=" | "ne" => actual != expected,
        ">" | "gt" => compare_values(actual, expected) == Some(std::cmp::Ordering::Greater),
        "<" | "lt" => compare_values(actual, expected) == Some(std::cmp::Ordering::Less),
        ">=" | "gte" => {
            matches!(
                compare_values(actual, expected),
                Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            )
        }
        "<=" | "lte" => {
            matches!(
                compare_values(actual, expected),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            )
        }
        "contains" => {
            if let (Some(haystack), Some(needle)) = (actual.as_str(), expected.as_str()) {
                haystack.contains(needle)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Compare two JSON values numerically
fn compare_values(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a.as_f64(), b.as_f64()) {
        (Some(a_num), Some(b_num)) => a_num.partial_cmp(&b_num),
        _ => match (a.as_str(), b.as_str()) {
            (Some(a_str), Some(b_str)) => Some(a_str.cmp(b_str)),
            _ => None,
        },
    }
}

// DSL Compiler

/// Compiles a parsed `WorkflowDefinition` into an executable `CompiledGraphImpl<JsonState>`.
///
/// This bridges the declarative DSL layer (YAML/TOML) to the StateGraph
/// execution engine, enabling users to define workflows in configuration
/// files and execute them via `invoke()`, `stream()`, or `step()`.
///
/// # Supported Node Types
///
/// | DSL Type       | Adapter            | Behavior                          |
/// |----------------|--------------------|-----------------------------------|
/// | `Start`        | `PassthroughNode`  | No-op entry point                 |
/// | `End`          | `PassthroughNode`  | No-op exit point                  |
/// | `Task`         | `DslTaskNode`      | Executor placeholder              |
/// | `Condition`    | `DslConditionNode` | Evaluates condition, sets route   |
/// | `Parallel`     | `PassthroughNode`  | Marker (parallelism via edges)    |
/// | `Join`         | `DslJoinNode`      | Records join metadata             |
/// | `Transform`    | `DslTransformNode` | Records transform definition      |
///
/// # Unsupported Node Types (Future PRs)
///
/// - `LlmAgent` — requires LLM provider registry
/// - `Loop` — requires runtime loop state machine
/// - `SubWorkflow` — requires recursive workflow loading
/// - `Wait` — requires external event system
pub struct DslCompiler;

impl DslCompiler {
    /// Compile a `WorkflowDefinition` into an executable `CompiledGraphImpl<JsonState>`.
    ///
    /// This performs validation, builds the graph, and compiles it in one step.
    /// Returns an error if any `LlmAgent` nodes are used, since no agent registry is provided.
    ///
    /// # Errors
    ///
    /// Returns `DslError` if:
    /// - The definition is invalid (no start/end node, dangling edges)
    /// - An `LlmAgent`, `Loop`, `SubWorkflow`, or `Wait` node type is used
    /// - Graph compilation fails (unreachable nodes, etc.)
    pub fn compile(def: WorkflowDefinition) -> DslResult<CompiledGraphImpl<JsonState>> {
        Self::compile_with_agents(def, &HashMap::new())
    }

    /// Compile a `WorkflowDefinition` with access to a registry of `LLMAgent` instances.
    ///
    /// This is required if the workflow definition contains `LlmAgent` nodes.
    ///
    /// # Errors
    ///
    /// Extends `compile()` errors, and will also return `DslError::MissingAgentInRegistry` if
    /// an `LlmAgent` node references an `agent_id` not found in the `agent_registry`.
    pub fn compile_with_agents(
        def: WorkflowDefinition,
        agent_registry: &HashMap<String, Arc<LLMAgent>>,
    ) -> DslResult<CompiledGraphImpl<JsonState>> {
        Self::validate(&def)?;
        let mut graph = StateGraphImpl::<JsonState>::build(&def.metadata.id);

        for node_def in &def.nodes {
            let node_func = Self::compile_node(node_def, agent_registry)?;
            let node_id = node_def.id();
            graph.add_node(node_id, node_func);
        }

        Self::wire_edges(&mut graph, &def)?;
        graph.compile().map_err(|e| DslError::Build(e.to_string()))
    }

    /// Compile a single `NodeDefinition` into a `Box<dyn NodeFunc<JsonState, Value>>`.
    fn compile_node(
        def: &NodeDefinition,
        agent_registry: &HashMap<String, Arc<LLMAgent>>,
    ) -> DslResult<Box<dyn NodeFunc<JsonState, Value>>> {
        match def {
            NodeDefinition::Start { id, .. } => {
                Ok(Box::new(PassthroughNode::new(id)) as Box<dyn NodeFunc<JsonState, Value>>)
            }
            NodeDefinition::End { id, .. } => {
                Ok(Box::new(PassthroughNode::new(id)) as Box<dyn NodeFunc<JsonState, Value>>)
            }
            NodeDefinition::Task { id, executor, .. } => {
                Ok(Box::new(DslTaskNode::new(id, executor.clone()))
                    as Box<dyn NodeFunc<JsonState, Value>>)
            }
            NodeDefinition::Condition { id, condition, .. } => {
                Ok(Box::new(DslConditionNode::new(id, condition.clone()))
                    as Box<dyn NodeFunc<JsonState, Value>>)
            }
            NodeDefinition::Parallel { id, .. } => {
                Ok(Box::new(PassthroughNode::new(id)) as Box<dyn NodeFunc<JsonState, Value>>)
            }
            NodeDefinition::Join { id, wait_for, .. } => {
                Ok(Box::new(DslJoinNode::new(id, wait_for.clone()))
                    as Box<dyn NodeFunc<JsonState, Value>>)
            }
            NodeDefinition::Transform { id, transform, .. } => {
                Ok(Box::new(DslTransformNode::new(id, transform.clone()))
                    as Box<dyn NodeFunc<JsonState, Value>>)
            }
            NodeDefinition::LlmAgent { id, agent, .. } => {
                let agent_id = match agent {
                    AgentRef::Registry { agent_id } => agent_id,
                    AgentRef::Inline(_) => {
                        return Err(DslError::InlineAgentNotSupported(id.to_string()));
                    }
                };
                if let Some(agent_instance) = agent_registry.get(agent_id) {
                    Ok(
                        Box::new(DslAgentNode::new(id, agent_instance.clone(), agent.clone()))
                            as Box<dyn NodeFunc<JsonState, Value>>,
                    )
                } else {
                    Err(DslError::MissingAgentInRegistry {
                        node_id: id.to_string(),
                        agent_id: agent_id.to_string(),
                    })
                }
            }
            NodeDefinition::Loop { id, .. } => Err(DslError::InvalidNodeType(format!(
                "Loop node '{}' is not yet supported by the DSL compiler.",
                id
            ))),
            NodeDefinition::SubWorkflow { id, .. } => Err(DslError::InvalidNodeType(format!(
                "SubWorkflow node '{}' is not yet supported by the DSL compiler.",
                id
            ))),
            NodeDefinition::Wait { id, .. } => Err(DslError::InvalidNodeType(format!(
                "Wait node '{}' is not yet supported by the DSL compiler.",
                id
            ))),
        }
    }

    /// Wire edges from the DSL definition into the StateGraph.
    fn wire_edges(
        graph: &mut StateGraphImpl<JsonState>,
        def: &WorkflowDefinition,
    ) -> DslResult<()> {
        let start_id = def
            .nodes
            .iter()
            .find(|n| matches!(n, NodeDefinition::Start { .. }))
            .map(|n| n.id().to_string())
            .ok_or(DslError::MissingStartNode)?;

        let end_id = def
            .nodes
            .iter()
            .find(|n| matches!(n, NodeDefinition::End { .. }))
            .map(|n| n.id().to_string())
            .ok_or(DslError::MissingEndNode)?;

        graph.set_entry_point(&start_id);
        graph.set_finish_point(&end_id);

        // Group conditional edges by source: from → [(condition, to)]
        let mut conditional_map: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for edge in &def.edges {
            if let Some(ref condition) = edge.condition {
                conditional_map
                    .entry(edge.from.clone())
                    .or_default()
                    .push((condition.clone(), edge.to.clone()));
            } else {
                graph.add_edge(&edge.from, &edge.to);
            }
        }

        for (from, conditions) in conditional_map {
            let conditions_map: HashMap<String, String> = conditions.into_iter().collect();
            graph.add_conditional_edges(&from, conditions_map);
        }
        Ok(())
    }

    /// Validate a workflow definition for compilation.
    ///
    /// This reuses the parser's validation logic and adds compiler-specific checks.
    fn validate(def: &WorkflowDefinition) -> DslResult<()> {
        let node_ids: Vec<&str> = def.nodes.iter().map(|n| n.id()).collect();

        if !def
            .nodes
            .iter()
            .any(|n| matches!(n, NodeDefinition::Start { .. }))
        {
            return Err(DslError::MissingStartNode);
        }

        if !def
            .nodes
            .iter()
            .any(|n| matches!(n, NodeDefinition::End { .. }))
        {
            return Err(DslError::MissingEndNode);
        }

        for edge in &def.edges {
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

        let mut seen = std::collections::HashSet::new();
        for id in &node_ids {
            if !seen.insert(*id) {
                return Err(DslError::DuplicateNodeId(id.to_string()));
            }
        }
        Ok(())
    }
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LLMAgentBuilder, MockLLMProvider};
    use mofa_kernel::workflow::CompiledGraph;
    use mofa_kernel::workflow::GraphState;

    /// Helper: parse YAML into WorkflowDefinition
    fn parse_yaml(yaml: &str) -> WorkflowDefinition {
        serde_yaml::from_str(yaml).expect("Failed to parse YAML")
    }

    // Compilation Tests

    #[test]
    fn test_compile_minimal_workflow() {
        let yaml = r#"
metadata:
  id: minimal
  name: Minimal Workflow
nodes:
  - type: start
    id: start
  - type: end
    id: end
edges:
  - from: start
    to: end
"#;
        let def = parse_yaml(yaml);
        let compiled = DslCompiler::compile(def);
        assert!(
            compiled.is_ok(),
            "Minimal workflow should compile: {:?}",
            compiled.err()
        );
    }

    #[test]
    fn test_compile_linear_workflow() {
        let yaml = r#"
metadata:
  id: linear
  name: Linear Workflow
nodes:
  - type: start
    id: start
  - type: task
    id: process
    name: Process Data
    executor_type: none
  - type: task
    id: validate
    name: Validate Result
    executor_type: none
  - type: end
    id: end
edges:
  - from: start
    to: process
  - from: process
    to: validate
  - from: validate
    to: end
"#;
        let def = parse_yaml(yaml);
        let compiled = DslCompiler::compile(def);
        assert!(
            compiled.is_ok(),
            "Linear workflow should compile: {:?}",
            compiled.err()
        );
    }

    #[tokio::test]
    async fn test_compile_and_invoke_roundtrip() {
        let yaml = r#"
metadata:
  id: roundtrip
  name: Round Trip Test
nodes:
  - type: start
    id: start
  - type: task
    id: process
    name: Process
    executor_type: none
  - type: end
    id: end
edges:
  - from: start
    to: process
  - from: process
    to: end
"#;
        let def = parse_yaml(yaml);
        let compiled = DslCompiler::compile(def).expect("Should compile");
        let result = compiled.invoke(JsonState::new(), None).await;
        assert!(result.is_ok(), "Invoke should succeed: {:?}", result.err());
    }

    #[test]
    fn test_compile_conditional_edges() {
        let yaml = r#"
metadata:
  id: conditional
  name: Conditional Workflow
nodes:
  - type: start
    id: start
  - type: condition
    id: check
    name: Check Value
    condition:
      condition_type: value
      field: score
      operator: ">="
      value: 80
  - type: task
    id: pass
    name: Pass
    executor_type: none
  - type: task
    id: fail
    name: Fail
    executor_type: none
  - type: end
    id: end
edges:
  - from: start
    to: check
  - from: check
    to: pass
    condition: "true"
  - from: check
    to: fail
    condition: "false"
  - from: pass
    to: end
  - from: fail
    to: end
"#;
        let def = parse_yaml(yaml);
        let compiled = DslCompiler::compile(def);
        assert!(
            compiled.is_ok(),
            "Conditional workflow should compile: {:?}",
            compiled.err()
        );
    }

    #[test]
    fn test_compile_with_transform() {
        let yaml = r#"
metadata:
  id: transform_test
  name: Transform Test
nodes:
  - type: start
    id: start
  - type: transform
    id: transform1
    name: Apply Template
    transform_type: template
    template: "Hello {{ name }}"
  - type: end
    id: end
edges:
  - from: start
    to: transform1
  - from: transform1
    to: end
"#;
        let def = parse_yaml(yaml);
        let compiled = DslCompiler::compile(def);
        assert!(
            compiled.is_ok(),
            "Transform workflow should compile: {:?}",
            compiled.err()
        );
    }

    #[test]
    fn test_compile_with_join() {
        let yaml = r#"
metadata:
  id: join_test
  name: Join Test
nodes:
  - type: start
    id: start
  - type: task
    id: branch_a
    name: Branch A
    executor_type: none
  - type: task
    id: branch_b
    name: Branch B
    executor_type: none
  - type: join
    id: merge
    name: Merge Results
    wait_for:
      - branch_a
      - branch_b
  - type: end
    id: end
edges:
  - from: start
    to: branch_a
  - from: start
    to: branch_b
  - from: branch_a
    to: merge
  - from: branch_b
    to: merge
  - from: merge
    to: end
"#;
        let def = parse_yaml(yaml);
        let compiled = DslCompiler::compile(def);
        assert!(
            compiled.is_ok(),
            "Join workflow should compile: {:?}",
            compiled.err()
        );
    }

    // Validation Error Tests

    #[test]
    fn test_compile_missing_start_node() {
        let yaml = r#"
metadata:
  id: no_start
  name: No Start
nodes:
  - type: end
    id: end
edges: []
"#;
        let def = parse_yaml(yaml);
        let err = match DslCompiler::compile(def) {
            Err(e) => e,
            Ok(_) => panic!("Expected error, got Ok"),
        };
        assert!(
            matches!(err, DslError::MissingStartNode),
            "Expected validation error about missing start, got: {:?}",
            err
        );
    }

    #[test]
    fn test_compile_missing_end_node() {
        let yaml = r#"
metadata:
  id: no_end
  name: No End
nodes:
  - type: start
    id: start
edges: []
"#;
        let def = parse_yaml(yaml);
        let err = match DslCompiler::compile(def) {
            Err(e) => e,
            Ok(_) => panic!("Expected error, got Ok"),
        };
        assert!(
            matches!(err, DslError::MissingEndNode),
            "Expected validation error about missing end, got: {:?}",
            err
        );
    }

    #[test]
    fn test_compile_invalid_edge_reference() {
        let yaml = r#"
metadata:
  id: bad_edge
  name: Bad Edge
nodes:
  - type: start
    id: start
  - type: end
    id: end
edges:
  - from: start
    to: nonexistent
"#;
        let def = parse_yaml(yaml);
        let err = match DslCompiler::compile(def) {
            Err(e) => e,
            Ok(_) => panic!("Expected error, got Ok"),
        };
        assert!(
            matches!(err, DslError::InvalidEdge { .. }),
            "Expected InvalidEdge error, got: {:?}",
            err
        );
    }

    #[test]
    fn test_compile_duplicate_node_ids() {
        let yaml = r#"
metadata:
  id: duplicates
  name: Duplicates
nodes:
  - type: start
    id: start
  - type: task
    id: start
    name: Duplicate
    executor_type: none
  - type: end
    id: end
edges:
  - from: start
    to: end
"#;
        let def = parse_yaml(yaml);
        let err = match DslCompiler::compile(def) {
            Err(e) => e,
            Ok(_) => panic!("Expected error, got Ok"),
        };
        assert!(
            matches!(err, DslError::DuplicateNodeId(_)),
            "Expected duplicate node ID error, got: {:?}",
            err
        );
    }

    #[test]
    fn test_compile_unsupported_llm_agent_missing_registry() {
        let yaml = r#"
metadata:
  id: llm_test_missing_registry
  name: LLM Test Missing Registry
nodes:
  - type: start
    id: start
  - type: llm_agent
    id: agent1
    name: My Agent
    agent:
      agent_id: test_missing
  - type: end
    id: end
edges:
  - from: start
    to: agent1
  - from: agent1
    to: end
"#;
        let def = parse_yaml(yaml);
        let err = match DslCompiler::compile(def) {
            Err(e) => e,
            Ok(_) => panic!("Expected error, got Ok"),
        };
        assert!(
            matches!(err, DslError::MissingAgentInRegistry { .. }),
            "Expected missing registry validation error, got: {:?}",
            err
        );
    }

    #[tokio::test]
    async fn test_llm_agent_missing_input_returns_safe_error() {
        let yaml = r#"
metadata:
  id: llm_missing_input
  name: LLM Missing Input
nodes:
  - type: start
    id: start
  - type: llm_agent
    id: agent1
    name: Classify
    agent:
      agent_id: classifier
  - type: end
    id: end
edges:
  - from: start
    to: agent1
  - from: agent1
    to: end
"#;
        let def = parse_yaml(yaml);

        let provider = Arc::new(MockLLMProvider::new("mock-provider").with_default_response("ok"));
        let agent = Arc::new(
            LLMAgentBuilder::new()
                .with_id("classifier")
                .with_name("Classifier")
                .with_provider(provider)
                .build(),
        );
        let mut registry = HashMap::new();
        registry.insert("classifier".to_string(), agent);

        let compiled =
            DslCompiler::compile_with_agents(def, &registry).expect("should compile with registry");

        let mut state = JsonState::new();
        state
            .apply_update("secret", serde_json::json!("token-123"))
            .await
            .unwrap();
        state
            .apply_update("tenant", serde_json::json!("acme"))
            .await
            .unwrap();

        let err = compiled.invoke(state, None).await.unwrap_err().to_string();
        assert!(err.contains("requires 'input' key"));
        assert!(err.contains("Available keys"));
        assert!(!err.contains("token-123"));
    }

    // Condition Evaluation Tests

    #[test]
    fn test_evaluate_condition_equality() {
        assert!(evaluate_condition(
            &serde_json::json!("hello"),
            "==",
            &serde_json::json!("hello")
        ));
        assert!(!evaluate_condition(
            &serde_json::json!("hello"),
            "==",
            &serde_json::json!("world")
        ));
    }

    #[test]
    fn test_evaluate_condition_numeric() {
        assert!(evaluate_condition(
            &serde_json::json!(10),
            ">",
            &serde_json::json!(5)
        ));
        assert!(evaluate_condition(
            &serde_json::json!(5),
            "<=",
            &serde_json::json!(5)
        ));
        assert!(!evaluate_condition(
            &serde_json::json!(3),
            ">=",
            &serde_json::json!(5)
        ));
    }

    #[test]
    fn test_evaluate_condition_contains() {
        assert!(evaluate_condition(
            &serde_json::json!("hello world"),
            "contains",
            &serde_json::json!("world")
        ));
        assert!(!evaluate_condition(
            &serde_json::json!("hello"),
            "contains",
            &serde_json::json!("world")
        ));
    }

    #[test]
    fn test_evaluate_condition_inequality() {
        assert!(evaluate_condition(
            &serde_json::json!(1),
            "!=",
            &serde_json::json!(2)
        ));
        assert!(!evaluate_condition(
            &serde_json::json!(1),
            "!=",
            &serde_json::json!(1)
        ));
    }

    #[tokio::test]
    async fn test_condition_node_value_match() {
        let condition = ConditionDef::Value {
            field: "score".into(),
            operator: ">=".into(),
            value: serde_json::json!(80),
        };
        let node = DslConditionNode::new("check", condition);
        let mut state = JsonState::new();
        state
            .apply_update("score", serde_json::json!(90))
            .await
            .unwrap();
        let ctx = RuntimeContext::new("test");
        let cmd = node.call(&mut state, &ctx).await.unwrap();

        assert!(!cmd.updates.is_empty());
        assert_eq!(cmd.updates[0].key, "check");
        assert_eq!(cmd.updates[0].value, serde_json::json!("true"));
    }

    #[tokio::test]
    async fn test_condition_node_value_no_match() {
        let condition = ConditionDef::Value {
            field: "score".into(),
            operator: ">=".into(),
            value: serde_json::json!(80),
        };
        let node = DslConditionNode::new("check", condition);
        let mut state = JsonState::new();
        state
            .apply_update("score", serde_json::json!(50))
            .await
            .unwrap();
        let ctx = RuntimeContext::new("test");
        let cmd = node.call(&mut state, &ctx).await.unwrap();
        assert_eq!(cmd.updates[0].value, serde_json::json!("false"));
    }

    #[tokio::test]
    async fn test_task_node_function_executor() {
        let node = DslTaskNode::new(
            "task1",
            TaskExecutorDef::Function {
                function: "my_function".into(),
            },
        );
        let mut state = JsonState::new();
        let ctx = RuntimeContext::new("test");
        let cmd = node.call(&mut state, &ctx).await.unwrap();
        assert!(!cmd.updates.is_empty());
        assert_eq!(cmd.updates[0].key, "task1");
    }
}
