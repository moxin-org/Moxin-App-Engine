use mofa_kernel::agent::types::error::{GlobalError, GlobalResult};
#[cfg(feature = "monitoring")]
pub use mofa_monitoring::*;

// Unified error conversions (GlobalError <-> runtime errors)
pub mod error_conversions;
// =============================================================================
// MoFA Runtime - Agent Lifecycle and Execution Management
// =============================================================================
//
// This module provides runtime infrastructure for managing agent execution.
// It follows microkernel architecture principles by depending only on the
// kernel layer for core abstractions.
//
// Main Components:
// - AgentBuilder: Builder pattern for constructing agents
// - SimpleRuntime: Multi-agent coordination (non-dora mode)
// - AgentRuntime: Dora-rs integration (with `dora` feature)
// - run_agents: Simplified agent execution helper
//
// =============================================================================

pub mod agent;
pub mod builder;
pub mod config;
pub mod fallback;
pub mod interrupt;
pub mod rag;
pub mod retry;
pub mod runner;
pub mod security;
pub mod swarm;

// Dora adapter module (only compiled when dora feature is enabled)
#[cfg(feature = "dora")]
pub mod dora_adapter;

// Native dataflow module — always compiled, zero Dora dependency
pub mod native_dataflow;

// =============================================================================
// Re-exports from Kernel (minimal, only what runtime needs)
// =============================================================================
//
// Runtime needs these core types from kernel for its functionality:
// - MoFAAgent: Core agent trait that runtime executes
// - AgentConfig: Configuration structure
// - AgentMetadata: Agent metadata
// - AgentEvent, AgentMessage: Event and message types
// - AgentPlugin: Plugin trait for extensibility
//
// These are re-exported for user convenience when working with runtime APIs.
// =============================================================================

pub use interrupt::*;

// Security governance module
pub use security::{SecurityConfig, SecurityError, SecurityEvent, SecurityService};

// Core agent trait - runtime executes agents implementing this trait
pub use mofa_kernel::agent::MoFAAgent;

pub use mofa_kernel::agent::AgentMetadata;
// Core types needed for runtime operations
pub use mofa_kernel::core::AgentConfig;
pub use mofa_kernel::message::{AgentEvent, AgentMessage};

// Plugin system - runtime supports plugins
pub use mofa_kernel::plugin::AgentPlugin;

// Import from mofa-foundation
// Import from mofa-kernel

// Import from mofa-plugins
use mofa_plugins::AgentPlugin as PluginAgent;

// External dependencies
use std::collections::HashMap;
use std::time::Duration;

// Dora feature dependencies
#[cfg(feature = "dora")]
use crate::dora_adapter::{
    ChannelConfig, DataflowConfig, DoraAgentNode, DoraChannel, DoraDataflow, DoraError,
    DoraNodeConfig, DoraResult, MessageEnvelope,
};
#[cfg(feature = "dora")]
use ::tracing::{debug, info};
#[cfg(feature = "dora")]
use std::sync::Arc;
#[cfg(feature = "dora")]
use tokio::sync::RwLock;

// Private import for internal use
use mofa_kernel::message::StreamType;

/// 智能体构建器 - 提供流式 API
/// Agent Builder - Provides a fluent API
pub struct AgentBuilder {
    agent_id: String,
    name: String,
    capabilities: Vec<String>,
    dependencies: Vec<String>,
    plugins: Vec<Box<dyn PluginAgent>>,
    node_config: HashMap<String, String>,
    inputs: Vec<String>,
    outputs: Vec<String>,
    max_concurrent_tasks: usize,
    default_timeout: Duration,
}
// ------------------------------
// 简化的 SDK API
// Simplified SDK API
// ------------------------------

pub use crate::runner::run_agents;

impl AgentBuilder {
    /// 创建新的 AgentBuilder
    /// Creates a new AgentBuilder
    pub fn new(agent_id: &str, name: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            name: name.to_string(),
            capabilities: Vec::new(),
            dependencies: Vec::new(),
            plugins: Vec::new(),
            node_config: HashMap::new(),
            inputs: vec!["task_input".to_string()],
            outputs: vec!["task_output".to_string()],
            max_concurrent_tasks: 10,
            default_timeout: Duration::from_secs(30),
        }
    }

    /// 添加能力
    /// Adds a capability
    pub fn with_capability(mut self, capability: &str) -> Self {
        self.capabilities.push(capability.to_string());
        self
    }

    /// 添加多个能力
    /// Adds multiple capabilities
    pub fn with_capabilities(mut self, capabilities: Vec<&str>) -> Self {
        for cap in capabilities {
            self.capabilities.push(cap.to_string());
        }
        self
    }

    /// 添加依赖
    /// Adds a dependency
    pub fn with_dependency(mut self, dependency: &str) -> Self {
        self.dependencies.push(dependency.to_string());
        self
    }

    /// 添加插件
    /// Adds a plugin
    pub fn with_plugin(mut self, plugin: Box<dyn AgentPlugin>) -> Self {
        self.plugins.push(plugin);
        self
    }

    /// 添加输入端口
    /// Adds an input port
    pub fn with_input(mut self, input: &str) -> Self {
        self.inputs.push(input.to_string());
        self
    }

    /// 添加输出端口
    /// Adds an output port
    pub fn with_output(mut self, output: &str) -> Self {
        self.outputs.push(output.to_string());
        self
    }

    /// 设置最大并发任务数
    /// Sets max concurrent tasks
    pub fn with_max_concurrent_tasks(mut self, max: usize) -> Self {
        self.max_concurrent_tasks = max;
        self
    }

    /// 设置默认超时
    /// Sets default timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    /// 添加自定义配置
    /// Adds custom configuration
    pub fn with_config(mut self, key: &str, value: &str) -> Self {
        self.node_config.insert(key.to_string(), value.to_string());
        self
    }

    /// 构建智能体配置
    /// Builds agent configuration
    pub fn build_config(&self) -> AgentConfig {
        AgentConfig {
            agent_id: self.agent_id.clone(),
            name: self.name.clone(),
            node_config: self.node_config.clone(),
        }
    }

    /// 构建元数据
    /// Builds metadata
    pub fn build_metadata(&self) -> AgentMetadata {
        use mofa_kernel::agent::AgentCapabilities;
        use mofa_kernel::agent::AgentState;

        // 将 Vec<String> 转换为 AgentCapabilities
        // Convert Vec<String> to AgentCapabilities
        let agent_capabilities = AgentCapabilities::builder()
            .tags(self.capabilities.clone())
            .build();

        AgentMetadata {
            id: self.agent_id.clone(),
            name: self.name.clone(),
            description: None,
            version: None,
            capabilities: agent_capabilities,
            state: AgentState::Created,
        }
    }

    /// 构建 DoraNodeConfig
    /// Builds DoraNodeConfig
    #[cfg(feature = "dora")]
    pub fn build_node_config(&self) -> DoraNodeConfig {
        DoraNodeConfig {
            node_id: self.agent_id.clone(),
            name: self.name.clone(),
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            event_buffer_size: self.max_concurrent_tasks * 10,
            default_timeout: self.default_timeout,
            custom_config: self.node_config.clone(),
        }
    }

    /// 使用提供的 MoFAAgent 实现构建运行时
    /// Builds runtime with provided MoFAAgent implementation
    #[cfg(feature = "dora")]
    pub async fn with_agent<A: MoFAAgent>(self, agent: A) -> DoraResult<AgentRuntime<A>> {
        let node_config = self.build_node_config();
        let metadata = self.build_metadata();
        let config = self.build_config();

        let node = DoraAgentNode::new(node_config);
        let interrupt = node.interrupt().clone();

        let context = mofa_kernel::agent::AgentContext::new(self.agent_id.clone());

        Ok(AgentRuntime {
            agent,
            node: Arc::new(node),
            metadata,
            config,
            interrupt,
            plugins: self.plugins,
            context,
        })
    }

    /// 构建并启动智能体（需要提供 MoFAAgent 实现）
    /// Builds and starts the agent (requires MoFAAgent implementation)
    #[cfg(feature = "dora")]
    pub async fn build_and_start<A: MoFAAgent>(self, agent: A) -> DoraResult<AgentRuntime<A>> {
        let runtime: AgentRuntime<A> = self.with_agent(agent).await?;
        runtime.start().await?;
        Ok(runtime)
    }

    /// 使用提供的 MoFAAgent 实现构建简单运行时（非 dora 模式）
    /// Builds simple runtime with MoFAAgent implementation (non-dora mode)
    #[cfg(not(feature = "dora"))]
    pub async fn with_agent<A: MoFAAgent>(self, agent: A) -> GlobalResult<SimpleAgentRuntime<A>> {
        let metadata = self.build_metadata();
        let config = self.build_config();
        let interrupt = AgentInterrupt::new();

        // 创建事件通道
        // Creates event channel
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(100);
        let context = mofa_kernel::agent::AgentContext::new(self.agent_id.clone());

        Ok(SimpleAgentRuntime {
            agent,
            metadata,
            config,
            interrupt,
            plugins: self.plugins,
            inputs: self.inputs,
            outputs: self.outputs,
            max_concurrent_tasks: self.max_concurrent_tasks,
            default_timeout: self.default_timeout,
            event_tx,
            event_rx: Some(event_rx),
            context,
            security_service: None,
        })
    }

    /// 构建并启动智能体（非 dora 模式）
    /// Builds and starts the agent (non-dora mode)
    #[cfg(not(feature = "dora"))]
    pub async fn build_and_start<A: MoFAAgent>(
        self,
        agent: A,
    ) -> GlobalResult<SimpleAgentRuntime<A>> {
        let mut runtime = self.with_agent(agent).await?;
        runtime.start().await?;
        Ok(runtime)
    }
}

/// 智能体运行时
/// Agent Runtime
#[cfg(feature = "dora")]
pub struct AgentRuntime<A: MoFAAgent> {
    agent: A,
    node: Arc<DoraAgentNode>,
    metadata: AgentMetadata,
    config: AgentConfig,
    interrupt: AgentInterrupt,
    plugins: Vec<Box<dyn AgentPlugin>>,
    context: mofa_kernel::agent::AgentContext,
}

#[cfg(feature = "dora")]
impl<A: MoFAAgent> AgentRuntime<A> {
    /// 获取智能体引用
    /// Gets agent reference
    pub fn agent(&self) -> &A {
        &self.agent
    }

    /// 获取可变智能体引用
    /// Gets mutable agent reference
    pub fn agent_mut(&mut self) -> &mut A {
        &mut self.agent
    }

    /// 获取节点
    /// Gets node reference
    pub fn node(&self) -> &Arc<DoraAgentNode> {
        &self.node
    }

    /// 获取元数据
    /// Gets metadata reference
    pub fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    /// 获取配置
    /// Gets configuration reference
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// 获取中断句柄
    /// Gets interrupt handle
    pub fn interrupt(&self) -> &AgentInterrupt {
        &self.interrupt
    }

    /// 初始化插件
    /// Initializes plugins
    pub async fn init_plugins(&mut self) -> DoraResult<()> {
        for plugin in &mut self.plugins {
            plugin
                .init_plugin()
                .await
                .map_err(|e| DoraError::OperatorError(e.to_string()))?;
        }
        Ok(())
    }

    /// 启动运行时
    /// Starts the runtime
    pub async fn start(&self) -> DoraResult<()> {
        self.node.init().await?;
        info!("AgentRuntime {} started", self.metadata.id);
        Ok(())
    }

    /// 运行事件循环
    /// Runs the event loop
    pub async fn run_event_loop(&mut self) -> DoraResult<()> {
        // 使用已存储的 CoreAgentContext 初始化智能体
        // Initialize agent with the stored CoreAgentContext
        self.agent
            .initialize(&self.context)
            .await
            .map_err(|e| DoraError::Internal(e.to_string()))?;

        // 初始化插件
        // Initialize plugins
        self.init_plugins().await?;

        let event_loop = self.node.create_event_loop();

        loop {
            // 检查中断
            // Check for interrupts
            if event_loop.should_interrupt() {
                debug!("Interrupt signal received for {}", self.metadata.id);
                self.interrupt.reset();
            }

            // 获取下一个事件
            // Get the next event
            match event_loop.next_event().await {
                Some(AgentEvent::Shutdown) => {
                    info!("Received shutdown event");
                    break;
                }
                Some(event) => {
                    // 处理事件前检查中断
                    // Check for interrupts before processing event
                    if self.interrupt.check() {
                        debug!("Interrupt signal received for {}", self.metadata.id);
                        self.interrupt.reset();
                    }

                    // 将事件转换为输入并使用 execute
                    // Convert event to input and call execute
                    use mofa_kernel::agent::types::AgentInput;
                    use mofa_kernel::message::TaskRequest;

                    let input = match event.clone() {
                        AgentEvent::TaskReceived(task) => AgentInput::text(task.content),
                        AgentEvent::Custom(data, _) => AgentInput::text(data),
                        _ => AgentInput::text(format!("{:?}", event)),
                    };

                    self.agent
                        .execute(input, &self.context)
                        .await
                        .map_err(|e| DoraError::Internal(e.to_string()))?;
                }
                None => {
                    // 无事件，继续等待
                    // No events, continue waiting
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }

        // 关闭智能体
        // Shut down the agent
        self.agent
            .shutdown()
            .await
            .map_err(|e| DoraError::Internal(e.to_string()))?;

        Ok(())
    }

    /// 停止运行时
    /// Stops the runtime
    pub async fn stop(&self) -> DoraResult<()> {
        self.interrupt.trigger();
        self.node.stop().await?;
        info!("AgentRuntime {} stopped", self.metadata.id);
        Ok(())
    }

    /// 发送消息到输出
    /// Sends message to output
    pub async fn send_output(&self, output_id: &str, message: &AgentMessage) -> DoraResult<()> {
        self.node.send_message(output_id, message).await
    }

    /// 注入事件
    /// Injects an event
    pub async fn inject_event(&self, event: AgentEvent) -> DoraResult<()> {
        self.node.inject_event(event).await
    }
}

// ============================================================================
// 非 dora 运行时实现 - SimpleAgentRuntime
// Non-dora runtime implementation - SimpleAgentRuntime
// ============================================================================

/// 简单智能体运行时 - 不依赖 dora-rs 的轻量级运行时
/// Simple Agent Runtime - Lightweight runtime not dependent on dora-rs
#[cfg(not(feature = "dora"))]
pub struct SimpleAgentRuntime<A: MoFAAgent> {
    agent: A,
    metadata: AgentMetadata,
    config: AgentConfig,
    interrupt: AgentInterrupt,
    plugins: Vec<Box<dyn AgentPlugin>>,
    inputs: Vec<String>,
    outputs: Vec<String>,
    max_concurrent_tasks: usize,
    default_timeout: Duration,
    // 添加事件通道
    // Add event channel
    event_tx: tokio::sync::mpsc::Sender<AgentEvent>,
    event_rx: Option<tokio::sync::mpsc::Receiver<AgentEvent>>,
    pub(crate) context: mofa_kernel::agent::AgentContext,
    // Security service for RBAC and other security checks
    security_service: Option<std::sync::Arc<security::SecurityService>>,
}

#[cfg(not(feature = "dora"))]
impl<A: MoFAAgent> SimpleAgentRuntime<A> {
    /// 获取智能体引用
    /// Gets agent reference
    pub fn agent(&self) -> &A {
        &self.agent
    }

    /// 获取可变智能体引用
    /// Gets mutable agent reference
    pub fn agent_mut(&mut self) -> &mut A {
        &mut self.agent
    }

    /// 获取元数据
    /// Gets metadata reference
    pub fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    /// 获取配置
    /// Gets configuration reference
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// 获取上下文
    /// Gets context reference
    pub fn context(&self) -> &mofa_kernel::agent::AgentContext {
        &self.context
    }

    /// 获取中断句柄
    /// Gets interrupt handle
    pub fn interrupt(&self) -> &AgentInterrupt {
        &self.interrupt
    }

    /// 获取输入端口列表
    /// Gets list of input ports
    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }

    /// 获取输出端口列表
    /// Gets list of output ports
    pub fn outputs(&self) -> &[String] {
        &self.outputs
    }

    /// 获取最大并发任务数
    /// Gets max concurrent tasks
    pub fn max_concurrent_tasks(&self) -> usize {
        self.max_concurrent_tasks
    }

    /// 获取默认超时时间
    /// Gets default timeout duration
    pub fn default_timeout(&self) -> Duration {
        self.default_timeout
    }

    /// 注入事件
    pub async fn inject_event(&self, event: AgentEvent) {
        let _ = self.event_tx.send(event).await;
    }

    /// 初始化插件
    /// Initializes plugins
    pub async fn init_plugins(&mut self) -> GlobalResult<()> {
        for plugin in &mut self.plugins {
            plugin
                .init_plugin()
                .await
                .map_err(|e| GlobalError::Other(e.to_string()))?;
        }
        Ok(())
    }

    /// 启动运行时
    /// Starts the runtime
    pub async fn start(&mut self) -> GlobalResult<()> {
        // 初始化智能体 - 使用存储的 context
        // Initialize agent - using stored context
        self.agent.initialize(&self.context).await?;
        // 初始化插件
        // Initialize plugins
        self.init_plugins().await?;
        ::tracing::info!("SimpleAgentRuntime {} started", self.metadata.id);
        Ok(())
    }

    /// Set the security service for RBAC and other security checks
    pub fn with_security_service(
        mut self,
        security_service: std::sync::Arc<security::SecurityService>,
    ) -> Self {
        self.security_service = Some(security_service);
        self
    }

    /// 处理单个事件
    /// Processes a single event
    pub async fn handle_event(&mut self, event: AgentEvent) -> GlobalResult<()> {
        // Security interceptor: Check permissions before processing
        if let Some(security) = &self.security_service
            && security.is_rbac_enabled()
            && let Some(authorizer) = security.authorizer()
        {
            // Check if agent has permission to execute
            match authorizer
                .check_permission(&self.metadata.id, "execute", "agent")
                .await
            {
                Ok(auth_result) if auth_result.is_denied() => {
                    ::tracing::warn!(
                        agent_id = %self.metadata.id,
                        reason = %auth_result.reason().unwrap_or("unknown"),
                        "Permission denied for agent execution"
                    );
                    return Err(GlobalError::Other(format!(
                        "Permission denied: {}",
                        auth_result.reason().unwrap_or("unknown")
                    )));
                }
                Err(e) => {
                    // Handle security check failure based on fail mode
                    match security.config().fail_mode {
                        security::types::SecurityFailMode::FailClosed => {
                            return Err(GlobalError::Other(format!(
                                "Security check failed: {}",
                                e
                            )));
                        }
                        security::types::SecurityFailMode::FailOpen => {
                            ::tracing::warn!(
                                "Security check failed, allowing due to fail-open mode: {}",
                                e
                            );
                        }
                    }
                }
                _ => {} // Permission granted, continue
            }
        }

        // 检查中断 - 注意：MoFAAgent 没有 on_interrupt 方法
        // Check interrupt - Note: MoFAAgent lacks on_interrupt method
        // 中断处理需要由 Agent 内部自行处理或通过 AgentMessaging 扩展
        // Interrupts must be handled internally or via AgentMessaging extensions
        if self.interrupt.check() {
            // 中断信号，可以选择停止或通知 agent
            // Interrupt signal; can choose to stop or notify the agent
            ::tracing::debug!("Interrupt signal received for {}", self.metadata.id);
            self.interrupt.reset();
        }

        // 将事件转换为输入并使用 execute
        // Convert event to input and call execute
        use mofa_kernel::agent::types::AgentInput;

        // 尝试将事件转换为输入
        // Try to convert event to input
        let input = match event {
            AgentEvent::TaskReceived(task) => AgentInput::text(task.content),
            AgentEvent::Shutdown => {
                ::tracing::info!("Shutdown event received for {}", self.metadata.id);
                return Ok(());
            }
            AgentEvent::Custom(data, _) => AgentInput::text(data),
            _ => AgentInput::text(format!("{:?}", event)),
        };

        let _output = self.agent.execute(input, &self.context).await?;
        Ok(())
    }

    /// 运行事件循环（使用内部事件接收器）
    /// Runs event loop (using internal event receiver)
    pub async fn run(&mut self) -> GlobalResult<()> {
        // 获取内部事件接收器
        // Get internal event receiver
        let event_rx = self
            .event_rx
            .take()
            .ok_or_else(|| GlobalError::Other("Event receiver already taken".to_string()))?;

        self.run_with_receiver(event_rx).await
    }

    /// 运行事件循环（使用事件通道）
    /// Runs event loop (using event channel)
    pub async fn run_with_receiver(
        &mut self,
        mut event_rx: tokio::sync::mpsc::Receiver<AgentEvent>,
    ) -> GlobalResult<()> {
        loop {
            // 检查中断
            // Check for interrupts
            if self.interrupt.check() {
                ::tracing::debug!("Interrupt signal received for {}", self.metadata.id);
                self.interrupt.reset();
            }

            // 等待事件
            // Wait for event
            match tokio::time::timeout(Duration::from_millis(100), event_rx.recv()).await {
                Ok(Some(AgentEvent::Shutdown)) => {
                    ::tracing::info!("Received shutdown event");
                    break;
                }
                Ok(Some(event)) => {
                    // 使用 handle_event 方法（它会将事件转换为 execute 调用）
                    // Use handle_event (converts event to execute call)
                    self.handle_event(event).await?;
                }
                Ok(None) => {
                    // 通道关闭
                    // Channel closed
                    break;
                }
                Err(_) => {
                    // 超时，继续等待
                    // Timeout, continue waiting
                    continue;
                }
            }
        }

        // 关闭智能体 - 使用 shutdown 而不是 destroy
        // Shut down agent - use shutdown instead of destroy
        self.agent.shutdown().await?;
        Ok(())
    }

    /// 停止运行时
    /// Stops the runtime
    pub async fn stop(&mut self) -> GlobalResult<()> {
        self.interrupt.trigger();
        self.agent.shutdown().await?;
        ::tracing::info!("SimpleAgentRuntime {} stopped", self.metadata.id);
        Ok(())
    }

    /// 触发中断
    /// Triggers an interrupt
    pub fn trigger_interrupt(&self) {
        self.interrupt.trigger();
    }
}

// ============================================================================
// 简单多智能体运行时 - SimpleRuntime
// Simple Multi-Agent Runtime - SimpleRuntime
// ============================================================================

/// 简单运行时 - 管理多个智能体的协同运行（非 dora 版本）
/// Simple Runtime - Manages collaborative operation of multiple agents (non-dora version)
#[cfg(not(feature = "dora"))]
pub struct SimpleRuntime {
    agents: std::sync::Arc<tokio::sync::RwLock<HashMap<String, SimpleAgentInfo>>>,
    agent_roles: std::sync::Arc<tokio::sync::RwLock<HashMap<String, String>>>,
    message_bus: std::sync::Arc<SimpleMessageBus>,
}

/// 智能体信息
/// Agent Information
#[cfg(not(feature = "dora"))]
pub struct SimpleAgentInfo {
    pub metadata: AgentMetadata,
    pub config: AgentConfig,
    pub event_tx: tokio::sync::mpsc::Sender<AgentEvent>,
}

/// 流状态信息
/// Stream Status Information
#[cfg(not(feature = "dora"))]
#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub stream_id: String,
    pub stream_type: StreamType,
    pub metadata: HashMap<String, String>,
    pub subscribers: Vec<String>,
    pub sequence: u64,
    pub is_paused: bool,
}

/// 简单消息总线
/// Simple Message Bus
#[cfg(not(feature = "dora"))]
pub struct SimpleMessageBus {
    subscribers: tokio::sync::RwLock<HashMap<String, Vec<tokio::sync::mpsc::Sender<AgentEvent>>>>,
    topic_subscribers: tokio::sync::RwLock<HashMap<String, Vec<String>>>,
    // 流支持
    // Stream support
    streams: tokio::sync::RwLock<HashMap<String, StreamInfo>>,
}

#[cfg(not(feature = "dora"))]
impl SimpleMessageBus {
    /// 创建新的消息总线
    pub fn new() -> Self {
        Self {
            subscribers: tokio::sync::RwLock::new(HashMap::new()),
            topic_subscribers: tokio::sync::RwLock::new(HashMap::new()),
            streams: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    /// 注册智能体
    /// Register an agent
    pub async fn register(&self, agent_id: &str, tx: tokio::sync::mpsc::Sender<AgentEvent>) {
        let mut subs = self.subscribers.write().await;
        subs.insert(agent_id.to_string(), vec![tx]);
    }

    /// Unregister an agent and clean up its topic subscriptions
    pub async fn unregister(&self, agent_id: &str) {
        {
            let mut subs = self.subscribers.write().await;
            subs.remove(agent_id);
        }

        {
            let mut topics = self.topic_subscribers.write().await;
            for subscriber_ids in topics.values_mut() {
                subscriber_ids.retain(|id| id != agent_id);
            }
            topics.retain(|_, subscriber_ids| !subscriber_ids.is_empty());
        }

        {
            let mut streams = self.streams.write().await;
            streams.retain(|_, stream_info| {
                stream_info.subscribers.retain(|id| id != agent_id);
                !stream_info.subscribers.is_empty()
            });
        }
    }

    /// 订阅主题
    /// Subscribe to a topic
    pub async fn subscribe(&self, agent_id: &str, topic: &str) {
        let mut topics = self.topic_subscribers.write().await;
        let subscriber_ids = topics.entry(topic.to_string()).or_insert_with(Vec::new);
        if !subscriber_ids.iter().any(|id| id == agent_id) {
            subscriber_ids.push(agent_id.to_string());
        }
    }

    /// 发送点对点消息
    /// Send point-to-point message
    pub async fn send_to(&self, target_id: &str, event: AgentEvent) -> GlobalResult<()> {
        let senders = {
            let subs = self.subscribers.read().await;
            subs.get(target_id).cloned().unwrap_or_default()
        };

        for tx in senders {
            let _ = tx.send(event.clone()).await;
        }
        Ok(())
    }

    /// 广播消息给所有智能体
    /// Broadcast message to all agents
    pub async fn broadcast(&self, event: AgentEvent) -> GlobalResult<()> {
        let senders = {
            let subs = self.subscribers.read().await;
            subs.values()
                .flat_map(|agent_senders| agent_senders.iter().cloned())
                .collect::<Vec<_>>()
        };

        for tx in senders {
            let _ = tx.send(event.clone()).await;
        }
        Ok(())
    }

    /// 发布到主题
    /// Publish to a topic
    pub async fn publish(&self, topic: &str, event: AgentEvent) -> GlobalResult<()> {
        let agent_ids = {
            let topics = self.topic_subscribers.read().await;
            topics.get(topic).cloned().unwrap_or_default()
        };

        let senders = {
            let subs = self.subscribers.read().await;
            let mut senders = Vec::new();
            for agent_id in &agent_ids {
                if let Some(agent_senders) = subs.get(agent_id) {
                    for tx in agent_senders {
                        senders.push(tx.clone());
                    }
                }
            }
            senders
        };

        for tx in senders {
            let _ = tx.send(event.clone()).await;
        }

        Ok(())
    }

    // ---------------------------------
    // 流支持方法
    // Stream support methods
    // ---------------------------------

    /// 创建流
    /// Create a stream
    pub async fn create_stream(
        &self,
        stream_id: &str,
        stream_type: StreamType,
        metadata: HashMap<String, String>,
    ) -> GlobalResult<()> {
        {
            let mut streams = self.streams.write().await;
            if streams.contains_key(stream_id) {
                return Err(GlobalError::Other(format!(
                    "Stream {} already exists",
                    stream_id
                )));
            }

            // 创建流信息
            // Create stream information
            let stream_info = StreamInfo {
                stream_id: stream_id.to_string(),
                stream_type: stream_type.clone(),
                metadata: metadata.clone(),
                subscribers: Vec::new(),
                sequence: 0,
                is_paused: false,
            };

            streams.insert(stream_id.to_string(), stream_info);
        }

        // 广播流创建事件
        // Broadcast stream creation event
        self.broadcast(AgentEvent::StreamCreated {
            stream_id: stream_id.to_string(),
            stream_type,
            metadata,
        })
        .await
    }

    /// 关闭流
    /// Close a stream
    pub async fn close_stream(&self, stream_id: &str, reason: &str) -> GlobalResult<()> {
        let subscribers = {
            let mut streams = self.streams.write().await;
            streams
                .remove(stream_id)
                .map(|stream_info| stream_info.subscribers)
                .unwrap_or_default()
        };

        if subscribers.is_empty() {
            return Ok(());
        }

        // 广播流关闭事件
        // Broadcast stream closure event
        let event = AgentEvent::StreamClosed {
            stream_id: stream_id.to_string(),
            reason: reason.to_string(),
        };

        let senders = {
            let subs = self.subscribers.read().await;
            let mut senders = Vec::new();
            for agent_id in &subscribers {
                if let Some(agent_senders) = subs.get(agent_id) {
                    for tx in agent_senders {
                        senders.push(tx.clone());
                    }
                }
            }
            senders
        };

        for tx in senders {
            let _ = tx.send(event.clone()).await;
        }

        Ok(())
    }

    /// 订阅流
    /// Subscribe to a stream
    pub async fn subscribe_stream(&self, agent_id: &str, stream_id: &str) -> GlobalResult<()> {
        let should_broadcast = {
            let mut streams = self.streams.write().await;
            if let Some(stream_info) = streams.get_mut(stream_id) {
                // 检查是否已订阅
                // Check if already subscribed
                if !stream_info.subscribers.contains(&agent_id.to_string()) {
                    stream_info.subscribers.push(agent_id.to_string());
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if should_broadcast {
            // 广播订阅事件
            // Broadcast subscription event
            self.broadcast(AgentEvent::StreamSubscription {
                stream_id: stream_id.to_string(),
                subscriber_id: agent_id.to_string(),
            })
            .await?;
        }
        Ok(())
    }

    /// 取消订阅流
    /// Unsubscribe from a stream
    pub async fn unsubscribe_stream(&self, agent_id: &str, stream_id: &str) -> GlobalResult<()> {
        let should_broadcast = {
            let mut streams = self.streams.write().await;
            if let Some(stream_info) = streams.get_mut(stream_id) {
                // 移除订阅者
                // Remove subscriber
                if let Some(pos) = stream_info.subscribers.iter().position(|id| id == agent_id) {
                    stream_info.subscribers.remove(pos);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if should_broadcast {
            // 广播取消订阅事件
            // Broadcast unsubscription event
            self.broadcast(AgentEvent::StreamUnsubscription {
                stream_id: stream_id.to_string(),
                subscriber_id: agent_id.to_string(),
            })
            .await?;
        }
        Ok(())
    }

    /// 发送流消息
    /// Send stream message
    pub async fn send_stream_message(&self, stream_id: &str, message: Vec<u8>) -> GlobalResult<()> {
        let stream_delivery = {
            let mut streams = self.streams.write().await;
            if let Some(stream_info) = streams.get_mut(stream_id) {
                // 如果流被暂停，直接返回
                // If stream is paused, return immediately
                if stream_info.is_paused {
                    None
                } else {
                    // 生成序列号
                    // Generate sequence number
                    let sequence = stream_info.sequence;
                    stream_info.sequence += 1;

                    // 构造流消息事件
                    // Construct stream message event
                    let event = AgentEvent::StreamMessage {
                        stream_id: stream_id.to_string(),
                        message,
                        sequence,
                    };

                    Some((event, stream_info.subscribers.clone()))
                }
            } else {
                None
            }
        };

        let Some((event, subscribers)) = stream_delivery else {
            return Ok(());
        };

        let senders = {
            let subs = self.subscribers.read().await;
            let mut senders = Vec::new();
            for agent_id in &subscribers {
                if let Some(agent_senders) = subs.get(agent_id) {
                    for tx in agent_senders {
                        senders.push(tx.clone());
                    }
                }
            }
            senders
        };

        for tx in senders {
            let _ = tx.send(event.clone()).await;
        }

        Ok(())
    }

    /// 暂停流
    /// Pause a stream
    pub async fn pause_stream(&self, stream_id: &str) -> GlobalResult<()> {
        let mut streams = self.streams.write().await;
        if let Some(stream_info) = streams.get_mut(stream_id) {
            stream_info.is_paused = true;
        }
        Ok(())
    }

    /// 恢复流
    /// Resume a stream
    pub async fn resume_stream(&self, stream_id: &str) -> GlobalResult<()> {
        let mut streams = self.streams.write().await;
        if let Some(stream_info) = streams.get_mut(stream_id) {
            stream_info.is_paused = false;
        }
        Ok(())
    }

    /// 获取流信息
    /// Get stream information
    pub async fn get_stream_info(&self, stream_id: &str) -> GlobalResult<Option<StreamInfo>> {
        let streams = self.streams.read().await;
        Ok(streams.get(stream_id).cloned())
    }
}

#[cfg(not(feature = "dora"))]
impl Default for SimpleMessageBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "dora"))]
impl SimpleRuntime {
    /// 创建新的简单运行时
    /// Create a new simple runtime
    pub fn new() -> Self {
        Self {
            agents: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            agent_roles: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            message_bus: std::sync::Arc::new(SimpleMessageBus::new()),
        }
    }

    /// 注册智能体
    /// Register an agent
    pub async fn register_agent(
        &self,
        metadata: AgentMetadata,
        config: AgentConfig,
        role: &str,
    ) -> GlobalResult<tokio::sync::mpsc::Receiver<AgentEvent>> {
        self.register_agent_with_capacity(metadata, config, role, 100)
            .await
    }

    /// 注册智能体并指定事件队列容量
    /// Register an agent with explicit event queue capacity
    pub async fn register_agent_with_capacity(
        &self,
        metadata: AgentMetadata,
        config: AgentConfig,
        role: &str,
        queue_capacity: usize,
    ) -> GlobalResult<tokio::sync::mpsc::Receiver<AgentEvent>> {
        let agent_id = metadata.id.clone();
        let capacity = queue_capacity.max(1);
        let (tx, rx) = tokio::sync::mpsc::channel(capacity);

        // 注册到消息总线
        // Register to the message bus
        self.message_bus.register(&agent_id, tx.clone()).await;

        // 添加智能体信息
        // Add agent information
        let mut agents = self.agents.write().await;
        agents.insert(
            agent_id.clone(),
            SimpleAgentInfo {
                metadata,
                config,
                event_tx: tx,
            },
        );

        // 记录角色
        // Record role
        let mut roles = self.agent_roles.write().await;
        roles.insert(agent_id.clone(), role.to_string());

        ::tracing::info!("Agent {} registered with role {}", agent_id, role);
        Ok(rx)
    }

    /// 注销智能体并清理其路由信息
    /// Unregister an agent and clean up its routing entries
    pub async fn unregister_agent(&self, agent_id: &str) -> GlobalResult<bool> {
        let removed = {
            let mut agents = self.agents.write().await;
            agents.remove(agent_id).is_some()
        };

        if removed {
            {
                let mut roles = self.agent_roles.write().await;
                roles.remove(agent_id);
            }
            self.message_bus.unregister(agent_id).await;
            ::tracing::info!("Agent {} unregistered", agent_id);
        }

        Ok(removed)
    }

    /// 获取消息总线
    /// Get the message bus
    pub fn message_bus(&self) -> &std::sync::Arc<SimpleMessageBus> {
        &self.message_bus
    }

    /// 获取指定角色的智能体列表
    /// Get list of agents by specific role
    pub async fn get_agents_by_role(&self, role: &str) -> Vec<String> {
        let roles = self.agent_roles.read().await;
        roles
            .iter()
            .filter(|(_, r)| *r == role)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// 发送消息给指定智能体
    /// Send message to a specific agent
    pub async fn send_to_agent(&self, target_id: &str, event: AgentEvent) -> GlobalResult<()> {
        self.message_bus.send_to(target_id, event).await
    }

    /// 广播消息给所有智能体
    /// Broadcast message to all agents
    pub async fn broadcast(&self, event: AgentEvent) -> GlobalResult<()> {
        self.message_bus.broadcast(event).await
    }

    /// 发布到主题
    /// Publish to a topic
    pub async fn publish_to_topic(&self, topic: &str, event: AgentEvent) -> GlobalResult<()> {
        self.message_bus.publish(topic, event).await
    }

    /// 订阅主题
    /// Subscribe to a topic
    pub async fn subscribe_topic(&self, agent_id: &str, topic: &str) -> GlobalResult<()> {
        self.message_bus.subscribe(agent_id, topic).await;
        Ok(())
    }

    // ---------------------------------
    // 流支持方法
    // Stream support methods
    // ---------------------------------

    /// 创建流
    /// Create stream
    pub async fn create_stream(
        &self,
        stream_id: &str,
        stream_type: StreamType,
        metadata: std::collections::HashMap<String, String>,
    ) -> GlobalResult<()> {
        self.message_bus
            .create_stream(stream_id, stream_type, metadata)
            .await
    }

    /// 关闭流
    /// Close stream
    pub async fn close_stream(&self, stream_id: &str, reason: &str) -> GlobalResult<()> {
        self.message_bus.close_stream(stream_id, reason).await
    }

    /// 订阅流
    /// Subscribe to stream
    pub async fn subscribe_stream(&self, agent_id: &str, stream_id: &str) -> GlobalResult<()> {
        self.message_bus.subscribe_stream(agent_id, stream_id).await
    }

    /// 取消订阅流
    /// Unsubscribe from stream
    pub async fn unsubscribe_stream(&self, agent_id: &str, stream_id: &str) -> GlobalResult<()> {
        self.message_bus
            .unsubscribe_stream(agent_id, stream_id)
            .await
    }

    /// 发送流消息
    /// Send stream message
    pub async fn send_stream_message(&self, stream_id: &str, message: Vec<u8>) -> GlobalResult<()> {
        self.message_bus
            .send_stream_message(stream_id, message)
            .await
    }

    /// 暂停流
    /// Pause stream
    pub async fn pause_stream(&self, stream_id: &str) -> GlobalResult<()> {
        self.message_bus.pause_stream(stream_id).await
    }

    /// 恢复流
    /// Resume stream
    pub async fn resume_stream(&self, stream_id: &str) -> GlobalResult<()> {
        self.message_bus.resume_stream(stream_id).await
    }

    /// 获取流信息
    /// Get stream info
    pub async fn get_stream_info(&self, stream_id: &str) -> GlobalResult<Option<StreamInfo>> {
        self.message_bus.get_stream_info(stream_id).await
    }

    /// 停止所有智能体
    /// Stop all agents
    pub async fn stop_all(&self) -> GlobalResult<()> {
        self.message_bus.broadcast(AgentEvent::Shutdown).await?;
        ::tracing::info!("SimpleRuntime stopped");
        Ok(())
    }
}

#[cfg(not(feature = "dora"))]
impl Default for SimpleRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, not(feature = "dora")))]
mod tests {
    use super::SimpleMessageBus;
    use mofa_kernel::message::{AgentEvent, StreamType};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio::time::{Duration, timeout};

    #[tokio::test]
    async fn send_to_does_not_block_register_on_backpressure() {
        let bus = Arc::new(SimpleMessageBus::new());
        let (slow_tx, mut slow_rx) = mpsc::channel(1);
        bus.register("slow", slow_tx.clone()).await;

        slow_tx.send(AgentEvent::Shutdown).await.unwrap();

        let bus_for_send = Arc::clone(&bus);
        let send_task =
            tokio::spawn(async move { bus_for_send.send_to("slow", AgentEvent::Shutdown).await });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let (new_tx, _new_rx) = mpsc::channel(1);
        timeout(Duration::from_millis(200), bus.register("new", new_tx))
            .await
            .expect("register should not block while send_to waits");

        let _ = slow_rx.recv().await;
        send_task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn send_stream_message_does_not_block_pause_on_backpressure() {
        let bus = Arc::new(SimpleMessageBus::new());
        bus.create_stream("stream-a", StreamType::DataStream, HashMap::new())
            .await
            .unwrap();

        let (slow_tx, mut slow_rx) = mpsc::channel(1);
        bus.register("slow", slow_tx.clone()).await;
        bus.subscribe_stream("slow", "stream-a").await.unwrap();
        let _ = slow_rx.recv().await;

        slow_tx.send(AgentEvent::Shutdown).await.unwrap();

        let bus_for_send = Arc::clone(&bus);
        let send_task = tokio::spawn(async move {
            bus_for_send
                .send_stream_message("stream-a", b"data".to_vec())
                .await
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        timeout(Duration::from_millis(200), bus.pause_stream("stream-a"))
            .await
            .expect("pause_stream should not block while send_stream_message waits")
            .unwrap();

        let _ = slow_rx.recv().await;
        send_task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn re_registration_replaces_stale_sender() {
        let bus = SimpleMessageBus::new();

        let (tx1, rx1) = tokio::sync::mpsc::channel(1);
        bus.register("agent-a", tx1).await;
        drop(rx1); // simulate agent restart

        let (tx2, _rx2) = tokio::sync::mpsc::channel(1);
        bus.register("agent-a", tx2).await;

        let subs = bus.subscribers.read().await;
        assert_eq!(subs["agent-a"].len(), 1);
    }
}

/// 智能体节点存储类型
/// Storage type for agent nodes
#[cfg(feature = "dora")]
type AgentNodeMap = HashMap<String, Arc<DoraAgentNode>>;

/// MoFA 运行时 - 管理多个智能体的协同运行
/// MoFA Runtime - Manages collaborative operation of multiple agents
#[cfg(feature = "dora")]
pub struct MoFARuntime {
    dataflow: Option<DoraDataflow>,
    channel: Arc<DoraChannel>,
    agents: Arc<RwLock<AgentNodeMap>>,
    agent_roles: Arc<RwLock<HashMap<String, String>>>,
}

#[cfg(feature = "dora")]
impl MoFARuntime {
    /// 创建新的运行时
    /// Create new runtime
    pub async fn new() -> Self {
        let channel_config = ChannelConfig::default();
        Self {
            dataflow: None,
            channel: Arc::new(DoraChannel::new(channel_config)),
            agents: Arc::new(RwLock::new(HashMap::new())),
            agent_roles: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 使用 Dataflow 配置创建运行时
    /// Create runtime with Dataflow configuration
    pub async fn with_dataflow(dataflow_config: DataflowConfig) -> Self {
        let dataflow = DoraDataflow::new(dataflow_config);
        let channel_config = ChannelConfig::default();
        Self {
            dataflow: Some(dataflow),
            channel: Arc::new(DoraChannel::new(channel_config)),
            agents: Arc::new(RwLock::new(HashMap::new())),
            agent_roles: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册智能体节点
    /// Register agent node
    pub async fn register_agent(&self, node: DoraAgentNode, role: &str) -> DoraResult<()> {
        let agent_id = node.config().node_id.clone();

        // 注册到通道
        // Register to channel
        self.channel.register_agent(&agent_id).await?;

        // 添加到 dataflow（如果存在）
        // Add to dataflow (if it exists)
        if let Some(ref dataflow) = self.dataflow {
            dataflow.add_node(node).await?;
        } else {
            let mut agents: tokio::sync::RwLockWriteGuard<'_, AgentNodeMap> =
                self.agents.write().await;
            agents.insert(agent_id.clone(), Arc::new(node));
        }

        // 记录角色
        // Record role
        let mut roles = self.agent_roles.write().await;
        roles.insert(agent_id.clone(), role.to_string());

        info!("Agent {} registered with role {}", agent_id, role);
        Ok(())
    }

    /// 连接两个智能体
    /// Connect two agents
    pub async fn connect_agents(
        &self,
        source_id: &str,
        source_output: &str,
        target_id: &str,
        target_input: &str,
    ) -> DoraResult<()> {
        if let Some(ref dataflow) = self.dataflow {
            dataflow
                .connect(source_id, source_output, target_id, target_input)
                .await?;
        }
        Ok(())
    }

    /// 获取通道
    /// Get channel
    pub fn channel(&self) -> &Arc<DoraChannel> {
        &self.channel
    }

    /// 获取指定角色的智能体列表
    /// Get agent list by specific role
    pub async fn get_agents_by_role(&self, role: &str) -> Vec<String> {
        let roles = self.agent_roles.read().await;
        roles
            .iter()
            .filter(|(_, r)| *r == role)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// 发送消息给指定智能体
    /// Send message to specific agent
    pub async fn send_to_agent(
        &self,
        sender_id: &str,
        receiver_id: &str,
        message: &AgentMessage,
    ) -> DoraResult<()> {
        let envelope = MessageEnvelope::from_agent_message(sender_id, message)?.to(receiver_id);
        self.channel.send_p2p(envelope).await
    }

    /// 广播消息给所有智能体
    /// Broadcast message to all agents
    pub async fn broadcast(&self, sender_id: &str, message: &AgentMessage) -> DoraResult<()> {
        let envelope = MessageEnvelope::from_agent_message(sender_id, message)?;
        self.channel.broadcast(envelope).await
    }

    /// 发布到主题
    /// Publish to topic
    pub async fn publish_to_topic(
        &self,
        sender_id: &str,
        topic: &str,
        message: &AgentMessage,
    ) -> DoraResult<()> {
        let envelope = MessageEnvelope::from_agent_message(sender_id, message)?.with_topic(topic);
        self.channel.publish(envelope).await
    }

    /// 订阅主题
    /// Subscribe to topic
    pub async fn subscribe_topic(&self, agent_id: &str, topic: &str) -> DoraResult<()> {
        self.channel.subscribe(agent_id, topic).await
    }

    /// 构建并启动运行时
    /// Build and start runtime
    pub async fn build_and_start(&self) -> DoraResult<()> {
        if let Some(ref dataflow) = self.dataflow {
            dataflow.build().await?;
            dataflow.start().await?;
        } else {
            // 初始化所有独立注册的智能体
            // Initialize all independently registered agents
            let agents: tokio::sync::RwLockReadGuard<'_, AgentNodeMap> = self.agents.read().await;
            for (id, node) in agents.iter() {
                node.init().await?;
                debug!("Agent {} initialized", id);
            }
        }
        info!("MoFARuntime started");
        Ok(())
    }

    /// 停止运行时
    /// Stop runtime
    pub async fn stop(&self) -> DoraResult<()> {
        if let Some(ref dataflow) = self.dataflow {
            dataflow.stop().await?;
        } else {
            let agents: tokio::sync::RwLockReadGuard<'_, AgentNodeMap> = self.agents.read().await;
            for node in agents.values() {
                node.stop().await?;
            }
        }
        info!("MoFARuntime stopped");
        Ok(())
    }

    /// 暂停运行时
    /// Pause runtime
    pub async fn pause(&self) -> DoraResult<()> {
        if let Some(ref dataflow) = self.dataflow {
            dataflow.pause().await?;
        }
        Ok(())
    }

    /// 恢复运行时
    /// Resume runtime
    pub async fn resume(&self) -> DoraResult<()> {
        if let Some(ref dataflow) = self.dataflow {
            dataflow.resume().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[cfg(not(feature = "dora"))]
mod test_agent_context {
    use crate::builder::AgentBuilder;
    use async_trait::async_trait;
    use mofa_kernel::agent::types::AgentOutput;
    use mofa_kernel::agent::{AgentContext, AgentInput, AgentResult, MoFAAgent};
    use mofa_kernel::message::{AgentEvent, AgentMessage, TaskPriority, TaskRequest};
    use serde_json::json;

    struct ContextPersistenceAgent;

    #[async_trait]
    impl MoFAAgent for ContextPersistenceAgent {
        fn id(&self) -> &str {
            "test-persistence-agent"
        }

        fn name(&self) -> &str {
            "Test Persistence Agent"
        }

        fn capabilities(&self) -> &mofa_kernel::agent::AgentCapabilities {
            // we need to return a static reference or cache it, but for test, we can just use a dummy
            // Actually, capabilities returns &AgentCapabilities, which is often tied to self in real agents.
            // Let's just create a static one using once_cell or standard lazy init, or hold it in the struct.
            unimplemented!("Not needed for this particular test")
        }

        fn state(&self) -> mofa_kernel::agent::AgentState {
            mofa_kernel::agent::AgentState::Ready
        }

        async fn initialize(&mut self, _ctx: &AgentContext) -> AgentResult<()> {
            Ok(())
        }

        async fn execute(
            &mut self,
            _input: AgentInput,
            ctx: &AgentContext,
        ) -> AgentResult<AgentOutput> {
            let current_count: u64 = ctx
                .get("test_run_count")
                .await
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            let new_count = current_count + 1;
            ctx.set("test_run_count", json!(new_count)).await;

            Ok(AgentOutput::text(format!("Count is now {}", new_count)))
        }

        async fn shutdown(&mut self) -> AgentResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_agent_context_persistence() {
        // Create an agent with the simple runtime (non-dora)
        let builder = AgentBuilder::new("test-persistence-agent", "Test Agent");

        let mut runtime = builder
            .with_agent(ContextPersistenceAgent)
            .await
            .expect("Failed to build agent runtime");

        runtime.start().await.expect("Failed to start runtime");

        let task = TaskRequest {
            task_id: "test1".to_string(),
            content: "run".to_string(),
            priority: TaskPriority::Normal,
            deadline: None,
            metadata: std::collections::HashMap::new(),
        };

        // Handle first event
        let event1 = AgentEvent::TaskReceived(task.clone());
        runtime
            .handle_event(event1)
            .await
            .expect("Failed to handle first event");

        // The context should hold our json(1) -> u64
        let val1 = runtime
            .context
            .get("test_run_count")
            .await
            .expect("test_run_count should be set after first event");
        assert_eq!(val1.as_u64().unwrap(), 1);

        // Handle second event
        let event2 = AgentEvent::TaskReceived(task);
        runtime
            .handle_event(event2)
            .await
            .expect("Failed to handle second event");

        // The context should have persisted, so score should now be 2
        let val2 = runtime
            .context
            .get("test_run_count")
            .await
            .expect("test_run_count should be set after second event");
        assert_eq!(val2.as_u64().unwrap(), 2);
    }
}

// Additional message-bus tests
#[cfg(test)]
#[cfg(not(feature = "dora"))]
mod test_message_bus {
    use super::*;
    use mofa_kernel::message::AgentEvent;
    use std::time::Duration;

    #[tokio::test]
    async fn unregister_removes_routing_and_prevents_delivery() {
        let bus = SimpleMessageBus::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(4);
        bus.register("agent-x", tx).await;

        // Subscribe to topic
        bus.subscribe("agent-x", "topic-z").await;

        // Ensure subscription exists
        {
            let topics = bus.topic_subscribers.read().await;
            let subs = topics.get("topic-z").cloned().unwrap_or_default();
            assert!(subs.iter().any(|id| id == "agent-x"));
        }

        // Unregister the agent
        bus.unregister("agent-x").await;

        // Confirm routing cleaned up
        {
            let topics = bus.topic_subscribers.read().await;
            assert!(
                !topics
                    .get("topic-z")
                    .map(|v| v.iter().any(|id| id == "agent-x"))
                    .unwrap_or(false)
            );
        }

        // Confirm subscribers mapping cleaned up as well
        {
            let subs = bus.subscribers.read().await;
            assert!(
                !subs.contains_key("agent-x"),
                "subscriber entry should be removed"
            );
        }

        // Publish to topic - should not be delivered
        // Drain any pending messages that may have been queued earlier
        while rx.try_recv().is_ok() {}

        bus.publish("topic-z", AgentEvent::Custom("nada".to_string(), vec![]))
            .await
            .expect("publish");

        match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            Ok(Some(msg)) => panic!("unexpected message after unregister: {msg:?}"),
            Ok(None) => { /* channel closed */ }
            Err(_) => { /* timed out as expected */ }
        }
    }
}
