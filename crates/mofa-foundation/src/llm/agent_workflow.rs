//! LLM Agent 工作流编排
//! LLM Agent Workflow Orchestration
//!
//! 将 LLMAgent 与 WorkflowGraph 结合，提供高级的多 Agent 工作流编排能力
//! Combines LLMAgent with WorkflowGraph to provide advanced multi-agent workflow orchestration
//!
//! # 功能特性
//! # Features
//!
//! - **Agent 节点**: 将 LLMAgent 封装为工作流节点
//! - **Agent Node**: Encapsulates LLMAgent as a workflow node
//! - **条件路由**: 基于 LLM 输出进行条件分支
//! - **Conditional Routing**: Branching logic based on LLM output
//! - **并行执行**: 多个 Agent 并行处理
//! - **Parallel Execution**: Concurrent processing by multiple agents
//! - **流式响应**: 支持工作流中的流式输出
//! - **Streaming Response**: Supports streaming output within workflows
//! - **会话共享**: 工作流节点间共享会话上下文
//! - **Session Sharing**: Shared session context between workflow nodes
//!
//! # 示例
//! # Example
//!
//! ```rust,ignore
//! use mofa_foundation::llm::{AgentWorkflow, LLMAgent};
//! use std::sync::Arc;
//!
//! // 创建 Agent 工作流
//! // Create an Agent workflow
//! let workflow = AgentWorkflow::new("content-pipeline")
//!     .add_agent("researcher", researcher_agent)
//!     .add_agent("writer", writer_agent)
//!     .add_agent("editor", editor_agent)
//!     .chain(["researcher", "writer", "editor"])
//!     .build();
//!
//! // 执行工作流
//! // Execute the workflow
//! let result = workflow.run("Write an article about Rust").await?;
//! ```

use super::agent::LLMAgent;
use super::types::{LLMError, LLMResult};
use std::collections::HashMap;
use std::future::Future;
use std::panic;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Agent 工作流节点类型
/// Agent workflow node types
#[derive(Debug, Clone)]
pub enum AgentNodeType {
    /// 开始节点
    /// Start node
    Start,
    /// 结束节点
    /// End node
    End,
    /// LLM Agent 节点
    /// LLM Agent node
    Agent,
    /// 条件路由节点
    /// Conditional router node
    Router,
    /// 并行分发节点
    /// Parallel dispatch node
    Parallel,
    /// 聚合节点
    /// Join/Aggregation node
    Join,
    /// 转换节点
    /// Transformation node
    Transform,
}

/// 节点输入/输出值
/// Node input/output values
#[derive(Debug, Clone)]
pub enum AgentValue {
    /// 空值
    /// Null value
    Null,
    /// 文本
    /// Text string
    Text(String),
    /// 多个文本（用于并行结果）
    /// Multiple strings (for parallel results)
    Texts(Vec<String>),
    /// 键值对
    /// Key-value pairs
    Map(HashMap<String, String>),
    /// JSON 值
    /// JSON value
    Json(serde_json::Value),
}

impl AgentValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            AgentValue::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn into_text(self) -> String {
        match self {
            AgentValue::Text(s) => s,
            AgentValue::Texts(v) => v.join("\n"),
            AgentValue::Map(m) => serde_json::to_string(&m).unwrap_or_default(),
            AgentValue::Json(j) => j.to_string(),
            AgentValue::Null => String::new(),
        }
    }

    pub fn as_texts(&self) -> Option<&Vec<String>> {
        match self {
            AgentValue::Texts(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&HashMap<String, String>> {
        match self {
            AgentValue::Map(m) => Some(m),
            _ => None,
        }
    }
}

impl From<String> for AgentValue {
    fn from(s: String) -> Self {
        AgentValue::Text(s)
    }
}

impl From<&str> for AgentValue {
    fn from(s: &str) -> Self {
        AgentValue::Text(s.to_string())
    }
}

impl From<Vec<String>> for AgentValue {
    fn from(v: Vec<String>) -> Self {
        AgentValue::Texts(v)
    }
}

impl From<HashMap<String, String>> for AgentValue {
    fn from(m: HashMap<String, String>) -> Self {
        AgentValue::Map(m)
    }
}

/// 路由决策函数类型
/// Router decision function type
pub type RouterFn =
    Arc<dyn Fn(AgentValue) -> Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync>;

/// 转换函数类型
/// Transformation function type
pub type TransformFn =
    Arc<dyn Fn(AgentValue) -> Pin<Box<dyn Future<Output = AgentValue> + Send>> + Send + Sync>;

/// 聚合函数类型
/// Aggregation/Join function type
pub type JoinFn = Arc<
    dyn Fn(HashMap<String, AgentValue>) -> Pin<Box<dyn Future<Output = AgentValue> + Send>>
        + Send
        + Sync,
>;

/// Agent 工作流节点
/// Agent workflow node
pub struct AgentNode {
    /// 节点 ID
    /// Node ID
    pub id: String,
    /// 节点名称
    /// Node name
    pub name: String,
    /// 节点类型
    /// Node type
    pub node_type: AgentNodeType,
    /// Agent 引用（仅 Agent 节点）
    /// Agent reference (Agent nodes only)
    agent: Option<Arc<LLMAgent>>,
    /// 路由函数（仅 Router 节点）
    /// Router function (Router nodes only)
    router: Option<RouterFn>,
    /// 转换函数（仅 Transform 节点）
    /// Transform function (Transform nodes only)
    transform: Option<TransformFn>,
    /// 聚合函数（仅 Join 节点）
    /// Join function (Join nodes only)
    join_fn: Option<JoinFn>,
    /// 等待的节点列表（仅 Join 节点）
    /// List of nodes to wait for (Join nodes only)
    wait_for: Vec<String>,
    /// 提示词模板（Agent 节点使用）
    /// Prompt template (Used by Agent nodes)
    prompt_template: Option<String>,
    /// 会话 ID（用于多轮对话）
    /// Session ID (Used for multi-turn dialogue)
    session_id: Option<String>,
}

impl AgentNode {
    /// 创建开始节点
    /// Create a start node
    pub fn start() -> Self {
        Self {
            id: "start".to_string(),
            name: "Start".to_string(),
            node_type: AgentNodeType::Start,
            agent: None,
            router: None,
            transform: None,
            join_fn: None,
            wait_for: Vec::new(),
            prompt_template: None,
            session_id: None,
        }
    }

    /// 创建结束节点
    /// Create an end node
    pub fn end() -> Self {
        Self {
            id: "end".to_string(),
            name: "End".to_string(),
            node_type: AgentNodeType::End,
            agent: None,
            router: None,
            transform: None,
            join_fn: None,
            wait_for: Vec::new(),
            prompt_template: None,
            session_id: None,
        }
    }

    /// 创建 Agent 节点
    /// Create an Agent node
    pub fn agent(id: impl Into<String>, agent: Arc<LLMAgent>) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            node_type: AgentNodeType::Agent,
            agent: Some(agent),
            router: None,
            transform: None,
            join_fn: None,
            wait_for: Vec::new(),
            prompt_template: None,
            session_id: None,
        }
    }

    /// 创建路由节点
    /// Create a router node
    pub fn router<F, Fut>(id: impl Into<String>, router_fn: F) -> Self
    where
        F: Fn(AgentValue) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = String> + Send + 'static,
    {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            node_type: AgentNodeType::Router,
            agent: None,
            router: Some(Arc::new(move |input| Box::pin(router_fn(input)))),
            transform: None,
            join_fn: None,
            wait_for: Vec::new(),
            prompt_template: None,
            session_id: None,
        }
    }

    /// 创建并行节点
    /// Create a parallel node
    pub fn parallel(id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            node_type: AgentNodeType::Parallel,
            agent: None,
            router: None,
            transform: None,
            join_fn: None,
            wait_for: Vec::new(),
            prompt_template: None,
            session_id: None,
        }
    }

    /// 创建聚合节点
    /// Create a join node
    pub fn join(id: impl Into<String>, wait_for: Vec<String>) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            node_type: AgentNodeType::Join,
            agent: None,
            router: None,
            transform: None,
            join_fn: None,
            wait_for,
            prompt_template: None,
            session_id: None,
        }
    }

    /// 创建带自定义聚合函数的聚合节点
    /// Create a join node with a custom aggregation function
    pub fn join_with<F, Fut>(id: impl Into<String>, wait_for: Vec<String>, join_fn: F) -> Self
    where
        F: Fn(HashMap<String, AgentValue>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = AgentValue> + Send + 'static,
    {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            node_type: AgentNodeType::Join,
            agent: None,
            router: None,
            transform: None,
            join_fn: Some(Arc::new(move |inputs| Box::pin(join_fn(inputs)))),
            wait_for,
            prompt_template: None,
            session_id: None,
        }
    }

    /// 创建转换节点
    /// Create a transform node
    pub fn transform<F, Fut>(id: impl Into<String>, transform_fn: F) -> Self
    where
        F: Fn(AgentValue) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = AgentValue> + Send + 'static,
    {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            node_type: AgentNodeType::Transform,
            agent: None,
            router: None,
            transform: Some(Arc::new(move |input| Box::pin(transform_fn(input)))),
            join_fn: None,
            wait_for: Vec::new(),
            prompt_template: None,
            session_id: None,
        }
    }

    /// 设置名称
    /// Set node name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// 设置提示词模板
    /// Set prompt template
    ///
    /// 模板中可以使用 `{input}` 占位符
    /// `{input}` placeholder can be used in the template
    pub fn with_prompt_template(mut self, template: impl Into<String>) -> Self {
        self.prompt_template = Some(template.into());
        self
    }

    /// 设置会话 ID
    /// Set session ID
    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }
}

/// Agent 工作流边
/// Agent workflow edge
#[derive(Debug, Clone)]
pub struct AgentEdge {
    /// 源节点 ID
    /// Source node ID
    pub from: String,
    /// 目标节点 ID
    /// Target node ID
    pub to: String,
    /// 条件（用于路由）
    /// Condition (used for routing)
    pub condition: Option<String>,
}

impl AgentEdge {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            condition: None,
        }
    }

    pub fn conditional(
        from: impl Into<String>,
        to: impl Into<String>,
        condition: impl Into<String>,
    ) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            condition: Some(condition.into()),
        }
    }
}

/// Agent 工作流执行上下文
/// Agent workflow execution context
pub struct AgentWorkflowContext {
    /// 工作流 ID
    /// Workflow ID
    pub workflow_id: String,
    /// 执行 ID
    /// Execution ID
    pub execution_id: String,
    /// 节点输出
    /// Node outputs
    node_outputs: Arc<RwLock<HashMap<String, AgentValue>>>,
    /// Cached router decisions keyed by router node ID
    router_decisions: Arc<RwLock<HashMap<String, String>>>,
    /// 共享会话 ID（用于多 Agent 共享上下文）
    /// Shared session ID (for cross-agent context sharing)
    shared_session_id: Option<String>,
    /// 变量存储
    /// Variable storage
    variables: Arc<RwLock<HashMap<String, String>>>,
    /// Maximum number of execution steps before aborting (prevents infinite loops)
    pub max_steps: usize,
}

impl AgentWorkflowContext {
    pub fn new(workflow_id: impl Into<String>) -> Self {
        Self {
            workflow_id: workflow_id.into(),
            execution_id: uuid::Uuid::now_v7().to_string(),
            node_outputs: Arc::new(RwLock::new(HashMap::new())),
            router_decisions: Arc::new(RwLock::new(HashMap::new())),
            shared_session_id: None,
            variables: Arc::new(RwLock::new(HashMap::new())),
            max_steps: 25, // Default limit matching LangGraph convention
        }
    }

    /// Set the maximum number of execution steps
    pub fn with_max_steps(mut self, max_steps: usize) -> Self {
        self.max_steps = max_steps;
        self
    }

    pub fn with_shared_session(mut self, session_id: impl Into<String>) -> Self {
        self.shared_session_id = Some(session_id.into());
        self
    }

    pub async fn set_output(&self, node_id: &str, value: AgentValue) {
        let mut outputs = self.node_outputs.write().await;
        outputs.insert(node_id.to_string(), value);
    }

    pub async fn get_output(&self, node_id: &str) -> Option<AgentValue> {
        let outputs = self.node_outputs.read().await;
        outputs.get(node_id).cloned()
    }

    pub async fn get_outputs(&self, node_ids: &[String]) -> HashMap<String, AgentValue> {
        let outputs = self.node_outputs.read().await;
        node_ids
            .iter()
            .filter_map(|id| outputs.get(id).map(|v| (id.clone(), v.clone())))
            .collect()
    }

    pub async fn set_router_decision(&self, node_id: &str, route: &str) {
        let mut decisions = self.router_decisions.write().await;
        decisions.insert(node_id.to_string(), route.to_string());
    }

    pub async fn get_router_decision(&self, node_id: &str) -> Option<String> {
        let decisions = self.router_decisions.read().await;
        decisions.get(node_id).cloned()
    }

    pub async fn set_variable(&self, key: &str, value: &str) {
        let mut vars = self.variables.write().await;
        vars.insert(key.to_string(), value.to_string());
    }

    pub async fn get_variable(&self, key: &str) -> Option<String> {
        let vars = self.variables.read().await;
        vars.get(key).cloned()
    }
}

impl Clone for AgentWorkflowContext {
    fn clone(&self) -> Self {
        Self {
            workflow_id: self.workflow_id.clone(),
            execution_id: self.execution_id.clone(),
            node_outputs: self.node_outputs.clone(),
            router_decisions: self.router_decisions.clone(),
            shared_session_id: self.shared_session_id.clone(),
            variables: self.variables.clone(),
            max_steps: self.max_steps,
        }
    }
}

/// Agent 工作流
/// Agent Workflow
pub struct AgentWorkflow {
    /// 工作流 ID
    /// Workflow ID
    pub id: String,
    /// 工作流名称
    /// Workflow name
    pub name: String,
    /// 节点映射
    /// Node mapping
    nodes: HashMap<String, AgentNode>,
    /// 边列表
    /// Edge list
    edges: Vec<AgentEdge>,
    /// 邻接表（源节点 -> 边列表）
    /// Adjacency list (source node -> edges)
    adjacency: HashMap<String, Vec<AgentEdge>>,
}

impl AgentWorkflow {
    /// 创建新的工作流构建器
    /// Create a new workflow builder
    pub fn builder(id: impl Into<String>) -> AgentWorkflowBuilder {
        AgentWorkflowBuilder::new(id)
    }

    /// 执行工作流
    /// Run the workflow
    pub async fn run(&self, input: impl Into<AgentValue>) -> LLMResult<AgentValue> {
        let ctx = AgentWorkflowContext::new(&self.id);
        self.run_with_context(&ctx, input).await
    }

    /// 使用指定上下文执行工作流
    /// Run the workflow with a specified context
    pub async fn run_with_context(
        &self,
        ctx: &AgentWorkflowContext,
        input: impl Into<AgentValue>,
    ) -> LLMResult<AgentValue> {
        tracing::info!(workflow_id = %self.id, execution_id = %ctx.execution_id, "Agent workflow execution started");
        let input = input.into();
        let mut current_node_id = "start".to_string();
        let mut current_input = input;
        let mut step_count: usize = 0;

        loop {
            step_count += 1;
            if step_count > ctx.max_steps {
                return Err(LLMError::Other(format!(
                    "Workflow '{}' exceeded maximum step limit of {}. \
                     Possible infinite loop detected. \
                     Use AgentWorkflowContext::with_max_steps() to increase the limit if needed.",
                    self.id, ctx.max_steps
                )));
            }

            let node = self
                .nodes
                .get(&current_node_id)
                .ok_or_else(|| LLMError::Other(format!("Node '{}' not found", current_node_id)))?;

            // 执行节点
            // Execute node
            let output = self.execute_node(ctx, node, current_input.clone()).await?;

            // 保存输出
            // Save output
            ctx.set_output(&current_node_id, output.clone()).await;

            // 确定下一个节点
            // Determine next node
            match self.get_next_node(ctx, &current_node_id, &output).await {
                Some(next_id) => {
                    current_node_id = next_id;
                    current_input = output;
                }
                None => {
                    // 工作流结束
                    // Workflow finished
                    return Ok(output);
                }
            }
        }
    }

    /// 执行单个节点
    /// Execute a single node
    async fn execute_node(
        &self,
        ctx: &AgentWorkflowContext,
        node: &AgentNode,
        input: AgentValue,
    ) -> LLMResult<AgentValue> {
        tracing::info!(workflow_id = %self.id, node_id = %node.id, node_type = ?node.node_type, "Executing workflow node");
        match node.node_type {
            AgentNodeType::Start | AgentNodeType::End => Ok(input),

            AgentNodeType::Agent => {
                let agent = node
                    .agent
                    .as_ref()
                    .ok_or_else(|| LLMError::Other("Agent not set".to_string()))?;

                // 构建提示词
                // Construct prompt
                let prompt = if let Some(ref template) = node.prompt_template {
                    template.replace("{input}", &input.clone().into_text())
                } else {
                    input.clone().into_text()
                };

                // 确定会话 ID
                // Determine session ID
                let session_id = node
                    .session_id
                    .clone()
                    .or_else(|| ctx.shared_session_id.clone());

                // 发送消息
                // Send message
                let response = if let Some(sid) = session_id {
                    // 确保会话存在
                    // Ensure session exists
                    let _ = agent.get_or_create_session(&sid).await;
                    agent.chat_with_session(&sid, &prompt).await?
                } else {
                    agent.ask(&prompt).await?
                };

                Ok(AgentValue::Text(response))
            }

            AgentNodeType::Router => {
                let router = node
                    .router
                    .as_ref()
                    .ok_or_else(|| LLMError::Other("Router function not set".to_string()))?;
                
                // Panic containment for user-provided router closure
                let route = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                    router(input.clone())
                }))
                .map_err(|payload| {
                    let panic_msg = extract_panic_message(payload);
                    tracing::error!(
                        workflow_id = %self.id,
                        node_id = %node.id,
                        error = %panic_msg,
                        "Panic caught in router function"
                    );
                    LLMError::Other(format!(
                        "Router node '{}' panicked: {}",
                        node.id, panic_msg
                    ))
                })?
                .await;
                
                ctx.set_router_decision(&node.id, &route).await;
                // 路由节点返回原输入，路由决策在 get_next_node 中使用
                // Router returns original input; decision is used in get_next_node
                Ok(input)
            }

            AgentNodeType::Parallel => {
                // 并行节点直接传递输入，实际并行执行在工作流执行逻辑中处理
                // Parallel node passes input; concurrent logic is handled in workflow execution
                Ok(input)
            }

            AgentNodeType::Join => {
                // 收集所有前置节点的输出
                // Collect outputs from all predecessor nodes
                let outputs = ctx.get_outputs(&node.wait_for).await;

                if let Some(ref join_fn) = node.join_fn {
                    // Panic containment for user-provided join closure
                    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                        join_fn(outputs)
                    }))
                    .map_err(|payload| {
                        let panic_msg = extract_panic_message(payload);
                        tracing::error!(
                            workflow_id = %self.id,
                            node_id = %node.id,
                            error = %panic_msg,
                            "Panic caught in join function"
                        );
                        LLMError::Other(format!(
                            "Join node '{}' panicked: {}",
                            node.id, panic_msg
                        ))
                    })?
                    .await;
                    Ok(result)
                } else {
                    // 默认聚合：合并所有文本输出
                    // Default aggregation: merge all text outputs
                    let texts: Vec<String> = outputs.into_values().map(|v| v.into_text()).collect();
                    Ok(AgentValue::Texts(texts))
                }
            }

            AgentNodeType::Transform => {
                let transform = node
                    .transform
                    .as_ref()
                    .ok_or_else(|| LLMError::Other("Transform function not set".to_string()))?;
                
                // Panic containment for user-provided transform closure
                let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                    transform(input)
                }))
                .map_err(|payload| {
                    let panic_msg = extract_panic_message(payload);
                    tracing::error!(
                        workflow_id = %self.id,
                        node_id = %node.id,
                        error = %panic_msg,
                        "Panic caught in transform function"
                    );
                    LLMError::Other(format!(
                        "Transform node '{}' panicked: {}",
                        node.id, panic_msg
                    ))
                })?
                .await;
                
                Ok(result)
            }
        }
    }

    /// 获取下一个节点
    /// Get the next node
    async fn get_next_node(
        &self,
        ctx: &AgentWorkflowContext,
        current_id: &str,
        output: &AgentValue,
    ) -> Option<String> {
        let node = self.nodes.get(current_id)?;

        // 结束节点没有后续
        // End node has no successor
        if matches!(node.node_type, AgentNodeType::End) {
            return None;
        }

        let edges = self.adjacency.get(current_id)?;

        // 路由节点：根据路由函数结果选择边
        // Router node: select edge based on router function result
        if matches!(node.node_type, AgentNodeType::Router) {
            let route = match ctx.get_router_decision(current_id).await {
                Some(route) => route,
                None => {
                    let router = node.router.as_ref()?;
                    
                    // Panic containment for router in get_next_node
                    // If panic occurs, log error and end workflow gracefully
                    let router_result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                        router(output.clone())
                    }));
                    
                    match router_result {
                        Ok(router_future) => router_future.await,
                        Err(payload) => {
                            let panic_msg = extract_panic_message(payload);
                            tracing::error!(
                                workflow_id = %self.id,
                                node_id = %current_id,
                                error = %panic_msg,
                                "Panic caught in router function during node selection"
                            );
                            // Return None to end workflow gracefully
                            return None;
                        }
                    }
                }
            };

            for edge in edges {
                if edge.condition.as_ref() == Some(&route) {
                    return Some(edge.to.clone());
                }
            }
            // 如果没有匹配的条件边，使用默认边（无条件）
            // If no conditional edge matches, use the default unconditional edge
            for edge in edges {
                if edge.condition.is_none() {
                    return Some(edge.to.clone());
                }
            }
            return None;
        }

        // 非路由节点：使用第一条边
        // Non-router node: use the first available edge
        edges.first().map(|e| e.to.clone())
    }

    /// 获取节点
    /// Get a node
    pub fn get_node(&self, id: &str) -> Option<&AgentNode> {
        self.nodes.get(id)
    }

    /// 获取所有节点 ID
    /// Get all node IDs
    pub fn node_ids(&self) -> Vec<&str> {
        self.nodes.keys().map(|s| s.as_str()).collect()
    }
}

/// Agent 工作流构建器
/// Agent Workflow Builder
pub struct AgentWorkflowBuilder {
    id: String,
    name: String,
    nodes: HashMap<String, AgentNode>,
    edges: Vec<AgentEdge>,
    current_node: Option<String>,
}

impl AgentWorkflowBuilder {
    /// 创建新的构建器
    /// Create a new builder
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        let mut builder = Self {
            name: id.clone(),
            id,
            nodes: HashMap::new(),
            edges: Vec::new(),
            current_node: None,
        };
        // 自动添加 start 节点
        // Automatically add the start node
        builder
            .nodes
            .insert("start".to_string(), AgentNode::start());
        builder.current_node = Some("start".to_string());
        builder
    }

    /// 设置名称
    /// Set the name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// 添加 Agent 节点
    /// Add an Agent node
    pub fn add_agent(mut self, id: impl Into<String>, agent: Arc<LLMAgent>) -> Self {
        let id = id.into();
        let node = AgentNode::agent(&id, agent);
        self.nodes.insert(id, node);
        self
    }

    /// 添加带提示词模板的 Agent 节点
    /// Add an Agent node with a prompt template
    pub fn add_agent_with_template(
        mut self,
        id: impl Into<String>,
        agent: Arc<LLMAgent>,
        template: impl Into<String>,
    ) -> Self {
        let id = id.into();
        let node = AgentNode::agent(&id, agent).with_prompt_template(template);
        self.nodes.insert(id, node);
        self
    }

    /// 添加路由节点
    /// Add a router node
    pub fn add_router<F, Fut>(mut self, id: impl Into<String>, router_fn: F) -> Self
    where
        F: Fn(AgentValue) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = String> + Send + 'static,
    {
        let id = id.into();
        let node = AgentNode::router(&id, router_fn);
        self.nodes.insert(id, node);
        self
    }

    /// 添加基于 LLM 的智能路由节点
    /// Add an LLM-based intelligent router node
    pub fn add_llm_router(
        mut self,
        id: impl Into<String>,
        router_agent: Arc<LLMAgent>,
        routes: Vec<String>,
    ) -> Self {
        let id = id.into();
        let routes_str = routes.join(", ");
        let prompt = format!(
            "Based on the following input, choose the most appropriate route. \
            Available routes: {}. \
            Respond with ONLY the route name, nothing else.\n\nInput: {{input}}",
            routes_str
        );

        let routes_clone = routes.clone();
        let router_fn = move |input: AgentValue| {
            let agent = router_agent.clone();
            let prompt = prompt.replace("{input}", &input.into_text());
            let valid_routes = routes_clone.clone();
            async move {
                match agent.ask(&prompt).await {
                    Ok(response) => {
                        let route = response.trim().to_string();
                        if valid_routes.contains(&route) {
                            route
                        } else {
                            valid_routes.first().cloned().unwrap_or_default()
                        }
                    }
                    Err(_) => valid_routes.first().cloned().unwrap_or_default(),
                }
            }
        };

        let node = AgentNode::router(&id, router_fn);
        self.nodes.insert(id, node);
        self
    }

    /// 添加转换节点
    /// Add a transform node
    pub fn add_transform<F, Fut>(mut self, id: impl Into<String>, transform_fn: F) -> Self
    where
        F: Fn(AgentValue) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = AgentValue> + Send + 'static,
    {
        let id = id.into();
        let node = AgentNode::transform(&id, transform_fn);
        self.nodes.insert(id, node);
        self
    }

    /// 添加并行节点
    /// Add a parallel node
    pub fn add_parallel(mut self, id: impl Into<String>) -> Self {
        let id = id.into();
        let node = AgentNode::parallel(&id);
        self.nodes.insert(id, node);
        self
    }

    /// 添加聚合节点
    /// Add a join node
    pub fn add_join(mut self, id: impl Into<String>, wait_for: Vec<&str>) -> Self {
        let id = id.into();
        let wait_for: Vec<String> = wait_for.into_iter().map(|s| s.to_string()).collect();
        let node = AgentNode::join(&id, wait_for);
        self.nodes.insert(id, node);
        self
    }

    /// 添加带自定义函数的聚合节点
    /// Add a join node with a custom function
    pub fn add_join_with<F, Fut>(
        mut self,
        id: impl Into<String>,
        wait_for: Vec<&str>,
        join_fn: F,
    ) -> Self
    where
        F: Fn(HashMap<String, AgentValue>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = AgentValue> + Send + 'static,
    {
        let id = id.into();
        let wait_for: Vec<String> = wait_for.into_iter().map(|s| s.to_string()).collect();
        let node = AgentNode::join_with(&id, wait_for, join_fn);
        self.nodes.insert(id, node);
        self
    }

    /// 添加边
    /// Add an edge
    pub fn connect(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.edges.push(AgentEdge::new(from, to));
        self
    }

    /// 添加条件边
    /// Add a conditional edge
    pub fn connect_on(
        mut self,
        from: impl Into<String>,
        to: impl Into<String>,
        condition: impl Into<String>,
    ) -> Self {
        self.edges.push(AgentEdge::conditional(from, to, condition));
        self
    }

    /// 链式连接多个节点
    /// Chain multiple nodes together
    ///
    /// 自动将 start 连接到第一个节点，并将最后一个节点连接到 end
    /// Automatically connects 'start' to the first node and the last node to 'end'
    pub fn chain<S: Into<String> + Clone>(mut self, node_ids: impl IntoIterator<Item = S>) -> Self {
        let ids: Vec<String> = node_ids.into_iter().map(|s| s.into()).collect();

        if ids.is_empty() {
            return self;
        }

        // 连接 start 到第一个节点
        // Connect start to the first node
        self.edges.push(AgentEdge::new("start", &ids[0]));

        // 连接中间节点
        // Connect intermediate nodes
        for i in 0..ids.len() - 1 {
            self.edges.push(AgentEdge::new(&ids[i], &ids[i + 1]));
        }

        // 添加 end 节点并连接
        // Add end node and connect it
        self.nodes.insert("end".to_string(), AgentNode::end());
        self.edges.push(AgentEdge::new(ids.last().unwrap(), "end"));

        self
    }

    /// 配置并行执行
    /// Configure parallel execution
    ///
    /// 从 parallel_node 分发到多个 Agent，然后在 join_node 聚合
    /// Dispatches from parallel_node to multiple agents, then aggregates at join_node
    pub fn parallel_agents(
        mut self,
        parallel_id: impl Into<String>,
        agent_ids: Vec<&str>,
        join_id: impl Into<String>,
    ) -> Self {
        let parallel_id = parallel_id.into();
        let join_id = join_id.into();

        // 添加并行节点
        // Add parallel node
        self.nodes
            .insert(parallel_id.clone(), AgentNode::parallel(&parallel_id));

        // 添加聚合节点
        // Add join node
        let wait_for: Vec<String> = agent_ids.iter().map(|s| s.to_string()).collect();
        self.nodes
            .insert(join_id.clone(), AgentNode::join(&join_id, wait_for));

        // 连接并行节点到各个 Agent
        // Connect parallel node to each agent
        for agent_id in &agent_ids {
            self.edges.push(AgentEdge::new(&parallel_id, *agent_id));
            self.edges.push(AgentEdge::new(*agent_id, &join_id));
        }

        self
    }

    /// 构建工作流
    /// Build the workflow
    #[must_use]
    pub fn build(self) -> AgentWorkflow {
        // 构建邻接表
        // Build adjacency list
        let mut adjacency: HashMap<String, Vec<AgentEdge>> = HashMap::new();
        for edge in &self.edges {
            adjacency
                .entry(edge.from.clone())
                .or_default()
                .push(edge.clone());
        }

        AgentWorkflow {
            id: self.id,
            name: self.name,
            nodes: self.nodes,
            edges: self.edges,
            adjacency,
        }
    }
}

// ============================================================================
// Panic Containment Helper Functions
// ============================================================================

/// Extract panic message from payload
/// Handles cases: &str, String, and unknown panic payload
fn extract_panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "Unknown panic payload".to_string()
    }
}

// ============================================================================
// 便捷函数
// Helper functions
// ============================================================================

/// 创建简单的顺序 Agent 工作流
/// Create a simple sequential agent workflow
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// let workflow = agent_chain("my-pipeline", vec![
///     ("researcher", researcher_agent),
///     ("writer", writer_agent),
/// ]);
/// ```
pub fn agent_chain<S: Into<String>>(
    id: S,
    agents: Vec<(impl Into<String>, Arc<LLMAgent>)>,
) -> AgentWorkflow {
    let mut builder = AgentWorkflowBuilder::new(id);
    let mut ids = Vec::new();

    for (agent_id, agent) in agents {
        let agent_id = agent_id.into();
        ids.push(agent_id.clone());
        builder = builder.add_agent(agent_id, agent);
    }

    builder.chain(ids).build()
}

/// 创建并行 Agent 工作流
/// Create a parallel agent workflow
///
/// 所有 Agent 同时处理输入，结果合并后返回
/// All agents process input simultaneously; results are merged before returning
pub fn agent_parallel<S: Into<String>>(
    id: S,
    agents: Vec<(impl Into<String>, Arc<LLMAgent>)>,
) -> AgentWorkflow {
    let mut builder = AgentWorkflowBuilder::new(id);
    let mut ids = Vec::new();

    for (agent_id, agent) in agents {
        let agent_id = agent_id.into();
        ids.push(agent_id.clone());
        builder = builder.add_agent(agent_id, agent);
    }

    let ids_ref: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();

    builder
        .parallel_agents("parallel", ids_ref, "join")
        .connect("start", "parallel")
        .connect("join", "end")
        .build()
}

/// 创建带路由的 Agent 工作流
/// Create an agent workflow with routing
///
/// 根据路由 Agent 的决策选择执行哪个 Agent
/// Selects which agent to execute based on the router agent's decision
pub fn agent_router<S: Into<String>>(
    id: S,
    router_agent: Arc<LLMAgent>,
    routes: Vec<(impl Into<String>, Arc<LLMAgent>)>,
) -> AgentWorkflow {
    let mut builder = AgentWorkflowBuilder::new(id);
    let mut route_names = Vec::new();

    for (route_id, agent) in routes {
        let route_id = route_id.into();
        route_names.push(route_id.clone());
        builder = builder.add_agent(&route_id, agent);
    }

    // 添加 LLM 路由器
    // Add LLM router
    builder = builder.add_llm_router("router", router_agent, route_names.clone());

    // 连接 start -> router
    // Connect start -> router
    builder = builder.connect("start", "router");

    // 添加 end 节点
    // Add end node
    builder.nodes.insert("end".to_string(), AgentNode::end());

    // 连接 router -> 各个 route -> end
    // Connect router -> each route -> end
    for route_name in &route_names {
        builder = builder.connect_on("router", route_name, route_name);
        builder = builder.connect(route_name, "end");
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_value_conversions() {
        let v: AgentValue = "hello".into();
        assert_eq!(v.as_text(), Some("hello"));

        let v: AgentValue = "world".to_string().into();
        assert_eq!(v.as_text(), Some("world"));

        let v: AgentValue = vec!["a".to_string(), "b".to_string()].into();
        assert_eq!(v.as_texts().map(|v| v.len()), Some(2));
    }

    #[test]
    fn test_workflow_builder() {
        let workflow = AgentWorkflowBuilder::new("test")
            .with_name("Test Workflow")
            .add_transform("uppercase", |input: AgentValue| async move {
                AgentValue::Text(input.into_text().to_uppercase())
            })
            .chain(["uppercase"])
            .build();

        assert_eq!(workflow.node_ids().len(), 3); // start, uppercase, end
    }

    #[tokio::test]
    async fn test_trace_propagation_across_workflow_nodes() {
        // Build a multi-node workflow: start -> step1 -> step2 -> end
        // Each step transforms the input, exercising the tracing::info! instrumentation
        // in run_with_context and execute_node.
        let workflow = AgentWorkflowBuilder::new("trace-test")
            .add_transform("step1", |input: AgentValue| async move {
                AgentValue::Text(format!("{}-step1", input.into_text()))
            })
            .add_transform("step2", |input: AgentValue| async move {
                AgentValue::Text(format!("{}-step2", input.into_text()))
            })
            .chain(["step1", "step2"])
            .build();

        let result = workflow.run("init").await.expect("workflow should succeed");
        // Verify execution correctness — tracing instrumentation must not alter behavior
        assert_eq!(result.as_text(), Some("init-step1-step2"));
    }

    #[tokio::test]
    async fn test_router_node_reuses_cached_route_decision() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let invocations = Arc::new(AtomicUsize::new(0));
        let router_invocations = invocations.clone();

        let workflow = AgentWorkflowBuilder::new("router-caching-test")
            .add_router("router", move |_input: AgentValue| {
                let router_invocations = router_invocations.clone();
                async move {
                    if router_invocations.fetch_add(1, Ordering::SeqCst) == 0 {
                        "left".to_string()
                    } else {
                        "right".to_string()
                    }
                }
            })
            .add_transform("left", |input: AgentValue| async move {
                AgentValue::Text(format!("left:{}", input.into_text()))
            })
            .add_transform("right", |input: AgentValue| async move {
                AgentValue::Text(format!("right:{}", input.into_text()))
            })
            .connect("start", "router")
            .connect_on("router", "left", "left")
            .connect_on("router", "right", "right");

        let mut workflow = workflow;
        workflow.nodes.insert("end".to_string(), AgentNode::end());
        let workflow = workflow
            .connect("left", "end")
            .connect("right", "end")
            .build();

        let result = workflow.run("seed").await.expect("workflow should succeed");

        assert_eq!(result.as_text(), Some("left:seed"));
        assert_eq!(invocations.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_chain_builder() {
        let workflow = AgentWorkflowBuilder::new("chain-test")
            .add_transform("step1", |input| async move { input })
            .add_transform("step2", |input| async move { input })
            .add_transform("step3", |input| async move { input })
            .chain(["step1", "step2", "step3"])
            .build();

        assert!(workflow.get_node("start").is_some());
        assert!(workflow.get_node("end").is_some());
        assert!(workflow.get_node("step1").is_some());
        assert!(workflow.get_node("step2").is_some());
        assert!(workflow.get_node("step3").is_some());
    }

    // ============================================================================
    // Panic Containment Tests
    // ============================================================================

    #[test]
    fn test_extract_panic_message() {
        // Test &str panic payload
        let payload: Box<dyn std::any::Any + Send> = Box::new("test panic message");
        let msg = extract_panic_message(payload);
        assert_eq!(msg, "test panic message");

        // Test String panic payload
        let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("string panic"));
        let msg = extract_panic_message(payload);
        assert_eq!(msg, "string panic");

        // Test unknown panic payload
        let payload: Box<dyn std::any::Any + Send> = Box::new(42i32);
        let msg = extract_panic_message(payload);
        assert_eq!(msg, "Unknown panic payload");
    }
}
