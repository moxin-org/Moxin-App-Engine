//! ReAct 核心类型和逻辑
//! ReAct core types and logic

use crate::llm::{LLMAgent, LLMError, LLMResult, Tool};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::Instrument;

/// ReAct 步骤类型
/// ReAct step types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReActStepType {
    /// 思考步骤
    /// Reasoning step
    Thought,
    /// 行动步骤
    /// Execution step
    Action,
    /// 观察步骤 (工具执行结果)
    /// Observation step (tool execution result)
    Observation,
    /// 最终答案
    /// Final answer
    FinalAnswer,
}

/// ReAct 执行步骤
/// ReAct execution step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReActStep {
    /// 步骤类型
    /// Type of the step
    pub step_type: ReActStepType,
    /// 步骤内容
    /// Content of the step
    pub content: String,
    /// 使用的工具名称 (仅 Action 步骤)
    /// Used tool name (Action steps only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// 工具输入 (仅 Action 步骤)
    /// Tool input (Action steps only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<String>,
    /// 步骤序号
    /// Step sequence number
    pub step_number: usize,
    /// 时间戳 (毫秒)
    /// Timestamp (milliseconds)
    pub timestamp: u64,
}

impl ReActStep {
    pub fn thought(content: impl Into<String>, step_number: usize) -> Self {
        Self {
            step_type: ReActStepType::Thought,
            content: content.into(),
            tool_name: None,
            tool_input: None,
            step_number,
            timestamp: Self::current_timestamp(),
        }
    }

    pub fn action(
        tool_name: impl Into<String>,
        tool_input: impl Into<String>,
        step_number: usize,
    ) -> Self {
        let tool_name = tool_name.into();
        let tool_input = tool_input.into();
        Self {
            step_type: ReActStepType::Action,
            content: format!("Action: {}[{}]", tool_name, tool_input),
            tool_name: Some(tool_name),
            tool_input: Some(tool_input),
            step_number,
            timestamp: Self::current_timestamp(),
        }
    }

    pub fn observation(content: impl Into<String>, step_number: usize) -> Self {
        Self {
            step_type: ReActStepType::Observation,
            content: content.into(),
            tool_name: None,
            tool_input: None,
            step_number,
            timestamp: Self::current_timestamp(),
        }
    }

    pub fn final_answer(content: impl Into<String>, step_number: usize) -> Self {
        Self {
            step_type: ReActStepType::FinalAnswer,
            content: content.into(),
            tool_name: None,
            tool_input: None,
            step_number,
            timestamp: Self::current_timestamp(),
        }
    }

    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

/// ReAct 执行结果
/// ReAct execution result
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReActResult {
    /// 任务 ID
    /// Task ID
    pub task_id: String,
    /// 原始任务
    /// Original task
    pub task: String,
    /// 最终答案
    /// Final answer
    pub answer: String,
    /// 执行步骤
    /// Execution steps
    pub steps: Vec<ReActStep>,
    /// 是否成功
    /// Whether successful
    pub success: bool,
    /// 错误信息 (如果失败)
    /// Error message (if failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 总迭代次数
    /// Total iterations
    pub iterations: usize,
    /// 总耗时 (毫秒)
    /// Total duration (ms)
    pub duration_ms: u64,
}

impl ReActResult {
    pub fn success(
        task_id: impl Into<String>,
        task: impl Into<String>,
        answer: impl Into<String>,
        steps: Vec<ReActStep>,
        iterations: usize,
        duration_ms: u64,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            task: task.into(),
            answer: answer.into(),
            steps,
            success: true,
            error: None,
            iterations,
            duration_ms,
        }
    }

    pub fn failed(
        task_id: impl Into<String>,
        task: impl Into<String>,
        error: impl Into<String>,
        steps: Vec<ReActStep>,
        iterations: usize,
        duration_ms: u64,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            task: task.into(),
            answer: String::new(),
            steps,
            success: false,
            error: Some(error.into()),
            iterations,
            duration_ms,
        }
    }
}

/// ReAct 工具 trait
/// ReAct tool trait
///
/// 实现此 trait 以创建自定义工具
/// Implement this trait to create custom tools
#[async_trait::async_trait]
pub trait ReActTool: Send + Sync {
    /// 工具名称 (用于 LLM 调用)
    /// Tool name (for LLM calling)
    fn name(&self) -> &str;

    /// 工具描述 (用于 LLM 理解工具功能)
    /// Tool description (for LLM understanding)
    fn description(&self) -> &str;

    /// 参数 JSON Schema (可选)
    /// Parameter JSON Schema (optional)
    fn parameters_schema(&self) -> Option<serde_json::Value> {
        None
    }

    /// 执行工具
    /// Execute the tool
    ///
    /// # 参数
    /// # Parameters
    /// - `input`: 工具输入 (可以是 JSON 字符串或普通文本)
    /// - `input`: Tool input (can be JSON or plain text)
    ///
    /// # 返回
    /// # Returns
    /// 工具执行结果
    /// Tool execution result
    async fn execute(&self, input: &str) -> Result<String, String>;

    /// 转换为 LLM Tool 定义
    /// Convert to LLM Tool definition
    fn to_llm_tool(&self) -> Tool {
        let params = self.parameters_schema().unwrap_or_else(|| {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "The input for the tool"
                    }
                },
                "required": ["input"]
            })
        });

        Tool::function(self.name(), self.description(), params)
    }
}

/// ReAct 配置
/// ReAct configuration
#[derive(Debug, Clone)]
pub struct ReActConfig {
    /// 最大迭代次数
    /// Maximum iterations
    pub max_iterations: usize,
    /// 是否启用流式输出
    /// Enable streaming output
    pub stream_output: bool,
    /// 思考温度
    /// Thinking temperature
    pub temperature: f32,
    /// 自定义系统提示词
    /// Custom system prompt
    pub system_prompt: Option<String>,
    /// 是否在思考过程中显示详细信息
    /// Show verbose info during reasoning
    pub verbose: bool,
    /// 每步最大 token 数
    /// Max tokens per step
    pub max_tokens_per_step: Option<u32>,
    /// Per-tool-call timeout. Default: 30 seconds.
    pub tool_timeout: Duration,
}

impl Default for ReActConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            stream_output: false,
            temperature: 0.7,
            system_prompt: None,
            verbose: true,
            max_tokens_per_step: Some(2048),
            tool_timeout: Duration::from_secs(30),
        }
    }
}

impl ReActConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn with_stream_output(mut self, enabled: bool) -> Self {
        self.stream_output = enabled;
        self
    }

    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = temp;
        self
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }
}

/// ReAct Agent 核心实现
/// ReAct Agent core implementation
pub struct ReActAgent {
    /// LLM Agent
    /// LLM Agent
    llm: Arc<LLMAgent>,
    /// 工具注册表
    /// Tool registry
    tools: Arc<RwLock<HashMap<String, Arc<dyn ReActTool>>>>,
    /// 配置
    /// Configuration
    config: ReActConfig,
}

impl ReActAgent {
    /// 创建构建器
    /// Create builder
    pub fn builder() -> ReActAgentBuilder {
        ReActAgentBuilder::new()
    }

    /// 使用 LLM 和配置创建
    /// Create with LLM and config
    pub fn new(llm: Arc<LLMAgent>, config: ReActConfig) -> Self {
        Self {
            llm,
            tools: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// 使用 LLM、配置和初始工具创建
    /// Create with LLM, config, and initial tools
    pub fn with_tools(
        llm: Arc<LLMAgent>,
        config: ReActConfig,
        tools: HashMap<String, Arc<dyn ReActTool>>,
    ) -> Self {
        Self {
            llm,
            tools: Arc::new(RwLock::new(tools)),
            config,
        }
    }

    /// 注册工具
    /// Register tool
    pub async fn register_tool(&self, tool: Arc<dyn ReActTool>) {
        let mut tools = self.tools.write().await;
        tools.insert(tool.name().to_string(), tool);
    }

    /// 获取所有工具
    /// Get all tools
    pub async fn get_tools(&self) -> Vec<Arc<dyn ReActTool>> {
        let tools = self.tools.read().await;
        tools.values().cloned().collect()
    }

    /// 执行任务
    /// Execute task
    pub async fn run(&self, task: impl Into<String>) -> LLMResult<ReActResult> {
        let task = task.into();
        let task_id = uuid::Uuid::now_v7().to_string();
        let start_time = std::time::Instant::now();

        let mut steps = Vec::new();
        let mut step_number = 0;

        // 构建系统提示词
        // Build system prompt
        let system_prompt = self.build_system_prompt().await;

        // 构建初始消息
        // Build initial messages
        let mut conversation = vec![format!("Task: {}", task)];

        for iteration in 0..self.config.max_iterations {
            step_number += 1;

            // 获取 LLM 响应
            // Get LLM response
            let prompt = self.build_prompt(&system_prompt, &conversation).await;
            let response = self.llm.ask(&prompt).await?;

            // 解析响应
            // Parse response
            let parsed = self.parse_response(&response);

            match parsed {
                ParsedResponse::Thought(thought) => {
                    steps.push(ReActStep::thought(&thought, step_number));
                    conversation.push(format!("Thought: {}", thought));

                    if self.config.verbose {
                        tracing::info!("Thought: {}", thought);
                    }
                }
                ParsedResponse::Action { tool, input } => {
                    steps.push(ReActStep::action(&tool, &input, step_number));
                    conversation.push(format!("Action: {}[{}]", tool, input));

                    if self.config.verbose {
                        tracing::info!("Action: {}[{}]", tool, input);
                    }

                    // 执行工具
                    // Execute tool
                    step_number += 1;
                    let observation = self.execute_tool(&tool, &input).await;
                    steps.push(ReActStep::observation(&observation, step_number));
                    conversation.push(format!("Observation: {}", observation));

                    if self.config.verbose {
                        tracing::info!("Observation: {}", observation);
                    }
                }
                ParsedResponse::FinalAnswer(answer) => {
                    steps.push(ReActStep::final_answer(&answer, step_number));

                    if self.config.verbose {
                        tracing::info!("Final Answer: {}", answer);
                    }

                    return Ok(ReActResult::success(
                        task_id,
                        &task,
                        answer,
                        steps,
                        iteration + 1,
                        start_time.elapsed().as_millis() as u64,
                    ));
                }
                ParsedResponse::Error(err) => {
                    return Ok(ReActResult::failed(
                        task_id,
                        &task,
                        err,
                        steps,
                        iteration + 1,
                        start_time.elapsed().as_millis() as u64,
                    ));
                }
            }
        }

        // 达到最大迭代次数
        // Max iterations reached
        Ok(ReActResult::failed(
            task_id,
            &task,
            format!("Max iterations ({}) exceeded", self.config.max_iterations),
            steps,
            self.config.max_iterations,
            start_time.elapsed().as_millis() as u64,
        ))
    }

    /// 执行任务并实时流式返回步骤
    /// Execute task and stream steps in real-time
    pub async fn run_streaming(
        &self,
        task: impl Into<String>,
        step_tx: tokio::sync::mpsc::Sender<ReActStep>,
    ) -> LLMResult<ReActResult> {
        let task = task.into();
        let task_id = uuid::Uuid::now_v7().to_string();
        let start_time = std::time::Instant::now();

        let mut steps = Vec::new();
        let mut step_number = 0;

        let system_prompt = self.build_system_prompt().await;
        let mut conversation = vec![format!("Task: {}", task)];

        for iteration in 0..self.config.max_iterations {
            step_number += 1;

            let prompt = self.build_prompt(&system_prompt, &conversation).await;
            let response = self.llm.ask(&prompt).await?;
            let parsed = self.parse_response(&response);

            match parsed {
                ParsedResponse::Thought(thought) => {
                    let step = ReActStep::thought(&thought, step_number);
                    // Stream step immediately as it is produced
                    let _ = step_tx.send(step.clone()).await;
                    steps.push(step);
                    conversation.push(format!("Thought: {}", thought));

                    if self.config.verbose {
                        tracing::info!("Thought: {}", thought);
                    }
                }
                ParsedResponse::Action { tool, input } => {
                    let step = ReActStep::action(&tool, &input, step_number);
                    let _ = step_tx.send(step.clone()).await;
                    steps.push(step);
                    conversation.push(format!("Action: {}[{}]", tool, input));

                    if self.config.verbose {
                        tracing::info!("Action: {}[{}]", tool, input);
                    }

                    // Execute tool
                    step_number += 1;
                    let observation = self.execute_tool(&tool, &input).await;
                    let obs_step = ReActStep::observation(&observation, step_number);
                    let _ = step_tx.send(obs_step.clone()).await;
                    steps.push(obs_step);
                    conversation.push(format!("Observation: {}", observation));

                    if self.config.verbose {
                        tracing::info!("Observation: {}", observation);
                    }
                }
                ParsedResponse::FinalAnswer(answer) => {
                    let step = ReActStep::final_answer(&answer, step_number);
                    let _ = step_tx.send(step.clone()).await;
                    steps.push(step);

                    if self.config.verbose {
                        tracing::info!("Final Answer: {}", answer);
                    }

                    return Ok(ReActResult::success(
                        task_id,
                        &task,
                        answer,
                        steps,
                        iteration + 1,
                        start_time.elapsed().as_millis() as u64,
                    ));
                }
                ParsedResponse::Error(err) => {
                    return Ok(ReActResult::failed(
                        task_id,
                        &task,
                        err,
                        steps,
                        iteration + 1,
                        start_time.elapsed().as_millis() as u64,
                    ));
                }
            }
        }

        Ok(ReActResult::failed(
            task_id,
            &task,
            format!("Max iterations ({}) exceeded", self.config.max_iterations),
            steps,
            self.config.max_iterations,
            start_time.elapsed().as_millis() as u64,
        ))
    }

    /// 构建系统提示词
    /// Build system prompt
    async fn build_system_prompt(&self) -> String {
        if let Some(ref custom_prompt) = self.config.system_prompt {
            return custom_prompt.clone();
        }

        let tools = self.tools.read().await;
        let tool_descriptions: Vec<String> = tools
            .values()
            .map(|t| format!("- {}: {}", t.name(), t.description()))
            .collect();

        format!(
            r#"You are a ReAct (Reasoning and Acting) agent. You solve tasks by thinking step by step and using available tools.

Available tools:
{}

You must respond in one of these formats:

1. When you need to think:
Thought: <your reasoning about what to do next>

2. When you want to use a tool:
Action: <tool_name>[<input>]

3. When you have the final answer:
Final Answer: <your final answer to the task>

Rules:
- Always start with a Thought
- Use tools when you need external information
- Be concise and focused
- Provide a Final Answer when you have enough information
- If a tool returns an error, think about alternatives"#,
            tool_descriptions.join("\n")
        )
    }

    /// 构建完整提示词
    /// Build complete prompt
    async fn build_prompt(&self, system_prompt: &str, conversation: &[String]) -> String {
        format!("{}\n\n{}", system_prompt, conversation.join("\n"))
    }

    /// 解析 LLM 响应
    /// Parse LLM response
    fn parse_response(&self, response: &str) -> ParsedResponse {
        let response = response.trim();

        // 检查 Final Answer
        // Check Final Answer
        if let Some(answer) = response.strip_prefix("Final Answer:") {
            return ParsedResponse::FinalAnswer(answer.trim().to_string());
        }

        // 检查 Action
        // Check Action
        if let Some(action_part) = response.strip_prefix("Action:") {
            let action_part = action_part.trim();
            if let Some(bracket_start) = action_part.find('[')
                && let Some(bracket_end) = action_part.rfind(']')
            {
                let tool = action_part[..bracket_start].trim().to_string();
                let input = action_part[bracket_start + 1..bracket_end]
                    .trim()
                    .to_string();
                return ParsedResponse::Action { tool, input };
            }
            return ParsedResponse::Error(format!("Invalid action format: {}", action_part));
        }

        // 检查 Thought
        // Check Thought
        if let Some(thought) = response.strip_prefix("Thought:") {
            return ParsedResponse::Thought(thought.trim().to_string());
        }

        // 尝试从混合响应中提取
        // Try extracting from mixed response
        for line in response.lines() {
            let line = line.trim();
            if line.starts_with("Final Answer:") {
                return ParsedResponse::FinalAnswer(
                    line.strip_prefix("Final Answer:")
                        .unwrap()
                        .trim()
                        .to_string(),
                );
            }
            if line.starts_with("Action:") {
                let action_part = line.strip_prefix("Action:").unwrap().trim();
                if let Some(bracket_start) = action_part.find('[')
                    && let Some(bracket_end) = action_part.rfind(']')
                {
                    let tool = action_part[..bracket_start].trim().to_string();
                    let input = action_part[bracket_start + 1..bracket_end]
                        .trim()
                        .to_string();
                    return ParsedResponse::Action { tool, input };
                }
            }
            if line.starts_with("Thought:") {
                return ParsedResponse::Thought(
                    line.strip_prefix("Thought:").unwrap().trim().to_string(),
                );
            }
        }

        // 默认作为 Thought 处理
        // Handle as Thought by default
        ParsedResponse::Thought(response.to_string())
    }

    /// 执行工具（带超时保护）
    /// Execute tool (with timeout protection)
    async fn execute_tool(&self, tool_name: &str, input: &str) -> String {
        let span = tracing::info_span!("react.tool_call", tool = %tool_name);
        let timeout_dur = self.config.tool_timeout;

        // Clone the Arc while holding the read lock, then release the lock
        // before the potentially long-running tool.execute() call. Holding
        // a tokio RwLock read guard across an .await blocks all concurrent
        // write operations (e.g. register_tool) for the full tool duration.
        let maybe_tool: Option<Arc<dyn ReActTool>> = {
            let tools = self.tools.read().await;
            tools.get(tool_name).cloned()
        }; // read guard dropped here, before any .await

        async {
            match maybe_tool {
                Some(tool) => match tokio::time::timeout(timeout_dur, tool.execute(input)).await {
                    Ok(Ok(result)) => result,
                    Ok(Err(e)) => format!("Tool error: {}", e),
                    Err(_) => format!("Tool '{}' timed out after {:?}", tool_name, timeout_dur),
                },
                None => {
                    let tools = self.tools.read().await;
                    format!(
                        "Tool '{}' not found. Available tools: {:?}",
                        tool_name,
                        tools.keys().collect::<Vec<_>>()
                    )
                }
            }
        }
        .instrument(span)
        .await
    }
}

/// 解析后的响应
/// Parsed response
enum ParsedResponse {
    Thought(String),
    Action { tool: String, input: String },
    FinalAnswer(String),
    Error(String),
}

/// ReAct Agent 构建器
/// ReAct Agent builder
pub struct ReActAgentBuilder {
    llm: Option<Arc<LLMAgent>>,
    tools: Vec<Arc<dyn ReActTool>>,
    config: ReActConfig,
}

impl ReActAgentBuilder {
    pub fn new() -> Self {
        Self {
            llm: None,
            tools: Vec::new(),
            config: ReActConfig::default(),
        }
    }

    /// 设置 LLM Agent
    /// Set LLM Agent
    pub fn with_llm(mut self, llm: Arc<LLMAgent>) -> Self {
        self.llm = Some(llm);
        self
    }

    /// 添加工具
    /// Add tool
    pub fn with_tool(mut self, tool: Arc<dyn ReActTool>) -> Self {
        self.tools.push(tool);
        self
    }

    /// 添加多个工具
    /// Add multiple tools
    pub fn with_tools(mut self, tools: Vec<Arc<dyn ReActTool>>) -> Self {
        self.tools.extend(tools);
        self
    }

    /// 设置最大迭代次数
    /// Set max iterations
    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.config.max_iterations = max;
        self
    }

    /// 设置温度
    /// Set temperature
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.config.temperature = temp;
        self
    }

    /// 设置系统提示词
    /// Set system prompt
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.config.system_prompt = Some(prompt.into());
        self
    }

    /// 设置是否详细输出
    /// Set verbose output
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.config.verbose = verbose;
        self
    }

    /// 设置完整配置
    /// Set full config
    pub fn with_config(mut self, config: ReActConfig) -> Self {
        self.config = config;
        self
    }

    /// 构建 ReAct Agent
    /// Build ReAct Agent
    pub fn build(self) -> LLMResult<ReActAgent> {
        let llm = self
            .llm
            .ok_or_else(|| LLMError::ConfigError("LLM agent not set".to_string()))?;

        // 同步构造工具字典，避免 tokio::spawn 导致的竞态条件
        // Construct tool map synchronously to avoid tokio::spawn race condition
        let mut tool_map = HashMap::new();
        for tool in self.tools {
            tool_map.insert(tool.name().to_string(), tool);
        }

        let agent = ReActAgent::with_tools(llm, self.config, tool_map);

        Ok(agent)
    }

    /// 异步构建 (确保工具已注册)
    /// Async build (ensure tools are registered)
    pub async fn build_async(self) -> LLMResult<ReActAgent> {
        let llm = self
            .llm
            .ok_or_else(|| LLMError::ConfigError("LLM agent not set".to_string()))?;

        let agent = ReActAgent::new(llm, self.config);

        // 注册工具
        // Register tools
        for tool in self.tools {
            agent.register_tool(tool).await;
        }

        Ok(agent)
    }
}

impl Default for ReActAgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_react_step_creation() {
        let thought = ReActStep::thought("I need to search for information", 1);
        assert!(matches!(thought.step_type, ReActStepType::Thought));

        let action = ReActStep::action("search", "capital of France", 2);
        assert!(matches!(action.step_type, ReActStepType::Action));
        assert_eq!(action.tool_name, Some("search".to_string()));

        let observation = ReActStep::observation("Paris is the capital of France", 3);
        assert!(matches!(observation.step_type, ReActStepType::Observation));

        let answer = ReActStep::final_answer("Paris", 4);
        assert!(matches!(answer.step_type, ReActStepType::FinalAnswer));
    }

    #[test]
    fn test_react_config() {
        let config = ReActConfig::new()
            .with_max_iterations(5)
            .with_temperature(0.5)
            .with_verbose(false);

        assert_eq!(config.max_iterations, 5);
        assert_eq!(config.temperature, 0.5);
        assert!(!config.verbose);
    }

    /// 验证 `build()` 修复了竞态条件。
    /// Verify that `build()` fixes the race condition.
    ///
    /// 在修复之前，`build()` 使用 `tokio::spawn` 注册工具，导致工具不会立即生效。
    /// Now we construct the map synchronously, so tools must be available immediately.
    #[tokio::test]
    async fn test_build_sync_tools_available() {
        struct DummyTool;

        #[async_trait::async_trait]
        impl ReActTool for DummyTool {
            fn name(&self) -> &str {
                "dummy"
            }
            fn description(&self) -> &str {
                "desc"
            }
            async fn execute(&self, _input: &str) -> Result<String, String> {
                Ok("ok".to_string())
            }
        }

        // We can't easily construct a real LLMAgent without a provider,
        // but we can just use the internal methods directly to test the builder logic.
        let llm = Arc::new(
            crate::llm::LLMAgentBuilder::new()
                .with_provider(Arc::new(crate::llm::openai::OpenAIProvider::with_config(
                    crate::llm::openai::OpenAIConfig::new("dummy".to_string()),
                )))
                .build(),
        );

        let agent = ReActAgent::builder()
            .with_llm(llm)
            .with_tool(Arc::new(DummyTool))
            .build()
            .expect("build should succeed");

        // Immediately check — should be 1, because token::spawn is no longer used
        let tools = agent.get_tools().await;
        assert_eq!(
            tools.len(),
            1,
            "FIX CONFIRMED: tools are available immediately after build() is called synchronously."
        );
        assert_eq!(tools[0].name(), "dummy");
    }
}
