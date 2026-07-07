//! Agent 模块
//! Agent Module
//!
//! 重新导出 mofa-kernel 的 agent 模块，并添加 runtime 特定的功能
//! Re-export mofa-kernel agent modules and add runtime-specific features

// 从 mofa-kernel 重新导出核心模块
// Re-export core modules from mofa-kernel
pub use mofa_kernel::agent::capabilities;
pub use mofa_kernel::agent::context;
pub use mofa_kernel::agent::core;
pub use mofa_kernel::agent::error;
pub use mofa_kernel::agent::traits;
pub use mofa_kernel::agent::types;

// Runtime 特定的模块
// Runtime specific modules
pub mod config;
pub mod execution;
pub mod plugins;
pub mod registry;
pub mod swarm_registry;

// 重新导出常用类型
// Re-export commonly used types
pub use crate::runner::{
    AgentRunner, AgentRunnerBuilder, CronMisfirePolicy, CronRunConfig, PeriodicMissedTickPolicy,
    PeriodicRunConfig, RunnerState, RunnerStats, run_agents,
};
pub use execution::{ExecutionEngine, ExecutionOptions, ExecutionResult, ExecutionStatus};
pub use mofa_kernel::agent::registry::AgentFactory;
pub use registry::{AgentRegistry, RegistryStats};
pub use swarm_registry::SwarmAgentRegistry;
