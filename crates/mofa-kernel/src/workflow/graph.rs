//! State Graph Traits
//!
//! Defines the core graph interfaces for building and executing workflows.
//! Inspired by LangGraph's StateGraph API.

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;

use crate::agent::error::AgentResult;

use super::policy::NodePolicy;
use super::{Command, GraphConfig, GraphState, Reducer, RuntimeContext};

/// Type alias for the boxed stream returned by graph execution.
pub type GraphStream<'a, S, V> =
    Pin<Box<dyn Stream<Item = AgentResult<StreamEvent<S, V>>> + Send + 'a>>;

/// Special node ID for the graph entry point
pub const START: &str = "__START__";

/// Special node ID for the graph exit point
pub const END: &str = "__END__";

/// Node function trait
///
/// Implement this trait to define custom node behavior.
/// Nodes receive the current state and runtime context,
/// and return a Command that can update state and control flow.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_kernel::workflow::{NodeFunc, Command, RuntimeContext};
///
/// struct ProcessNode;
///
/// #[async_trait]
/// impl NodeFunc<MyState> for ProcessNode {
///     async fn call(&self, state: &mut MyState, ctx: &RuntimeContext) -> AgentResult<Command> {
///         // Process the state
///         let input = state.messages.last().cloned().unwrap_or_default();
///
///         // Return command with state update and control flow
///         Ok(Command::new()
///             .update("result", json!(format!("Processed: {}", input)))
///             .goto("next_node"))
///     }
///
///     fn name(&self) -> &str {
///         "process"
///     }
/// }
/// ```
#[async_trait]
pub trait NodeFunc<S: GraphState, V = serde_json::Value>: Send + Sync
where
    V: serde::Serialize + Send + Sync + 'static + std::clone::Clone,
{
    /// Execute the node
    ///
    /// # Arguments
    /// * `state` - Mutable reference to the current state
    /// * `ctx` - Runtime context with execution metadata
    ///
    /// # Returns
    /// A Command containing state updates and control flow directive
    async fn call(&self, state: &mut S, ctx: &RuntimeContext<V>) -> AgentResult<Command<V>>;

    /// Returns the node name/identifier
    fn name(&self) -> &str;

    /// Optional description of what this node does
    fn description(&self) -> Option<&str> {
        None
    }
}

/// Edge target definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EdgeTarget {
    /// Single target node
    Single(String),
    /// Conditional edges with route names to node IDs
    Conditional(HashMap<String, String>),
    /// Multiple parallel targets
    Parallel(Vec<String>),
}

impl EdgeTarget {
    /// Create a single target edge
    pub fn single(target: impl Into<String>) -> Self {
        Self::Single(target.into())
    }

    /// Create conditional edges
    pub fn conditional(routes: HashMap<String, String>) -> Self {
        Self::Conditional(routes)
    }

    /// Create parallel edges
    pub fn parallel(targets: Vec<String>) -> Self {
        Self::Parallel(targets)
    }

    /// Check if this is a conditional edge
    pub fn is_conditional(&self) -> bool {
        matches!(self, Self::Conditional(_))
    }

    /// Get all target node IDs
    pub fn targets(&self) -> Vec<&str> {
        match self {
            Self::Single(t) => vec![t],
            Self::Conditional(routes) => routes.values().map(|s| s.as_str()).collect(),
            Self::Parallel(targets) => targets.iter().map(|s| s.as_str()).collect(),
        }
    }
}

/// State graph builder trait
///
/// Defines the interface for building stateful workflow graphs.
/// Implementations should provide a fluent API for constructing graphs.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_kernel::workflow::{StateGraph, START, END};
///
/// let graph = StateGraphImpl::<MyState>::new("my_workflow")
///     // Add reducers for state keys
///     .add_reducer("messages", Box::new(AppendReducer))
///     .add_reducer("result", Box::new(OverwriteReducer))
///     // Add nodes
///     .add_node("process", Box::new(ProcessNode))
///     .add_node("validate", Box::new(ValidateNode))
///     // Add edges
///     .add_edge(START, "process")
///     .add_edge("process", "validate")
///     .add_edge("validate", END)
///     // Compile
///     .compile()?;
/// ```
#[async_trait]
pub trait StateGraph: Send + Sync {
    /// The state type for this graph
    type State: GraphState;

    /// The compiled graph type produced by this builder
    type Compiled: CompiledGraph<Self::State, serde_json::Value>;

    /// Create a new graph with the given ID
    fn new(id: impl Into<String>) -> Self;

    /// Add a node to the graph
    ///
    /// # Arguments
    /// * `id` - Unique node identifier
    /// * `node` - Node function implementation
    fn add_node(
        &mut self,
        id: impl Into<String>,
        node: Box<dyn NodeFunc<Self::State>>,
    ) -> &mut Self;

    /// Add an edge between two nodes
    ///
    /// # Arguments
    /// * `from` - Source node ID (use START for entry edge)
    /// * `to` - Target node ID (use END for exit edge)
    fn add_edge(&mut self, from: impl Into<String>, to: impl Into<String>) -> &mut Self;

    /// Add conditional edges from a node
    ///
    /// # Arguments
    /// * `from` - Source node ID
    /// * `conditions` - Map of condition names to target node IDs
    ///
    /// # Example
    /// ```rust,ignore
    /// graph.add_conditional_edges("classify", HashMap::from([
    ///     ("type_a".to_string(), "handle_a".to_string()),
    ///     ("type_b".to_string(), "handle_b".to_string()),
    /// ]));
    /// ```
    fn add_conditional_edges(
        &mut self,
        from: impl Into<String>,
        conditions: HashMap<String, String>,
    ) -> &mut Self;

    /// Add parallel edges from a node
    ///
    /// # Arguments
    /// * `from` - Source node ID
    /// * `targets` - List of target node IDs to execute in parallel
    fn add_parallel_edges(&mut self, from: impl Into<String>, targets: Vec<String>) -> &mut Self;

    /// Set the entry point (equivalent to add_edge(START, node))
    fn set_entry_point(&mut self, node: impl Into<String>) -> &mut Self;

    /// Set a finish point (equivalent to add_edge(node, END))
    fn set_finish_point(&mut self, node: impl Into<String>) -> &mut Self;

    /// Add a reducer for a state key
    ///
    /// # Arguments
    /// * `key` - State key name
    /// * `reducer` - Reducer implementation
    fn add_reducer(&mut self, key: impl Into<String>, reducer: Box<dyn Reducer>) -> &mut Self;

    /// Set the graph configuration
    fn with_config(&mut self, config: GraphConfig) -> &mut Self;

    /// Attach a fault-tolerance [`NodePolicy`] to a specific node.
    ///
    /// Policies are opt-in and the default is a no-op (errors propagate as-is).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use mofa_kernel::workflow::policy::{NodePolicy, RetryCondition};
    ///
    /// graph.with_node_policy("llm_call", NodePolicy {
    ///     max_retries: 3,
    ///     retry_backoff_ms: 200,
    ///     retry_condition: RetryCondition::OnTransient(vec!["timeout".to_string()]),
    ///     fallback_node: Some("fallback_handler".to_string()),
    ///     circuit_open_after: 5,
    ///     ..Default::default()
    /// });
    /// ```
    fn with_node_policy(&mut self, node_id: impl Into<String>, policy: NodePolicy) -> &mut Self;

    /// Retrieve the [`NodePolicy`] for a node, if one has been set.
    fn node_policy(&self, node_id: &str) -> Option<&NodePolicy>;

    /// Get the graph ID
    fn id(&self) -> &str;

    /// Compile the graph into an executable form
    ///
    /// This validates the graph structure and prepares it for execution.
    fn compile(self) -> AgentResult<Self::Compiled>;
}

/// Compiled graph trait for execution
///
/// A compiled graph can be invoked with an initial state and
/// returns the final state after execution.
#[async_trait]
pub trait CompiledGraph<S: GraphState, V = serde_json::Value>: Send + Sync
where
    V: serde::Serialize + serde::de::DeserializeOwned + Clone + Send + Sync + 'static,
{
    /// Get the graph ID
    fn id(&self) -> &str;

    /// Execute the graph synchronously
    ///
    /// # Arguments
    /// * `input` - Initial state
    /// * `config` - Optional runtime configuration (uses defaults if None)
    ///
    /// # Returns
    /// The final state after graph execution completes
    async fn invoke(&self, input: S, config: Option<RuntimeContext<V>>) -> AgentResult<S>;

    /// Execute the graph with streaming output
    ///
    /// Returns a stream of (node_id, state) pairs as each node completes.
    fn stream(&self, input: S, config: Option<RuntimeContext<V>>) -> GraphStream<'_, S, V>;

    /// Execute a single step of the graph
    ///
    /// Useful for debugging or interactive execution.
    /// # Returns
    /// Step execution result containing next state and command
    async fn step(
        &self,
        input: S,
        config: Option<RuntimeContext<V>>,
    ) -> AgentResult<StepResult<S, V>>;

    /// Validate that a state is valid for this graph
    fn validate_state(&self, state: &S) -> AgentResult<()>;

    /// Get the graph's state schema
    fn state_schema(&self) -> HashMap<String, String>;
}

/// Stream event from graph execution
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum StreamEvent<S: GraphState, V = serde_json::Value> {
    /// A node started executing
    NodeStart { node_id: String, state: S },
    /// A node finished executing
    NodeEnd {
        node_id: String,
        state: S,
        command: Command<V>,
    },
    /// Graph execution completed
    End { final_state: S },
    /// Error occurred
    Error {
        node_id: Option<String>,
        error: String,
    },
    /// A node execution failed and is being retried.
    NodeRetry {
        /// The node being retried
        node_id: String,
        /// Which retry attempt this is (1-indexed)
        attempt: u32,
        /// Error message from the failed attempt
        error: String,
    },
    /// Execution has been routed from a failed node to its fallback node.
    NodeFallback {
        /// The primary node that failed
        from_node: String,
        /// The fallback node receiving execution
        to_node: String,
        /// Human-readable reason for the fallback
        reason: String,
    },
    /// A node's circuit breaker has opened due to repeated failures.
    CircuitOpened {
        /// The node whose circuit is now open
        node_id: String,
    },
    /// A node's circuit breaker has returned to the Closed state.
    CircuitClosed {
        /// The node whose circuit is now closed
        node_id: String,
    },
}

/// Result of a single step execution
#[must_use]
#[derive(Debug, Clone)]
pub struct StepResult<S: GraphState, V = serde_json::Value> {
    /// Current state after the step
    pub state: S,
    /// Which node was executed
    pub node_id: String,
    /// Command returned by the node
    pub command: Command<V>,
    /// Whether execution is complete
    pub is_complete: bool,
    /// Next node to execute (if any)
    pub next_node: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_target_single() {
        let target = EdgeTarget::single("node_a");
        assert!(!target.is_conditional());
        assert_eq!(target.targets(), vec!["node_a"]);
    }

    #[test]
    fn test_edge_target_conditional() {
        let mut routes = HashMap::new();
        routes.insert("condition_a".to_string(), "node_a".to_string());
        routes.insert("condition_b".to_string(), "node_b".to_string());

        let target = EdgeTarget::conditional(routes);
        assert!(target.is_conditional());

        let targets = target.targets();
        assert_eq!(targets.len(), 2);
        assert!(targets.contains(&"node_a"));
        assert!(targets.contains(&"node_b"));
    }

    #[test]
    fn test_edge_target_parallel() {
        let target = EdgeTarget::parallel(vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        assert!(!target.is_conditional());
        assert_eq!(target.targets(), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_constants() {
        assert_eq!(START, "__START__");
        assert_eq!(END, "__END__");
    }
}
