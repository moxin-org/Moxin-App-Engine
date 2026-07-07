//! MoFA Agent 核心接口 - 统一抽象
//! MoFA Agent Core Interface - Unified Abstraction
//!
//! 本模块定义了 MoFA 框架的统一 Agent 抽象，遵循微内核架构原则：
//! This module defines the unified Agent abstraction for the MoFA framework, following micro-kernel principles:
//! - 核心统一：MoFAAgent 提供唯一的 Agent 接口
//! - Core Unification: MoFAAgent provides the sole Agent interface
//! - 可选扩展：通过扩展 trait 提供额外功能
//! - Optional Extensions: Provide additional functionality via extension traits
//! - 清晰层次：核心接口 + 可选扩展
//! - Clear Hierarchy: Core interface + optional extensions
//!
//! # 架构设计
//! # Architecture Design
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                    MoFAAgent (统一核心接口)                          │
//! │                    MoFAAgent (Unified Core Interface)                │
//! │  • id(), name(), capabilities()                                     │
//! │  • initialize(), execute(), shutdown()                              │
//! │  • state()                                                          │
//! └─────────────────────────────────────────────────────────────────────┘
//!                               │
//!         ┌─────────────────────┼─────────────────────┐
//!         ▼                     ▼                     ▼
//! ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
//! │AgentLifecycle│    │AgentMessaging│    │AgentPlugin   │
//! │  (可选扩展)   │    │  (可选扩展)   │    │  (可选扩展)   │
//! │ (Opt Extension)│ │ (Opt Extension)││ (Opt Extension)│
//! │• pause()     │    │• handle_     │    │   Support    │
//! │• resume()    │    │  message()   │    │              │
//! │              │    │• handle_     │    │• register_   │
//! │              │    │  event()     │    │  plugin()    │
//! └──────────────┘    └──────────────┘    │• unregister  │
//!                                         │  _plugin()   │
//!                                         └──────────────┘
//! ```

pub use crate::agent::context::AgentEvent;
use crate::agent::{
    AgentCapabilities, AgentContext,
    error::AgentResult,
    types::{AgentInput, AgentOutput, AgentState, InterruptResult},
};
use async_trait::async_trait;

// ============================================================================
// MoFAAgent - 统一核心接口
// MoFAAgent - Unified Core Interface
// ============================================================================

/// MoFA Agent 统一接口
/// MoFA Agent Unified Interface
///
/// 这是 MoFA 框架中所有 Agent 必须实现的统一接口。
/// This is the unified interface that all Agents in the MoFA framework must implement.
/// 整合了之前的 AgentCore 和 MoFAAgent 功能，提供一致的抽象。
/// Integrates previous AgentCore and MoFAAgent functions into a consistent abstraction.
///
/// # 设计原则
/// # Design Principles
///
/// 1. **统一接口**：所有 Agent 实现同一个 trait
/// 1. **Unified Interface**: All Agents implement the same trait
/// 2. **最小化核心**：只包含最基本的方法
/// 2. **Minimal Core**: Contains only the most essential methods
/// 3. **可选扩展**：通过扩展 trait 提供额外功能
/// 3. **Optional Extensions**: Provide additional features via extension traits
/// 4. **一致性**：统一接口与扩展约定
/// 4. **Consistency**: Consistent interface and extension conventions
///
/// # 必须实现的方法
/// # Required Methods
///
/// - `id()` - 唯一标识符
/// - `id()` - Unique identifier
/// - `name()` - 人类可读名称
/// - `name()` - Human-readable name
/// - `capabilities()` - 能力描述
/// - `capabilities()` - Capability descriptions
/// - `initialize()` - 初始化
/// - `initialize()` - Initialization
/// - `execute()` - 执行任务（核心方法）
/// - `execute()` - Task execution (core method)
/// - `shutdown()` - 关闭
/// - `shutdown()` - Shutdown
/// - `state()` - 获取状态
/// - `state()` - State query
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// use mofa_kernel::agent::core::MoFAAgent;
/// use mofa_kernel::agent::prelude::*;
///
/// struct MyAgent {
///     id: String,
///     name: String,
///     capabilities: AgentCapabilities,
///     state: AgentState,
/// }
///
/// #[async_trait]
/// impl MoFAAgent for MyAgent {
///     fn id(&self) -> &str { &self.id }
///     fn name(&self) -> &str { &self.name }
///     fn capabilities(&self) -> &AgentCapabilities { &self.capabilities }
///     fn state(&self) -> AgentState { self.state.clone() }
///
///     async fn initialize(&mut self, _ctx: &CoreAgentContext) -> AgentResult<()> {
///         self.state = AgentState::Ready;
///         Ok(())
///     }
///
///     async fn execute(&mut self, input: AgentInput, _ctx: &CoreAgentContext) -> AgentResult<AgentOutput> {
///         // 处理输入并返回输出
///         Ok(AgentOutput::text("Response"))
///     }
///
///     async fn shutdown(&mut self) -> AgentResult<()> {
///         self.state = AgentState::Shutdown;
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait MoFAAgent: Send + Sync + 'static {
    // ========================================================================
    // 标识和元数据
    // Identification and Metadata
    // ========================================================================

    /// 获取唯一标识符
    /// Get unique identifier
    ///
    /// ID 应该在 Agent 的整个生命周期内保持不变。
    /// The ID should remain constant throughout the Agent's entire lifecycle.
    /// 用于在注册中心、日志、监控系统中唯一标识 Agent。
    /// Used to uniquely identify the Agent in registries, logs, and monitoring systems.
    fn id(&self) -> &str;

    /// 获取人类可读名称
    /// Get human-readable name
    ///
    /// 名称用于显示和日志记录，不需要唯一。
    /// Names are used for display and logging and do not need to be unique.
    fn name(&self) -> &str;

    /// 获取能力描述
    /// Get capability description
    ///
    /// 返回 Agent 支持的能力，用于：
    /// Returns the capabilities supported by the Agent, used for:
    /// - Agent 发现和路由
    /// - Agent discovery and routing
    /// - 能力匹配和验证
    /// - Capability matching and verification
    /// - 多智能体协调
    /// - Multi-agent coordination
    fn capabilities(&self) -> &AgentCapabilities;

    // ========================================================================
    // 核心生命周期
    // Core Lifecycle
    // ========================================================================

    /// 初始化 Agent
    /// Initialize Agent
    ///
    /// 在执行任务前调用，用于：
    /// Called before task execution, used for:
    /// - 加载资源和配置
    /// - Loading resources and configurations
    /// - 建立连接（数据库、网络等）
    /// - Establishing connections (DB, network, etc.)
    /// - 初始化内部状态
    /// - Initializing internal state
    ///
    /// # 参数
    /// # Parameters
    ///
    /// - `ctx`: 执行上下文，提供配置、事件总线等
    /// - `ctx`: Execution context, providing config, event bus, etc.
    ///
    /// # 状态转换
    /// # State Transitions
    ///
    /// Created -> Initializing -> Ready
    async fn initialize(&mut self, ctx: &AgentContext) -> AgentResult<()>;

    /// 执行任务 - 核心方法
    /// Execute Task - Core Method
    ///
    /// 这是 Agent 的主要执行方法，处理输入并返回输出。
    /// This is the primary execution method, processing input and returning output.
    ///
    /// # 参数
    /// # Parameters
    ///
    /// - `input`: 输入数据，支持多种格式（文本、JSON、二进制等）
    /// - `input`: Input data supporting multiple formats (text, JSON, binary, etc.)
    /// - `ctx`: 执行上下文，提供状态、事件、中断等
    /// - `ctx`: Execution context providing state, events, interrupts, etc.
    ///
    /// # 返回
    /// # Returns
    ///
    /// 返回执行结果，包含输出内容、元数据、工具使用等。
    /// Returns execution result, including output, metadata, tool usage, etc.
    ///
    /// # 状态转换
    /// # State Transitions
    ///
    /// Ready -> Executing -> Ready
    async fn execute(&self, input: AgentInput, ctx: &AgentContext) -> AgentResult<AgentOutput>;

    /// 关闭 Agent
    /// Shutdown Agent
    ///
    /// 优雅关闭，释放资源：
    /// Graceful shutdown, releasing resources:
    /// - 保存状态
    /// - Save state
    /// - 关闭连接
    /// - Close connections
    /// - 清理资源
    /// - Clean up resources
    ///
    /// # 状态转换
    /// # State Transitions
    ///
    /// * -> ShuttingDown -> Shutdown
    async fn shutdown(&mut self) -> AgentResult<()>;

    /// 中断 Agent
    /// Interrupt Agent
    ///
    /// 发送中断信号，Agent 可以选择如何响应：
    /// Send interrupt signal; the Agent can choose how to respond:
    /// - 立即停止
    /// - Stop immediately
    /// - 完成当前步骤后停止
    /// - Stop after finishing the current step
    /// - 忽略中断
    /// - Ignore the interrupt
    ///
    /// # 返回
    /// # Returns
    ///
    /// 返回中断处理结果。
    /// Returns the result of the interrupt handling.
    ///
    /// # 默认实现
    /// # Default Implementation
    ///
    /// 默认返回 `InterruptResult::Acknowledged`。
    /// Returns `InterruptResult::Acknowledged` by default.
    ///
    /// # 状态转换
    /// # State Transitions
    ///
    /// Executing -> Interrupted
    async fn interrupt(&mut self) -> AgentResult<InterruptResult> {
        Ok(InterruptResult::Acknowledged)
    }

    // ========================================================================
    // 状态查询
    // State Query
    // ========================================================================

    /// 获取当前状态
    /// Get current state
    ///
    /// 返回 Agent 的当前状态，用于：
    /// Returns the current state of the Agent, used for:
    /// - 健康检查
    /// - Health checks
    /// - 状态监控
    /// - Status monitoring
    /// - 状态转换验证
    /// - State transition verification
    fn state(&self) -> AgentState;
}

// ============================================================================
// AgentLifecycle - 生命周期扩展
// AgentLifecycle - Lifecycle Extension
// ============================================================================

/// Agent 生命周期扩展
/// Agent Lifecycle Extension
///
/// 提供额外的生命周期控制方法，用于需要更细粒度控制的场景。
/// Provides extra lifecycle control methods for scenarios needing finer granularity.
/// 这个 trait 是可选的，只有需要这些功能的 Agent 才需要实现。
/// This trait is optional; only Agents requiring these features need to implement it.
///
/// # 提供的方法
/// # Provided Methods
///
/// - `pause()` - 暂停执行
/// - `pause()` - Pause execution
/// - `resume()` - 恢复执行
/// - `resume()` - Resume execution
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// use mofa_kernel::agent::core::AgentLifecycle;
///
/// #[async_trait]
/// impl AgentLifecycle for MyAgent {
///     async fn pause(&mut self) -> AgentResult<()> {
///         self.state = AgentState::Paused;
///         Ok(())
///     }
///
///     async fn resume(&mut self) -> AgentResult<()> {
///         self.state = AgentState::Ready;
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait AgentLifecycle: MoFAAgent {
    /// 暂停 Agent
    /// Pause Agent
    ///
    /// 暂停当前执行，保持状态以便后续恢复。
    /// Pause current execution, preserving state for later resumption.
    ///
    /// # 状态转换
    /// # State Transitions
    ///
    /// Executing -> Paused
    async fn pause(&mut self) -> AgentResult<()>;

    /// 恢复 Agent
    /// Resume Agent
    ///
    /// 从暂停状态恢复执行。
    /// Resume execution from a paused state.
    ///
    /// # 状态转换
    /// # State Transitions
    ///
    /// Paused -> Ready
    async fn resume(&mut self) -> AgentResult<()>;
}

// ============================================================================
// AgentMessaging - 消息处理扩展
// AgentMessaging - Messaging Extension
// ============================================================================

/// Agent 消息处理扩展
/// Agent Messaging Extension
///
/// 提供消息和事件处理能力，用于需要与其他 Agent 或系统交互的场景。
/// Provides messaging and event handling for interactions with other Agents or systems.
/// 这个 trait 是可选的，只有需要消息处理的 Agent 才需要实现。
/// This trait is optional; only Agents requiring message handling need to implement it.
///
/// # 提供的方法
/// # Provided Methods
///
/// - `handle_message()` - 处理消息
/// - `handle_message()` - Handle message
/// - `handle_event()` - 处理事件
/// - `handle_event()` - Handle event
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// use mofa_kernel::agent::core::AgentMessaging;
///
/// #[async_trait]
/// impl AgentMessaging for MyAgent {
///     async fn handle_message(&mut self, msg: AgentMessage) -> AgentResult<AgentMessage> {
///         // 处理消息并返回响应
///         Ok(AgentMessage::new("response"))
///     }
///
///     async fn handle_event(&mut self, event: AgentEvent) -> AgentResult<()> {
///         // 处理事件
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait AgentMessaging: MoFAAgent {
    /// 处理消息
    /// Handle message
    ///
    /// 接收来自其他 Agent 或系统的消息，处理后返回响应。
    /// Receives messages from other Agents or systems, processing and returning responses.
    ///
    /// # 参数
    /// # Parameters
    ///
    /// - `msg`: 接收到的消息
    /// - `msg`: Received message
    ///
    /// # 返回
    /// # Returns
    ///
    /// 返回响应消息
    /// Returns response message
    async fn handle_message(&mut self, msg: AgentMessage) -> AgentResult<AgentMessage>;

    /// 处理事件
    /// Handle event
    ///
    /// 处理来自事件总线的事件，用于响应系统级别的通知。
    /// Processes events from the event bus to respond to system-level notifications.
    ///
    /// # 参数
    /// # Parameters
    ///
    /// - `event`: 事件对象
    /// - `event`: Event object
    async fn handle_event(&mut self, event: AgentEvent) -> AgentResult<()>;
}

// ============================================================================
// AgentPluginSupport - 插件支持扩展
// AgentPluginSupport - Plugin Support Extension
// ============================================================================

/// Agent 插件支持扩展
/// Agent Plugin Support Extension
///
/// 提供插件管理能力，允许在运行时动态扩展 Agent 功能。
/// Provides plugin management, allowing dynamic Agent extension at runtime.
/// 这个 trait 是可选的，只有需要插件系统的 Agent 才需要实现。
/// This trait is optional; only Agents requiring a plugin system need to implement it.
///
/// # 提供的方法
/// # Provided Methods
///
/// - `register_plugin()` - 注册插件
/// - `register_plugin()` - Register plugin
/// - `unregister_plugin()` - 注销插件
/// - `unregister_plugin()` - Unregister plugin
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// use mofa_kernel::agent::core::AgentPluginSupport;
///
/// #[async_trait]
/// impl AgentPluginSupport for MyAgent {
///     fn register_plugin(&mut self, plugin: Box<dyn AgentPlugin>) -> AgentResult<()> {
///         self.plugins.push(plugin);
///         Ok(())
///     }
///
///     fn unregister_plugin(&mut self, plugin_id: &str) -> AgentResult<()> {
///         self.plugins.retain(|p| p.id() != plugin_id);
///         Ok(())
///     }
/// }
/// ```
pub trait AgentPluginSupport: MoFAAgent {
    /// 注册插件
    /// Register plugin
    ///
    /// 向 Agent 注册一个新插件，扩展其功能。
    /// Registers a new plugin with the Agent to extend its functionality.
    ///
    /// # 参数
    /// # Parameters
    ///
    /// - `plugin`: 插件对象，实现 AgentPlugin trait
    /// - `plugin`: Plugin object, implementing the AgentPlugin trait
    ///
    /// # 返回
    /// # Returns
    ///
    /// 返回错误如果插件已存在或注册失败
    /// Returns error if plugin already exists or registration fails
    fn register_plugin(&mut self, plugin: Box<dyn crate::plugin::AgentPlugin>) -> AgentResult<()>;

    /// 注销插件
    /// Unregister plugin
    ///
    /// 从 Agent 中移除一个插件。
    /// Removes a plugin from the Agent.
    ///
    /// # 参数
    /// # Parameters
    ///
    /// - `plugin_id`: 要移除的插件 ID
    /// - `plugin_id`: The ID of the plugin to remove
    ///
    /// # 返回
    /// # Returns
    ///
    /// 返回错误如果插件不存在
    /// Returns error if the plugin does not exist
    fn unregister_plugin(&mut self, plugin_id: &str) -> AgentResult<()>;
}

// ============================================================================
// 辅助类型
// Auxiliary Types
// ============================================================================

/// Agent 消息
/// Agent Message
///
/// 用于 Agent 之间的通信
/// Used for communication between Agents
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentMessage {
    /// 消息类型
    /// Message type
    #[serde(rename = "type")]
    pub msg_type: String,

    /// 消息内容
    /// Message content
    pub content: serde_json::Value,

    /// 发送者 ID
    /// Sender ID
    pub sender_id: String,

    /// 接收者 ID
    /// Recipient ID
    pub recipient_id: String,

    /// 时间戳
    /// Timestamp
    pub timestamp: i64,

    /// 消息 ID
    /// Message ID
    pub id: String,
}

impl AgentMessage {
    /// 创建新消息
    /// Create new message
    pub fn new(msg_type: impl Into<String>) -> Self {
        let timestamp = crate::utils::now_ms() as i64;

        Self {
            msg_type: msg_type.into(),
            content: serde_json::json!({}),
            sender_id: String::new(),
            recipient_id: String::new(),
            timestamp,
            id: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// 设置内容
    /// Set content
    pub fn with_content(mut self, content: serde_json::Value) -> Self {
        self.content = content;
        self
    }

    /// 设置发送者
    /// Set sender
    pub fn with_sender(mut self, sender_id: impl Into<String>) -> Self {
        self.sender_id = sender_id.into();
        self
    }

    /// 设置接收者
    /// Set recipient
    pub fn with_recipient(mut self, recipient_id: impl Into<String>) -> Self {
        self.recipient_id = recipient_id.into();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentCapabilities, AgentContext, AgentInput, AgentOutput, AgentState};

    struct MinimalAgent {
        caps: AgentCapabilities,
    }

    impl MinimalAgent {
        fn new() -> Self {
            Self {
                caps: AgentCapabilities::default(),
            }
        }
    }

    #[async_trait]
    impl MoFAAgent for MinimalAgent {
        fn id(&self) -> &str {
            "minimal"
        }

        fn name(&self) -> &str {
            "Minimal"
        }

        fn capabilities(&self) -> &AgentCapabilities {
            &self.caps
        }

        async fn initialize(&mut self, _ctx: &AgentContext) -> AgentResult<()> {
            Ok(())
        }

        async fn execute(
            &mut self,
            _input: AgentInput,
            _ctx: &AgentContext,
        ) -> AgentResult<AgentOutput> {
            Ok(AgentOutput::text("ok"))
        }

        async fn shutdown(&mut self) -> AgentResult<()> {
            Ok(())
        }

        fn state(&self) -> AgentState {
            AgentState::Ready
        }
    }

    #[tokio::test]
    async fn default_interrupt_returns_acknowledged() {
        let mut agent = MinimalAgent::new();
        let res = agent.interrupt().await.expect("interrupt should succeed");
        assert!(matches!(res, InterruptResult::Acknowledged));
    }

    #[test]
    fn agent_message_builder_sets_fields() {
        let msg = AgentMessage::new("event")
            .with_content(serde_json::json!({"k":"v"}))
            .with_sender("s1")
            .with_recipient("r1");

        assert_eq!(msg.msg_type, "event");
        assert_eq!(msg.content, serde_json::json!({"k":"v"}));
        assert_eq!(msg.sender_id, "s1");
        assert_eq!(msg.recipient_id, "r1");
        assert!(!msg.id.is_empty());
    }
}
