//! Standard LLM-based agent execution engine
//!
//! Provides specialized execution for LLM-based agents with:
//! - LLM chat completion with tool calling
//! - Tool execution loop with iteration limits
//! - Session management
//! - Message history tracking
//!
//! # Architecture
//!
//! This module uses composition over inheritance:
//! - Composes `BaseAgent` for MoFAAgent functionality
//! - Adds LLM-specific functionality on top
//!
//! ```text
//! +-------------------------------------------------------------+
//! |                     AgentExecutor                            |
//! +-------------------------------------------------------------+
//! |           BaseAgent (MoFAAgent implementation)               |
//! |   - id, name, capabilities, state                            |
//! |   - initialize, execute, shutdown                            |
//! +-------------------------------------------------------------+
//! |  + llm: Arc<dyn LLMProvider>                                |
//! |  + context: Arc<RwLock<PromptContext>>                       |
//! |  + tools: Arc<RwLock<SimpleToolRegistry>>                     |
//! |  + sessions: Arc<SessionManager>                              |
//! |  + config: AgentExecutorConfig                                |
//! +-------------------------------------------------------------+
//! ```

use async_trait::async_trait;
use mofa_kernel::agent::components::context_compressor::ContextCompressor;
use mofa_kernel::agent::context::AgentContext;
use mofa_kernel::agent::error::{AgentError, AgentResult};
use mofa_kernel::agent::types::{ChatCompletionRequest, ChatMessage, LLMProvider, ToolDefinition};
use mofa_kernel::agent::{AgentCapabilities, AgentState, MoFAAgent};
use mofa_kernel::agent::{AgentInput, AgentOutput, InputType, OutputType};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::agent::base::BaseAgent;
use crate::agent::context::prompt::PromptContext;

use super::components::tool::SimpleToolRegistry;
use super::{Session, SessionManager};
use mofa_kernel::agent::components::memory::Memory;
use mofa_kernel::agent::components::tool::{Tool, ToolInput, ToolRegistry};

// ============================================================================
// Agent Executor Configuration
// ============================================================================

/// Agent execution configuration
#[derive(Clone)]
pub struct AgentExecutorConfig {
    /// Maximum tool iterations per message
    pub max_iterations: usize,
    /// Session timeout (optional)
    pub session_timeout: Option<std::time::Duration>,
    /// Default model to use
    pub default_model: Option<String>,
    /// Temperature for LLM calls
    pub temperature: Option<f32>,
    /// Max tokens for LLM responses
    pub max_tokens: Option<u32>,
    /// Token budget for the conversation context sent to the LLM.
    /// When the estimated token count exceeds this value and a compressor is
    /// configured, compression is triggered automatically.  Defaults to 4096.
    pub max_context_tokens: usize,
    /// Per-tool-call timeout. If a single tool execution exceeds this
    /// duration, it is cancelled and an error is returned. Default: 30s.
    pub tool_timeout: Duration,
}

impl Default for AgentExecutorConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            session_timeout: None,
            default_model: None,
            temperature: None,
            max_tokens: None,
            max_context_tokens: 4096,
            tool_timeout: Duration::from_secs(30),
        }
    }
}

impl AgentExecutorConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = Some(model.into());
        self
    }

    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set the maximum number of context tokens before compression is triggered.
    pub fn with_max_context_tokens(mut self, n: usize) -> Self {
        self.max_context_tokens = n;
        self
    }
}

// ============================================================================
// Agent Executor
// ============================================================================

/// Standard LLM-based agent executor
///
/// This executor handles the complete agent loop:
/// 1. Build context with system prompt, history, and current message
/// 2. Call LLM with tool definitions
/// 3. Execute tools if called
/// 4. Repeat until no more tool calls or max iterations reached
///
/// Uses composition with `BaseAgent` to avoid reimplementing MoFAAgent.
///
/// # Architecture
///
/// ```text
/// AgentExecutor
/// ├── BaseAgent (provides MoFAAgent implementation)
/// └── LLM-specific fields (llm, context, tools, sessions, config)
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use mofa_foundation::agent::executor::{AgentExecutor, AgentExecutorConfig};
/// use std::sync::Arc;
///
/// let llm = Arc::new(MyLLMProvider::new());
/// let executor = AgentExecutor::new(llm, "/path/to/workspace").await?;
///
/// let response = executor.process_message("session", "Hello").await?;
/// ```
pub struct AgentExecutor {
    /// Base agent provides MoFAAgent implementation
    base: BaseAgent,

    /// ===== LLM-specific fields =====
    /// LLM provider
    llm: Arc<dyn LLMProvider>,
    /// Prompt context builder
    context: Arc<RwLock<PromptContext>>,
    /// Tool registry
    tools: Arc<RwLock<SimpleToolRegistry>>,
    /// Session manager
    sessions: Arc<SessionManager>,
    /// Configuration
    config: AgentExecutorConfig,
    /// Optional long-term memory backend.
    ///
    /// When set, the executor retrieves relevant memories before each turn
    /// and injects them into the system prompt so the agent can refer to
    /// past interactions across sessions.
    memory: Option<Arc<RwLock<dyn Memory>>>,
    /// Optional context compressor applied before each LLM call when the
    /// estimated token count exceeds `config.max_context_tokens`.
    compressor: Option<Arc<dyn ContextCompressor>>,
}

impl AgentExecutor {
    /// Create a new agent executor
    pub async fn new(llm: Arc<dyn LLMProvider>, workspace: impl AsRef<Path>) -> AgentResult<Self> {
        let workspace = workspace.as_ref();
        let context = Arc::new(RwLock::new(PromptContext::new(workspace).await?));
        let sessions = Arc::new(SessionManager::with_jsonl(workspace).await?);
        let tools = Arc::new(RwLock::new(SimpleToolRegistry::new()));

        // Create base agent with appropriate capabilities
        let base = BaseAgent::new(uuid::Uuid::now_v7().to_string(), "LLMExecutor")
            .with_description("LLM-based agent with tool calling")
            .with_version("1.0.0")
            .with_capabilities(
                AgentCapabilities::builder()
                    .tag("llm")
                    .tag("tool-calling")
                    .input_type(InputType::Text)
                    .output_type(OutputType::Text)
                    .supports_tools(true)
                    .build(),
            );

        Ok(Self {
            base,
            llm,
            context,
            tools,
            sessions,
            config: AgentExecutorConfig::default(),
            memory: None,
            compressor: None,
        })
    }

    /// Create with custom configuration
    pub async fn with_config(
        llm: Arc<dyn LLMProvider>,
        workspace: impl AsRef<Path>,
        config: AgentExecutorConfig,
    ) -> AgentResult<Self> {
        let workspace = workspace.as_ref();
        let context = Arc::new(RwLock::new(PromptContext::new(workspace).await?));
        let sessions = Arc::new(SessionManager::with_jsonl(workspace).await?);
        let tools = Arc::new(RwLock::new(SimpleToolRegistry::new()));

        // Create base agent with appropriate capabilities
        let base = BaseAgent::new(uuid::Uuid::now_v7().to_string(), "LLMExecutor")
            .with_description("LLM-based agent with tool calling")
            .with_version("1.0.0")
            .with_capabilities(
                AgentCapabilities::builder()
                    .tag("llm")
                    .tag("tool-calling")
                    .input_type(InputType::Text)
                    .output_type(OutputType::Text)
                    .supports_tools(true)
                    .build(),
            );

        Ok(Self {
            base,
            llm,
            context,
            tools,
            sessions,
            config,
            memory: None,
            compressor: None,
        })
    }

    /// Attach a long-term memory backend to this executor.
    ///
    /// When memory is set, the executor will:
    /// 1. Query the memory store for entries relevant to the incoming message.
    /// 2. Prepend those memories to the system prompt so the agent has context
    ///    from past sessions.
    /// 3. Store each completed user/assistant exchange in memory after the turn.
    ///
    /// Any type implementing the kernel `Memory` trait can be plugged in —
    /// `EpisodicMemory`, `SemanticMemory`, `InMemoryStorage`, etc.
    pub fn with_memory(mut self, memory: Arc<RwLock<dyn Memory>>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Register a tool
    pub async fn register_tool(
        &self,
        tool: Arc<dyn mofa_kernel::agent::components::tool::DynTool>,
    ) -> AgentResult<()> {
        let mut tools = self.tools.write().await;
        tools.register(tool)
    }

    /// Attach a context compressor.
    ///
    /// When set, the compressor is called automatically inside
    /// `process_message` whenever the estimated token count for the built
    /// message list exceeds `config.max_context_tokens`.
    pub fn with_compressor(mut self, compressor: Arc<dyn ContextCompressor>) -> Self {
        self.compressor = Some(compressor);
        self
    }

    /// Process a user message
    pub async fn process_message(
        &mut self,
        session_key: &str,
        message: &str,
    ) -> AgentResult<String> {
        // 1. Get or create session
        let session = self.sessions.get_or_create(session_key).await;

        // 2. Build system prompt
        let mut system_prompt = {
            let mut ctx = self.context.write().await;
            ctx.build_system_prompt().await?
        };

        // 3. Inject relevant memories into system prompt (if memory is configured)
        if let Some(mem) = &self.memory {
            let mem_read = mem.read().await;
            let recalled = mem_read.search(message, 5).await.unwrap_or_default();
            if !recalled.is_empty() {
                let memory_block: String = recalled
                    .iter()
                    .filter_map(|item| item.value.as_text().map(|t| format!("- {t}")))
                    .collect::<Vec<_>>()
                    .join("\n");
                system_prompt = format!(
                    "{system_prompt}\n\n## Relevant memories from past sessions\n{memory_block}"
                );
            }
        }

        // 4. Build messages
        let mut messages = self
            .build_messages(&session, &system_prompt, message)
            .await?;

        // 4. Compress context if a compressor is configured and the token
        //    budget is exceeded.
        if let Some(compressor) = &self.compressor {
            let token_count = compressor.count_tokens(&messages);
            if token_count > self.config.max_context_tokens {
                messages = compressor
                    .compress(messages, self.config.max_context_tokens)
                    .await?;
            }
        }

        // 5. Run agent loop
        let response = self.run_agent_loop(&mut messages).await?;

        // 6. Update session
        let mut session_updated = session.clone();
        session_updated.add_message("user", message);
        session_updated.add_message("assistant", &response);
        self.sessions.save(&session_updated).await?;

        // 7. Store this exchange in long-term memory (if configured)
        if let Some(mem) = &self.memory {
            use mofa_kernel::agent::components::memory::Message as MemMessage;
            let mut mem_write = mem.write().await;
            mem_write
                .add_to_history(session_key, MemMessage::user(message))
                .await
                .ok();
            mem_write
                .add_to_history(session_key, MemMessage::assistant(&response))
                .await
                .ok();
        }

        Ok(response)
    }

    /// Build the message list for LLM
    async fn build_messages(
        &self,
        session: &Session,
        system_prompt: &str,
        current_message: &str,
    ) -> AgentResult<Vec<ChatMessage>> {
        let mut messages = Vec::new();

        // System prompt
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: Some(system_prompt.to_string()),
            tool_call_id: None,
            tool_calls: None,
        });

        // History
        let history = session.get_history(50); // Limit to recent messages
        for msg in history {
            messages.push(ChatMessage {
                role: msg.role,
                content: Some(msg.content),
                tool_call_id: None,
                tool_calls: None,
            });
        }

        // Current message
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: Some(current_message.to_string()),
            tool_call_id: None,
            tool_calls: None,
        });

        Ok(messages)
    }

    /// Run the main agent loop with LLM and tool execution
    async fn run_agent_loop(&self, messages: &mut Vec<ChatMessage>) -> AgentResult<String> {
        for _iteration in 0..self.config.max_iterations {
            // Get tool definitions
            let tools = {
                let tools_guard = self.tools.read().await;
                tools_guard.list()
            };

            // Convert to OpenAI format
            let tool_definitions = if tools.is_empty() {
                None
            } else {
                Some(
                    tools
                        .iter()
                        .map(|t| ToolDefinition {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: t.parameters_schema.clone(),
                        })
                        .collect(),
                )
            };

            // Call LLM
            let request = ChatCompletionRequest {
                messages: messages.clone(),
                model: self.config.default_model.clone(),
                tools: tool_definitions,
                temperature: self.config.temperature,
                max_tokens: self.config.max_tokens,
            };

            let response = self.llm.chat(request).await?;

            // Check for tool calls
            if let Some(tool_calls) = response.tool_calls {
                if tool_calls.is_empty() {
                    // No more tools, return response
                    return Ok(response.content.unwrap_or_default());
                }

                // Add assistant message with tool calls
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: response.content,
                    tool_call_id: None,
                    tool_calls: Some(tool_calls.clone()),
                });

                // Execute tools
                for tool_call in tool_calls {
                    // Convert arguments to HashMap
                    let _args_map: HashMap<String, Value> =
                        if let Value::Object(map) = &tool_call.arguments {
                            map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                        } else {
                            return Err(AgentError::ExecutionFailed(format!(
                                "Invalid tool arguments for {}: {:?}",
                                tool_call.name, tool_call.arguments
                            )));
                        };

                    let result = {
                        let tools_guard = self.tools.read().await;
                        if let Some(tool) = tools_guard.get(&tool_call.name) {
                            match tool
                                .execute_dynamic(
                                    tool_call.arguments.clone(),
                                    &AgentContext::new("executor"),
                                )
                                .await
                            {
                                Ok(out) => {
                                    mofa_kernel::agent::components::tool::ToolResult::success(out)
                                }
                                Err(e) => {
                            let timeout_dur = self.config.tool_timeout;
                            match tokio::time::timeout(
                                timeout_dur,
                                tool.execute_dynamic(
                                    tool_call.arguments.clone(),
                                    &AgentContext::new("executor"),
                                ),
                            )
                            .await
                            {
                                Ok(Ok(out)) => {
                                    mofa_kernel::agent::components::tool::ToolResult::success(out)
                                }
                                Ok(Err(e)) => {
                                    mofa_kernel::agent::components::tool::ToolResult::failure(
                                        e.to_string(),
                                    )
                                }
                                Err(_) => {
                                    mofa_kernel::agent::components::tool::ToolResult::failure(
                                        format!(
                                            "Tool '{}' timed out after {:?}",
                                            tool_call.name, timeout_dur
                                        ),
                                    )
                                }
                            }
                        } else {
                            return Err(AgentError::ExecutionFailed(format!(
                                "Tool not found: {}",
                                tool_call.name
                            )));
                        }
                    };

                    // ToolResult is a struct with success bool and output
                    let result_str = if result.success {
                        result.to_string_output()
                    } else {
                        format!(
                            "Error: {}",
                            result.error.unwrap_or_else(|| "Unknown error".to_string())
                        )
                    };

                    // Add tool result message
                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: Some(result_str),
                        tool_call_id: Some(tool_call.id.clone()),
                        tool_calls: None,
                    });
                }
            } else {
                // No tool calls, return response
                return Ok(response.content.unwrap_or_default());
            }
        }

        // Max iterations exceeded
        Ok("I've completed processing but hit the maximum iteration limit.".to_string())
    }

    /// Get the session manager
    pub fn sessions(&self) -> &Arc<SessionManager> {
        &self.sessions
    }

    /// Get the tool registry
    pub fn tools(&self) -> &Arc<RwLock<SimpleToolRegistry>> {
        &self.tools
    }

    /// Get the prompt context
    pub fn context(&self) -> &Arc<RwLock<PromptContext>> {
        &self.context
    }

    /// Get the LLM provider
    pub fn llm(&self) -> &Arc<dyn LLMProvider> {
        &self.llm
    }

    /// Get the configuration
    pub fn config(&self) -> &AgentExecutorConfig {
        &self.config
    }

    /// Get mutable reference to base agent
    pub fn base_mut(&mut self) -> &mut BaseAgent {
        &mut self.base
    }

    /// Get reference to base agent
    pub fn base(&self) -> &BaseAgent {
        &self.base
    }
}

// ============================================================================
// MoFAAgent Trait Implementation via Delegation
// ============================================================================

#[async_trait]
impl MoFAAgent for AgentExecutor {
    fn id(&self) -> &str {
        self.base.id()
    }

    fn name(&self) -> &str {
        self.base.name()
    }

    fn capabilities(&self) -> &AgentCapabilities {
        self.base.capabilities()
    }

    fn state(&self) -> AgentState {
        self.base.state()
    }

    async fn initialize(&mut self, ctx: &AgentContext) -> AgentResult<()> {
        // Initialize base agent
        self.base.initialize(ctx).await?;

        // Additional executor-specific initialization
        self.base.transition_to(AgentState::Ready)?;

        Ok(())
    }

    async fn execute(
        &mut self,
        input: AgentInput,
        _ctx: &AgentContext,
    ) -> AgentResult<AgentOutput> {
        // For simplicity, use the text content from the input
        let message = input.as_text().unwrap_or("");
        let session_key = "default"; // Use default session for now

        // Process the message using the executor
        let response = self.process_message(session_key, message).await?;

        // Return the response as AgentOutput
        Ok(AgentOutput::text(response))
    }

    async fn shutdown(&mut self) -> AgentResult<()> {
        // Shutdown base agent
        self.base.shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use mofa_kernel::agent::types::ChatCompletionResponse;
    use mofa_kernel::agent::types::ToolCall;

    #[test]
    fn agent_executor_config_builder_methods_apply_values() {
        let config = AgentExecutorConfig::new()
            .with_max_iterations(3)
            .with_model("m1")
            .with_temperature(0.25)
            .with_max_context_tokens(1234);

        assert_eq!(config.max_iterations, 3);
        assert_eq!(config.default_model.as_deref(), Some("m1"));
        assert_eq!(config.temperature, Some(0.25));
        assert_eq!(config.max_context_tokens, 1234);
        assert_eq!(config.tool_timeout, Duration::from_secs(30));
    }

    struct MockProvider;

    #[async_trait]
    impl LLMProvider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        async fn chat(
            &self,
            _request: ChatCompletionRequest,
        ) -> AgentResult<ChatCompletionResponse> {
            Ok(ChatCompletionResponse {
                content: Some("ok".to_string()),
                tool_calls: Some(Vec::<ToolCall>::new()),
                usage: None,
            })
        }
    }

    #[tokio::test]
    async fn executor_new_builds_expected_base_and_config() {
        let dir = std::env::temp_dir().join(format!("mofa-executor-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");

        let config = AgentExecutorConfig::new().with_max_iterations(4);
        let executor = AgentExecutor::with_config(Arc::new(MockProvider), &dir, config.clone())
            .await
            .expect("executor should initialize");

        assert_eq!(executor.base().name(), "LLMExecutor");
        assert_eq!(executor.config().max_iterations, 4);
        assert_eq!(executor.state(), AgentState::Created);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
