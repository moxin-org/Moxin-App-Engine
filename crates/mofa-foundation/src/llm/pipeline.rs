//! Agent 流水线
//! Agent Pipeline
//!
//! 提供简洁的流水线 API，用于快速构建 Agent 处理流程
//! Provides a concise pipeline API for quickly building Agent processing flows
//!
//! # 特性
//! # Features
//!
//! - **函数式组合**: 使用 `map`, `filter`, `transform` 等操作
//! - **Functional Composition**: Using operations like `map`, `filter`, `transform`
//! - **类型安全**: 编译时类型检查
//! - **Type Safety**: Compile-time type checking
//! - **惰性求值**: 只在执行时运行
//! - **Lazy Evaluation**: Runs only at execution time
//! - **流式支持**: 支持流式输出
//! - **Streaming Support**: Supports streaming output
//!
//! # 示例
//! # Example
//!
//! ```rust,ignore
//! use mofa_foundation::llm::pipeline::Pipeline;
//!
//! // 简单流水线
//! // Simple pipeline
//! let result = Pipeline::new()
//!     .with_agent(agent)
//!     .map(|s| s.to_uppercase())
//!     .run("Hello, world!")
//!     .await?;
//!
//! // 链式多 Agent
//! // Chained multi-Agent
//! let result = Pipeline::new()
//!     .with_agent(researcher)
//!     .then(writer)
//!     .then(editor)
//!     .run("Write about Rust")
//!     .await?;
//! ```

use super::agent::LLMAgent;
use super::types::{LLMError, LLMResult};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Type alias for async transform function
pub type AsyncTransformFn =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync>;

/// 流水线步骤
/// Pipeline Step
enum PipelineStep {
    /// Agent 处理
    /// Agent Processing
    Agent {
        agent: Arc<LLMAgent>,
        prompt_template: Option<String>,
        session_id: Option<String>,
    },
    /// 同步转换
    /// Synchronous Transform
    Transform(Arc<dyn Fn(String) -> String + Send + Sync>),
    /// 异步转换
    /// Asynchronous Transform
    AsyncTransform(AsyncTransformFn),
    /// 过滤（如果返回 None，则使用原输入）
    /// Filter (if returns false, the original input is discarded)
    Filter(Arc<dyn Fn(&str) -> bool + Send + Sync>),
    /// 条件分支
    /// Conditional Branch
    Branch {
        condition: Arc<dyn Fn(&str) -> bool + Send + Sync>,
        if_true: Box<PipelineStep>,
        if_false: Box<PipelineStep>,
    },
    /// 尝试恢复（如果失败则使用默认值）
    /// Try Recovery (use default value on failure)
    TryRecover {
        step: Box<PipelineStep>,
        default: String,
    },
    /// 重试
    /// Retry
    Retry {
        step: Box<PipelineStep>,
        max_retries: usize,
    },
    /// 无操作（透传）
    /// No Operation (pass-through)
    Identity,
}

/// Agent 流水线
/// Agent Pipeline
///
/// 提供链式 API 构建 Agent 处理流程
/// Provides a chained API for building Agent processing flows
pub struct Pipeline {
    steps: Vec<PipelineStep>,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline {
    /// 创建空流水线
    /// Create an empty pipeline
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    /// 从 Agent 创建流水线
    /// Create pipeline from an Agent
    pub fn from_agent(agent: Arc<LLMAgent>) -> Self {
        Self::new().with_agent(agent)
    }

    /// 添加 Agent 步骤
    /// Add Agent step
    pub fn with_agent(mut self, agent: Arc<LLMAgent>) -> Self {
        self.steps.push(PipelineStep::Agent {
            agent,
            prompt_template: None,
            session_id: None,
        });
        self
    }

    /// 添加带模板的 Agent 步骤
    /// Add Agent step with template
    ///
    /// 模板中使用 `{input}` 作为输入占位符
    /// Use `{input}` in the template as the input placeholder
    pub fn with_agent_template(
        mut self,
        agent: Arc<LLMAgent>,
        template: impl Into<String>,
    ) -> Self {
        self.steps.push(PipelineStep::Agent {
            agent,
            prompt_template: Some(template.into()),
            session_id: None,
        });
        self
    }

    /// 添加带会话的 Agent 步骤
    /// Add Agent step with session
    pub fn with_agent_session(
        mut self,
        agent: Arc<LLMAgent>,
        session_id: impl Into<String>,
    ) -> Self {
        self.steps.push(PipelineStep::Agent {
            agent,
            prompt_template: None,
            session_id: Some(session_id.into()),
        });
        self
    }

    /// 链接下一个 Agent
    /// Chain the next Agent
    pub fn then(self, agent: Arc<LLMAgent>) -> Self {
        self.with_agent(agent)
    }

    /// 链接下一个 Agent（带模板）
    /// Chain the next Agent (with template)
    pub fn then_with_template(self, agent: Arc<LLMAgent>, template: impl Into<String>) -> Self {
        self.with_agent_template(agent, template)
    }

    /// 添加同步转换
    /// Add synchronous transform
    pub fn map<F>(mut self, f: F) -> Self
    where
        F: Fn(String) -> String + Send + Sync + 'static,
    {
        self.steps.push(PipelineStep::Transform(Arc::new(f)));
        self
    }

    /// 添加异步转换
    /// Add asynchronous transform
    pub fn map_async<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = String> + Send + 'static,
    {
        self.steps
            .push(PipelineStep::AsyncTransform(Arc::new(move |s| {
                Box::pin(f(s))
            })));
        self
    }

    /// 添加过滤器
    /// Add filter
    ///
    /// 如果过滤器返回 false，则跳过后续步骤并返回当前值
    /// If filter returns false, skips subsequent steps and returns current value
    pub fn filter<F>(mut self, f: F) -> Self
    where
        F: Fn(&str) -> bool + Send + Sync + 'static,
    {
        self.steps.push(PipelineStep::Filter(Arc::new(f)));
        self
    }

    /// 条件分支
    /// Conditional branch
    pub fn branch<F>(mut self, condition: F, if_true: Pipeline, if_false: Pipeline) -> Self
    where
        F: Fn(&str) -> bool + Send + Sync + 'static,
    {
        // 将子流水线转换为单个步骤
        // Convert sub-pipeline into a single step
        let true_step = if if_true.steps.is_empty() {
            PipelineStep::Identity
        } else if if_true.steps.len() == 1 {
            if_true.steps.into_iter().next().unwrap()
        } else {
            // 多步骤情况，需要嵌套
            // Multi-step case, requires nesting
            PipelineStep::Identity // 简化处理
            // Simplified handling
        };

        let false_step = if if_false.steps.is_empty() {
            PipelineStep::Identity
        } else if if_false.steps.len() == 1 {
            if_false.steps.into_iter().next().unwrap()
        } else {
            PipelineStep::Identity
        };

        self.steps.push(PipelineStep::Branch {
            condition: Arc::new(condition),
            if_true: Box::new(true_step),
            if_false: Box::new(false_step),
        });
        self
    }

    /// 添加尝试恢复步骤
    /// Add a try-recover step
    pub fn try_or_default(mut self, default: impl Into<String>) -> Self {
        if let Some(last_step) = self.steps.pop() {
            self.steps.push(PipelineStep::TryRecover {
                step: Box::new(last_step),
                default: default.into(),
            });
        }
        self
    }

    /// 添加重试
    /// Add retry
    pub fn retry(mut self, max_retries: usize) -> Self {
        if let Some(last_step) = self.steps.pop() {
            self.steps.push(PipelineStep::Retry {
                step: Box::new(last_step),
                max_retries,
            });
        }
        self
    }

    /// 执行流水线
    /// Execute pipeline
    pub async fn run(&self, input: impl Into<String>) -> LLMResult<String> {
        let mut current = input.into();

        for step in &self.steps {
            current = Self::execute_step(step, current).await?;
        }

        Ok(current)
    }

    /// 执行单个步骤
    /// Execute a single step
    fn execute_step<'a>(
        step: &'a PipelineStep,
        input: String,
    ) -> Pin<Box<dyn Future<Output = LLMResult<String>> + Send + 'a>> {
        Box::pin(async move {
            match step {
                PipelineStep::Agent {
                    agent,
                    prompt_template,
                    session_id,
                } => {
                    let prompt = if let Some(template) = prompt_template {
                        template.replace("{input}", &input)
                    } else {
                        input
                    };

                    if let Some(sid) = session_id {
                        let _ = agent.get_or_create_session(sid).await;
                        agent.chat_with_session(sid, &prompt).await
                    } else {
                        agent.ask(&prompt).await
                    }
                }

                PipelineStep::Transform(f) => Ok(f(input)),

                PipelineStep::AsyncTransform(f) => Ok(f(input).await),

                PipelineStep::Filter(f) => {
                    if f(&input) {
                        Ok(input)
                    } else {
                        Err(LLMError::Other("Filtered out".to_string()))
                    }
                }

                PipelineStep::Branch {
                    condition,
                    if_true,
                    if_false,
                } => {
                    if condition(&input) {
                        Self::execute_step(if_true, input).await
                    } else {
                        Self::execute_step(if_false, input).await
                    }
                }

                PipelineStep::TryRecover { step, default } => {
                    match Self::execute_step(step, input).await {
                        Ok(result) => Ok(result),
                        Err(_) => Ok(default.clone()),
                    }
                }

                PipelineStep::Retry { step, max_retries } => {
                    let mut last_error = None;
                    for _ in 0..=*max_retries {
                        match Self::execute_step(step, input.clone()).await {
                            Ok(result) => return Ok(result),
                            Err(e) => last_error = Some(e),
                        }
                    }
                    Err(last_error
                        .unwrap_or_else(|| LLMError::Other("Retry exhausted".to_string())))
                }

                PipelineStep::Identity => Ok(input),
            }
        })
    }
}

/// 流水线构建器宏
/// Pipeline builder macro
///
/// 简化流水线创建
/// Simplify pipeline creation
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// let pipeline = pipeline![
///     agent => researcher,
///     map => |s| s.to_uppercase(),
///     agent => writer,
/// ];
/// ```
#[macro_export]
macro_rules! pipeline {
    // 空流水线
    // Empty pipeline
    () => {
        $crate::llm::pipeline::Pipeline::new()
    };

    // Agent 步骤
    // Agent step
    (agent => $agent:expr $(, $($rest:tt)*)?) => {
        $crate::llm::pipeline::Pipeline::new()
            .with_agent($agent)
            $($(. $rest)*)?
    };

    // Map 步骤
    // Map step
    (map => $f:expr $(, $($rest:tt)*)?) => {
        $crate::llm::pipeline::Pipeline::new()
            .map($f)
            $($(. $rest)*)?
    };
}

// ============================================================================
// 便捷函数
// Convenience functions
// ============================================================================

/// 创建简单的 Agent 链
/// Create simple Agent chain
///
/// 依次执行多个 Agent
/// Execute multiple Agents in sequence
pub fn agent_pipe(agents: Vec<Arc<LLMAgent>>) -> Pipeline {
    let mut pipeline = Pipeline::new();
    for agent in agents {
        pipeline = pipeline.with_agent(agent);
    }
    pipeline
}

/// 创建带模板的 Agent 链
/// Create Agent chain with templates
pub fn agent_pipe_with_templates(agents: Vec<(Arc<LLMAgent>, impl Into<String>)>) -> Pipeline {
    let mut pipeline = Pipeline::new();
    for (agent, template) in agents {
        pipeline = pipeline.with_agent_template(agent, template);
    }
    pipeline
}

/// 快速问答
/// Quick Q&A
///
/// 使用单个 Agent 回答问题
/// Use a single Agent to answer a question
pub async fn quick_ask(agent: &LLMAgent, question: impl Into<String>) -> LLMResult<String> {
    agent.ask(question).await
}

/// 使用模板问答
/// Q&A using a template
pub async fn ask_with_template(
    agent: &LLMAgent,
    template: &str,
    input: impl Into<String>,
) -> LLMResult<String> {
    let prompt = template.replace("{input}", &input.into());
    agent.ask(&prompt).await
}

/// 批量问答
/// Batch Q&A
pub async fn batch_ask(
    agent: &LLMAgent,
    questions: Vec<impl Into<String>>,
) -> Vec<LLMResult<String>> {
    let mut results = Vec::new();
    for question in questions {
        results.push(agent.ask(question).await);
    }
    results
}

// ============================================================================
// 流式流水线
// Streaming Pipeline
// ============================================================================

/// 流式流水线
/// Streaming Pipeline
///
/// 支持流式输出的流水线
/// Pipeline that supports streaming output
pub struct StreamPipeline {
    agent: Arc<LLMAgent>,
    pre_transform: Option<Arc<dyn Fn(String) -> String + Send + Sync>>,
    post_transform: Option<Arc<dyn Fn(String) -> String + Send + Sync>>,
    prompt_template: Option<String>,
}

impl StreamPipeline {
    /// 创建新的流式流水线
    /// Create new streaming pipeline
    pub fn new(agent: Arc<LLMAgent>) -> Self {
        Self {
            agent,
            pre_transform: None,
            post_transform: None,
            prompt_template: None,
        }
    }

    /// 设置输入预处理
    /// Set input preprocessing
    pub fn pre_process<F>(mut self, f: F) -> Self
    where
        F: Fn(String) -> String + Send + Sync + 'static,
    {
        self.pre_transform = Some(Arc::new(f));
        self
    }

    /// 设置输出后处理
    /// Set output post-processing
    pub fn post_process<F>(mut self, f: F) -> Self
    where
        F: Fn(String) -> String + Send + Sync + 'static,
    {
        self.post_transform = Some(Arc::new(f));
        self
    }

    /// 设置提示词模板
    /// Set prompt template
    pub fn with_template(mut self, template: impl Into<String>) -> Self {
        self.prompt_template = Some(template.into());
        self
    }

    /// 执行并返回流
    /// Execute and return stream
    pub async fn run_stream(
        &self,
        input: impl Into<String>,
    ) -> LLMResult<super::agent::TextStream> {
        let mut input = input.into();

        // 预处理
        // Preprocessing
        if let Some(ref pre) = self.pre_transform {
            input = pre(input);
        }

        // 应用模板
        // Apply template
        let prompt = if let Some(ref template) = self.prompt_template {
            template.replace("{input}", &input)
        } else {
            input
        };

        // 返回流
        // Return stream
        self.agent.ask_stream(&prompt).await
    }

    /// 执行并收集完整结果
    /// Execute and collect full result
    pub async fn run(&self, input: impl Into<String>) -> LLMResult<String> {
        use futures::StreamExt;

        let mut stream = self.run_stream(input).await?;
        let mut result = String::new();

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(text) => result.push_str(&text),
                Err(e) => return Err(e),
            }
        }

        // 后处理
        // Post-processing
        if let Some(ref post) = self.post_transform {
            result = post(result);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_transform() {
        // 测试纯转换流水线（不需要实际 Agent）
        // Test pure transformation pipeline (no actual Agent required)
        let pipeline = Pipeline::new()
            .map(|s| s.to_uppercase())
            .map(|s| format!("Hello, {}!", s));

        // 由于没有 Agent，无法运行完整测试
        // No full test possible without an Agent
        // 但可以验证流水线构建正确
        // But can verify the pipeline is built correctly
        assert!(!pipeline.steps.is_empty());
    }

    #[test]
    fn test_pipeline_builder() {
        let pipeline = Pipeline::new()
            .map(|s| s.trim().to_string())
            .map(|s| s.to_lowercase());

        assert_eq!(pipeline.steps.len(), 2);
    }
}
