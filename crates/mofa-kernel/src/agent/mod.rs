//! 统一 Agent 框架
//! Unified Agent Framework
//!
//! 提供模块化、可组合、可扩展的 Agent 架构
//! Provides a modular, composable, and extensible Agent architecture
//!
//! # 架构概述
//! # Architecture Overview
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                 统一 Agent 框架                                     │
//! │             Unified Agent Framework                                 │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │  ┌─────────────────────────────────────────────────────────────┐    │
//! │  │                     MoFAAgent Trait                         │    │
//! │  │  (统一 Agent 接口：id, capabilities, execute, interrupt)      │    │
//! │  │  (Unified Agent Interface: id, capabilities, execute, etc.) │    │
//! │  └───────────────────────────┬─────────────────────────────────┘    │
//! │                              │                                      │
//! │  ┌───────────────────────────┼───────────────────────────────────┐  │
//! │  │          Modular Components (组件化设计)                      │  │
//! │  │          Modular Components (Component-based Design)          │  │
//! │  │  ┌──────────┐  ┌──────────┐  ┌────────┐  ┌──────────────┐    │  │
//! │  │  │ Reasoner │  │  Tool    │  │ Memory │  │  Coordinator │    │  │
//! │  │  │   推理器   │  │   工具  │  │  记忆   │ │    协调器     │    │  │
//! │  │  │  Reasoner  │  │   Tool │  │ Memory │  │  Coordinator │    │  │
//! │  │  └──────────┘  └──────────┘  └────────┘  └──────────────┘    │  │
//! │  └───────────────────────────────────────────────────────────────┘  │
//! │                                                                     │
//! │  ┌─────────────────────────────────────────────────────────────┐    │
//! │  │        AgentRegistry (runtime 注册中心实现)                   │    │
//! │  │        AgentRegistry (Runtime Registry Implementation)       │    │
//! │  └─────────────────────────────────────────────────────────────┘    │
//! │                                                                     │
//! │  ┌─────────────────────────────────────────────────────────────┐    │
//! │  │               CoreAgentContext (统一上下文)                  │    │
//! │  │               CoreAgentContext (Unified Context)             │    │
//! │  └─────────────────────────────────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # 核心概念
//! # Core Concepts
//!
//! ## MoFAAgent Trait
//!
//! 所有 Agent 实现的统一接口：
//! Unified interface for all Agent implementations:
//!
//! ```rust,ignore
//! use mofa_kernel::agent::prelude::*;
//!
//! #[async_trait]
//! impl MoFAAgent for MyAgent {
//!     fn id(&self) -> &str { "my-agent" }
//!     fn name(&self) -> &str { "My Agent" }
//!     fn capabilities(&self) -> &AgentCapabilities { &self.caps }
//!
//!     async fn initialize(&mut self, ctx: &CoreAgentContext) -> AgentResult<()> {
//!         Ok(())
//!     }
//!
//!     async fn execute(&mut self, input: AgentInput, ctx: &CoreAgentContext) -> AgentResult<AgentOutput> {
//!         Ok(AgentOutput::text("Hello!"))
//!     }
//!
//!     async fn interrupt(&mut self) -> AgentResult<InterruptResult> {
//!         Ok(InterruptResult::Acknowledged)
//!     }
//!
//!     async fn shutdown(&mut self) -> AgentResult<()> {
//!         Ok(())
//!     }
//!
//!     fn state(&self) -> AgentState {
//!         AgentState::Ready
//!     }
//! }
//! ```
//!
//! ## AgentCapabilities
//!
//! 描述 Agent 的能力，用于发现和路由：
//! Describes Agent capabilities, used for discovery and routing:
//!
//! ```rust,ignore
//! let caps = AgentCapabilities::builder()
//!     .tag("llm")
//!     .tag("coding")
//!     .input_type(InputType::Text)
//!     .output_type(OutputType::Text)
//!     .supports_streaming(true)
//!     .supports_tools(true)
//!     .build();
//! ```
//!
//! ## CoreAgentContext
//!
//! 执行上下文，在 Agent 执行过程中传递状态：
//! Execution context, passing states during Agent execution:
//!
//! ```rust,ignore
//! let ctx = CoreAgentContext::new("execution-123");
//! ctx.set("user_id", "user-456").await;
//! ctx.emit_event(AgentEvent::new("task_started", json!({}))).await;
//! ```
//!
//! # 模块结构
//! # Module Structure
//!
//! - `core` - AgentCore 微内核接口（最小化核心）
//! - `core` - AgentCore micro-kernel interface (minimal core)
//! - `traits` - MoFAAgent trait 定义
//! - `traits` - MoFAAgent trait definition
//! - `types` - AgentInput, AgentOutput, AgentState 等类型
//! - `types` - Types like AgentInput, AgentOutput, AgentState, etc.
//! - `capabilities` - AgentCapabilities 能力描述
//! - `capabilities` - AgentCapabilities capability description
//! - `context` - CoreAgentContext 执行上下文
//! - `context` - CoreAgentContext execution context
//! - `error` - 错误类型定义
//! - `error` - Error type definitions
//! - `components` - 组件 trait (Reasoner, Tool, Memory, Coordinator)
//! - `components` - Component traits (Reasoner, Tool, Memory, Coordinator)
//! - `config` - 配置系统
//! - `config` - Configuration system
//! - `registry` - Agent 注册中心
//! - `registry` - Agent registry
//! - `tools` - 统一工具系统
//! - `tools` - Unified tool system

// 核心模块
// Core modules
pub mod capabilities;
pub mod context;
pub mod core;
pub mod error;
pub mod traits;
pub mod types;

// 组件模块
// Component modules
pub mod components;

// 协调模块
// Coordination module
pub mod coordination;

// 配置模块
// Configuration modules
pub mod config;

// 注册中心
// Registry
pub mod registry;

// Agent capability manifest
pub mod manifest;

// 工具系统
// Tool system

// 执行引擎与运行器已迁移到 mofa-runtime
// Execution engine and runner have been migrated to mofa-runtime

// 秘书Agent抽象
// Secretary Agent abstraction
pub mod plugins;
pub mod secretary;

// AgentPlugin 统一到 plugin 模块
// AgentPlugin unified into the plugin module
pub use crate::plugin::AgentPlugin;
// 重新导出核心类型
// Re-export core types
pub use capabilities::{
    AgentCapabilities, AgentCapabilitiesBuilder, AgentRequirements, AgentRequirementsBuilder,
    ReasoningStrategy,
};
pub use context::{AgentContext, AgentEvent, ContextConfig, EventBus};
pub use core::{
    // MoFAAgent - 统一的 Agent 接口
    // MoFAAgent - Unified Agent interface
    AgentLifecycle,
    AgentMessage,
    AgentMessaging,
    AgentPluginSupport,
    MoFAAgent,
};
pub use coordination::{
    AgentMemory, ConflictDetector, ConflictInfo, CoordinationError, CoordinationGovernor,
    CoordinationResult, GovernanceConfig, HandoffContext, HandoffPacket, HandoffProtocol,
    MemoryObject, MemoryRef, ResolutionStrategy,
};
pub use error::{AgentError, AgentReport, AgentResult, IntoAgentReport};
pub use traits::{AgentMetadata, AgentStats, DynAgent, HealthStatus};
pub use types::event::execution as execution_events;
// Event type constants are available via types::event::lifecycle, types::event::execution, etc.
// Note: Aliased to avoid conflict with existing modules (plugins, etc.)
pub use types::event::lifecycle;
pub use types::event::message as message_events;
pub use types::event::plugin as plugin_events;
pub use types::event::state as state_events;
pub use types::{
    AgentInput,
    AgentOutput,
    AgentState,
    // LLM types
    ChatCompletionRequest,
    ChatCompletionResponse,
    ChatMessage,
    ErrorCategory,
    ErrorContext,
    EventBuilder,
    GlobalError,
    GlobalEvent,
    GlobalMessage,
    GlobalReport,
    GlobalResult,
    InputType,
    InterruptResult,
    IntoGlobalReport,
    LLMProvider,
    MessageContent,
    MessageMetadata,
    OutputContent,
    // Global types
    OutputType,
    ReasoningStep,
    ReasoningStepType,
    TokenUsage,
    ToolCall,
    ToolDefinition,
    ToolUsage,
};

// 重新导出组件
// Re-export components
pub use components::{
    context_compressor::{CompressionStrategy, ContextCompressor},
    coordinator::{CoordinationPattern, Coordinator},
    mcp::{McpClient, McpServerConfig, McpServerInfo, McpToolInfo, McpTransportConfig},
    memory::{Embedder, Memory, MemoryItem, MemoryStats, MemoryValue, Message, MessageRole},
    reasoner::{Reasoner, ReasoningResult},
    tool::{Tool, ToolDescriptor, ToolInput, ToolMetadata, ToolResult},
};

// 重新导出工厂接口
// Re-export factory interface
pub use registry::AgentFactory;

// Re-export manifest types
pub use manifest::{AgentManifest, AgentManifestBuilder};

// 重新导出配置
// Re-export configuration
pub use config::{AgentConfig, AgentType};
#[cfg(feature = "config")]
pub use config::{ConfigFormat, ConfigLoader};

/// Prelude 模块 - 常用类型导入
/// Prelude module - Common type imports
pub mod prelude {
    pub use super::capabilities::{
        AgentCapabilities, AgentCapabilitiesBuilder, AgentRequirements, ReasoningStrategy,
    };
    pub use super::context::{AgentContext, AgentEvent, ContextConfig};
    pub use super::core::{
        // MoFAAgent - 统一的 Agent 接口
        // MoFAAgent - Unified Agent interface
        AgentLifecycle,
        AgentMessage,
        AgentMessaging,
        AgentPluginSupport,
        MoFAAgent,
    };
    pub use super::error::{AgentError, AgentResult};
    pub use super::traits::{AgentMetadata, DynAgent, HealthStatus};
    pub use super::types::{
        AgentInput,
        AgentOutput,
        AgentState,
        // LLM types
        ChatCompletionRequest,
        ChatMessage,
        InputType,
        InterruptResult,
        LLMProvider,
        OutputType,
        TokenUsage,
        ToolUsage,
    };
    // AgentPlugin 统一到 plugin 模块
    // AgentPlugin unified into the plugin module
    pub use crate::plugin::AgentPlugin;
    pub use async_trait::async_trait;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MinimalAgent {
        caps: AgentCapabilities,
        state: AgentState,
    }

    impl MinimalAgent {
        fn new() -> Self {
            Self {
                caps: AgentCapabilitiesBuilder::new().build(),
                state: AgentState::Created,
            }
        }
    }

    #[async_trait::async_trait]
    impl MoFAAgent for MinimalAgent {
        fn id(&self) -> &str {
            "minimal-agent"
        }

        fn name(&self) -> &str {
            "Minimal Agent"
        }

        fn capabilities(&self) -> &AgentCapabilities {
            &self.caps
        }

        async fn initialize(&mut self, _ctx: &AgentContext) -> AgentResult<()> {
            self.state = AgentState::Ready;
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
            self.state = AgentState::Shutdown;
            Ok(())
        }

        fn state(&self) -> AgentState {
            self.state.clone()
        }
    }

    struct FailingAgent {
        caps: AgentCapabilities,
    }

    impl FailingAgent {
        fn new() -> Self {
            Self {
                caps: AgentCapabilitiesBuilder::new().build(),
            }
        }
    }

    #[async_trait::async_trait]
    impl MoFAAgent for FailingAgent {
        fn id(&self) -> &str {
            "failing-agent"
        }

        fn name(&self) -> &str {
            "Failing Agent"
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
            Err(AgentError::ExecutionFailed("failure".to_string()))
        }

        async fn shutdown(&mut self) -> AgentResult<()> {
            Ok(())
        }

        fn state(&self) -> AgentState {
            AgentState::Ready
        }
    }

    #[tokio::test]
    async fn minimal_agent_executes_successfully() {
        let mut agent = MinimalAgent::new();
        let ctx = AgentContext::new("exec-1");
        agent.initialize(&ctx).await.unwrap();
        let out = agent
            .execute(AgentInput::text("hello"), &ctx)
            .await
            .unwrap();
        assert_eq!(out.to_text(), "ok");
    }

    #[tokio::test]
    async fn failing_agent_returns_execution_error() {
        let mut agent = FailingAgent::new();
        let ctx = AgentContext::new("exec-2");
        let result = agent.execute(AgentInput::text("x"), &ctx).await;
        assert!(matches!(result, Err(AgentError::ExecutionFailed(_))));
    }
}
