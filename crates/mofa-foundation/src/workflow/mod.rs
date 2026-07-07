//! Graph-based Workflow Orchestration
//!
//! 提供基于有向图的工作流编排系统，支持：
//! Provides a workflow orchestration system based on directed graphs, supporting:
//! - 多种节点类型（任务、条件、并行、聚合、循环）
//! - Multiple node types (task, condition, parallel, join, loop)
//! - DAG 拓扑排序执行
//! - DAG topological sort execution
//! - 并行执行与同步
//! - Parallel execution and synchronization
//! - 状态管理与数据传递
//! - State management and data transfer
//! - 错误处理与重试
//! - Error handling and retries
//! - 检查点与恢复
//! - Checkpoints and recovery
//! - DSL (YAML/TOML) 配置支持
//! - DSL (YAML/TOML) configuration support
//! - Time-Travel Debugger telemetry and session recording
//!
//! # StateGraph API (LangGraph-inspired)
//!
//! 新的 StateGraph API 提供了更直观的工作流构建方式：
//! The new StateGraph API provides a more intuitive way to build workflows:
//!
//! ```rust,ignore
//! use mofa_foundation::workflow::{StateGraphImpl, AppendReducer, OverwriteReducer};
//! use mofa_kernel::workflow::{StateGraph, START, END};
//!
//! let graph = StateGraphImpl::<MyState>::new("my_workflow")
//!     .add_reducer("messages", Box::new(AppendReducer))
//!     .add_node("process", Box::new(ProcessNode))
//!     .add_edge(START, "process")
//!     .add_edge("process", END)
//!     .compile()?;
//!
//! let result = graph.invoke(initial_state, None).await?;
//! ```

mod builder;
mod execution_event;
mod executor;
mod fault_tolerance;
mod graph;
mod node;
mod profiler;
mod reducers;
pub mod session_recorder;
mod state;
mod state_graph;
pub mod telemetry;

pub mod dsl;
pub mod versioning;

// Re-export kernel workflow types for convenience
pub use mofa_kernel::workflow::{
    Command, CompiledGraph, ControlFlow, END, GraphConfig, GraphState, JsonState, NodeFunc,
    Reducer, ReducerType, RemainingSteps, RuntimeContext, START, SendCommand, StateSchema,
    StateUpdate,
};

// Re-export kernel telemetry types
pub use mofa_kernel::workflow::telemetry::{
    DebugEvent, DebugSession, SessionRecorder, TelemetryEmitter,
};

// Re-export kernel StateGraph trait
pub use mofa_kernel::workflow::StateGraph;

// Foundation-specific exports
pub use builder::*;
pub use dsl::*;
pub use execution_event::{ExecutionEvent, ExecutionEventEnvelope, SCHEMA_VERSION};
pub use executor::*;
pub use graph::*;
pub use mofa_kernel::workflow::policy::NodePolicy;
pub use node::*;
pub use profiler::*;
pub use reducers::*;
pub use session_recorder::{
    FileSessionRecorder, FileSessionRecorderConfig, InMemorySessionRecorder,
};
pub use state::*;
pub use state_graph::{CompiledGraphImpl, StateGraphImpl};
pub use telemetry::{ChannelTelemetryEmitter, RecordingTelemetryEmitter};
pub use versioning::*;
