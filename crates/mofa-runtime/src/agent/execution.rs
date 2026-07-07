//! 执行引擎
//! Execution Engine
//!
//! 提供 Agent 执行、工作流编排、错误处理等功能
//! Provides Agent execution, workflow orchestration, and error handling

use crate::agent::context::{AgentContext, AgentEvent};
use crate::agent::core::MoFAAgent;
use crate::agent::error::{AgentError, AgentResult};
use crate::agent::plugins::{PluginExecutor, PluginRegistry, SimplePluginRegistry};
use crate::agent::registry::AgentRegistry;
use crate::agent::types::{AgentInput, AgentOutput, AgentState};
use crate::fallback::{FallbackStrategy, NoFallback};
use crate::retry::{RetryConfig, RetryPolicy};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, Semaphore};
use tokio::time::timeout;
use tracing::Instrument;

/// 执行选项
/// Execution options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionOptions {
    /// 超时时间 (毫秒)
    /// Timeout duration (milliseconds)
    #[serde(default)]
    pub timeout_ms: Option<u64>,

    /// 是否启用追踪
    /// Whether tracing is enabled
    #[serde(default = "default_tracing")]
    pub tracing_enabled: bool,

    /// 重试次数
    /// Number of retries
    #[serde(default)]
    pub max_retries: usize,

    /// 重试延迟 (毫秒)
    /// Retry delay (milliseconds)
    #[serde(default = "default_retry_delay")]
    pub retry_delay_ms: u64,

    /// Policy-driven retry configuration.
    #[serde(default)]
    pub retry_config: Option<RetryConfig>,

    /// 自定义参数
    /// Custom parameters
    #[serde(default)]
    pub custom: HashMap<String, serde_json::Value>,
}

fn default_tracing() -> bool {
    true
}

fn default_retry_delay() -> u64 {
    1000
}

impl Default for ExecutionOptions {
    fn default() -> Self {
        Self {
            timeout_ms: None,
            tracing_enabled: true,
            max_retries: 0,
            retry_delay_ms: default_retry_delay(),
            retry_config: None,
            custom: HashMap::new(),
        }
    }
}

impl ExecutionOptions {
    /// 创建新的执行选项
    /// Create new execution options
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置超时
    /// Set timeout duration
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// 设置重试
    /// Set retry settings
    pub fn with_retry(mut self, max_retries: usize, retry_delay_ms: u64) -> Self {
        self.max_retries = max_retries;
        self.retry_delay_ms = retry_delay_ms;
        self.retry_config = Some(RetryConfig {
            max_attempts: max_retries + 1,
            policy: RetryPolicy::Fixed {
                delay_ms: retry_delay_ms,
            },
        });
        self
    }

    /// Set a custom retry policy.
    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = Some(config);
        self
    }

    /// 禁用追踪
    /// Disable tracing
    pub fn without_tracing(mut self) -> Self {
        self.tracing_enabled = false;
        self
    }

    /// Resolves the effective [`RetryConfig`] from the explicit field or
    /// legacy `max_retries` / `retry_delay_ms` values.
    pub(crate) fn effective_retry_config(&self) -> RetryConfig {
        if let Some(ref cfg) = self.retry_config {
            return cfg.clone();
        }
        RetryConfig {
            max_attempts: self.max_retries + 1,
            policy: RetryPolicy::Fixed {
                delay_ms: self.retry_delay_ms,
            },
        }
    }
}

/// 执行状态
/// Execution status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    /// 待执行
    /// Pending execution
    Pending,
    /// 执行中
    /// Currently running
    Running,
    /// 成功
    /// Success
    Success,
    /// 失败
    /// Failed
    Failed,
    /// 超时
    /// Timeout
    Timeout,
    /// 中断
    /// Interrupted
    Interrupted,
    /// 重试中
    /// Retrying attempt
    Retrying { attempt: usize },
}

/// 执行结果
/// Execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// 执行 ID
    /// Execution ID
    pub execution_id: String,
    /// Agent ID
    /// Agent ID
    pub agent_id: String,
    /// 状态
    /// Status
    pub status: ExecutionStatus,
    /// 输出
    /// Output
    pub output: Option<AgentOutput>,
    /// 错误信息
    /// Error message
    pub error: Option<String>,
    /// 执行时间 (毫秒)
    /// Execution duration (ms)
    pub duration_ms: u64,
    /// 重试次数
    /// Retry count
    pub retries: usize,
    /// 元数据
    /// Metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ExecutionResult {
    /// 创建成功结果
    /// Create success result
    pub fn success(
        execution_id: String,
        agent_id: String,
        output: AgentOutput,
        duration_ms: u64,
    ) -> Self {
        Self {
            execution_id,
            agent_id,
            status: ExecutionStatus::Success,
            output: Some(output),
            error: None,
            duration_ms,
            retries: 0,
            metadata: HashMap::new(),
        }
    }

    /// 创建失败结果
    /// Create failure result
    pub fn failure(
        execution_id: String,
        agent_id: String,
        error: String,
        duration_ms: u64,
    ) -> Self {
        Self {
            execution_id,
            agent_id,
            status: ExecutionStatus::Failed,
            output: None,
            error: Some(error),
            duration_ms,
            retries: 0,
            metadata: HashMap::new(),
        }
    }

    /// 是否成功
    /// Check if successful
    pub fn is_success(&self) -> bool {
        self.status == ExecutionStatus::Success
    }

    /// 添加元数据
    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

/// 执行引擎
/// Execution Engine
///
/// 提供 Agent 执行、工作流编排等功能
/// Provides Agent execution and workflow orchestration functions
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// use mofa_runtime::agent::execution::{ExecutionEngine, ExecutionOptions};
///
/// let registry = AgentRegistry::new();
/// // ... 注册 Agent ...
/// // ... register agent ...
///
/// let engine = ExecutionEngine::new(registry);
///
/// let result = engine.execute(
///     "my-agent",
///     AgentInput::text("Hello"),
///     ExecutionOptions::default(),
/// ).await?;
///
/// if result.is_success() {
///     info!("Output: {:?}", result.output);
/// }
/// ```
#[derive(Clone)]
pub struct ExecutionEngine {
    /// Agent 注册中心
    /// Agent Registry
    registry: Arc<AgentRegistry>,
    /// 插件执行器
    /// Plugin Executor
    plugin_executor: PluginExecutor,
    /// Fallback strategy invoked after all retries are exhausted.
    fallback: Arc<dyn FallbackStrategy>,
}

impl ExecutionEngine {
    /// 创建新的执行引擎
    /// Create a new execution engine
    pub fn new(registry: Arc<AgentRegistry>) -> Self {
        Self {
            registry,
            plugin_executor: PluginExecutor::new(Arc::new(SimplePluginRegistry::new())),
            fallback: Arc::new(NoFallback),
        }
    }

    /// Attach a [`FallbackStrategy`] that is invoked when all retries fail.
    pub fn with_fallback(mut self, fallback: Arc<dyn FallbackStrategy>) -> Self {
        self.fallback = fallback;
        self
    }

    /// 创建带有自定义插件注册中心的执行引擎
    /// Create execution engine with custom plugin registry
    pub fn with_plugin_registry(
        registry: Arc<AgentRegistry>,
        plugin_registry: Arc<dyn PluginRegistry>,
    ) -> Self {
        Self {
            registry,
            plugin_executor: PluginExecutor::new(plugin_registry),
            fallback: Arc::new(NoFallback),
        }
    }

    /// 执行 Agent
    /// Execute Agent
    pub async fn execute(
        &self,
        agent_id: &str,
        input: AgentInput,
        options: ExecutionOptions,
    ) -> AgentResult<ExecutionResult> {
        let execution_id = uuid::Uuid::now_v7().to_string();
        let start_time = std::time::Instant::now();
        tracing::info!(agent_id = %agent_id, execution_id = %execution_id, "Agent execution started");

        // 获取 Agent
        // Get Agent
        let agent = self
            .registry
            .get(agent_id)
            .await
            .ok_or_else(|| AgentError::NotFound(format!("Agent not found: {}", agent_id)))?;

        // 创建上下文
        // Create context
        let ctx = AgentContext::new(&execution_id);

        // 发送开始事件
        // Emit start event
        if options.tracing_enabled {
            ctx.emit_event(AgentEvent::new(
                "execution_started",
                serde_json::json!({
                    "agent_id": agent_id,
                    "execution_id": execution_id,
                }),
            ))
            .await;
        }

        // 插件执行阶段1: 请求处理前 - 数据处理
        // Plugin Stage 1: Pre-request - Data processing
        let processed_input = self
            .plugin_executor
            .execute_pre_request(input, &ctx)
            .await?;

        // 插件执行阶段2: 上下文组装前
        // Plugin Stage 2: Pre-context assembly
        self.plugin_executor
            .execute_stage(crate::agent::plugins::PluginStage::PreContext, &ctx)
            .await?;

        // 执行 (带超时和重试)
        // Execute (with timeout and retries)
        let retry_cfg = options.effective_retry_config();
        let result = self
            .execute_with_options(&agent, processed_input.clone(), &ctx, &options, &retry_cfg)
            .await;

        let duration_ms = u64::try_from(start_time.elapsed().as_millis()).unwrap_or(u64::MAX);

        // 构建结果
        // Build result
        let execution_result = match result {
            Ok((output, retries)) => {
                // 插件执行阶段3: LLM响应后
                // Plugin Stage 3: Post-LLM response
                let processed_output = self
                    .plugin_executor
                    .execute_post_response(output, &ctx)
                    .await?;

                // 插件执行阶段4: 整个流程完成后
                // Plugin Stage 4: Post-process after workflow completion
                self.plugin_executor
                    .execute_stage(crate::agent::plugins::PluginStage::PostProcess, &ctx)
                    .await?;

                if options.tracing_enabled {
                    ctx.emit_event(AgentEvent::new(
                        "execution_completed",
                        serde_json::json!({
                            "agent_id": agent_id,
                            "execution_id": execution_id,
                            "duration_ms": duration_ms,
                            "retries": retries,
                        }),
                    ))
                    .await;
                }

                let mut r = ExecutionResult::success(
                    execution_id,
                    agent_id.to_string(),
                    processed_output,
                    duration_ms,
                );
                r.retries = retries;
                r
            }
            Err(e) => {
                // Try graceful degradation before giving up.
                let attempts = retry_cfg.max_attempts;
                if let Some(fallback_output) =
                    self.fallback.on_failure(agent_id, &e, attempts).await
                {
                    let mut r = ExecutionResult::success(
                        execution_id,
                        agent_id.to_string(),
                        fallback_output,
                        duration_ms,
                    );
                    r.retries = attempts.saturating_sub(1);
                    r.metadata
                        .insert("fallback".into(), serde_json::json!(true));
                    r.metadata
                        .insert("fallback_reason".into(), serde_json::json!(e.to_string()));
                    return Ok(r);
                }

                let status = match &e {
                    AgentError::Timeout { .. } => ExecutionStatus::Timeout,
                    AgentError::Interrupted => ExecutionStatus::Interrupted,
                    _ => ExecutionStatus::Failed,
                };

                if options.tracing_enabled {
                    ctx.emit_event(AgentEvent::new(
                        "execution_failed",
                        serde_json::json!({
                            "agent_id": agent_id,
                            "execution_id": execution_id,
                            "error": e.to_string(),
                            "duration_ms": duration_ms,
                        }),
                    ))
                    .await;
                }

                ExecutionResult {
                    execution_id,
                    agent_id: agent_id.to_string(),
                    status,
                    output: None,
                    error: Some(e.to_string()),
                    duration_ms,
                    retries: attempts.saturating_sub(1),
                    metadata: HashMap::new(),
                }
            }
        };

        Ok(execution_result)
    }

    /// 带选项执行
    /// Execute with options
    async fn execute_with_options(
        &self,
        agent: &Arc<RwLock<dyn MoFAAgent>>,
        input: AgentInput,
        ctx: &AgentContext,
        options: &ExecutionOptions,
        retry_cfg: &RetryConfig,
    ) -> AgentResult<(AgentOutput, usize)> {
        let max_attempts = retry_cfg.max_attempts.max(1);
        let mut last_error = None;

        for attempt in 0..max_attempts {
            let attempt_span = tracing::info_span!(
                "agent.attempt",
                attempt = attempt,
                max_attempts = max_attempts
            );
            if attempt > 0 {
                let delay = retry_cfg.policy.delay_for(attempt - 1);
                tokio::time::sleep(delay).await;
            }

            let result = async { self.execute_once(agent, input.clone(), ctx, options).await }
                .instrument(attempt_span)
                .await;

            match result {
                Ok(output) => return Ok((output, attempt)),
                Err(e) => {
                    if !e.is_retryable() {
                        return Err(e);
                    }
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| AgentError::ExecutionFailed("Unknown error".to_string())))
    }

    /// 单次执行
    /// Single execution attempt
    async fn execute_once(
        &self,
        agent: &Arc<RwLock<dyn MoFAAgent>>,
        input: AgentInput,
        ctx: &AgentContext,
        options: &ExecutionOptions,
    ) -> AgentResult<AgentOutput> {
        let mut agent_guard = agent.write().await;

        // 确保 Agent 已初始化
        // Ensure Agent is initialized
        if agent_guard.state() == AgentState::Created {
            agent_guard.initialize(ctx).await?;
        }

        // 检查状态
        // Check state
        if agent_guard.state() != AgentState::Ready {
            return Err(AgentError::invalid_state_transition(
                agent_guard.state(),
                &AgentState::Executing,
            ));
        }

        // 执行 (带超时)
        // Execute (with timeout)
        if let Some(timeout_ms) = options.timeout_ms {
            let duration = Duration::from_millis(timeout_ms);
            match timeout(duration, agent_guard.execute(input, ctx)).await {
                Ok(result) => result,
                Err(_) => Err(AgentError::timeout(timeout_ms)),
            }
        } else {
            agent_guard.execute(input, ctx).await
        }
    }

    /// 批量执行
    /// Batch execution
    pub async fn execute_batch(
        &self,
        executions: Vec<(String, AgentInput)>,
        options: ExecutionOptions,
    ) -> Vec<AgentResult<ExecutionResult>> {
        let mut results = Vec::new();

        for (agent_id, input) in executions {
            let result = self.execute(&agent_id, input, options.clone()).await;
            results.push(result);
        }

        results
    }

    /// 并行执行多个 Agent
    /// Execute multiple agents in parallel
    /// Respects concurrency limits to prevent resource exhaustion.
    /// Default concurrency: 10 concurrent tasks
    pub async fn execute_parallel(
        &self,
        executions: Vec<(String, AgentInput)>,
        options: ExecutionOptions,
    ) -> Vec<AgentResult<ExecutionResult>> {
        // Use Semaphore to limit concurrent task spawning
        // Default: 10 concurrent tasks (respects RuntimeConfig::max_concurrent_tasks default)
        let max_concurrent = 10;
        let semaphore = Arc::new(Semaphore::new(max_concurrent));
        let mut handles = Vec::new();

        for (agent_id, input) in executions {
            let engine = self.clone();
            let opts = options.clone();
            let sem = semaphore.clone();

            let span = tracing::info_span!("agent.parallel", agent_id = %agent_id);

            // Acquire a permit before spawning the task
            let permit = sem.acquire().await.expect("semaphore should never be closed");

            let handle = tokio::spawn(
                async move {
                    // Hold the permit for the entire execution
                    let _permit = permit;
                    engine.execute(&agent_id, input, opts).await
                }
                .instrument(span),
            );

            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(Err(AgentError::ExecutionFailed(e.to_string()))),
            }
        }

        results
    }

    /// 中断执行
    /// Interrupt execution
    pub async fn interrupt(&self, agent_id: &str) -> AgentResult<()> {
        let agent = self
            .registry
            .get(agent_id)
            .await
            .ok_or_else(|| AgentError::NotFound(format!("Agent not found: {}", agent_id)))?;

        let mut agent_guard = agent.write().await;
        agent_guard.interrupt().await?;

        Ok(())
    }

    /// 中断所有执行中的 Agent
    /// Interrupt all currently executing Agents
    pub async fn interrupt_all(&self) -> AgentResult<Vec<String>> {
        let executing = self.registry.find_by_state(AgentState::Executing).await;

        let mut interrupted = Vec::new();
        for metadata in executing {
            if self.interrupt(&metadata.id).await.is_ok() {
                interrupted.push(metadata.id);
            }
        }

        Ok(interrupted)
    }

    /// 注册插件
    /// Register plugin
    pub fn register_plugin(
        &self,
        plugin: Arc<dyn crate::agent::plugins::Plugin>,
    ) -> AgentResult<()> {
        // 现在 PluginRegistry 支持 &self 注册，因为使用了内部可变性
        // PluginRegistry now supports &self registration via interior mutability
        self.plugin_executor.registry.register(plugin)
    }

    /// 移除插件
    /// Unregister plugin
    pub fn unregister_plugin(&self, name: &str) -> AgentResult<bool> {
        // 现在 PluginRegistry 支持 &self 注销，因为使用了内部可变性
        // PluginRegistry now supports &self unregistration via interior mutability
        self.plugin_executor.registry.unregister(name)
    }

    /// 列出所有插件
    /// List all plugins
    pub fn list_plugins(&self) -> Vec<Arc<dyn crate::agent::plugins::Plugin>> {
        self.plugin_executor.registry.list()
    }

    /// 插件数量
    /// Plugin count
    pub fn plugin_count(&self) -> usize {
        self.plugin_executor.registry.count()
    }
}

// ============================================================================
// 工作流执行
// Workflow Execution
// ============================================================================

/// 工作流步骤
/// Workflow step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    /// 步骤 ID
    /// Step ID
    pub id: String,
    /// Agent ID
    /// Agent ID
    pub agent_id: String,
    /// 输入转换
    /// Input transformation
    #[serde(default)]
    pub input_transform: Option<String>,
    /// 依赖的步骤
    /// Dependent steps
    #[serde(default)]
    pub depends_on: Vec<String>,
}

/// 工作流定义
/// Workflow definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    /// 工作流 ID
    /// Workflow ID
    pub id: String,
    /// 工作流名称
    /// Workflow name
    pub name: String,
    /// 步骤列表
    /// List of steps
    pub steps: Vec<WorkflowStep>,
}

impl ExecutionEngine {
    /// 执行工作流
    /// Execute workflow
    pub async fn execute_workflow(
        &self,
        workflow: &Workflow,
        initial_input: AgentInput,
        options: ExecutionOptions,
    ) -> AgentResult<HashMap<String, ExecutionResult>> {
        let _workflow_span = tracing::info_span!("workflow.execute", workflow_id = %workflow.id, workflow_name = %workflow.name);
        tracing::info!(parent: &_workflow_span, "Workflow execution started");
        let mut results: HashMap<String, ExecutionResult> = HashMap::new();
        let mut completed: Vec<String> = Vec::new();

        // 简单的拓扑排序执行
        // Simple topological sort execution
        while completed.len() < workflow.steps.len() {
            let mut executed_any = false;

            for step in &workflow.steps {
                // 跳过已完成的步骤
                // Skip completed steps
                if completed.contains(&step.id) {
                    continue;
                }

                // 检查依赖
                // Check dependencies
                let deps_satisfied = step.depends_on.iter().all(|dep| completed.contains(dep));
                if !deps_satisfied {
                    continue;
                }

                // 准备输入
                // Prepare input
                let input = if step.depends_on.is_empty() {
                    initial_input.clone()
                } else {
                    // 使用前一个步骤的输出作为输入
                    // Use output of the previous step as input
                    let prev_step = step.depends_on.last().unwrap();
                    if let Some(prev_result) = results.get(prev_step) {
                        if let Some(output) = &prev_result.output {
                            AgentInput::text(output.to_text())
                        } else {
                            initial_input.clone()
                        }
                    } else {
                        initial_input.clone()
                    }
                };

                // 执行步骤
                // Execute step
                let result = self.execute(&step.agent_id, input, options.clone()).await?;
                results.insert(step.id.clone(), result);
                completed.push(step.id.clone());
                executed_any = true;
            }

            if !executed_any && completed.len() < workflow.steps.len() {
                return Err(AgentError::ExecutionFailed(
                    "Workflow has circular dependencies".to_string(),
                ));
            }
        }

        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// ScheduledAgentRunner implementation
// ---------------------------------------------------------------------------

/// Allow `ExecutionEngine` to be used directly as the runner inside
/// `CronScheduler` (which lives in `mofa-foundation`) without creating a
/// cyclic crate dependency.
#[async_trait::async_trait]
impl mofa_kernel::scheduler::ScheduledAgentRunner for ExecutionEngine {
    async fn run_scheduled(
        &self,
        agent_id: &str,
        input: mofa_kernel::agent::types::AgentInput,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.execute(agent_id, input, ExecutionOptions::default())
            .await
            .map(|_| ())
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::capabilities::AgentCapabilities;
    use crate::agent::context::AgentContext;
    use crate::agent::core::MoFAAgent;
    use crate::agent::types::AgentState;

    // 测试用 Agent (内联实现，不依赖 BaseAgent)
    // Agent for testing (inline implementation, no BaseAgent dependency)
    struct TestAgent {
        id: String,
        response: String,
        capabilities: AgentCapabilities,
        state: AgentState,
    }

    impl TestAgent {
        fn new(id: &str, response: &str) -> Self {
            Self {
                id: id.to_string(),
                response: response.to_string(),
                capabilities: AgentCapabilities::default(),
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
            &self.id
        }

        fn capabilities(&self) -> &AgentCapabilities {
            &self.capabilities
        }

        fn state(&self) -> AgentState {
            self.state.clone()
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
            Ok(AgentOutput::text(&self.response))
        }

        async fn shutdown(&mut self) -> AgentResult<()> {
            self.state = AgentState::Shutdown;
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_execution_engine_basic() {
        let registry = Arc::new(AgentRegistry::new());

        // 注册测试 Agent
        // Register test agent
        let agent = Arc::new(RwLock::new(TestAgent::new("test-agent", "Hello, World!")));
        registry.register(agent).await.unwrap();

        // 创建引擎并执行
        // Create engine and execute
        let engine = ExecutionEngine::new(registry);
        let result = engine
            .execute(
                "test-agent",
                AgentInput::text("input"),
                ExecutionOptions::default(),
            )
            .await
            .unwrap();

        assert!(result.is_success());
        assert_eq!(result.output.unwrap().to_text(), "Hello, World!");
    }

    #[tokio::test]
    async fn test_execution_timeout() {
        let registry = Arc::new(AgentRegistry::new());
        let agent = Arc::new(RwLock::new(TestAgent::new("slow-agent", "response")));
        registry.register(agent).await.unwrap();

        let engine = ExecutionEngine::new(registry);
        let result = engine
            .execute(
                "slow-agent",
                AgentInput::text("input"),
                ExecutionOptions::default().with_timeout(1), // 1ms timeout
            )
            .await
            .unwrap();

        // 可能成功也可能超时，取决于执行速度
        // May succeed or timeout depending on execution speed
        assert!(
            result.status == ExecutionStatus::Success || result.status == ExecutionStatus::Timeout
        );
    }

    #[test]
    fn test_execution_options() {
        let options = ExecutionOptions::new()
            .with_timeout(5000)
            .with_retry(3, 500)
            .without_tracing();

        assert_eq!(options.timeout_ms, Some(5000));
        assert_eq!(options.max_retries, 3);
        assert_eq!(options.retry_delay_ms, 500);
        assert!(!options.tracing_enabled);
    }

    #[test]
    fn test_execution_options_retry_delay_default_is_consistent() {
        let options_from_default = ExecutionOptions::default();
        let options_from_serde: ExecutionOptions = serde_json::from_str("{}").unwrap();

        assert_eq!(options_from_default.retry_delay_ms, default_retry_delay());
        assert_eq!(
            options_from_default.retry_delay_ms,
            options_from_serde.retry_delay_ms
        );
    }
}
