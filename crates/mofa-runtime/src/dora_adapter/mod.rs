//! dora-rs 适配层
//! dora-rs Adaptation Layer
//!
//! 该模块提供 MoFA 框架与 dora-rs 的集成适配，包括：
//! This module provides the integration adaptation between MoFA and dora-rs, including:
//! - DoraNode 封装：智能体生命周期管理
//! - DoraNode Wrapper: Agent lifecycle management
//! - DoraOperator 封装：插件能力抽象
//! - DoraOperator Wrapper: Plugin capability abstraction
//! - DoraDataflow 封装：多智能体协同数据流
//! - DoraDataflow Wrapper: Multi-agent collaborative dataflow
//! - DoraChannel 封装：跨智能体通信通道
//! - DoraChannel Wrapper: Cross-agent communication channels
//! - DoraRuntime 封装：完整运行时支持（嵌入式/分布式）
//! - DoraRuntime Wrapper: Full runtime support (Embedded/Distributed)

pub mod channel;
mod dataflow;
mod error;
mod node;
mod operator;
pub mod runtime;

pub use channel::{ChannelConfig, ChannelManager, DoraChannel, MessageEnvelope, load_persisted_messages};
pub use dataflow::{DataflowBuilder, DataflowConfig, DataflowState, DoraDataflow, NodeConnection};
pub use error::{DoraError, DoraResult};
pub use node::{DoraAgentNode, DoraNodeConfig, NodeEventLoop};
pub use operator::{
    DoraPluginOperator, MoFAOperator, OperatorChain, OperatorConfig, OperatorInput, OperatorOutput,
    PluginOperatorAdapter,
};

// 导出运行时支持（真实 dora API）
// Export runtime support (Actual dora APIs)
pub use runtime::{
    // 运行时
    // Runtime
    DataflowResult,
    DistributedConfig,
    DoraRuntime,
    DoraRuntimeBuilder,
    EmbeddedConfig,
    LogDestination,
    LogDestinationType,
    // 配置
    // Configuration
    NodeResult,
    // 状态和结果
    // State and results
    RuntimeConfig,
    // 辅助函数
    // Helper functions
    RuntimeMode,
    RuntimeState,
    run_dataflow,
    run_dataflow_with_logs,
};
