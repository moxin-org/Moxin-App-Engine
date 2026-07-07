//! Agent 执行模式
//! Agent Execution Modes
//!
//! 提供 Chain（链式）和 Parallel（并行）模式的 Agent 执行支持
//! Provides support for Chain (sequential) and Parallel agent execution patterns
//!
//! # 架构
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         Agent 执行模式                                   │
//! │                         Agent Execution Modes                           │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │                                                                         │
//! │  Chain (链式模式)                                                        │
//! │  Chain (Sequential Mode)                                                │
//! │  ┌─────┐    ┌─────┐    ┌─────┐    ┌─────┐                               │
//! │  │Agent│───▶│Agent│───▶│Agent│───▶│Agent│                             │
//! │  │  1  │    │  2  │    │  3  │    │  N  │                               │
//! │  └─────┘    └─────┘    └─────┘    └─────┘                               │
//! │    input     output     output     output                               │
//! │              =input     =input     =final                               │
//! │                                                                         │
//! │  Parallel (并行模式)                                                     │
//! │  Parallel (Concurrent Mode)                                             │
//! │              ┌─────┐                                                    │
//! │           ┌─▶│Agent│──┐                                                 │
//! │           │  │  1  │  │                                                 │
//! │           │  └─────┘  │                                                 │
//! │  ┌─────┐  │  ┌─────┐  │  ┌──────────┐    ┌─────┐                        │
//! │  │Input│──┼─▶│Agent│──┼─▶│Aggregator│───▶│Output│                     │
//! │  └─────┘  │  │  2  │  │  └──────────┘    └─────┘                        │
//! │           │  └─────┘  │                                                 │
//! │           │  ┌─────┐  │                                                 │
//! │           └─▶│Agent│──┘                                                │
//! │              │  N  │                                                    │
//! │              └─────┘                                                    │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # 示例
//! # Examples
//!
//! ## Chain 模式
//! ## Chain Mode
//!
//! ```rust,ignore
//! use mofa_foundation::react::{ChainAgent, ReActAgent};
//!
//! // 创建链式 Agent
//! // Create a Chain Agent
//! let chain = ChainAgent::new()
//!     .add("researcher", researcher_agent)
//!     .add("writer", writer_agent)
//!     .add("editor", editor_agent)
//!     .with_transform(|prev_output, _next_name| {
//!         format!("Based on this: {}\n\nPlease continue.", prev_output)
//!     });
//!
//! let result = chain.run("Write an article about Rust").await?;
//! ```
//!
//! ## Parallel 模式
//! ## Parallel Mode
//!
//! ```rust,ignore
//! use mofa_foundation::react::{ParallelAgent, AggregationStrategy};
//!
//! // 创建并行 Agent
//! // Create a Parallel Agent
//! let parallel = ParallelAgent::new()
//!     .add("analyst1", analyst1_agent)
//!     .add("analyst2", analyst2_agent)
//!     .add("analyst3", analyst3_agent)
//!     .with_aggregation(AggregationStrategy::LLMSummarize(summarizer_agent));
//!
//! let result = parallel.run("Analyze market trends").await?;
//! ```

use super::core::ReActResult;
use crate::llm::{LLMAgent, LLMError, LLMResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::Instrument;

/// Type alias for mapper function in MapReduceAgent
pub type MapFunction = Arc<dyn Fn(&str) -> Vec<String> + Send + Sync>;

// ============================================================================
// 通用类型
// General Types
// ============================================================================

/// Agent 执行单元
/// Agent Execution Unit
///
/// 包装 ReActAgent 或 LLMAgent，提供统一的执行接口
/// Wraps ReActAgent or LLMAgent to provide a unified interface
#[derive(Clone)]
pub enum AgentUnit {
    /// ReAct Agent
    /// ReAct Agent
    ReAct(Arc<super::ReActAgent>),
    /// LLM Agent (简单问答)
    /// LLM Agent (Simple QA)
    LLM(Arc<LLMAgent>),
}

impl AgentUnit {
    /// 从 ReActAgent 创建
    /// Create from ReActAgent
    pub fn react(agent: Arc<super::ReActAgent>) -> Self {
        Self::ReAct(agent)
    }

    /// 从 LLMAgent 创建
    /// Create from LLMAgent
    pub fn llm(agent: Arc<LLMAgent>) -> Self {
        Self::LLM(agent)
    }

    /// 执行任务
    /// Execute Task
    pub async fn run(&self, task: impl Into<String>) -> LLMResult<AgentOutput> {
        let task = task.into();
        let start = std::time::Instant::now();

        match self {
            AgentUnit::ReAct(agent) => {
                let result = agent.run(&task).await?;
                Ok(AgentOutput {
                    content: result.answer.clone(),
                    task,
                    success: result.success,
                    error: result.error.clone(),
                    duration_ms: result.duration_ms,
                    metadata: Some(AgentOutputMetadata::ReAct(result)),
                })
            }
            AgentUnit::LLM(agent) => {
                let response = agent.ask(&task).await?;
                Ok(AgentOutput {
                    content: response,
                    task,
                    success: true,
                    error: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                    metadata: None,
                })
            }
        }
    }
}

/// Agent 输出
/// Agent Output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutput {
    /// 输出内容
    /// Output content
    pub content: String,
    /// 原始任务
    /// Original task
    pub task: String,
    /// 是否成功
    /// Success status
    pub success: bool,
    /// 错误信息
    /// Error message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 执行耗时 (毫秒)
    /// Execution time (ms)
    pub duration_ms: u64,
    /// 额外元数据
    /// Additional metadata
    #[serde(skip)]
    pub metadata: Option<AgentOutputMetadata>,
}

/// Agent 输出元数据
/// Agent Output Metadata
#[derive(Debug, Clone)]
pub enum AgentOutputMetadata {
    /// ReAct 执行结果
    /// ReAct execution result
    ReAct(ReActResult),
}

// ============================================================================
// Chain Agent (链式模式)
// Chain Agent (Sequential Mode)
// ============================================================================

/// 链式 Agent 执行模式
/// Chain Agent Execution Mode
///
/// 多个 Agent 串行执行，前一个的输出作为后一个的输入
/// Multiple agents execute serially; output of one is input for the next
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// let chain = ChainAgent::new()
///     .add("step1", agent1)
///     .add("step2", agent2)
///     .add("step3", agent3);
///
/// let result = chain.run("Initial task").await?;
/// ```
pub struct ChainAgent {
    /// Agent 列表 (保持插入顺序)
    /// Agent list (maintains insertion order)
    agents: Vec<(String, AgentUnit)>,
    /// 输入转换函数
    /// Input transformation function
    transform: Option<TransformFn>,
    /// 是否在失败时继续
    /// Whether to continue on error
    continue_on_error: bool,
    /// 是否详细输出
    /// Enable verbose output
    verbose: bool,
    /// 超时时间 (毫秒)
    /// Timeout in milliseconds
    timeout_ms: Option<u64>,
}

/// 输入转换函数类型
/// Input transformation function type
type TransformFn = Arc<dyn Fn(&str, &str) -> String + Send + Sync>;

impl ChainAgent {
    /// 创建新的链式 Agent
    /// Create a new Chain Agent
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            transform: None,
            continue_on_error: false,
            verbose: true,
            timeout_ms: None,
        }
    }

    /// 添加 ReAct Agent 到链中
    /// Add ReAct Agent to the chain
    pub fn add(mut self, name: impl Into<String>, agent: Arc<super::ReActAgent>) -> Self {
        self.agents.push((name.into(), AgentUnit::react(agent)));
        self
    }

    /// 添加 LLM Agent 到链中
    /// Add LLM Agent to the chain
    pub fn add_llm(mut self, name: impl Into<String>, agent: Arc<LLMAgent>) -> Self {
        self.agents.push((name.into(), AgentUnit::llm(agent)));
        self
    }

    /// 添加通用 AgentUnit
    /// Add generic AgentUnit
    pub fn add_unit(mut self, name: impl Into<String>, unit: AgentUnit) -> Self {
        self.agents.push((name.into(), unit));
        self
    }

    /// 设置输入转换函数
    /// Set input transformation function
    ///
    /// 转换函数接收前一个 Agent 的输出和下一个 Agent 的名称，返回转换后的输入
    /// Receives previous output and next agent name, returns transformed input
    ///
    /// # 示例
    /// # Example
    ///
    /// ```rust,ignore
    /// chain.with_transform(|prev_output, next_name| {
    ///     format!("Previous result: {}\n\nTask for {}: continue the analysis", prev_output, next_name)
    /// })
    /// ```
    pub fn with_transform<F>(mut self, f: F) -> Self
    where
        F: Fn(&str, &str) -> String + Send + Sync + 'static,
    {
        self.transform = Some(Arc::new(f));
        self
    }

    /// 设置是否在失败时继续执行
    /// Set whether to continue execution on failure
    pub fn with_continue_on_error(mut self, continue_on_error: bool) -> Self {
        self.continue_on_error = continue_on_error;
        self
    }

    /// 设置是否详细输出
    /// Set verbose output mode
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// 设置超时时间
    /// Set timeout duration
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// 执行链式 Agent
    /// Run Chain Agent
    pub async fn run(&self, initial_task: impl Into<String>) -> LLMResult<ChainResult> {
        let initial_task = initial_task.into();
        let start_time = std::time::Instant::now();
        let chain_id = uuid::Uuid::now_v7().to_string();

        let mut step_results = Vec::new();
        let mut current_input = initial_task.clone();
        let mut final_output = String::new();
        let mut all_success = true;

        for (idx, (name, agent)) in self.agents.iter().enumerate() {
            if self.verbose {
                tracing::info!("[Chain] Step {}: {} - Starting", idx + 1, name);
            }

            // 执行 Agent
            // Execute Agent
            let result = if let Some(t_ms) = self.timeout_ms {
                match tokio::time::timeout(
                    std::time::Duration::from_millis(t_ms),
                    agent.run(&current_input),
                )
                .await
                {
                    Ok(res) => res,
                    Err(_) => Err(crate::llm::types::LLMError::Timeout(format!(
                        "Agent '{}' timed out after {}ms in chain",
                        name, t_ms
                    ))),
                }
            } else {
                agent.run(&current_input).await
            };

            match result {
                Ok(output) => {
                    if self.verbose {
                        tracing::info!(
                            "[Chain] Step {}: {} - Completed in {}ms",
                            idx + 1,
                            name,
                            output.duration_ms
                        );
                    }

                    step_results.push(ChainStepResult {
                        step: idx + 1,
                        agent_name: name.clone(),
                        input: current_input.clone(),
                        output: output.clone(),
                        success: output.success,
                    });

                    if !output.success {
                        all_success = false;
                        if !self.continue_on_error {
                            return Ok(ChainResult {
                                chain_id,
                                initial_task,
                                final_output: output.content.clone(),
                                steps: step_results,
                                success: false,
                                error: output.error,
                                total_duration_ms: start_time.elapsed().as_millis() as u64,
                            });
                        }
                    }

                    final_output = output.content.clone();

                    // 转换输入给下一个 Agent
                    // Transform input for the next Agent
                    if idx < self.agents.len() - 1 {
                        let next_name = &self.agents[idx + 1].0;
                        current_input = if let Some(ref transform) = self.transform {
                            transform(&output.content, next_name)
                        } else {
                            output.content.clone()
                        };
                    }
                }
                Err(e) => {
                    all_success = false;
                    step_results.push(ChainStepResult {
                        step: idx + 1,
                        agent_name: name.clone(),
                        input: current_input.clone(),
                        output: AgentOutput {
                            content: String::new(),
                            task: current_input.clone(),
                            success: false,
                            error: Some(e.to_string()),
                            duration_ms: 0,
                            metadata: None,
                        },
                        success: false,
                    });

                    if !self.continue_on_error {
                        return Ok(ChainResult {
                            chain_id,
                            initial_task,
                            final_output: String::new(),
                            steps: step_results,
                            success: false,
                            error: Some(e.to_string()),
                            total_duration_ms: start_time.elapsed().as_millis() as u64,
                        });
                    }
                }
            }
        }

        Ok(ChainResult {
            chain_id,
            initial_task,
            final_output,
            steps: step_results,
            success: all_success,
            error: None,
            total_duration_ms: start_time.elapsed().as_millis() as u64,
        })
    }

    /// 获取链中的 Agent 数量
    /// Get the number of agents in the chain
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// 检查链是否为空
    /// Check if the chain is empty
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

impl Default for ChainAgent {
    fn default() -> Self {
        Self::new()
    }
}

/// 链式执行结果
/// Chain execution results
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainResult {
    /// 链 ID
    /// Chain ID
    pub chain_id: String,
    /// 初始任务
    /// Initial task
    pub initial_task: String,
    /// 最终输出
    /// Final output
    pub final_output: String,
    /// 各步骤结果
    /// Results of each step
    pub steps: Vec<ChainStepResult>,
    /// 是否全部成功
    /// Overall success status
    pub success: bool,
    /// 错误信息
    /// Error message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 总耗时 (毫秒)
    /// Total duration (ms)
    pub total_duration_ms: u64,
}

impl ChainResult {
    /// 获取指定步骤的结果
    /// Get result of a specific step
    pub fn get_step(&self, step: usize) -> Option<&ChainStepResult> {
        self.steps.get(step.saturating_sub(1))
    }

    /// 获取指定 Agent 的结果
    /// Get result by Agent name
    pub fn get_by_name(&self, name: &str) -> Option<&ChainStepResult> {
        self.steps.iter().find(|s| s.agent_name == name)
    }
}

/// 链式执行步骤结果
/// Chain step execution result
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainStepResult {
    /// 步骤序号
    /// Step index
    pub step: usize,
    /// Agent 名称
    /// Agent name
    pub agent_name: String,
    /// 输入
    /// Input
    pub input: String,
    /// 输出
    /// Output
    pub output: AgentOutput,
    /// 是否成功
    /// Success status
    pub success: bool,
}

// ============================================================================
// Parallel Agent (并行模式)
// Parallel Agent (Concurrent Mode)
// ============================================================================

/// 并行 Agent 执行模式
/// Parallel Agent Execution Mode
///
/// 多个 Agent 并行执行同一任务，然后聚合结果
/// Multiple agents run the same task concurrently, then results aggregate
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// let parallel = ParallelAgent::new()
///     .add("expert1", agent1)
///     .add("expert2", agent2)
///     .add("expert3", agent3)
///     .with_aggregation(AggregationStrategy::Concatenate);
///
/// let result = parallel.run("Analyze this problem").await?;
/// ```
pub struct ParallelAgent {
    /// Agent 列表
    /// Agent list
    agents: Vec<(String, AgentUnit)>,
    /// 聚合策略
    /// Aggregation strategy
    aggregation: AggregationStrategy,
    /// 是否在有失败时仍继续聚合
    /// Whether to aggregate on partial failures
    aggregate_on_partial_failure: bool,
    /// 超时时间 (毫秒)
    /// Timeout in milliseconds
    timeout_ms: Option<u64>,
    /// 是否详细输出
    /// Enable verbose output
    verbose: bool,
    /// 任务模板 (可为不同 Agent 定制任务)
    /// Task templates (for agent-specific tasks)
    task_templates: HashMap<String, String>,
}

/// 聚合策略
/// Aggregation Strategy
#[derive(Clone)]
pub enum AggregationStrategy {
    /// 简单拼接所有输出
    /// Simple concatenation of all outputs
    Concatenate,
    /// 使用分隔符拼接
    /// Join with a specific separator
    ConcatenateWithSeparator(String),
    /// 返回第一个成功的结果
    /// Return the first successful result
    FirstSuccess,
    /// 返回所有结果 (JSON 格式)
    /// Collect all results as JSON
    CollectAll,
    /// 投票选择 (适用于分类任务)
    /// Majority voting (for classification)
    Vote,
    /// 使用 LLM 总结聚合
    /// Use LLM to summarize and aggregate
    LLMSummarize(Arc<LLMAgent>),
    /// 自定义聚合函数
    /// Custom aggregation function
    Custom(Arc<dyn Fn(Vec<ParallelStepResult>) -> String + Send + Sync>),
}

impl ParallelAgent {
    /// 创建新的并行 Agent
    /// Create a new Parallel Agent
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            aggregation: AggregationStrategy::Concatenate,
            aggregate_on_partial_failure: true,
            timeout_ms: None,
            verbose: true,
            task_templates: HashMap::new(),
        }
    }

    /// 添加 ReAct Agent
    /// Add ReAct Agent
    pub fn add(mut self, name: impl Into<String>, agent: Arc<super::ReActAgent>) -> Self {
        self.agents.push((name.into(), AgentUnit::react(agent)));
        self
    }

    /// 添加 LLM Agent
    /// Add LLM Agent
    pub fn add_llm(mut self, name: impl Into<String>, agent: Arc<LLMAgent>) -> Self {
        self.agents.push((name.into(), AgentUnit::llm(agent)));
        self
    }

    /// 添加通用 AgentUnit
    /// Add generic AgentUnit
    pub fn add_unit(mut self, name: impl Into<String>, unit: AgentUnit) -> Self {
        self.agents.push((name.into(), unit));
        self
    }

    /// 设置聚合策略
    /// Set aggregation strategy
    pub fn with_aggregation(mut self, strategy: AggregationStrategy) -> Self {
        self.aggregation = strategy;
        self
    }

    /// 设置是否在部分失败时仍聚合
    /// Set whether to aggregate on partial failure
    pub fn with_aggregate_on_partial_failure(mut self, enabled: bool) -> Self {
        self.aggregate_on_partial_failure = enabled;
        self
    }

    /// 设置超时时间
    /// Set timeout duration
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// 设置是否详细输出
    /// Set verbose output mode
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// 设置特定 Agent 的任务模板
    /// Set task template for specific Agent
    ///
    /// 模板中可使用 `{task}` 占位符表示原始任务
    /// Use `{task}` placeholder for the original task
    ///
    /// # 示例
    /// # Example
    ///
    /// ```rust,ignore
    /// parallel.with_task_template("analyst", "As a financial analyst, {task}");
    /// ```
    pub fn with_task_template(
        mut self,
        agent_name: impl Into<String>,
        template: impl Into<String>,
    ) -> Self {
        self.task_templates
            .insert(agent_name.into(), template.into());
        self
    }

    /// 执行并行 Agent
    /// Run Parallel Agent
    pub async fn run(&self, task: impl Into<String>) -> LLMResult<ParallelResult> {
        let task = task.into();
        let start_time = std::time::Instant::now();
        let parallel_id = uuid::Uuid::now_v7().to_string();

        if self.verbose {
            tracing::info!("[Parallel] Starting {} agents for task", self.agents.len());
        }

        // 准备所有任务
        // Prepare all tasks
        let mut handles = Vec::new();

        for (name, agent) in &self.agents {
            let name = name.clone();
            let agent = agent.clone();
            let task_input = self.prepare_task(&name, &task);
            let verbose = self.verbose;

            let span = tracing::info_span!("parallel_agent.branch", agent_name = %name);
            let handle = tokio::spawn(
                async move {
                    if verbose {
                        tracing::info!("[Parallel] Agent '{}' starting", name);
                    }

                    let result = agent.run(&task_input).await;

                    if verbose {
                        match &result {
                            Ok(output) => {
                                tracing::info!(
                                    "[Parallel] Agent '{}' completed in {}ms",
                                    name,
                                    output.duration_ms
                                );
                            }
                            Err(e) => {
                                tracing::warn!("[Parallel] Agent '{}' failed: {}", name, e);
                            }
                        }
                    }

                    (name, task_input, result)
                }
                .instrument(span),
            );

            handles.push(handle);
        }

        // 等待所有任务完成
        // Wait for all tasks to complete
        let mut step_results = Vec::new();
        let mut all_success = true;

        for handle in handles {
            match handle.await {
                Ok((name, input, result)) => match result {
                    Ok(output) => {
                        if !output.success {
                            all_success = false;
                        }
                        step_results.push(ParallelStepResult {
                            agent_name: name,
                            input,
                            output,
                            success: true,
                        });
                    }
                    Err(e) => {
                        all_success = false;
                        step_results.push(ParallelStepResult {
                            agent_name: name,
                            input,
                            output: AgentOutput {
                                content: String::new(),
                                task: task.clone(),
                                success: false,
                                error: Some(e.to_string()),
                                duration_ms: 0,
                                metadata: None,
                            },
                            success: false,
                        });
                    }
                },
                Err(e) => {
                    all_success = false;
                    step_results.push(ParallelStepResult {
                        agent_name: "unknown".to_string(),
                        input: task.clone(),
                        output: AgentOutput {
                            content: String::new(),
                            task: task.clone(),
                            success: false,
                            error: Some(format!("Task join error: {}", e)),
                            duration_ms: 0,
                            metadata: None,
                        },
                        success: false,
                    });
                }
            }
        }

        // 聚合结果
        // Aggregate results
        let aggregated_output = if all_success || self.aggregate_on_partial_failure {
            self.aggregate(&step_results).await?
        } else {
            String::new()
        };

        Ok(ParallelResult {
            parallel_id,
            task,
            aggregated_output,
            individual_results: step_results,
            success: all_success,
            total_duration_ms: start_time.elapsed().as_millis() as u64,
        })
    }

    /// 准备任务输入
    /// Prepare task input
    fn prepare_task(&self, agent_name: &str, task: &str) -> String {
        if let Some(template) = self.task_templates.get(agent_name) {
            template.replace("{task}", task)
        } else {
            task.to_string()
        }
    }

    /// 聚合结果
    /// Aggregate results
    async fn aggregate(&self, results: &[ParallelStepResult]) -> LLMResult<String> {
        let successful_results: Vec<&ParallelStepResult> =
            results.iter().filter(|r| r.success).collect();

        match &self.aggregation {
            AggregationStrategy::Concatenate => {
                let outputs: Vec<String> = successful_results
                    .iter()
                    .map(|r| format!("[{}]\n{}", r.agent_name, r.output.content))
                    .collect();
                Ok(outputs.join("\n\n"))
            }

            AggregationStrategy::ConcatenateWithSeparator(sep) => {
                let outputs: Vec<String> = successful_results
                    .iter()
                    .map(|r| format!("[{}]\n{}", r.agent_name, r.output.content))
                    .collect();
                Ok(outputs.join(sep))
            }

            AggregationStrategy::FirstSuccess => Ok(successful_results
                .first()
                .map(|r| r.output.content.clone())
                .unwrap_or_default()),

            AggregationStrategy::CollectAll => {
                let collected: Vec<serde_json::Value> = results
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "agent": r.agent_name,
                            "success": r.success,
                            "output": r.output.content,
                            "error": r.output.error,
                        })
                    })
                    .collect();
                Ok(serde_json::to_string_pretty(&collected).unwrap_or_else(|_| "[]".to_string()))
            }

            AggregationStrategy::Vote => {
                // 简单投票：统计相同输出的数量
                // Simple voting: count identical outputs
                let mut votes: HashMap<String, usize> = HashMap::new();
                for result in &successful_results {
                    let content = result.output.content.trim().to_lowercase();
                    *votes.entry(content).or_insert(0) += 1;
                }

                let winner = votes
                    .into_iter()
                    .max_by_key(|(_, count)| *count)
                    .map(|(content, _)| content)
                    .unwrap_or_default();

                // 返回原始大小写版本
                // Return original casing version
                Ok(successful_results
                    .iter()
                    .find(|r| r.output.content.trim().to_lowercase() == winner)
                    .map(|r| r.output.content.clone())
                    .unwrap_or(winner))
            }

            AggregationStrategy::LLMSummarize(llm) => {
                let outputs: Vec<String> = successful_results
                    .iter()
                    .map(|r| format!("Expert '{}' says:\n{}", r.agent_name, r.output.content))
                    .collect();

                let prompt = format!(
                    r#"You are tasked with synthesizing multiple expert opinions into a coherent summary.

Here are the expert opinions:

{}

Please provide a comprehensive synthesis that:
1. Identifies common themes and agreements
2. Notes any significant disagreements
3. Provides a balanced conclusion

Synthesized Summary:"#,
                    outputs.join("\n\n---\n\n")
                );

                llm.ask(&prompt).await
            }

            AggregationStrategy::Custom(f) => Ok(f(results.to_vec())),
        }
    }

    /// 获取 Agent 数量
    /// Get Agent count
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// 检查是否为空
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

impl Default for ParallelAgent {
    fn default() -> Self {
        Self::new()
    }
}

/// 并行执行结果
/// Parallel execution results
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelResult {
    /// 并行执行 ID
    /// Parallel ID
    pub parallel_id: String,
    /// 原始任务
    /// Original task
    pub task: String,
    /// 聚合后的输出
    /// Aggregated output
    pub aggregated_output: String,
    /// 各 Agent 的单独结果
    /// Individual results for each agent
    pub individual_results: Vec<ParallelStepResult>,
    /// 是否全部成功
    /// Overall success status
    pub success: bool,
    /// 总耗时 (毫秒)
    /// Total duration (ms)
    pub total_duration_ms: u64,
}

impl ParallelResult {
    /// 获取成功的结果数量
    /// Get count of successful results
    pub fn success_count(&self) -> usize {
        self.individual_results.iter().filter(|r| r.success).count()
    }

    /// 获取失败的结果数量
    /// Get count of failed results
    pub fn failure_count(&self) -> usize {
        self.individual_results
            .iter()
            .filter(|r| !r.success)
            .count()
    }

    /// 获取指定 Agent 的结果
    /// Get result by Agent name
    pub fn get_by_name(&self, name: &str) -> Option<&ParallelStepResult> {
        self.individual_results
            .iter()
            .find(|r| r.agent_name == name)
    }
}

/// 并行执行步骤结果
/// Parallel step execution result
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelStepResult {
    /// Agent 名称
    /// Agent name
    pub agent_name: String,
    /// 输入任务
    /// Input task
    pub input: String,
    /// 输出结果
    /// Output result
    pub output: AgentOutput,
    /// 是否成功
    /// Success status
    pub success: bool,
}

// ============================================================================
// 便捷构建函数
// Helper Construction Functions
// ============================================================================

/// 创建简单的链式 Agent
/// Create a simple Chain Agent
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// let chain = chain_agents(vec![
///     ("researcher", researcher_agent),
///     ("writer", writer_agent),
///     ("editor", editor_agent),
/// ]);
/// ```
pub fn chain_agents(agents: Vec<(&str, Arc<super::ReActAgent>)>) -> ChainAgent {
    let mut chain = ChainAgent::new();
    for (name, agent) in agents {
        chain = chain.add(name, agent);
    }
    chain
}

/// 创建简单的并行 Agent
/// Create a simple Parallel Agent
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// let parallel = parallel_agents(vec![
///     ("analyst1", analyst1_agent),
///     ("analyst2", analyst2_agent),
/// ]);
/// ```
pub fn parallel_agents(agents: Vec<(&str, Arc<super::ReActAgent>)>) -> ParallelAgent {
    let mut parallel = ParallelAgent::new();
    for (name, agent) in agents {
        parallel = parallel.add(name, agent);
    }
    parallel
}

/// 创建带 LLM 聚合的并行 Agent
/// Create a Parallel Agent with LLM summarizer
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// let parallel = parallel_agents_with_summarizer(
///     vec![
///         ("expert1", expert1_agent),
///         ("expert2", expert2_agent),
///     ],
///     summarizer_llm,
/// );
/// ```
pub fn parallel_agents_with_summarizer(
    agents: Vec<(&str, Arc<super::ReActAgent>)>,
    summarizer: Arc<LLMAgent>,
) -> ParallelAgent {
    parallel_agents(agents).with_aggregation(AggregationStrategy::LLMSummarize(summarizer))
}

// ============================================================================
// MapReduce 模式
// MapReduce Pattern
// ============================================================================

/// MapReduce Agent
/// MapReduce Agent
///
/// 将任务拆分、并行处理、然后归约结果
/// Splits tasks, processes them in parallel, then reduces results
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// let map_reduce = MapReduceAgent::new()
///     .with_mapper(|task| {
///         // 拆分任务为多个子任务
///         // Split task into multiple sub-tasks
///         task.split('\n').map(|s| s.to_string()).collect()
///     })
///     .with_worker(worker_agent)
///     .with_reducer(reducer_agent);
///
/// let result = map_reduce.run("line1\nline2\nline3").await?;
/// ```
pub struct MapReduceAgent {
    /// Map 函数 - 将输入拆分为多个子任务
    /// Map function - splits input into multiple sub-tasks
    mapper: Option<MapFunction>,
    /// 工作 Agent (处理子任务)
    /// Worker Agent (processes sub-tasks)
    worker: Option<AgentUnit>,
    /// Reduce Agent (聚合结果)
    /// Reduce Agent (aggregates results)
    reducer: Option<AgentUnit>,
    /// 并行度限制
    /// Concurrency limit
    concurrency_limit: Option<usize>,
    /// 是否详细输出
    /// Whether to use verbose output
    verbose: bool,
}

impl MapReduceAgent {
    /// 创建新的 MapReduce Agent
    /// Create a new MapReduce Agent
    pub fn new() -> Self {
        Self {
            mapper: None,
            worker: None,
            reducer: None,
            concurrency_limit: None,
            verbose: true,
        }
    }

    /// 设置 Map 函数
    /// Set the Map function
    pub fn with_mapper<F>(mut self, f: F) -> Self
    where
        F: Fn(&str) -> Vec<String> + Send + Sync + 'static,
    {
        self.mapper = Some(Arc::new(f));
        self
    }

    /// 设置工作 Agent (ReAct)
    /// Set the worker Agent (ReAct)
    pub fn with_worker(mut self, agent: Arc<super::ReActAgent>) -> Self {
        self.worker = Some(AgentUnit::react(agent));
        self
    }

    /// 设置工作 Agent (LLM)
    /// Set the worker Agent (LLM)
    pub fn with_worker_llm(mut self, agent: Arc<LLMAgent>) -> Self {
        self.worker = Some(AgentUnit::llm(agent));
        self
    }

    /// 设置 Reduce Agent (ReAct)
    /// Set the reduce Agent (ReAct)
    pub fn with_reducer(mut self, agent: Arc<super::ReActAgent>) -> Self {
        self.reducer = Some(AgentUnit::react(agent));
        self
    }

    /// 设置 Reduce Agent (LLM)
    /// Set the reduce Agent (LLM)
    pub fn with_reducer_llm(mut self, agent: Arc<LLMAgent>) -> Self {
        self.reducer = Some(AgentUnit::llm(agent));
        self
    }

    /// 设置并行度限制
    /// Set the concurrency limit
    pub fn with_concurrency_limit(mut self, limit: usize) -> Self {
        self.concurrency_limit = Some(limit);
        self
    }

    /// 设置是否详细输出
    /// Set whether to use verbose output
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// 执行 MapReduce
    /// Execute MapReduce
    pub async fn run(&self, input: impl Into<String>) -> LLMResult<MapReduceResult> {
        let input = input.into();
        let start_time = std::time::Instant::now();
        let mr_id = uuid::Uuid::now_v7().to_string();

        // Map 阶段
        // Map phase
        let mapper = self
            .mapper
            .as_ref()
            .ok_or_else(|| LLMError::ConfigError("Mapper not set".to_string()))?;

        let sub_tasks = mapper(&input);

        if self.verbose {
            tracing::info!("[MapReduce] Mapped to {} sub-tasks", sub_tasks.len());
        }

        // 并行处理阶段
        // Parallel processing phase
        let worker = self
            .worker
            .as_ref()
            .ok_or_else(|| LLMError::ConfigError("Worker not set".to_string()))?;

        let mut handles = Vec::new();
        let semaphore = self
            .concurrency_limit
            .map(|limit| Arc::new(tokio::sync::Semaphore::new(limit)));

        for (idx, sub_task) in sub_tasks.into_iter().enumerate() {
            let worker = worker.clone();
            let semaphore = semaphore.clone();
            let verbose = self.verbose;

            let span = tracing::info_span!("map_reduce.worker", sub_task_idx = idx);
            let handle = tokio::spawn(
                async move {
                    let _permit = if let Some(ref sem) = semaphore {
                        Some(sem.acquire().await)
                    } else {
                        None
                    };

                    if verbose {
                        tracing::info!("[MapReduce] Processing sub-task {}", idx + 1);
                    }

                    let result = worker.run(&sub_task).await;

                    if verbose {
                        match &result {
                            Ok(_) => tracing::info!("[MapReduce] Sub-task {} completed", idx + 1),
                            Err(e) => {
                                tracing::warn!("[MapReduce] Sub-task {} failed: {}", idx + 1, e)
                            }
                        }
                    }

                    (idx, sub_task, result)
                }
                .instrument(span),
            );

            handles.push(handle);
        }

        // 收集结果
        // Collect results
        let mut map_results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok((idx, sub_task, result)) => {
                    map_results.push(MapStepResult {
                        index: idx,
                        input: sub_task,
                        output: result.ok(),
                    });
                }
                Err(e) => {
                    map_results.push(MapStepResult {
                        index: map_results.len(),
                        input: String::new(),
                        output: None,
                    });
                    tracing::error!("[MapReduce] Task join error: {}", e);
                }
            }
        }

        // 按索引排序
        // Sort by index
        map_results.sort_by_key(|r| r.index);

        // Reduce 阶段
        // Reduce phase
        let reducer = self
            .reducer
            .as_ref()
            .ok_or_else(|| LLMError::ConfigError("Reducer not set".to_string()))?;

        let map_outputs: Vec<String> = map_results
            .iter()
            .filter_map(|r| r.output.as_ref().map(|o| o.content.clone()))
            .collect();

        let reduce_input = format!(
            "Please synthesize the following {} results:\n\n{}",
            map_outputs.len(),
            map_outputs
                .iter()
                .enumerate()
                .map(|(i, o)| format!("[Result {}]\n{}", i + 1, o))
                .collect::<Vec<_>>()
                .join("\n\n---\n\n")
        );

        if self.verbose {
            tracing::info!("[MapReduce] Starting reduce phase");
        }

        let reduce_output = reducer.run(&reduce_input).await?;

        Ok(MapReduceResult {
            mr_id,
            input,
            map_results,
            reduce_output,
            total_duration_ms: start_time.elapsed().as_millis() as u64,
        })
    }
}

impl Default for MapReduceAgent {
    fn default() -> Self {
        Self::new()
    }
}

/// MapReduce 执行结果
/// MapReduce execution result
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapReduceResult {
    /// MapReduce ID
    /// MapReduce ID
    pub mr_id: String,
    /// 原始输入
    /// Original input
    pub input: String,
    /// Map 阶段结果
    /// Map phase results
    pub map_results: Vec<MapStepResult>,
    /// Reduce 阶段输出
    /// Reduce phase output
    pub reduce_output: AgentOutput,
    /// 总耗时 (毫秒)
    /// Total duration (milliseconds)
    pub total_duration_ms: u64,
}

/// Map 步骤结果
/// Map step result
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapStepResult {
    /// 索引
    /// Index
    pub index: usize,
    /// 输入
    /// Input
    pub input: String,
    /// 输出
    /// Output
    pub output: Option<AgentOutput>,
}

// ============================================================================
// 测试
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_agent_builder() {
        let chain = ChainAgent::new()
            .with_continue_on_error(true)
            .with_timeout_ms(1000)
            .with_verbose(false);

        assert!(chain.is_empty());
        assert!(!chain.verbose);
        assert!(chain.continue_on_error);
        assert_eq!(chain.timeout_ms, Some(1000));
    }

    #[test]
    fn test_chain_agent_timeout_config() {
        let chain = ChainAgent::new()
            .with_continue_on_error(true)
            .with_timeout_ms(1500)
            .with_verbose(false);

        assert!(chain.is_empty());
        assert_eq!(chain.timeout_ms, Some(1500));
        assert!(chain.continue_on_error);
    }

    #[test]
    fn test_parallel_agent_builder() {
        let parallel = ParallelAgent::new()
            .with_aggregation(AggregationStrategy::Concatenate)
            .with_timeout_ms(5000)
            .with_verbose(false);

        assert!(parallel.is_empty());
        assert!(!parallel.verbose);
    }

    #[test]
    fn test_map_reduce_builder() {
        let mr = MapReduceAgent::new()
            .with_mapper(|s| s.lines().map(|l| l.to_string()).collect())
            .with_concurrency_limit(4);

        assert!(mr.mapper.is_some());
        assert_eq!(mr.concurrency_limit, Some(4));
    }
}
