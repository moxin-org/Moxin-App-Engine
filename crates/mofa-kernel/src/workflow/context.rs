//! Runtime Context for Workflow Execution
//!
//! Provides runtime information and configuration for workflow execution,
//! including recursion limit tracking and execution metadata.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Remaining steps tracker for recursion limit
///
/// Tracks and manages the remaining execution steps to prevent infinite loops.
/// This is actively decremented during execution and can be checked by nodes.
///
/// # Example
///
/// ```rust,ignore
/// let remaining = RemainingSteps::new(100);
///
/// // Check before proceeding
/// if remaining.is_exhausted() {
///     return Err(AgentError::RecursionLimitExceeded);
/// }
///
/// // Decrement after each step
/// remaining.decrement();
/// ```
#[derive(Debug, Clone)]
pub struct RemainingSteps {
    current: Arc<RwLock<u32>>,
    max: u32,
}

impl RemainingSteps {
    /// Create a new remaining steps tracker
    pub fn new(max: u32) -> Self {
        Self {
            current: Arc::new(RwLock::new(max)),
            max,
        }
    }

    /// Get the current remaining steps
    pub async fn current(&self) -> u32 {
        *self.current.read().await
    }

    /// Get the maximum steps allowed
    pub fn max(&self) -> u32 {
        self.max
    }

    /// Decrement the remaining steps by one
    pub async fn decrement(&self) -> u32 {
        let mut current = self.current.write().await;
        if *current > 0 {
            *current -= 1;
        }
        *current
    }

    /// Decrement by a specific amount
    pub async fn decrement_by(&self, amount: u32) -> u32 {
        let mut current = self.current.write().await;
        *current = current.saturating_sub(amount);
        *current
    }

    /// Check if steps are exhausted
    pub async fn is_exhausted(&self) -> bool {
        *self.current.read().await == 0
    }

    /// Check if we have at least N steps remaining
    pub async fn has_at_least(&self, n: u32) -> bool {
        *self.current.read().await >= n
    }

    /// Reset to maximum
    pub async fn reset(&self) {
        let mut current = self.current.write().await;
        *current = self.max;
    }

    /// Set to a specific value (cannot exceed max)
    pub async fn set(&self, value: u32) {
        let mut current = self.current.write().await;
        *current = value.min(self.max);
    }
}

/// Default stream buffer size for serde deserialization
fn default_stream_buffer_size() -> usize {
    100
}

/// Graph execution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphConfig<V = Value> {
    /// Maximum recursion depth
    pub max_steps: u32,

    /// Enable debug mode
    pub debug: bool,

    /// Enable checkpointing
    pub checkpoint_enabled: bool,

    /// Checkpoint interval (in steps)
    pub checkpoint_interval: u32,

    /// Timeout in milliseconds (0 = no timeout)
    pub timeout_ms: u64,

    /// Maximum parallel branches
    pub max_parallelism: usize,

    /// Stream channel buffer size (for `stream()` method)
    #[serde(default = "default_stream_buffer_size")]
    pub stream_buffer_size: usize,

    /// Custom configuration data
    #[serde(default)]
    pub custom: HashMap<String, V>,
}

impl<V: Clone> Default for GraphConfig<V> {
    fn default() -> Self {
        Self {
            max_steps: 100,
            debug: false,
            checkpoint_enabled: false,
            checkpoint_interval: 10,
            timeout_ms: 0,
            max_parallelism: 10,
            stream_buffer_size: 100,
            custom: HashMap::new(),
        }
    }
}

impl<V: Clone> GraphConfig<V> {
    /// Create a new config with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum recursion depth
    pub fn with_max_steps(mut self, max_steps: u32) -> Self {
        self.max_steps = max_steps;
        self
    }

    /// Enable debug mode
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Enable checkpointing
    pub fn with_checkpoints(mut self, enabled: bool, interval: u32) -> Self {
        self.checkpoint_enabled = enabled;
        self.checkpoint_interval = interval;
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Set maximum parallelism
    pub fn with_max_parallelism(mut self, max: usize) -> Self {
        self.max_parallelism = max;
        self
    }

    /// Set stream channel buffer size
    pub fn with_stream_buffer_size(mut self, size: usize) -> Self {
        self.stream_buffer_size = if size == 0 { 1 } else { size };
        self
    }

    /// Add a custom config value
    pub fn with_custom(mut self, key: impl Into<String>, value: V) -> Self {
        self.custom.insert(key.into(), value);
        self
    }

    /// Create RemainingSteps from this config
    pub fn remaining_steps(&self) -> RemainingSteps {
        RemainingSteps::new(self.max_steps)
    }
}

/// Runtime context passed to node functions
///
/// Contains non-state information about the current execution,
/// including execution ID, current node, remaining steps, and metadata.
#[derive(Debug, Clone)]
pub struct RuntimeContext<V: Clone + Send + Sync + 'static = Value> {
    /// Unique execution ID
    pub execution_id: String,

    /// Graph ID
    pub graph_id: String,

    /// Current node ID (updated during execution)
    pub current_node: Arc<RwLock<String>>,

    /// Remaining steps tracker
    pub remaining_steps: RemainingSteps,

    /// Graph configuration
    pub config: GraphConfig<V>,

    /// Execution metadata
    pub metadata: HashMap<String, V>,

    /// Parent execution ID (for sub-workflows)
    pub parent_execution_id: Option<String>,

    /// Execution tags
    pub tags: Vec<String>,
}

impl<V: Clone + Send + Sync + 'static> RuntimeContext<V> {
    /// Create a new runtime context
    pub fn new(graph_id: impl Into<String>) -> Self {
        Self {
            execution_id: Uuid::new_v4().to_string(),
            graph_id: graph_id.into(),
            current_node: Arc::new(RwLock::new(String::new())),
            remaining_steps: RemainingSteps::new(100),
            config: GraphConfig::default(),
            metadata: HashMap::new(),
            parent_execution_id: None,
            tags: Vec::new(),
        }
    }

    /// Create a context with a specific config
    pub fn with_config(graph_id: impl Into<String>, config: GraphConfig<V>) -> Self {
        let remaining_steps = config.remaining_steps();
        Self {
            execution_id: Uuid::new_v4().to_string(),
            graph_id: graph_id.into(),
            current_node: Arc::new(RwLock::new(String::new())),
            remaining_steps,
            config,
            metadata: HashMap::new(),
            parent_execution_id: None,
            tags: Vec::new(),
        }
    }

    /// Create a context for a sub-workflow
    pub fn for_sub_workflow(
        graph_id: impl Into<String>,
        parent_execution_id: impl Into<String>,
        config: GraphConfig<V>,
    ) -> Self {
        let remaining_steps = config.remaining_steps();
        Self {
            execution_id: Uuid::new_v4().to_string(),
            graph_id: graph_id.into(),
            current_node: Arc::new(RwLock::new(String::new())),
            remaining_steps,
            config,
            metadata: HashMap::new(),
            parent_execution_id: Some(parent_execution_id.into()),
            tags: Vec::new(),
        }
    }

    /// Get the current node ID
    pub async fn current_node(&self) -> String {
        self.current_node.read().await.clone()
    }

    /// Set the current node ID
    pub async fn set_current_node(&self, node_id: impl Into<String>) {
        let mut current = self.current_node.write().await;
        *current = node_id.into();
    }

    /// Check if recursion limit is reached
    pub async fn is_recursion_limit_reached(&self) -> bool {
        self.remaining_steps.is_exhausted().await
    }

    /// Decrement remaining steps
    pub async fn decrement_steps(&self) -> u32 {
        self.remaining_steps.decrement().await
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: V) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Check if debug mode is enabled
    pub fn is_debug(&self) -> bool {
        self.config.debug
    }

    /// Check if this is a sub-workflow execution
    pub fn is_sub_workflow(&self) -> bool {
        self.parent_execution_id.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_remaining_steps() {
        let steps = RemainingSteps::new(10);

        assert_eq!(steps.current().await, 10);
        assert_eq!(steps.max(), 10);
        assert!(!steps.is_exhausted().await);
        assert!(steps.has_at_least(5).await);

        steps.decrement().await;
        assert_eq!(steps.current().await, 9);

        steps.decrement_by(5).await;
        assert_eq!(steps.current().await, 4);

        steps.reset().await;
        assert_eq!(steps.current().await, 10);
    }

    #[tokio::test]
    async fn test_remaining_steps_exhausted() {
        let steps = RemainingSteps::new(2);

        assert!(!steps.is_exhausted().await);
        steps.decrement().await;
        assert!(!steps.is_exhausted().await);
        steps.decrement().await;
        assert!(steps.is_exhausted().await);

        // Should stay at 0
        steps.decrement().await;
        assert!(steps.is_exhausted().await);
    }

    #[test]
    fn test_graph_config() {
        let config = GraphConfig::<serde_json::Value>::new()
            .with_max_steps(50)
            .with_debug(true)
            .with_checkpoints(true, 5)
            .with_timeout(30000)
            .with_max_parallelism(4);

        assert_eq!(config.max_steps, 50);
        assert!(config.debug);
        assert!(config.checkpoint_enabled);
        assert_eq!(config.checkpoint_interval, 5);
        assert_eq!(config.timeout_ms, 30000);
        assert_eq!(config.max_parallelism, 4);
    }

    #[test]
    fn test_stream_buffer_size_default() {
        let config = GraphConfig::<serde_json::Value>::default();
        assert_eq!(config.stream_buffer_size, 100);
    }

    #[test]
    fn test_stream_buffer_size_custom() {
        let config = GraphConfig::<serde_json::Value>::new()
            .with_stream_buffer_size(5);
        assert_eq!(config.stream_buffer_size, 5);
    }

    #[test]
    fn test_stream_buffer_size_zero_clamped_to_one() {
        let config = GraphConfig::<serde_json::Value>::new()
            .with_stream_buffer_size(0);
        assert_eq!(config.stream_buffer_size, 1);
    }

    #[tokio::test]
    async fn test_runtime_context() {
        let ctx = RuntimeContext::<serde_json::Value>::new("test_graph")
            .with_metadata("key", serde_json::json!("value"))
            .with_tag("test");

        assert!(!ctx.execution_id.is_empty());
        assert_eq!(ctx.graph_id, "test_graph");
        assert!(ctx.current_node().await.is_empty());
        assert!(!ctx.is_sub_workflow());

        ctx.set_current_node("node_1").await;
        assert_eq!(ctx.current_node().await, "node_1");
    }

    #[tokio::test]
    async fn test_runtime_context_sub_workflow() {
        let ctx = RuntimeContext::<serde_json::Value>::for_sub_workflow(
            "sub_graph",
            "parent-execution-123",
            GraphConfig::default(),
        );

        assert!(ctx.is_sub_workflow());
        assert_eq!(
            ctx.parent_execution_id,
            Some("parent-execution-123".to_string())
        );
    }
}
