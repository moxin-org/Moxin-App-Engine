//! 统一 Agent 运行器
//! Unified Agent Runner
//!
//! 提供统一的 Agent 执行接口，可以运行任何实现了 `MoFAAgent` 的 Agent。
//! Provides a unified Agent execution interface to run any Agent implementing `MoFAAgent`.
//!
//! # 架构
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                   AgentRunner<T: MoFAAgent>                         │
//! │  ┌─────────────────────────────────────────────────────────────┐    │
//! │  │  状态管理                                                    │   │
//! │  │  Status Management                                          │   │
//! │  │  • RunnerState: Initializing, Running, Paused, Stopping     │   │
//! │  └─────────────────────────────────────────────────────────────┘   │
//! │  ┌─────────────────────────────────────────────────────────────┐   │
//! │  │  执行模式                                                    │   │
//! │  │  Execution Mode                                             │   │
//! │  │  • Single: 单次执行                                          │   │
//! │  │  • Single: Single execution                                 │   │
//! │  │  • EventLoop: 事件循环（支持 AgentMessaging）                │   │
//! │  │  • EventLoop: Event loop (supports AgentMessaging)          │   │
//! │  │  • Stream: 流式执行                                          │   │
//! │  │  • Stream: Stream execution                                 │   │
//! │  └─────────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # 示例
//! # Example
//!
//! ## 基本使用
//! ## Basic usage
//!
//! ```rust,ignore
//! use mofa_runtime::runner::AgentRunner;
//! use mofa_runtime::agent::MoFAAgent;
//!
//! #[tokio::main]
//! async fn main() -> AgentResult<()> {
//!     let agent = MyAgent::new();
//!     let mut runner = AgentRunner::new(agent).await?;
//!
//!     // 执行任务
//!     // Execute task
//!     let input = AgentInput::text("Hello, Agent!");
//!     let output = runner.execute(input).await?;
//!
//!     // 关闭
//!     // Shutdown
//!     runner.shutdown().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## 事件循环模式
//! ## Event loop mode
//!
//! ```rust,ignore
//! use mofa_runtime::runner::AgentRunner;
//! use mofa_runtime::agent::{MoFAAgent, AgentMessaging};
//!
//! struct MyEventAgent { }
//!
//! #[async_trait]
//! impl MoFAAgent for MyEventAgent { /* ... */ }
//!
//! #[async_trait]
//! impl AgentMessaging for MyEventAgent { /* ... */ }
//!
//! #[tokio::main]
//! async fn main() -> AgentResult<()> {
//!     let agent = MyEventAgent::new();
//!     let mut runner = AgentRunner::new(agent).await?;
//!
//!     // 运行事件循环
//!     // Run event loop
//!     runner.run_event_loop().await?;
//!
//!     Ok(())
//! }
//! ```

use crate::agent::capabilities::AgentCapabilities;
use crate::agent::context::{AgentContext, AgentEvent};
use crate::agent::core::{AgentLifecycle, AgentMessage, AgentMessaging, MoFAAgent};
use crate::agent::error::{AgentError, AgentResult};
use crate::agent::types::{AgentInput, AgentOutput, AgentState, InterruptResult};
use chrono::{DateTime, Utc};
use cron::Schedule;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, MissedTickBehavior};
use tracing;

/// 运行器状态
/// Runner state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RunnerState {
    /// 已创建
    /// Created
    Created,
    /// 初始化中
    /// Initializing
    Initializing,
    /// 运行中
    /// Running
    Running,
    /// 暂停
    /// Paused
    Paused,
    /// 停止中
    /// Stopping
    Stopping,
    /// 已停止
    /// Stopped
    Stopped,
    /// 错误
    /// Error
    Error,
}

/// 运行器统计信息
/// Runner statistics
#[derive(Debug, Clone, Default)]
pub struct RunnerStats {
    /// 总执行次数
    /// Total execution count
    pub total_executions: u64,
    /// 成功次数
    /// Success count
    pub successful_executions: u64,
    /// 失败次数
    /// Failure count
    pub failed_executions: u64,
    /// 平均执行时间（毫秒）
    /// Average execution time (ms)
    pub avg_execution_time_ms: f64,
    /// 最后执行时间
    /// Last execution time
    pub last_execution_time_ms: Option<u64>,
}

/// 周期执行配置
#[derive(Debug, Clone)]
pub struct PeriodicRunConfig {
    /// 执行间隔
    pub interval: Duration,
    /// 最大执行次数（必须大于 0）
    pub max_runs: u64,
    /// 是否立即执行第一轮（true: 立即执行；false: 等待一个间隔后执行）
    pub run_immediately: bool,
}

impl Default for PeriodicRunConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            max_runs: 1,
            run_immediately: true,
        }
    }
}

impl PeriodicRunConfig {
    fn validate(&self) -> AgentResult<()> {
        if self.interval.is_zero() {
            return Err(AgentError::ValidationFailed(
                "Periodic interval must be greater than 0".to_string(),
            ));
        }
        if self.max_runs == 0 {
            return Err(AgentError::ValidationFailed(
                "Periodic max_runs must be greater than 0".to_string(),
            ));
        }
        Ok(())
    }
}

/// 间隔调度的补偿策略
/// Missed tick policy for interval scheduling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeriodicMissedTickPolicy {
    /// 尽快补跑遗漏 tick
    /// Try to catch up missed ticks as fast as possible
    Burst,
    /// 将后续 tick 向后平移
    /// Delay subsequent ticks to preserve spacing
    Delay,
    /// 跳过遗漏 tick（推荐）
    /// Skip missed ticks (recommended)
    #[default]
    Skip,
}

impl From<PeriodicMissedTickPolicy> for MissedTickBehavior {
    fn from(value: PeriodicMissedTickPolicy) -> Self {
        match value {
            PeriodicMissedTickPolicy::Burst => MissedTickBehavior::Burst,
            PeriodicMissedTickPolicy::Delay => MissedTickBehavior::Delay,
            PeriodicMissedTickPolicy::Skip => MissedTickBehavior::Skip,
        }
    }
}

/// Cron 调度错过触发点时的策略
/// Misfire policy for cron scheduling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CronMisfirePolicy {
    /// 丢弃错过的触发点，继续等待下一次计划触发
    /// Drop missed triggers and wait for the next scheduled trigger
    #[default]
    Skip,
    /// 如果错过至少一次触发点，立即补跑一次
    /// Execute one immediate catch-up run when at least one trigger was missed
    RunOnce,
}

/// Cron 周期执行配置
/// Cron periodic run configuration
#[derive(Debug, Clone)]
pub struct CronRunConfig {
    /// Cron 表达式（支持秒级）
    /// Cron expression (second-level supported)
    pub expression: String,
    /// 最大执行次数（必须大于 0）
    /// Maximum number of runs (must be > 0)
    pub max_runs: u64,
    /// 是否立即执行第一轮（true: 立即执行；false: 等待下一次 cron 触发）
    /// Whether to execute the first run immediately
    pub run_immediately: bool,
    /// 错过触发点时的处理策略
    /// Handling policy when cron triggers are missed
    pub misfire_policy: CronMisfirePolicy,
}

impl Default for CronRunConfig {
    fn default() -> Self {
        Self {
            expression: "*/1 * * * * * *".to_string(),
            max_runs: 1,
            run_immediately: true,
            misfire_policy: CronMisfirePolicy::Skip,
        }
    }
}

impl CronRunConfig {
    fn parse_schedule(&self) -> AgentResult<Schedule> {
        if self.expression.trim().is_empty() {
            return Err(AgentError::ValidationFailed(
                "Cron expression must not be empty".to_string(),
            ));
        }
        if self.max_runs == 0 {
            return Err(AgentError::ValidationFailed(
                "Cron max_runs must be greater than 0".to_string(),
            ));
        }

        Schedule::from_str(self.expression.trim()).map_err(|e| {
            AgentError::ValidationFailed(format!(
                "Invalid cron expression '{}': {}",
                self.expression, e
            ))
        })
    }
}

/// 统一 Agent 运行器
/// Unified Agent Runner
///
/// 可以运行任何实现了 `MoFAAgent` 的 Agent。
/// Can run any Agent that implements `MoFAAgent`.
pub struct AgentRunner<T: MoFAAgent> {
    /// Agent 实例
    /// Agent instance
    agent: T,
    /// 执行上下文
    /// Execution context
    context: AgentContext,
    /// 运行器状态
    /// Runner state
    state: Arc<RwLock<RunnerState>>,
    /// 统计信息
    /// Statistics
    stats: Arc<RwLock<RunnerStats>>,
}

impl<T: MoFAAgent> AgentRunner<T> {
    /// 创建新的运行器
    /// Create a new runner
    ///
    /// 此方法会初始化 Agent。
    /// This method will initialize the Agent.
    pub async fn new(mut agent: T) -> AgentResult<Self> {
        let context = AgentContext::new(agent.id().to_string());

        // 初始化 Agent
        // Initialize Agent
        agent
            .initialize(&context)
            .await
            .map_err(|e| AgentError::InitializationFailed(e.to_string()))?;

        Ok(Self {
            agent,
            context,
            state: Arc::new(RwLock::new(RunnerState::Created)),
            stats: Arc::new(RwLock::new(RunnerStats::default())),
        })
    }

    /// 使用自定义上下文创建运行器
    /// Create runner with custom context
    pub async fn with_context(mut agent: T, context: AgentContext) -> AgentResult<Self> {
        agent
            .initialize(&context)
            .await
            .map_err(|e| AgentError::InitializationFailed(e.to_string()))?;

        Ok(Self {
            agent,
            context,
            state: Arc::new(RwLock::new(RunnerState::Created)),
            stats: Arc::new(RwLock::new(RunnerStats::default())),
        })
    }

    /// 获取 Agent 引用
    /// Get Agent reference
    pub fn agent(&self) -> &T {
        &self.agent
    }

    /// 获取 Agent 可变引用
    /// Get mutable Agent reference
    pub fn agent_mut(&mut self) -> &mut T {
        &mut self.agent
    }

    /// 获取执行上下文
    /// Get execution context
    pub fn context(&self) -> &AgentContext {
        &self.context
    }

    /// 获取运行器状态
    /// Get runner state
    pub async fn state(&self) -> RunnerState {
        *self.state.read().await
    }

    /// 获取统计信息
    /// Get statistics
    pub async fn stats(&self) -> RunnerStats {
        self.stats.read().await.clone()
    }

    /// 检查是否正在运行
    /// Check if running
    pub async fn is_running(&self) -> bool {
        matches!(
            *self.state.read().await,
            RunnerState::Running | RunnerState::Paused
        )
    }

    /// 执行单个任务
    /// Execute a single task
    ///
    /// # 参数
    /// # Parameters
    ///
    /// - `input`: 输入数据
    /// - `input`: Input data
    ///
    /// # 返回
    /// # Returns
    ///
    /// 返回 Agent 的输出。
    /// Returns the Agent's output.
    #[tracing::instrument(
        name = "agent.execute",
        skip(self, input),
        fields(
            agent_id = %self.agent.id(),
            execution_id = %self.context.execution_id
        )
    )]
    pub async fn execute(&mut self, input: AgentInput) -> AgentResult<AgentOutput> {
        // 检查状态
        // Check state
        let current_state = self.state().await;
        if !matches!(
            current_state,
            RunnerState::Running | RunnerState::Created | RunnerState::Stopped
        ) {
            return Err(AgentError::ValidationFailed(format!(
                "Cannot execute in state: {:?}",
                current_state
            )));
        }

        // 更新状态为运行中
        // Update state to Running
        *self.state.write().await = RunnerState::Running;

        let start = std::time::Instant::now();

        // 执行 Agent
        // Execute Agent
        let result = self.agent.execute(input, &self.context).await;

        let duration = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        // 更新统计信息
        // Update statistics
        let mut stats = self.stats.write().await;
        stats.total_executions += 1;
        stats.last_execution_time_ms = Some(duration);

        match &result {
            Ok(_) => {
                stats.successful_executions += 1;
            }
            Err(_) => {
                stats.failed_executions += 1;
            }
        }

        // 更新平均执行时间
        // Update average execution time
        let n = stats.total_executions as f64;
        stats.avg_execution_time_ms =
            (stats.avg_execution_time_ms * (n - 1.0) + duration as f64) / n;

        result
    }

    /// 批量执行多个任务
    /// Batch execute multiple tasks
    ///
    /// # 参数
    /// # Parameters
    ///
    /// - `inputs`: 输入数据列表
    /// - `inputs`: List of input data
    ///
    /// # 返回
    /// # Returns
    ///
    /// 返回输出列表，如果某个任务失败，返回对应错误。
    /// Returns a list of outputs; if a task fails, returns the corresponding error.
    pub async fn execute_batch(
        &mut self,
        inputs: Vec<AgentInput>,
    ) -> Vec<AgentResult<AgentOutput>> {
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            results.push(self.execute(input).await);
        }
        results
    }

    /// 按固定间隔周期执行同一输入
    ///
    /// 默认策略使用 `PeriodicMissedTickPolicy::Skip`，避免执行落后时“补跑风暴”。
    pub async fn run_periodic(
        &mut self,
        input: AgentInput,
        config: PeriodicRunConfig,
    ) -> AgentResult<Vec<AgentOutput>> {
        self.run_periodic_with_policy(input, config, PeriodicMissedTickPolicy::Skip)
            .await
    }

    /// 按固定间隔周期执行同一输入（可配置补偿策略）
    ///
    /// 执行串行进行，不允许同一 runner 内部重叠执行。
    pub async fn run_periodic_with_policy(
        &mut self,
        input: AgentInput,
        config: PeriodicRunConfig,
        missed_tick_policy: PeriodicMissedTickPolicy,
    ) -> AgentResult<Vec<AgentOutput>> {
        config.validate()?;

        let mut outputs = Vec::with_capacity(config.max_runs as usize);
        let mut ticker = tokio::time::interval(config.interval);
        ticker.set_missed_tick_behavior(missed_tick_policy.into());

        // interval 首次 tick 立即返回。若不希望立即执行，先消费这次即时 tick。
        if !config.run_immediately {
            ticker.tick().await;
        }

        for _ in 0..config.max_runs {
            ticker.tick().await;
            outputs.push(self.execute(input.clone()).await?);
        }

        Ok(outputs)
    }

    /// 使用 Cron 表达式周期执行同一输入
    ///
    /// 与 interval 版本一致，执行串行进行，不允许同一 runner 内部重叠执行。
    pub async fn run_periodic_cron(
        &mut self,
        input: AgentInput,
        config: CronRunConfig,
    ) -> AgentResult<Vec<AgentOutput>> {
        let schedule = config.parse_schedule()?;
        let mut outputs = Vec::with_capacity(config.max_runs as usize);
        let mut remaining = config.max_runs;
        let mut upcoming = schedule.upcoming(Utc);

        if config.run_immediately {
            outputs.push(self.execute(input.clone()).await?);
            remaining -= 1;
            if remaining == 0 {
                return Ok(outputs);
            }
        }

        while remaining > 0 {
            let now = Utc::now();
            let (next_fire_at, missed_any) = next_cron_fire_at(&mut upcoming, now)?;

            if missed_any && matches!(config.misfire_policy, CronMisfirePolicy::RunOnce) {
                outputs.push(self.execute(input.clone()).await?);
                remaining -= 1;
                if remaining == 0 {
                    break;
                }
            }

            let now = Utc::now();
            if next_fire_at > now {
                let wait = (next_fire_at - now).to_std().map_err(|e| {
                    AgentError::Other(format!("Failed to convert cron duration: {}", e))
                })?;

                if !wait.is_zero() {
                    tokio::time::sleep(wait).await;
                }
            }

            outputs.push(self.execute(input.clone()).await?);
            remaining -= 1;
        }

        Ok(outputs)
    }

    /// 暂停运行器
    /// Pause the runner
    ///
    /// 仅支持实现了 `AgentLifecycle` 的 Agent。
    /// Only supports Agents implementing `AgentLifecycle`.
    pub async fn pause(&mut self) -> AgentResult<()>
    where
        T: AgentLifecycle,
    {
        // 检查状态
        // Check state
        let current_state = self.state().await;
        if !matches!(current_state, RunnerState::Running) {
            return Err(AgentError::ValidationFailed(format!(
                "Cannot pause in state: {:?}",
                current_state
            )));
        }

        self.agent
            .pause()
            .await
            .map_err(|e| AgentError::Other(format!("Pause failed: {}", e)))?;

        *self.state.write().await = RunnerState::Paused;
        Ok(())
    }

    /// 恢复运行器
    /// Resume the runner
    ///
    /// 仅支持实现了 `AgentLifecycle` 的 Agent。
    /// Only supports Agents implementing `AgentLifecycle`.
    pub async fn resume(&mut self) -> AgentResult<()>
    where
        T: AgentLifecycle,
    {
        // 检查状态
        // Check state
        let current_state = self.state().await;
        if !matches!(current_state, RunnerState::Paused) {
            return Err(AgentError::ValidationFailed(format!(
                "Cannot resume in state: {:?}",
                current_state
            )));
        }

        self.agent
            .resume()
            .await
            .map_err(|e| AgentError::Other(format!("Resume failed: {}", e)))?;

        *self.state.write().await = RunnerState::Running;
        Ok(())
    }

    /// 关闭运行器
    /// Shutdown the runner
    ///
    /// 优雅关闭，释放资源。
    /// Graceful shutdown, releases resources.
    pub async fn shutdown(mut self) -> AgentResult<()> {
        self.agent
            .shutdown()
            .await
            .map_err(|e| AgentError::ShutdownFailed(e.to_string()))?;

        *self.state.write().await = RunnerState::Stopped;
        Ok(())
    }

    /// 中断当前执行
    /// Interrupt current execution
    pub async fn interrupt(&mut self) -> AgentResult<InterruptResult> {
        self.agent
            .interrupt()
            .await
            .map_err(|e| AgentError::Other(format!("Interrupt failed: {}", e)))
    }

    /// 消耗运行器，返回内部 Agent
    /// Consume runner, return internal Agent
    pub fn into_inner(self) -> T {
        self.agent
    }

    /// 获取 Agent ID
    /// Get Agent ID
    pub fn id(&self) -> &str {
        self.agent.id()
    }

    /// 获取 Agent 名称
    /// Get Agent name
    pub fn name(&self) -> &str {
        self.agent.name()
    }

    /// 获取 Agent 能力
    /// Get Agent capabilities
    pub fn capabilities(&self) -> &AgentCapabilities {
        self.agent.capabilities()
    }

    /// 获取 Agent 状态
    /// Get Agent state
    pub fn agent_state(&self) -> AgentState {
        self.agent.state()
    }
}

fn next_cron_fire_at<I>(upcoming: &mut I, now: DateTime<Utc>) -> AgentResult<(DateTime<Utc>, bool)>
where
    I: Iterator<Item = DateTime<Utc>>,
{
    let mut missed_any = false;

    for candidate in upcoming.by_ref() {
        if candidate <= now {
            missed_any = true;
            continue;
        }
        return Ok((candidate, missed_any));
    }

    Err(AgentError::Other(
        "Cron schedule has no upcoming execution time".to_string(),
    ))
}

/// 为支持消息处理的 Agent 提供的扩展方法
/// Extension methods for Agents supporting message processing
impl<T: MoFAAgent + AgentMessaging> AgentRunner<T> {
    /// 处理单个事件
    /// Handle a single event
    pub async fn handle_event(&mut self, event: AgentEvent) -> AgentResult<()> {
        self.agent.handle_event(event).await
    }

    /// 发送消息给 Agent
    /// Send message to Agent
    pub async fn send_message(&mut self, msg: AgentMessage) -> AgentResult<AgentMessage> {
        self.agent.handle_message(msg).await
    }
}

// ============================================================================
// 构建器模式
// Builder Pattern
// ============================================================================

/// AgentRunner 构建器
/// AgentRunner Builder
pub struct AgentRunnerBuilder<T: MoFAAgent> {
    agent: Option<T>,
    context: Option<AgentContext>,
}

impl<T: MoFAAgent> AgentRunnerBuilder<T> {
    /// 创建新的构建器
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            agent: None,
            context: None,
        }
    }

    /// 设置 Agent
    /// Set Agent
    pub fn with_agent(mut self, agent: T) -> Self {
        self.agent = Some(agent);
        self
    }

    /// 设置上下文
    /// Set context
    pub fn with_context(mut self, context: AgentContext) -> Self {
        self.context = Some(context);
        self
    }

    /// 构建运行器
    /// Build runner
    pub async fn build(self) -> AgentResult<AgentRunner<T>> {
        let agent = self
            .agent
            .ok_or_else(|| AgentError::ValidationFailed("Agent not set".to_string()))?;

        if let Some(context) = self.context {
            AgentRunner::with_context(agent, context).await
        } else {
            AgentRunner::new(agent).await
        }
    }
}

impl<T: MoFAAgent> Default for AgentRunnerBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 便捷函数
// Convenience Functions
// ============================================================================

/// 创建并运行 Agent（多次执行）
/// Create and run Agent (multiple executions)
pub async fn run_agents<T: MoFAAgent>(
    agent: T,
    inputs: Vec<AgentInput>,
) -> AgentResult<Vec<AgentOutput>> {
    let mut runner = AgentRunner::new(agent).await?;
    let results = runner.execute_batch(inputs).await;
    runner.shutdown().await?;
    results.into_iter().collect()
}

// ============================================================================
// GlobalResult-based APIs (Phase 4 unified error handling)
// ============================================================================

use mofa_foundation::recovery::RetryPolicy;
use mofa_kernel::agent::types::error::{GlobalError, GlobalResult};

impl<T: MoFAAgent> AgentRunner<T> {
    /// Execute a task returning `GlobalResult` (unified error type).
    ///
    /// This is the recommended entry point for new code that wants to use
    /// the unified error handling strategy. Errors are automatically
    /// converted from `AgentError` to `GlobalError::Agent`.
    pub async fn execute_global(&mut self, input: AgentInput) -> GlobalResult<AgentOutput> {
        self.execute(input).await.map_err(GlobalError::from)
    }

    /// Execute with automatic retry on retryable errors.
    ///
    /// Uses the kernel-level `RetryPolicy` for backoff and retry decisions.
    /// Only retries when `GlobalError::is_retryable()` returns true
    /// (e.g., LLM timeouts, runtime errors, WASM/Rhai errors).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let policy = RetryPolicy::builder()
    ///     .max_attempts(3)
    ///     .backoff(Backoff::exponential(100, 5000))
    ///     .build();
    ///
    /// let output = runner.execute_with_retry(input, policy).await?;
    /// ```
    pub async fn execute_with_retry(
        &mut self,
        input: AgentInput,
        policy: RetryPolicy,
    ) -> GlobalResult<AgentOutput> {
        let mut last_error = None;

        for attempt in 0..policy.max_attempts {
            // Backoff delay on retries
            if attempt > 0 {
                let delay = policy.backoff.delay_for(attempt - 1);
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
            }

            match self.execute(input.clone()).await {
                Ok(output) => return Ok(output),
                Err(agent_err) => {
                    let global_err = GlobalError::from(agent_err);
                    let is_last = attempt + 1 >= policy.max_attempts;
                    if is_last || !policy.should_retry(&global_err) {
                        return Err(global_err);
                    }
                    last_error = Some(global_err);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| GlobalError::Other("retry loop exhausted".to_string())))
    }
}

/// Create and run Agent with unified `GlobalResult` return type.
///
/// Same as [`run_agents`] but returns `GlobalResult` instead of `AgentResult`,
/// enabling seamless integration with the unified error handling system.
pub async fn run_agents_global<T: MoFAAgent>(
    agent: T,
    inputs: Vec<AgentInput>,
) -> GlobalResult<Vec<AgentOutput>> {
    let mut runner = AgentRunner::new(agent).await.map_err(GlobalError::from)?;

    let mut outputs = Vec::with_capacity(inputs.len());
    for input in inputs {
        outputs.push(runner.execute_global(input).await?);
    }

    runner.shutdown().await.map_err(GlobalError::from)?;
    Ok(outputs)
}

// ============================================================================
// 测试
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::capabilities::AgentCapabilitiesBuilder;
    use std::time::{Duration as StdDuration, Instant};

    struct TestAgent {
        id: String,
        name: String,
        state: AgentState,
    }

    impl TestAgent {
        fn new(id: &str, name: &str) -> Self {
            Self {
                id: id.to_string(),
                name: name.to_string(),
                state: AgentState::Created,
            }
        }
    }

    #[async_trait::async_trait]
    impl MoFAAgent for TestAgent {
        fn id(&self) -> &str {
            &self.id
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn capabilities(&self) -> &AgentCapabilities {
            static CAPS: std::sync::OnceLock<AgentCapabilities> = std::sync::OnceLock::new();
            CAPS.get_or_init(|| AgentCapabilitiesBuilder::new().build())
        }

        async fn initialize(&mut self, _ctx: &AgentContext) -> AgentResult<()> {
            self.state = AgentState::Ready;
            Ok(())
        }

        async fn execute(
            &mut self,
            input: AgentInput,
            _ctx: &AgentContext,
        ) -> AgentResult<AgentOutput> {
            self.state = AgentState::Executing;
            let text = input.to_text();
            Ok(AgentOutput::text(format!("Echo: {}", text)))
        }

        async fn shutdown(&mut self) -> AgentResult<()> {
            self.state = AgentState::Shutdown;
            Ok(())
        }

        fn state(&self) -> AgentState {
            self.state.clone()
        }
    }

    struct MisfireProbeAgent {
        id: String,
        name: String,
        state: AgentState,
        capabilities: AgentCapabilities,
        run_count: u64,
        first_run_delay: StdDuration,
        started_at: Instant,
    }

    impl MisfireProbeAgent {
        fn new(id: &str, name: &str, first_run_delay: StdDuration) -> Self {
            Self {
                id: id.to_string(),
                name: name.to_string(),
                state: AgentState::Created,
                capabilities: AgentCapabilitiesBuilder::new().build(),
                run_count: 0,
                first_run_delay,
                started_at: Instant::now(),
            }
        }
    }

    #[async_trait::async_trait]
    impl MoFAAgent for MisfireProbeAgent {
        fn id(&self) -> &str {
            &self.id
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn capabilities(&self) -> &AgentCapabilities {
            &self.capabilities
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
            self.state = AgentState::Executing;
            self.run_count += 1;

            if self.run_count == 1 {
                tokio::time::sleep(self.first_run_delay).await;
            }

            let elapsed_ms = self.started_at.elapsed().as_millis();
            self.state = AgentState::Ready;
            Ok(AgentOutput::text(format!(
                "run={} elapsed_ms={}",
                self.run_count, elapsed_ms
            )))
        }

        async fn shutdown(&mut self) -> AgentResult<()> {
            self.state = AgentState::Shutdown;
            Ok(())
        }

        fn state(&self) -> AgentState {
            self.state.clone()
        }
    }

    fn every_second_cron_expression() -> String {
        for expression in ["*/1 * * * * * *", "*/1 * * * * *"] {
            if Schedule::from_str(expression).is_ok() {
                return expression.to_string();
            }
        }

        panic!("No supported every-second cron expression format found");
    }

    fn parse_elapsed_ms(output: &AgentOutput) -> u128 {
        let text = output.to_text();
        text.split("elapsed_ms=")
            .nth(1)
            .expect("output should contain elapsed_ms marker")
            .trim()
            .parse::<u128>()
            .expect("elapsed_ms should be a valid integer")
    }

    #[tokio::test]
    async fn test_agent_runner_new() {
        let agent = TestAgent::new("test-001", "Test Agent");
        let runner = AgentRunner::new(agent).await.unwrap();

        assert_eq!(runner.id(), "test-001");
        assert_eq!(runner.name(), "Test Agent");
        // 初始化后状态是 Created（因为 initialize 已经完成）
        // State is Created after initialization (since initialize is complete)
        assert_eq!(runner.state().await, RunnerState::Created);
    }

    #[tokio::test]
    async fn test_agent_runner_execute() {
        let agent = TestAgent::new("test-002", "Test Agent");
        let mut runner = AgentRunner::new(agent).await.unwrap();

        let input = AgentInput::text("Hello");
        let output = runner.execute(input).await.unwrap();

        assert_eq!(output.to_text(), "Echo: Hello");

        let stats = runner.stats().await;
        assert_eq!(stats.total_executions, 1);
        assert_eq!(stats.successful_executions, 1);
    }

    #[tokio::test]
    async fn test_run_agents_function() {
        let agent = TestAgent::new("test-003", "Test Agent");
        let inputs = vec![AgentInput::text("Test")];
        let outputs = run_agents(agent, inputs).await.unwrap();

        assert_eq!(outputs[0].to_text(), "Echo: Test");
    }

    #[tokio::test]
    async fn test_agent_runner_run_periodic_executes_max_runs() {
        let agent = TestAgent::new("test-004", "Periodic Agent");
        let mut runner = AgentRunner::new(agent).await.unwrap();

        let outputs = runner
            .run_periodic(
                AgentInput::text("Tick"),
                PeriodicRunConfig {
                    interval: Duration::from_millis(10),
                    max_runs: 3,
                    run_immediately: true,
                },
            )
            .await
            .unwrap();

        assert_eq!(outputs.len(), 3);
        assert!(outputs.iter().all(|o| o.to_text() == "Echo: Tick"));

        let stats = runner.stats().await;
        assert_eq!(stats.total_executions, 3);
        assert_eq!(stats.successful_executions, 3);
    }

    #[tokio::test]
    async fn test_agent_runner_run_periodic_initial_delay_when_not_immediate() {
        let agent = TestAgent::new("test-005", "Delayed Periodic Agent");
        let mut runner = AgentRunner::new(agent).await.unwrap();

        let started = Instant::now();
        let outputs = runner
            .run_periodic(
                AgentInput::text("Delayed"),
                PeriodicRunConfig {
                    interval: Duration::from_millis(40),
                    max_runs: 1,
                    run_immediately: false,
                },
            )
            .await
            .unwrap();

        assert_eq!(outputs.len(), 1);
        assert!(started.elapsed() >= StdDuration::from_millis(30));
    }

    #[tokio::test]
    async fn test_agent_runner_run_periodic_rejects_invalid_config() {
        let agent = TestAgent::new("test-006", "Invalid Config Agent");
        let mut runner = AgentRunner::new(agent).await.unwrap();

        let err = runner
            .run_periodic(
                AgentInput::text("x"),
                PeriodicRunConfig {
                    interval: Duration::ZERO,
                    max_runs: 1,
                    run_immediately: true,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AgentError::ValidationFailed(_)));

        let err = runner
            .run_periodic(
                AgentInput::text("x"),
                PeriodicRunConfig {
                    interval: Duration::from_millis(10),
                    max_runs: 0,
                    run_immediately: true,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AgentError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn test_agent_runner_run_periodic_cron_executes_max_runs() {
        let agent = TestAgent::new("test-007", "Cron Agent");
        let mut runner = AgentRunner::new(agent).await.unwrap();
        let expression = every_second_cron_expression();

        let outputs = runner
            .run_periodic_cron(
                AgentInput::text("TickCron"),
                CronRunConfig {
                    expression,
                    max_runs: 2,
                    run_immediately: true,
                    misfire_policy: CronMisfirePolicy::Skip,
                },
            )
            .await
            .unwrap();

        assert_eq!(outputs.len(), 2);
        assert!(outputs.iter().all(|o| o.to_text() == "Echo: TickCron"));

        let stats = runner.stats().await;
        assert_eq!(stats.total_executions, 2);
        assert_eq!(stats.successful_executions, 2);
    }

    #[tokio::test]
    async fn test_agent_runner_run_periodic_cron_rejects_invalid_config() {
        let agent = TestAgent::new("test-008", "Cron Invalid Config Agent");
        let mut runner = AgentRunner::new(agent).await.unwrap();

        let err = runner
            .run_periodic_cron(
                AgentInput::text("x"),
                CronRunConfig {
                    expression: "".to_string(),
                    max_runs: 1,
                    run_immediately: true,
                    misfire_policy: CronMisfirePolicy::Skip,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AgentError::ValidationFailed(_)));

        let err = runner
            .run_periodic_cron(
                AgentInput::text("x"),
                CronRunConfig {
                    expression: "not-a-cron".to_string(),
                    max_runs: 1,
                    run_immediately: true,
                    misfire_policy: CronMisfirePolicy::Skip,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AgentError::ValidationFailed(_)));

        let err = runner
            .run_periodic_cron(
                AgentInput::text("x"),
                CronRunConfig {
                    expression: every_second_cron_expression(),
                    max_runs: 0,
                    run_immediately: true,
                    misfire_policy: CronMisfirePolicy::Skip,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AgentError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn test_agent_runner_run_periodic_cron_misfire_policy_run_once() {
        let expression = every_second_cron_expression();

        let mut skip_runner = AgentRunner::new(MisfireProbeAgent::new(
            "test-009-skip",
            "Cron Misfire Skip",
            StdDuration::from_millis(2200),
        ))
        .await
        .unwrap();

        let skip_outputs = skip_runner
            .run_periodic_cron(
                AgentInput::text("skip"),
                CronRunConfig {
                    expression: expression.clone(),
                    max_runs: 2,
                    run_immediately: false,
                    misfire_policy: CronMisfirePolicy::Skip,
                },
            )
            .await
            .unwrap();

        let mut run_once_runner = AgentRunner::new(MisfireProbeAgent::new(
            "test-009-run-once",
            "Cron Misfire RunOnce",
            StdDuration::from_millis(2200),
        ))
        .await
        .unwrap();

        let run_once_outputs = run_once_runner
            .run_periodic_cron(
                AgentInput::text("run-once"),
                CronRunConfig {
                    expression,
                    max_runs: 2,
                    run_immediately: false,
                    misfire_policy: CronMisfirePolicy::RunOnce,
                },
            )
            .await
            .unwrap();

        assert_eq!(skip_outputs.len(), 2);
        assert_eq!(run_once_outputs.len(), 2);

        let skip_gap =
            parse_elapsed_ms(&skip_outputs[1]).saturating_sub(parse_elapsed_ms(&skip_outputs[0]));
        let run_once_gap = parse_elapsed_ms(&run_once_outputs[1])
            .saturating_sub(parse_elapsed_ms(&run_once_outputs[0]));

        assert!(
            skip_gap >= 500,
            "skip policy should wait for a future cron slot, gap={}ms",
            skip_gap
        );
        assert!(
            run_once_gap < 400,
            "run-once policy should catch up immediately, gap={}ms",
            run_once_gap
        );
    }

    // ========================================================================
    // Lifecycle (pause / resume) tests
    // ========================================================================

    /// An agent whose pause and resume operations can be configured to fail.
    struct LifecycleTestAgent {
        id: String,
        name: String,
        state: AgentState,
        capabilities: AgentCapabilities,
        fail_pause: bool,
        fail_resume: bool,
    }

    impl LifecycleTestAgent {
        fn new(id: &str, name: &str) -> Self {
            Self {
                id: id.to_string(),
                name: name.to_string(),
                state: AgentState::Created,
                capabilities: AgentCapabilitiesBuilder::new().build(),
                fail_pause: false,
                fail_resume: false,
            }
        }

        fn with_failing_pause(mut self) -> Self {
            self.fail_pause = true;
            self
        }

        fn with_failing_resume(mut self) -> Self {
            self.fail_resume = true;
            self
        }
    }

    #[async_trait::async_trait]
    impl MoFAAgent for LifecycleTestAgent {
        fn id(&self) -> &str {
            &self.id
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn capabilities(&self) -> &AgentCapabilities {
            &self.capabilities
        }

        async fn initialize(&mut self, _ctx: &AgentContext) -> AgentResult<()> {
            self.state = AgentState::Ready;
            Ok(())
        }

        async fn execute(
            &mut self,
            input: AgentInput,
            _ctx: &AgentContext,
        ) -> AgentResult<AgentOutput> {
            self.state = AgentState::Executing;
            let text = input.to_text();
            Ok(AgentOutput::text(format!("Echo: {}", text)))
        }

        async fn shutdown(&mut self) -> AgentResult<()> {
            self.state = AgentState::Shutdown;
            Ok(())
        }

        fn state(&self) -> AgentState {
            self.state.clone()
        }
    }

    #[async_trait::async_trait]
    impl AgentLifecycle for LifecycleTestAgent {
        async fn pause(&mut self) -> AgentResult<()> {
            if self.fail_pause {
                return Err(AgentError::Other("simulated pause failure".to_string()));
            }
            self.state = AgentState::Paused;
            Ok(())
        }

        async fn resume(&mut self) -> AgentResult<()> {
            if self.fail_resume {
                return Err(AgentError::Other("simulated resume failure".to_string()));
            }
            self.state = AgentState::Ready;
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_pause_resume_success() {
        let agent = LifecycleTestAgent::new("lc-001", "Lifecycle Agent");
        let mut runner = AgentRunner::new(agent).await.unwrap();

        // Move the runner into Running state first.
        runner.execute(AgentInput::text("warmup")).await.unwrap();
        assert_eq!(runner.state().await, RunnerState::Running);

        // Pause should succeed and transition to Paused.
        runner.pause().await.unwrap();
        assert_eq!(runner.state().await, RunnerState::Paused);

        // Resume should succeed and transition back to Running.
        runner.resume().await.unwrap();
        assert_eq!(runner.state().await, RunnerState::Running);
    }

    #[tokio::test]
    async fn test_resume_failure_preserves_paused_state() {
        let agent = LifecycleTestAgent::new("lc-002", "Failing Resume Agent").with_failing_resume();
        let mut runner = AgentRunner::new(agent).await.unwrap();

        // Move into Running, then pause.
        runner.execute(AgentInput::text("warmup")).await.unwrap();
        runner.pause().await.unwrap();
        assert_eq!(runner.state().await, RunnerState::Paused);

        // Resume fails — the runner must stay Paused.
        let err = runner.resume().await.unwrap_err();
        assert!(
            format!("{}", err).contains("Resume failed"),
            "unexpected error: {}",
            err
        );
        assert_eq!(runner.state().await, RunnerState::Paused);
    }

    #[tokio::test]
    async fn test_pause_failure_preserves_running_state() {
        let agent = LifecycleTestAgent::new("lc-003", "Failing Pause Agent").with_failing_pause();
        let mut runner = AgentRunner::new(agent).await.unwrap();

        // Move into Running.
        runner.execute(AgentInput::text("warmup")).await.unwrap();
        assert_eq!(runner.state().await, RunnerState::Running);

        // Pause fails — the runner must stay Running.
        let err = runner.pause().await.unwrap_err();
        assert!(
            format!("{}", err).contains("Pause failed"),
            "unexpected error: {}",
            err
        );
        assert_eq!(runner.state().await, RunnerState::Running);
    }

    #[tokio::test]
    async fn test_resume_rejected_when_not_paused() {
        let agent = LifecycleTestAgent::new("lc-004", "Not Paused Agent");
        let mut runner = AgentRunner::new(agent).await.unwrap();

        // Runner starts in Created state — resume should be rejected.
        let err = runner.resume().await.unwrap_err();
        assert!(matches!(err, AgentError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn test_pause_rejected_when_not_running() {
        let agent = LifecycleTestAgent::new("lc-005", "Not Running Agent");
        let mut runner = AgentRunner::new(agent).await.unwrap();

        // Runner starts in Created state — pause should be rejected.
        let err = runner.pause().await.unwrap_err();
        assert!(matches!(err, AgentError::ValidationFailed(_)));
    }
}
