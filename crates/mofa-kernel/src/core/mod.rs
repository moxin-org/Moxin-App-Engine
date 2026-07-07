//! 核心类型定义
//! Core type definitions
//!
//! 此模块包含 Agent 相关的核心配置和元数据类型。
//! This module contains core configuration and metadata types related to the Agent.
pub mod interrupt;
pub mod error;
pub use crate::agent::{
    AgentContext, AgentEvent, AgentInput, AgentOutput, AgentState, types::InterruptResult,
};
pub use error::MofaError;

/// AgentConfig - Agent 配置
/// AgentConfig - Agent Configuration
///
/// 定义 Agent 的基本配置信息。
/// Defines the basic configuration information of the Agent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct AgentConfig {
    pub agent_id: String,
    pub name: String,
    pub node_config: std::collections::HashMap<String, String>,
}

impl AgentConfig {
    pub fn new(agent_id: &str, name: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            name: name.to_string(),
            node_config: std::collections::HashMap::new(),
        }
    }
}
