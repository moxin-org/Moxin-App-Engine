//! 高级 Agent 构建器 API
//! Advanced Agent Builder API
//!
//! 提供流式 API 来构建和运行智能体
//! Provides a fluent API to build and run agents
//!
//! 该模块支持两种运行时模式：
//! This module supports two runtime modes:
//! - 当启用 `dora` feature 时，使用 dora-rs 运行时
//! - When the `dora` feature is enabled, use the dora-rs runtime
//! - 当未启用 `dora` feature 时，使用内置的 SimpleRuntime
//! - When the `dora` feature is disabled, use the built-in SimpleRuntime

#[cfg(feature = "dora")]
use crate::dora_adapter::{
    ChannelConfig, DataflowConfig, DoraAgentNode, DoraChannel, DoraDataflow, DoraError,
    DoraNodeConfig, DoraResult, MessageEnvelope,
};
use crate::interrupt::AgentInterrupt;
use crate::{AgentConfig, AgentMetadata, MoFAAgent};
#[cfg(feature = "dora")]
use ::tracing::{debug, info};
use mofa_kernel::AgentPlugin;
use mofa_kernel::agent::types::error::{GlobalError, GlobalResult};
use mofa_kernel::message::AgentEvent;
#[cfg(feature = "dora")]
use mofa_kernel::message::AgentMessage;
use std::collections::HashMap;
#[cfg(feature = "dora")]
use std::sync::Arc;
use std::time::Duration;
#[cfg(feature = "dora")]
use tokio::sync::RwLock;

/// 智能体构建器 - 提供流式 API
/// Agent Builder - providing fluent API
pub struct AgentBuilder {
    agent_id: String,
    name: String,
    capabilities: Vec<String>,
    dependencies: Vec<String>,
    plugins: Vec<Box<dyn AgentPlugin>>,
    node_config: HashMap<String, String>,
    inputs: Vec<String>,
    outputs: Vec<String>,
    max_concurrent_tasks: usize,
    default_timeout: Duration,
}

impl AgentBuilder {
    /// 创建新的 AgentBuilder
    /// Create a new AgentBuilder
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
    /// Add capability
    pub fn with_capability(mut self, capability: &str) -> Self {
        self.capabilities.push(capability.to_string());
        self
    }

    /// 添加多个能力
    /// Add multiple capabilities
    pub fn with_capabilities(mut self, capabilities: Vec<&str>) -> Self {
        for cap in capabilities {
            self.capabilities.push(cap.to_string());
        }
        self
    }

    /// 添加依赖
    /// Add dependency
    pub fn with_dependency(mut self, dependency: &str) -> Self {
        self.dependencies.push(dependency.to_string());
        self
    }

    /// 添加插件
    /// Add plugin
    pub fn with_plugin(mut self, plugin: Box<dyn AgentPlugin>) -> Self {
        self.plugins.push(plugin);
        self
    }

    /// 添加输入端口
    /// Add input port
    pub fn with_input(mut self, input: &str) -> Self {
        self.inputs.push(input.to_string());
        self
    }

    /// 添加输出端口
    /// Add output port
    pub fn with_output(mut self, output: &str) -> Self {
        self.outputs.push(output.to_string());
        self
    }

    /// 设置最大并发任务数
    /// Set maximum concurrent tasks
    pub fn with_max_concurrent_tasks(mut self, max: usize) -> Self {
        self.max_concurrent_tasks = max;
        self
    }

    /// 设置默认超时
    /// Set default timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    /// 添加自定义配置
    /// Add custom configuration
    pub fn with_config(mut self, key: &str, value: &str) -> Self {
        self.node_config.insert(key.to_string(), value.to_string());
        self
    }

    /// 构建智能体配置
    /// Build agent configuration
    pub fn build_config(&self) -> AgentConfig {
        AgentConfig {
            agent_id: self.agent_id.clone(),
            name: self.name.clone(),
            node_config: self.node_config.clone(),
        }
    }

    /// 构建元数据
    /// Build metadata
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
    /// Build DoraNodeConfig
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
    /// Build runtime with provided MoFAAgent implementation
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
    /// Build and start agent (requires MoFAAgent implementation)
    #[cfg(feature = "dora")]
    pub async fn build_and_start<A: MoFAAgent>(self, agent: A) -> DoraResult<AgentRuntime<A>> {
        let runtime: AgentRuntime<A> = self.with_agent(agent).await?;
        runtime.start().await?;
        Ok(runtime)
    }

    /// 使用提供的 MoFAAgent 实现构建简单运行时（非 dora 模式）
    /// Build simple runtime with provided MoFAAgent implementation (non-dora mode)
    #[cfg(not(feature = "dora"))]
    pub async fn with_agent<A: MoFAAgent>(self, agent: A) -> GlobalResult<SimpleAgentRuntime<A>> {
        let metadata = self.build_metadata();
        let config = self.build_config();
        let interrupt = AgentInterrupt::new();
        let context = mofa_kernel::agent::AgentContext::new(self.agent_id.clone());
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(self.max_concurrent_tasks * 10);

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
        })
    }

    /// 构建并启动智能体（非 dora 模式）
    /// Build and start agent (non-dora mode)
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

#[cfg(test)]
#[cfg(not(feature = "dora"))]
mod simple_message_bus_tests {
    use super::*;
    use mofa_kernel::message::AgentEvent;
    use std::time::Duration;

    #[tokio::test]
    async fn subscribe_is_idempotent() {
        let bus = SimpleMessageBus::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentEvent>(4);
        bus.register("agent1", tx).await;

        // Subscribe twice
        bus.subscribe("agent1", "topic-a").await;
        bus.subscribe("agent1", "topic-a").await;

        // Publish once; the agent should receive exactly one delivery
        bus.publish("topic-a", AgentEvent::Custom("hi".to_string(), vec![]))
            .await
            .expect("publish");

        // First receive should succeed
        let first = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("expected first message within timeout");
        assert!(first.is_some());

        // Second receive should time out (no duplicate delivery)
        let second = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
        assert!(second.is_err(), "did not expect a second message");
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
    /// Get agent reference
    pub fn agent(&self) -> &A {
        &self.agent
    }

    /// 获取可变智能体引用
    /// Get mutable agent reference
    pub fn agent_mut(&mut self) -> &mut A {
        &mut self.agent
    }

    /// 获取节点
    /// Get node
    pub fn node(&self) -> &Arc<DoraAgentNode> {
        &self.node
    }

    /// 获取元数据
    /// Get metadata
    pub fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    /// 获取配置
    /// Get configuration
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// 获取中断句柄
    /// Get interrupt handle
    pub fn interrupt(&self) -> &AgentInterrupt {
        &self.interrupt
    }

    /// 初始化插件
    /// Initialize plugins
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
    /// Start runtime
    pub async fn start(&self) -> DoraResult<()> {
        self.node.init().await?;
        info!("AgentRuntime {} started", self.metadata.id);
        Ok(())
    }

    /// 运行事件循环
    /// Run event loop
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
            // Check for interruption
            if event_loop.should_interrupt() {
                debug!("Interrupt signal received");
                self.interrupt.reset();
            }

            // 获取下一个事件
            // Get next event
            match event_loop.next_event().await {
                Some(AgentEvent::Shutdown) => {
                    info!("Received shutdown event");
                    break;
                }
                Some(event) => {
                    // 处理事件前检查中断
                    // Check for interruption before processing event
                    if self.interrupt.check() {
                        debug!("Interrupt signal received");
                        self.interrupt.reset();
                    }

                    // 将事件转换为输入并执行
                    // Convert event to input and execute
                    use mofa_kernel::agent::types::AgentInput;
                    use mofa_kernel::message::TaskRequest;

                    let input = match event {
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
                    // No event, continue waiting
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }

        // 销毁智能体
        // Shutdown agent
        self.agent
            .shutdown()
            .await
            .map_err(|e| DoraError::Internal(e.to_string()))?;

        Ok(())
    }

    /// 停止运行时
    /// Stop runtime
    pub async fn stop(&self) -> DoraResult<()> {
        self.interrupt.trigger();
        self.node.stop().await?;
        info!("AgentRuntime {} stopped", self.metadata.id);
        Ok(())
    }

    /// 发送消息到输出
    /// Send message to output
    pub async fn send_output(&self, output_id: &str, message: &AgentMessage) -> DoraResult<()> {
        self.node.send_message(output_id, message).await
    }

    /// 注入事件
    /// Inject event
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
    event_tx: tokio::sync::mpsc::Sender<AgentEvent>,
    event_rx: Option<tokio::sync::mpsc::Receiver<AgentEvent>>,
    pub(crate) context: mofa_kernel::agent::AgentContext,
}

#[cfg(not(feature = "dora"))]
impl<A: MoFAAgent> SimpleAgentRuntime<A> {
    /// 获取智能体引用
    /// Get agent reference
    pub fn agent(&self) -> &A {
        &self.agent
    }

    /// 获取可变智能体引用
    /// Get mutable agent reference
    pub fn agent_mut(&mut self) -> &mut A {
        &mut self.agent
    }

    /// 获取元数据
    /// Get metadata
    pub fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    /// 获取配置
    /// Get configuration
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// 获取中断句柄
    /// Get interrupt handle
    pub fn interrupt(&self) -> &AgentInterrupt {
        &self.interrupt
    }

    /// 获取输入端口列表
    /// Get list of input ports
    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }

    /// 获取输出端口列表
    /// Get list of output ports
    pub fn outputs(&self) -> &[String] {
        &self.outputs
    }

    /// 获取最大并发任务数
    /// Get maximum concurrent tasks
    pub fn max_concurrent_tasks(&self) -> usize {
        self.max_concurrent_tasks
    }

    /// 获取默认超时时间
    /// Get default timeout duration
    pub fn default_timeout(&self) -> Duration {
        self.default_timeout
    }

    /// 初始化插件
    /// Initialize plugins
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
    /// Start runtime
    pub async fn start(&mut self) -> GlobalResult<()> {
        // 初始化智能体 - 使用存储的 context
        // Initialize agent - using stored context
        self.agent.initialize(&self.context).await?;
        // 初始化插件
        // Initialize plugins
        self.init_plugins().await?;
        tracing::info!("SimpleAgentRuntime {} started", self.metadata.id);
        Ok(())
    }

    /// 处理单个事件
    /// Process single event
    pub async fn handle_event(&mut self, event: AgentEvent) -> GlobalResult<()> {
        // 检查中断
        // Check for interruption
        if self.interrupt.check() {
            tracing::debug!("Interrupt signal received");
            self.interrupt.reset();
        }

        // 将事件转换为输入并执行
        // Convert event to input and execute
        use mofa_kernel::agent::types::AgentInput;

        let input = match event {
            AgentEvent::TaskReceived(task) => AgentInput::text(task.content),
            AgentEvent::Shutdown => {
                tracing::info!("Shutdown event received");
                return Ok(());
            }
            AgentEvent::Custom(data, _) => AgentInput::text(data),
            _ => AgentInput::text(format!("{:?}", event)),
        };

        let _output = self.agent.execute(input, &self.context).await?;
        Ok(())
    }

    /// 运行事件循环（使用事件通道）
    /// Run event loop (using event channel)
    pub async fn run_with_receiver(
        &mut self,
        mut event_rx: tokio::sync::mpsc::Receiver<AgentEvent>,
    ) -> GlobalResult<()> {
        loop {
            // 检查中断
            // Check for interruption
            if self.interrupt.check() {
                // 中断处理
                // Interruption handling
                tracing::debug!("Interrupt signal received");
                self.interrupt.reset();
            }

            // 等待事件
            // Wait for event
            match tokio::time::timeout(Duration::from_millis(100), event_rx.recv()).await {
                Ok(Some(AgentEvent::Shutdown)) => {
                    tracing::info!("Received shutdown event");
                    break;
                }
                Ok(Some(event)) => {
                    // 将事件转换为输入并执行
                    // Convert event to input and execute
                    use mofa_kernel::agent::types::AgentInput;
                    let input = match event {
                        AgentEvent::TaskReceived(task) => AgentInput::text(task.content),
                        AgentEvent::Custom(data, _) => AgentInput::text(data),
                        _ => AgentInput::text(format!("{:?}", event)),
                    };

                    self.agent.execute(input, &self.context).await?;
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

        // 销毁智能体
        // Shutdown agent
        self.agent.shutdown().await?;
        Ok(())
    }

    /// 停止运行时
    /// Stop runtime
    pub async fn stop(&mut self) -> GlobalResult<()> {
        self.interrupt.trigger();
        self.agent.shutdown().await?;
        tracing::info!("SimpleAgentRuntime {} stopped", self.metadata.id);
        Ok(())
    }

    /// 触发中断
    /// Trigger interruption
    pub fn trigger_interrupt(&self) {
        self.interrupt.trigger();
    }
}

// ============================================================================
// 简单多智能体运行时 - SimpleRuntime
// Simple multi-agent runtime - SimpleRuntime
// ============================================================================

/// 简单运行时 - 管理多个智能体的协同运行（非 dora 版本）
/// Simple Runtime - Manages collaborative operation of multiple agents (non-dora)
#[cfg(not(feature = "dora"))]
pub struct SimpleRuntime {
    agents: std::sync::Arc<tokio::sync::RwLock<HashMap<String, SimpleAgentInfo>>>,
    agent_roles: std::sync::Arc<tokio::sync::RwLock<HashMap<String, String>>>,
    message_bus: std::sync::Arc<SimpleMessageBus>,
}

/// 智能体信息
/// Agent information
#[cfg(not(feature = "dora"))]
pub struct SimpleAgentInfo {
    pub metadata: AgentMetadata,
    pub config: AgentConfig,
    pub event_tx: tokio::sync::mpsc::Sender<AgentEvent>,
}

/// 简单消息总线
/// Simple message bus
#[cfg(not(feature = "dora"))]
pub struct SimpleMessageBus {
    subscribers: tokio::sync::RwLock<HashMap<String, Vec<tokio::sync::mpsc::Sender<AgentEvent>>>>,
    topic_subscribers: tokio::sync::RwLock<HashMap<String, Vec<String>>>,
}

#[cfg(not(feature = "dora"))]
impl SimpleMessageBus {
    /// 创建新的消息总线
    /// Create a new message bus
    pub fn new() -> Self {
        Self {
            subscribers: tokio::sync::RwLock::new(HashMap::new()),
            topic_subscribers: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    /// 注册智能体
    /// Register agent
    ///
    /// Replaces any existing senders for `agent_id` so that stale clones from
    /// previous registrations (e.g. after an agent restart) do not accumulate
    /// in the Vec and leak memory.
    pub async fn register(&self, agent_id: &str, tx: tokio::sync::mpsc::Sender<AgentEvent>) {
        let mut subs = self.subscribers.write().await;
        subs.insert(agent_id.to_string(), vec![tx]);
    }

    /// 订阅主题
    /// Subscribe to topic
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
    /// Publish to topic
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
    /// Register agent
    pub async fn register_agent(
        &self,
        metadata: AgentMetadata,
        config: AgentConfig,
        role: &str,
    ) -> GlobalResult<tokio::sync::mpsc::Receiver<AgentEvent>> {
        let agent_id = metadata.id.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(100);

        // 注册到消息总线
        // Register to message bus
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

        tracing::info!("Agent {} registered with role {}", agent_id, role);
        Ok(rx)
    }

    /// 获取消息总线
    /// Get message bus
    pub fn message_bus(&self) -> &std::sync::Arc<SimpleMessageBus> {
        &self.message_bus
    }

    /// 获取指定角色的智能体列表
    /// Get list of agents by role
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
    pub async fn send_to_agent(&self, target_id: &str, event: AgentEvent) -> GlobalResult<()> {
        self.message_bus.send_to(target_id, event).await
    }

    /// 广播消息给所有智能体
    /// Broadcast message to all agents
    pub async fn broadcast(&self, event: AgentEvent) -> GlobalResult<()> {
        self.message_bus.broadcast(event).await
    }

    /// 发布到主题
    /// Publish to topic
    pub async fn publish_to_topic(&self, topic: &str, event: AgentEvent) -> GlobalResult<()> {
        self.message_bus.publish(topic, event).await
    }

    /// 订阅主题
    /// Subscribe to topic
    pub async fn subscribe_topic(&self, agent_id: &str, topic: &str) -> GlobalResult<()> {
        self.message_bus.subscribe(agent_id, topic).await;
        Ok(())
    }

    /// 停止所有智能体
    /// Stop all agents
    pub async fn stop_all(&self) -> GlobalResult<()> {
        self.message_bus.broadcast(AgentEvent::Shutdown).await?;
        tracing::info!("SimpleRuntime stopped");
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
    use mofa_kernel::message::AgentEvent;
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
    async fn broadcast_does_not_block_register_on_backpressure() {
        let bus = Arc::new(SimpleMessageBus::new());
        let (slow_tx, mut slow_rx) = mpsc::channel(1);
        bus.register("slow", slow_tx.clone()).await;

        slow_tx.send(AgentEvent::Shutdown).await.unwrap();

        let bus_for_send = Arc::clone(&bus);
        let send_task =
            tokio::spawn(async move { bus_for_send.broadcast(AgentEvent::Shutdown).await });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let (new_tx, _new_rx) = mpsc::channel(1);
        timeout(Duration::from_millis(200), bus.register("new", new_tx))
            .await
            .expect("register should not block while broadcast waits");

        let _ = slow_rx.recv().await;
        send_task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn publish_does_not_block_subscribe_on_backpressure() {
        let bus = Arc::new(SimpleMessageBus::new());
        let (slow_tx, mut slow_rx) = mpsc::channel(1);
        bus.register("slow", slow_tx.clone()).await;
        bus.subscribe("slow", "topic-a").await;

        slow_tx.send(AgentEvent::Shutdown).await.unwrap();

        let bus_for_send = Arc::clone(&bus);
        let send_task =
            tokio::spawn(
                async move { bus_for_send.publish("topic-a", AgentEvent::Shutdown).await },
            );

        tokio::time::sleep(Duration::from_millis(50)).await;

        timeout(
            Duration::from_millis(200),
            bus.subscribe("other", "topic-b"),
        )
        .await
        .expect("subscribe should not block while publish waits");

        let _ = slow_rx.recv().await;
        send_task.await.unwrap().unwrap();
    }
}

/// 智能体节点存储类型
/// Agent node storage type
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
        // Add to dataflow (if exists)
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
    /// Get list of agents by role
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
        self.channel.publish_legacy(envelope).await
    }

    /// 订阅主题
    /// Subscribe to topic
    pub async fn subscribe_topic(&self, agent_id: &str, topic: &str) -> DoraResult<()> {
        self.channel.subscribe_legacy(agent_id, topic).await
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
