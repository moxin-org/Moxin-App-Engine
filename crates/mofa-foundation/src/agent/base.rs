//! 基础 Agent 实现
//! Base Agent Implementation
//!
//! 提供了 MoFAAgent trait 的基础实现，可以作为其他 Agent 的基础
//! Provides a base implementation of the MoFAAgent trait, serving as a foundation for other Agents

use mofa_kernel::agent::{
    AgentCapabilities, AgentContext, AgentError, AgentOutput, AgentResult, AgentState, AgentStats,
    InterruptResult, MoFAAgent,
    AgentLifecycle, AgentMessage, AgentMessaging, AgentPluginSupport,
    context::AgentEvent,
};
use mofa_kernel::plugin::AgentPlugin;

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
    /// 已注册的插件
    /// Registered plugins
    plugins: Vec<Box<dyn AgentPlugin>>,
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
            plugins: Vec::new(),
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
        &mut self,
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

// ============================================================================
// AgentLifecycle Implementation
// ============================================================================

#[async_trait]
impl AgentLifecycle for BaseAgent {
    async fn pause(&mut self) -> AgentResult<()> {
        self.transition_to(AgentState::Paused)
    }

    async fn resume(&mut self) -> AgentResult<()> {
        self.transition_to(AgentState::Ready)
    }
}

// ============================================================================
// AgentMessaging Implementation
// ============================================================================

#[async_trait]
impl AgentMessaging for BaseAgent {
    async fn handle_message(&mut self, msg: AgentMessage) -> AgentResult<AgentMessage> {
        Ok(AgentMessage::new("response")
            .with_content(msg.content.clone())
            .with_sender(self.id.clone())
            .with_recipient(msg.sender_id.clone()))
    }

    async fn handle_event(&mut self, _event: AgentEvent) -> AgentResult<()> {
        Ok(())
    }
}

// ============================================================================
// AgentPluginSupport Implementation
// ============================================================================

impl AgentPluginSupport for BaseAgent {
    fn register_plugin(&mut self, plugin: Box<dyn AgentPlugin>) -> AgentResult<()> {
        let id = plugin.plugin_id().to_string();
        if self.plugins.iter().any(|p| p.plugin_id() == id) {
            return Err(AgentError::ValidationFailed(
                format!("Plugin '{}' is already registered", id),
            ));
        }
        self.plugins.push(plugin);
        Ok(())
    }

    fn unregister_plugin(&mut self, plugin_id: &str) -> AgentResult<()> {
        let before = self.plugins.len();
        self.plugins.retain(|p| p.plugin_id() != plugin_id);
        if self.plugins.len() == before {
            return Err(AgentError::ValidationFailed(
                format!("Plugin '{}' not found", plugin_id),
            ));
        }
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use mofa_kernel::plugin::{PluginContext, PluginMetadata, PluginResult, PluginState, PluginType};
    use std::any::Any;
    use std::collections::HashMap;

    /// Minimal test plugin for AgentPluginSupport tests.
    struct TestPlugin {
        metadata: PluginMetadata,
        state: PluginState,
    }

    impl TestPlugin {
        fn new(id: &str) -> Self {
            Self {
                metadata: PluginMetadata::new(id, id, PluginType::Custom("test".into())),
                state: PluginState::Unloaded,
            }
        }
    }

    #[async_trait]
    impl AgentPlugin for TestPlugin {
        fn metadata(&self) -> &PluginMetadata { &self.metadata }
        fn state(&self) -> PluginState { self.state.clone() }
        async fn load(&mut self, _ctx: &PluginContext) -> PluginResult<()> { Ok(()) }
        async fn init_plugin(&mut self) -> PluginResult<()> { Ok(()) }
        async fn start(&mut self) -> PluginResult<()> { Ok(()) }
        async fn stop(&mut self) -> PluginResult<()> { Ok(()) }
        async fn unload(&mut self) -> PluginResult<()> { Ok(()) }
        async fn execute(&mut self, input: String) -> PluginResult<String> { Ok(input) }
        fn as_any(&self) -> &dyn Any { self }
        fn as_any_mut(&mut self) -> &mut dyn Any { self }
        fn into_any(self: Box<Self>) -> Box<dyn Any> { self }
    }

    // -- AgentLifecycle tests --

    #[tokio::test]
    async fn test_lifecycle_pause_resume() {
        let mut agent = BaseAgent::new("lc-1", "Lifecycle Agent");
        let ctx = AgentContext::new("exec-1");
        agent.initialize(&ctx).await.unwrap(); // → Ready
        agent.transition_to(AgentState::Executing).unwrap();

        agent.pause().await.unwrap();
        assert_eq!(agent.state(), AgentState::Paused);

        agent.resume().await.unwrap();
        assert_eq!(agent.state(), AgentState::Ready);
    }

    #[tokio::test]
    async fn test_lifecycle_pause_wrong_state() {
        let mut agent = BaseAgent::new("lc-2", "Lifecycle Agent");
        let ctx = AgentContext::new("exec-2");
        agent.initialize(&ctx).await.unwrap(); // → Ready

        // Ready → Paused is not a valid transition
        let result = agent.pause().await;
        assert!(result.is_err());
    }

    // -- AgentMessaging tests --

    #[tokio::test]
    async fn test_messaging_handle_message() {
        let mut agent = BaseAgent::new("msg-1", "Messaging Agent");
        let msg = AgentMessage::new("request")
            .with_sender("other-agent")
            .with_recipient("msg-1");

        let resp = agent.handle_message(msg).await.unwrap();
        assert_eq!(resp.msg_type, "response");
        assert_eq!(resp.sender_id, "msg-1");
        assert_eq!(resp.recipient_id, "other-agent");
    }

    #[tokio::test]
    async fn test_messaging_handle_event() {
        let mut agent = BaseAgent::new("msg-2", "Messaging Agent");
        let event = AgentEvent::new("test-event", serde_json::json!("payload"));
        let result = agent.handle_event(event).await;
        assert!(result.is_ok());
    }

    // -- AgentPluginSupport tests --

    #[test]
    fn test_plugin_register_unregister() {
        let mut agent = BaseAgent::new("plug-1", "Plugin Agent");
        let plugin = Box::new(TestPlugin::new("test-plugin"));

        agent.register_plugin(plugin).unwrap();
        assert_eq!(agent.plugins.len(), 1);

        agent.unregister_plugin("test-plugin").unwrap();
        assert_eq!(agent.plugins.len(), 0);
    }

    #[test]
    fn test_plugin_register_duplicate() {
        let mut agent = BaseAgent::new("plug-2", "Plugin Agent");
        agent.register_plugin(Box::new(TestPlugin::new("dup"))).unwrap();

        let result = agent.register_plugin(Box::new(TestPlugin::new("dup")));
        assert!(result.is_err());
    }

    #[test]
    fn test_plugin_unregister_not_found() {
        let mut agent = BaseAgent::new("plug-3", "Plugin Agent");
        let result = agent.unregister_plugin("nonexistent");
        assert!(result.is_err());
    }
}
