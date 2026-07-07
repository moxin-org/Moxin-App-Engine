//! 基础 Agent 实现
//! Base Agent Implementation
//!
//! 提供了 MoFAAgent trait 的基础实现，可以作为其他 Agent 的基础
//! Provides a base implementation of the MoFAAgent trait, serving as a foundation for other Agents

use mofa_kernel::agent::{
    AgentCapabilities, AgentContext, AgentError, AgentOutput, AgentResult, AgentState, AgentStats,
    InterruptResult, MoFAAgent,
};

use async_trait::async_trait;

/// 基础 Agent 实现
/// Base Agent Implementation
///
/// 提供 Agent 的基础功能，可以被继承或组合
/// Provides fundamental Agent functionality, suitable for inheritance or composition
pub struct BaseAgent {
    /// Agent ID
    /// Agent ID
    pub id: String,
    /// Agent 名称
    /// Agent Name
    pub name: String,
    /// Agent 描述
    /// Agent Description
    pub description: Option<String>,
    /// Agent 版本
    /// Agent Version
    pub version: Option<String>,
    /// Agent 能力
    /// Agent Capabilities
    pub capabilities: AgentCapabilities,
    /// 当前状态
    /// Current State
    pub state: AgentState,
    /// 统计信息
    /// Statistical Information
    stats: AgentStats,
}

impl BaseAgent {
    /// 创建新的基础 Agent
    /// Create a new base Agent
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            version: None,
            capabilities: AgentCapabilities::default(),
            state: AgentState::Created,
            stats: AgentStats::default(),
        }
    }

    /// 设置描述
    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 设置版本
    /// Set version
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// 设置能力
    /// Set capabilities
    pub fn with_capabilities(mut self, capabilities: AgentCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// 转换状态
    /// Transition state
    pub fn transition_to(&mut self, new_state: AgentState) -> AgentResult<()> {
        if self.state.can_transition_to(&new_state) {
            self.state = new_state;
            Ok(())
        } else {
            Err(AgentError::invalid_state_transition(
                &self.state,
                &new_state,
            ))
        }
    }

    /// 记录成功执行
    /// Record successful execution
    pub fn record_success(&mut self, duration_ms: u64, tokens: u64, tool_calls: u64) {
        self.stats.total_executions += 1;
        self.stats.successful_executions += 1;
        self.stats.total_tokens_used += tokens;
        self.stats.total_tool_calls += tool_calls;

        // 更新平均执行时间
        // Update average execution time
        let n = self.stats.total_executions as f64;
        self.stats.avg_execution_time_ms =
            (self.stats.avg_execution_time_ms * (n - 1.0) + duration_ms as f64) / n;
    }

    /// 记录失败执行
    /// Record failed execution
    pub fn record_failure(&mut self) {
        self.stats.total_executions += 1;
        self.stats.failed_executions += 1;
    }

    /// 获取统计信息
    /// Get statistical information
    pub fn stats(&self) -> &AgentStats {
        &self.stats
    }

    /// 获取 ID
    /// Get ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// 获取名称
    /// Get name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 获取能力
    /// Get capabilities
    pub fn capabilities(&self) -> &AgentCapabilities {
        &self.capabilities
    }

    /// 获取状态
    /// Get state
    pub fn state(&self) -> AgentState {
        self.state.clone()
    }

    /// 初始化
    /// Initialize
    pub async fn initialize(&mut self, _ctx: &AgentContext) -> AgentResult<()> {
        self.transition_to(AgentState::Initializing)?;
        self.transition_to(AgentState::Ready)?;
        Ok(())
    }

    /// 中断
    /// Interrupt
    pub async fn interrupt(&mut self) -> AgentResult<InterruptResult> {
        Ok(InterruptResult::Acknowledged)
    }

    /// 关闭
    /// Shutdown
    pub async fn shutdown(&mut self) -> AgentResult<()> {
        self.transition_to(AgentState::ShuttingDown)?;
        self.transition_to(AgentState::Shutdown)?;
        Ok(())
    }
}

#[async_trait]
impl MoFAAgent for BaseAgent {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> &AgentCapabilities {
        &self.capabilities
    }

    async fn initialize(
        &mut self,
        _ctx: &mofa_kernel::agent::context::AgentContext,
    ) -> AgentResult<()> {
        self.transition_to(AgentState::Initializing)?;
        self.transition_to(AgentState::Ready)?;
        Ok(())
    }

    async fn execute(
        &self,
        _input: mofa_kernel::agent::AgentInput,
        _ctx: &mofa_kernel::agent::context::AgentContext,
    ) -> AgentResult<AgentOutput> {
        Ok(AgentOutput::text("BaseAgent execute"))
    }

    async fn shutdown(&mut self) -> AgentResult<()> {
        self.transition_to(AgentState::ShuttingDown)?;
        self.transition_to(AgentState::Shutdown)?;
        Ok(())
    }

    fn state(&self) -> AgentState {
        self.state.clone()
    }
}
