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
use mofa_kernel::agent::error::{AgentError, AgentResult};
use mofa_kernel::workflow::{
    Command, CompiledGraph, GraphState, JsonState, NodeFunc, RuntimeContext, StateGraph, END, START,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Runtime services required by the DSL compiler for executable task/sub-workflow nodes.
#[derive(Clone, Default)]
pub struct DslCompilerRuntime {
    task_executor: Option<Arc<dyn DslTaskExecutor>>,
    sub_workflows: HashMap<String, Arc<CompiledGraphImpl<JsonState>>>,
}

impl DslCompilerRuntime {
    /// Create an empty runtime with no registered executors or sub-workflows.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a task executor used by `task` and `loop` nodes.
    pub fn with_task_executor(mut self, task_executor: Arc<dyn DslTaskExecutor>) -> Self {
        self.task_executor = Some(task_executor);
        self
    }

    /// Register a compiled sub-workflow by workflow ID.
    pub fn with_sub_workflow(
        mut self,
        workflow_id: impl Into<String>,
        workflow: Arc<CompiledGraphImpl<JsonState>>,
    ) -> Self {
        self.sub_workflows.insert(workflow_id.into(), workflow);
        self
    }

    fn task_executor(&self) -> Option<Arc<dyn DslTaskExecutor>> {
        self.task_executor.clone()
    }

    fn sub_workflow(&self, workflow_id: &str) -> Option<Arc<CompiledGraphImpl<JsonState>>> {
        self.sub_workflows.get(workflow_id).cloned()
    }
}

/// Runtime adapter for executing DSL task bodies.
#[async_trait]
pub trait DslTaskExecutor: Send + Sync {
    /// Execute the given task definition against the current input/state snapshot.
    async fn execute(
        &self,
        node_id: &str,
        executor: &TaskExecutorDef,
        state: &JsonState,
        input: Value,
        ctx: &RuntimeContext<Value>,
    ) -> AgentResult<Value>;
}

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

/// A task node that executes DSL task definitions via the registered runtime executor.
///
/// `TaskExecutorDef::None` remains a no-op. All other executor kinds delegate to
/// the `DslTaskExecutor` supplied through `DslCompilerRuntime`.
struct DslTaskNode {
    node_name: String,
    executor: TaskExecutorDef,
    task_executor: Option<Arc<dyn DslTaskExecutor>>,
}

impl DslTaskNode {
    fn new(
        name: impl Into<String>,
        executor: TaskExecutorDef,
        task_executor: Option<Arc<dyn DslTaskExecutor>>,
    ) -> Self {
        Self {
            node_name: name.into(),
            executor,
            task_executor,
        }
    }
}

#[async_trait]
impl NodeFunc<JsonState, Value> for DslTaskNode {
    async fn call(
        &self,
        state: &mut JsonState,
        ctx: &RuntimeContext<Value>,
    ) -> AgentResult<Command<Value>> {
        match &self.executor {
            TaskExecutorDef::None => Ok(Command::new().continue_()),
            TaskExecutorDef::Function { .. }
            | TaskExecutorDef::Http { .. }
            | TaskExecutorDef::Script { .. } => {
                let output = self
                    .task_executor
                    .as_ref()
                    .ok_or_else(|| {
                        AgentError::ExecutionFailed(format!(
                            "Task node '{}' has no registered DSL task executor",
                            self.node_name
                        ))
                    })?
                    .execute(
                        &self.node_name,
                        &self.executor,
                        state,
                        state_input_or_json(state)?,
                        ctx,
                    )
                    .await?;

                Ok(Command::new().update(&self.node_name, output).continue_())
            }
        }
    }
    fn name(&self) -> &str {
        &self.node_name
    }
    fn description(&self) -> Option<&str> {
        Some("DSL task node")
    }
}

/// A loop node that executes a task body in-process for count-based loops.
struct DslLoopNode {
    node_name: String,
    body: TaskExecutorDef,
    max_iterations: u32,
    task_executor: Option<Arc<dyn DslTaskExecutor>>,
}

impl DslLoopNode {
    fn new(
        name: impl Into<String>,
        body: TaskExecutorDef,
        max_iterations: u32,
        task_executor: Option<Arc<dyn DslTaskExecutor>>,
    ) -> Self {
        Self {
            node_name: name.into(),
            body,
            max_iterations,
            task_executor,
        }
    }
}

#[async_trait]
impl NodeFunc<JsonState, Value> for DslLoopNode {
    async fn call(
        &self,
        state: &mut JsonState,
        ctx: &RuntimeContext<Value>,
    ) -> AgentResult<Command<Value>> {
        let mut current = state_input_or_json(state)?;

        for _ in 0..self.max_iterations {
            current = match &self.body {
                TaskExecutorDef::None => current,
                TaskExecutorDef::Function { .. }
                | TaskExecutorDef::Http { .. }
                | TaskExecutorDef::Script { .. } => {
                    self.task_executor
                        .as_ref()
                        .ok_or_else(|| {
                            AgentError::ExecutionFailed(format!(
                                "Loop node '{}' has no registered DSL task executor",
                                self.node_name
                            ))
                        })?
                        .execute(&self.node_name, &self.body, state, current, ctx)
                        .await?
                }
            };
        }

        Ok(Command::new().update(&self.node_name, current).continue_())
    }

    fn name(&self) -> &str {
        &self.node_name
    }

    fn description(&self) -> Option<&str> {
        Some("DSL count loop node")
    }
}

/// A sub-workflow node that invokes a previously compiled workflow graph.
struct DslSubWorkflowNode {
    node_name: String,
    workflow_id: String,
    sub_workflow: Arc<CompiledGraphImpl<JsonState>>,
}

impl DslSubWorkflowNode {
    fn new(
        name: impl Into<String>,
        workflow_id: impl Into<String>,
        sub_workflow: Arc<CompiledGraphImpl<JsonState>>,
    ) -> Self {
        Self {
            node_name: name.into(),
            workflow_id: workflow_id.into(),
            sub_workflow,
        }
    }
}

#[async_trait]
impl NodeFunc<JsonState, Value> for DslSubWorkflowNode {
    async fn call(
        &self,
        state: &mut JsonState,
        ctx: &RuntimeContext<Value>,
    ) -> AgentResult<Command<Value>> {
        let mut sub_ctx = RuntimeContext::for_sub_workflow(
            self.workflow_id.clone(),
            ctx.execution_id.clone(),
            ctx.config.clone(),
        );
        sub_ctx.metadata = ctx.metadata.clone();
        sub_ctx.tags = ctx.tags.clone();

        let final_state = self
            .sub_workflow
            .invoke(JsonState::from_json(state.to_json()?)?, Some(sub_ctx))
            .await?;
        let final_json = final_state.to_json()?;

        let mut command = Command::new()
            .update(&self.node_name, final_json.clone())
            .continue_();
        if let Value::Object(map) = final_json {
            for (key, value) in map {
                if key != self.node_name {
                    command = command.update(&key, value);
                }
            }
        }

        Ok(command)
    }

    fn name(&self) -> &str {
        &self.node_name
    }

    fn description(&self) -> Option<&str> {
        Some("DSL sub-workflow node")
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

fn state_input_or_json(state: &JsonState) -> AgentResult<Value> {
    Ok(match state.get_value::<Value>("input") {
        Some(input) => input,
        None => state.to_json()?,
    })
}

fn task_executor_kind(executor: &TaskExecutorDef) -> &'static str {
    match executor {
        TaskExecutorDef::Function { .. } => "function",
        TaskExecutorDef::Http { .. } => "http",
        TaskExecutorDef::Script { .. } => "script",
        TaskExecutorDef::None => "none",
    }
}

fn ensure_runtime_task_executor(
    node_id: &str,
    executor: &TaskExecutorDef,
    runtime: &DslCompilerRuntime,
) -> DslResult<Option<Arc<dyn DslTaskExecutor>>> {
    match executor {
        TaskExecutorDef::None => Ok(runtime.task_executor()),
        TaskExecutorDef::Function { .. }
        | TaskExecutorDef::Http { .. }
        | TaskExecutorDef::Script { .. } => runtime.task_executor().map(Some).ok_or_else(|| {
            DslError::Build(format!(
                "Task node '{}' uses '{}' executor but no DSL task executor was registered.",
                node_id,
                task_executor_kind(executor)
            ))
        }),
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
/// | `Task`         | `DslTaskNode`      | Runtime-backed task executor      |
/// | `Condition`    | `DslConditionNode` | Evaluates condition, sets route   |
/// | `Parallel`     | `PassthroughNode`  | Marker (parallelism via edges)    |
/// | `Join`         | `DslJoinNode`      | Records join metadata             |
/// | `Loop`         | `DslLoopNode`      | Count-based runtime loop          |
/// | `SubWorkflow`  | `DslSubWorkflowNode` | Invokes registered sub-workflow |
/// | `Transform`    | `DslTransformNode` | Records transform definition      |
///
/// # Unsupported Node Types (Future PRs)
///
/// - `LlmAgent` inline configs — requires inline agent construction support
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
    /// - An unsupported node mode is used (for example inline agents, wait nodes,
    ///   or non-count loops)
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
        Self::compile_with_runtime(def, agent_registry, &DslCompilerRuntime::default())
    }

    /// Compile a workflow definition with agent and runtime registries.
    pub fn compile_with_runtime(
        def: WorkflowDefinition,
        agent_registry: &HashMap<String, Arc<LLMAgent>>,
        runtime: &DslCompilerRuntime,
    ) -> DslResult<CompiledGraphImpl<JsonState>> {
        Self::validate(&def)?;
        let mut graph = StateGraphImpl::<JsonState>::build(&def.metadata.id);

        for node_def in &def.nodes {
            let node_func = Self::compile_node(node_def, agent_registry, runtime)?;
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
        runtime: &DslCompilerRuntime,
    ) -> DslResult<Box<dyn NodeFunc<JsonState, Value>>> {
        match def {
            NodeDefinition::Start { id, .. } => {
                Ok(Box::new(PassthroughNode::new(id)) as Box<dyn NodeFunc<JsonState, Value>>)
            }
            NodeDefinition::End { id, .. } => {
                Ok(Box::new(PassthroughNode::new(id)) as Box<dyn NodeFunc<JsonState, Value>>)
            }
            NodeDefinition::Task { id, executor, .. } => {
                let task_executor = ensure_runtime_task_executor(id, executor, runtime)?;
                Ok(
                    Box::new(DslTaskNode::new(id, executor.clone(), task_executor))
                        as Box<dyn NodeFunc<JsonState, Value>>,
                )
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
            NodeDefinition::Loop {
                id,
                body,
                condition,
                max_iterations,
                ..
            } => {
                let count = match condition {
                    LoopConditionDef::Count { max } => *max,
                    LoopConditionDef::While { .. } => {
                        return Err(DslError::Build(format!(
                            "Loop node '{}' uses a while-condition, but the DSL compiler currently only supports count-based loops.",
                            id
                        )));
                    }
                    LoopConditionDef::Until { .. } => {
                        return Err(DslError::Build(format!(
                            "Loop node '{}' uses an until-condition, but the DSL compiler currently only supports count-based loops.",
                            id
                        )));
                    }
                };

                let bounded_max = if *max_iterations > 0 {
                    count.min(*max_iterations)
                } else {
                    count
                };
                let task_executor = ensure_runtime_task_executor(id, body, runtime)?;

                Ok(Box::new(DslLoopNode::new(
                    id,
                    body.clone(),
                    bounded_max,
                    task_executor,
                )) as Box<dyn NodeFunc<JsonState, Value>>)
            }
            NodeDefinition::SubWorkflow {
                id, workflow_id, ..
            } => {
                let sub_workflow = runtime.sub_workflow(workflow_id).ok_or_else(|| {
                    DslError::Build(format!(
                        "SubWorkflow node '{}' references workflow '{}' but no compiled sub-workflow was registered.",
                        id, workflow_id
                    ))
                })?;

                Ok(Box::new(DslSubWorkflowNode::new(
                    id,
                    workflow_id.clone(),
                    sub_workflow,
                )) as Box<dyn NodeFunc<JsonState, Value>>)
            }
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

        // Group conditional edges by source: from → { condition: to }
        let mut conditional_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        for edge in &def.edges {
            if let Some(ref condition) = edge.condition {
                conditional_map
                    .entry(edge.from.clone())
                    .or_default()
                    .insert(condition.clone(), edge.to.clone());
            } else {
                graph.add_edge(&edge.from, &edge.to);
            }
        }

        for (from, conditions) in conditional_map {
            graph.add_conditional_edges(&from, conditions);
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
    use serde_json::json;
    use tokio::sync::Mutex;

    struct RecordingTaskExecutor {
        calls: Mutex<Vec<(String, Value)>>,
    }

    impl RecordingTaskExecutor {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl DslTaskExecutor for RecordingTaskExecutor {
        async fn execute(
            &self,
            node_id: &str,
            executor: &TaskExecutorDef,
            _state: &JsonState,
            input: Value,
            _ctx: &RuntimeContext<Value>,
        ) -> AgentResult<Value> {
            self.calls
                .lock()
                .await
                .push((node_id.to_string(), input.clone()));

            match executor {
                TaskExecutorDef::Function { function } => match function.as_str() {
                    "uppercase" => Ok(json!(input.as_str().unwrap_or_default().to_uppercase())),
                    "increment" => Ok(json!(input.as_i64().unwrap_or_default() + 1)),
                    other => Err(AgentError::ExecutionFailed(format!(
                        "Unknown test function '{}'",
                        other
                    ))),
                },
                TaskExecutorDef::Http { url, method } => Ok(json!({
                    "url": url,
                    "method": method.as_deref().unwrap_or("GET"),
                    "input": input,
                })),
                TaskExecutorDef::Script { script } => Ok(json!({
                    "script": script,
                    "input": input,
                })),
                TaskExecutorDef::None => Ok(input),
            }
        }
    }

    struct StaticUpdateNode {
        name: String,
        key: String,
        value: Value,
    }

    #[async_trait]
    impl NodeFunc<JsonState, Value> for StaticUpdateNode {
        async fn call(
            &self,
            _state: &mut JsonState,
            _ctx: &RuntimeContext<Value>,
        ) -> AgentResult<Command<Value>> {
            Ok(Command::new()
                .update(&self.key, self.value.clone())
                .continue_())
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

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

    #[tokio::test]
    async fn test_compile_task_node_uses_registered_executor() {
        let yaml = r#"
metadata:
  id: task_runtime
  name: Task Runtime
nodes:
  - type: start
    id: start
  - type: task
    id: process
    name: Process
    executor_type: function
    function: uppercase
  - type: end
    id: end
edges:
  - from: start
    to: process
  - from: process
    to: end
"#;
        let def = parse_yaml(yaml);
        let task_executor = Arc::new(RecordingTaskExecutor::new());
        let runtime = DslCompilerRuntime::new().with_task_executor(task_executor.clone());
        let compiled =
            DslCompiler::compile_with_runtime(def, &HashMap::new(), &runtime).expect("compile");

        let mut state = JsonState::new();
        state.apply_update("input", json!("hello")).await.unwrap();
        let final_state = compiled.invoke(state, None).await.expect("invoke");

        assert_eq!(final_state.get_value("process"), Some(json!("HELLO")));

        let calls = task_executor.calls.lock().await.clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "process");
        assert_eq!(calls[0].1, json!("hello"));
    }

    #[test]
    fn test_compile_task_node_without_runtime_executor_fails() {
        let yaml = r#"
metadata:
  id: task_missing_runtime
  name: Task Missing Runtime
nodes:
  - type: start
    id: start
  - type: task
    id: process
    name: Process
    executor_type: function
    function: uppercase
  - type: end
    id: end
edges:
  - from: start
    to: process
  - from: process
    to: end
"#;
        let def = parse_yaml(yaml);
        let err = DslCompiler::compile(def).expect_err("compile should fail");

        assert!(matches!(err, DslError::Build(_)));
        assert!(err
            .to_string()
            .contains("no DSL task executor was registered"));
    }

    #[tokio::test]
    async fn test_compile_count_loop_executes_task_body() {
        let yaml = r#"
metadata:
  id: loop_runtime
  name: Loop Runtime
nodes:
  - type: start
    id: start
  - type: loop
    id: repeat
    name: Repeat
    executor_type: function
    function: increment
    condition:
      condition_type: count
      max: 3
  - type: end
    id: end
edges:
  - from: start
    to: repeat
  - from: repeat
    to: end
"#;
        let def = parse_yaml(yaml);
        let task_executor = Arc::new(RecordingTaskExecutor::new());
        let runtime = DslCompilerRuntime::new().with_task_executor(task_executor.clone());
        let compiled =
            DslCompiler::compile_with_runtime(def, &HashMap::new(), &runtime).expect("compile");

        let mut state = JsonState::new();
        state.apply_update("input", json!(0)).await.unwrap();
        let final_state = compiled.invoke(state, None).await.expect("invoke");

        assert_eq!(final_state.get_value("repeat"), Some(json!(3)));
        assert_eq!(task_executor.calls.lock().await.len(), 3);
    }

    #[test]
    fn test_compile_non_count_loop_is_rejected() {
        let yaml = r#"
metadata:
  id: loop_while
  name: Loop While
nodes:
  - type: start
    id: start
  - type: loop
    id: repeat
    name: Repeat
    executor_type: none
    condition:
      condition_type: while
      expr: "input < 5"
  - type: end
    id: end
edges:
  - from: start
    to: repeat
  - from: repeat
    to: end
"#;
        let def = parse_yaml(yaml);
        let err = DslCompiler::compile(def).expect_err("compile should fail");

        assert!(matches!(err, DslError::Build(_)));
        assert!(err
            .to_string()
            .contains("currently only supports count-based loops"));
    }

    #[tokio::test]
    async fn test_compile_sub_workflow_invokes_registered_graph() {
        let yaml = r#"
metadata:
  id: parent_runtime
  name: Parent Runtime
nodes:
  - type: start
    id: start
  - type: sub_workflow
    id: run_child
    name: Run Child
    workflow_id: child_workflow
  - type: end
    id: end
edges:
  - from: start
    to: run_child
  - from: run_child
    to: end
"#;
        let def = parse_yaml(yaml);

        let mut child = StateGraphImpl::<JsonState>::new("child_workflow");
        child
            .add_node(
                "child_step",
                Box::new(StaticUpdateNode {
                    name: "child_step".to_string(),
                    key: "sub_result".to_string(),
                    value: json!("done"),
                }),
            )
            .add_edge(START, "child_step")
            .add_edge("child_step", END);
        let child = Arc::new(child.compile().expect("child compile"));

        let runtime = DslCompilerRuntime::new().with_sub_workflow("child_workflow", child);
        let compiled =
            DslCompiler::compile_with_runtime(def, &HashMap::new(), &runtime).expect("compile");

        let mut state = JsonState::new();
        state.apply_update("input", json!("hello")).await.unwrap();
        let final_state = compiled.invoke(state, None).await.expect("invoke");

        assert_eq!(final_state.get_value("sub_result"), Some(json!("done")));

        let snapshot: Value = final_state
            .get_value("run_child")
            .expect("sub-workflow snapshot should be stored");
        assert_eq!(snapshot["sub_result"], json!("done"));
        assert_eq!(snapshot["input"], json!("hello"));
    }

    #[test]
    fn test_compile_sub_workflow_without_runtime_registration_fails() {
        let yaml = r#"
metadata:
  id: parent_runtime_missing_child
  name: Parent Runtime Missing Child
nodes:
  - type: start
    id: start
  - type: sub_workflow
    id: run_child
    name: Run Child
    workflow_id: child_workflow
  - type: end
    id: end
edges:
  - from: start
    to: run_child
  - from: run_child
    to: end
"#;
        let def = parse_yaml(yaml);
        let err = DslCompiler::compile(def).expect_err("compile should fail");

        assert!(matches!(err, DslError::Build(_)));
        assert!(err
            .to_string()
            .contains("no compiled sub-workflow was registered"));
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
        let runtime_executor = Arc::new(RecordingTaskExecutor::new());
        let node = DslTaskNode::new(
            "task1",
            TaskExecutorDef::Function {
                function: "my_function".into(),
            },
            Some(runtime_executor),
        );
        let mut state = JsonState::new();
        state.apply_update("input", json!("hello")).await.unwrap();
        let ctx = RuntimeContext::new("test");
        let err = node.call(&mut state, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("Unknown test function"));
    }
}
