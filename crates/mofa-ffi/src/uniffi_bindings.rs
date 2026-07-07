//! UniFFI bindings implementation
//!
//! This module provides implementations for the types defined in mofa.udl,
//! exposing core MoFA functionality across Python, Kotlin, Swift, and Java.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::runtime::Runtime;
use tokio::sync::RwLock;

// =============================================================================
// Error Types
// =============================================================================

// MoFaError is defined in crate::error and re-exported at the crate root.
// Re-import here for use in the From impls below.
use crate::error::{MoFaError, MoFaResult};

// ── FFI boundary conversions: Report<*> → MoFaError ───────────────────────────
//
// The full causal chain is preserved in the Display output of the Report,
// which is forwarded as the error string. No information is silently discarded.

impl From<error_stack::Report<mofa_kernel::error::KernelError>> for MoFaError {
    fn from(r: error_stack::Report<mofa_kernel::error::KernelError>) -> Self {
        MoFaError::RuntimeError(r.to_string())
    }
}

impl From<error_stack::Report<mofa_kernel::agent::AgentError>> for MoFaError {
    fn from(r: error_stack::Report<mofa_kernel::agent::AgentError>) -> Self {
        MoFaError::RuntimeError(r.to_string())
    }
}

impl From<error_stack::Report<mofa_kernel::agent::types::GlobalError>> for MoFaError {
    fn from(r: error_stack::Report<mofa_kernel::agent::types::GlobalError>) -> Self {
        MoFaError::RuntimeError(r.to_string())
    }
}

impl From<error_stack::Report<mofa_foundation::llm::LLMError>> for MoFaError {
    fn from(r: error_stack::Report<mofa_foundation::llm::LLMError>) -> Self {
        MoFaError::LLMError(r.to_string())
    }
}

// =============================================================================
// Agent Lifecycle Types
// =============================================================================

/// Agent status (FFI-safe version of AgentState, without associated data)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Created,
    Initializing,
    Ready,
    Running,
    Executing,
    Paused,
    Interrupted,
    ShuttingDown,
    Shutdown,
    Failed,
    Destroyed,
    Error,
}

impl From<&mofa_kernel::agent::types::AgentState> for AgentStatus {
    fn from(state: &mofa_kernel::agent::types::AgentState) -> Self {
        use mofa_kernel::agent::types::AgentState;
        match state {
            AgentState::Created => AgentStatus::Created,
            AgentState::Initializing => AgentStatus::Initializing,
            AgentState::Ready => AgentStatus::Ready,
            AgentState::Running => AgentStatus::Running,
            AgentState::Executing => AgentStatus::Executing,
            AgentState::Paused => AgentStatus::Paused,
            AgentState::Interrupted => AgentStatus::Interrupted,
            AgentState::ShuttingDown => AgentStatus::ShuttingDown,
            AgentState::Shutdown => AgentStatus::Shutdown,
            AgentState::Failed => AgentStatus::Failed,
            AgentState::Destroyed => AgentStatus::Destroyed,
            AgentState::Error(_) => AgentStatus::Error,
            _ => AgentStatus::Error,
        }
    }
}

/// Token usage statistics
#[derive(Debug, Clone)]
pub struct TokenUsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Tool usage record
#[derive(Debug, Clone)]
pub struct ToolUsageRecord {
    pub name: String,
    pub input_json: String,
    pub output_json: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// Structured agent output
#[derive(Debug, Clone)]
pub struct AgentOutputInfo {
    pub content: String,
    pub content_type: String,
    pub tools_used: Vec<ToolUsageRecord>,
    pub duration_ms: u64,
    pub token_usage: Option<TokenUsageInfo>,
    pub metadata_json: String,
}

impl From<&mofa_kernel::agent::types::AgentOutput> for AgentOutputInfo {
    fn from(output: &mofa_kernel::agent::types::AgentOutput) -> Self {
        use mofa_kernel::agent::types::OutputContent;

        let (content, content_type) = match &output.content {
            OutputContent::Text(s) => (s.clone(), "text".to_string()),
            OutputContent::Texts(v) => (v.join("\n"), "texts".to_string()),
            OutputContent::Json(v) => (v.to_string(), "json".to_string()),
            OutputContent::Binary(_) => ("[binary]".to_string(), "binary".to_string()),
            OutputContent::Stream => ("[stream]".to_string(), "stream".to_string()),
            OutputContent::Error(e) => (e.clone(), "error".to_string()),
            OutputContent::Empty => (String::new(), "empty".to_string()),
            _ => ("".to_string(), "unknown".to_string()),
        };

        let tools_used = output
            .tools_used
            .iter()
            .map(|t| ToolUsageRecord {
                name: t.name.clone(),
                input_json: t.input.to_string(),
                output_json: t.output.as_ref().map(|v| v.to_string()),
                success: t.success,
                error: t.error.clone(),
                duration_ms: t.duration_ms,
            })
            .collect();

        let token_usage = output.token_usage.as_ref().map(|u| TokenUsageInfo {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        let metadata_json =
            serde_json::to_string(&output.metadata).unwrap_or_else(|_| "{}".to_string());

        AgentOutputInfo {
            content,
            content_type,
            tools_used,
            duration_ms: output.duration_ms,
            token_usage,
            metadata_json,
        }
    }
}

// =============================================================================
// LLM Types (matching UDL definitions)
// =============================================================================

/// LLM provider type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LLMProviderType {
    OpenAI,
    Ollama,
    Azure,
    Compatible,
    Anthropic,
    Gemini,
}

/// Chat message role
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

/// LLM configuration
#[derive(Debug, Clone)]
pub struct LLMConfig {
    pub provider: LLMProviderType,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub deployment: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub system_prompt: Option<String>,
}

/// Chat message
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

// =============================================================================
// Session Management Types
// =============================================================================

/// Session message info for FFI
#[derive(Debug, Clone)]
pub struct SessionMessageInfo {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

// =============================================================================
// Tool System Types
// =============================================================================

/// Tool description for listing
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub parameters_schema_json: String,
}

/// FFI tool execution result
#[derive(Debug, Clone)]
pub struct FfiToolResult {
    pub success: bool,
    pub output_json: String,
    pub error: Option<String>,
}

/// Callback interface for foreign-language tool implementations
pub trait FfiToolCallback: Send + Sync {
    fn name(&self) -> String;
    fn description(&self) -> String;
    fn parameters_schema_json(&self) -> String;
    fn execute(&self, arguments_json: String) -> FfiToolResult;
}

// =============================================================================
// Namespace functions
// =============================================================================

/// Get MoFA version
pub(crate) fn get_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Check if Dora runtime support is available
pub(crate) fn is_dora_available() -> bool {
    cfg!(feature = "dora")
}

/// Create a new LLM Agent Builder
pub(crate) fn new_llm_agent_builder() -> Result<std::sync::Arc<LLMAgentBuilder>, MoFaError> {
    LLMAgentBuilder::create()
}

// =============================================================================
// LLM Agent Implementation
// =============================================================================

/// LLM Agent - the main interface for LLM interactions
///
/// This is the primary interface exposed to Python, Kotlin, Swift, and Java.
pub struct LLMAgent {
    agent_id: String,
    name: String,
    inner: Arc<RwLock<mofa_foundation::llm::LLMAgent>>,
    _inner: std::marker::PhantomData<()>,
    runtime: Arc<Runtime>,
    _runtime: std::marker::PhantomData<()>,
}

impl LLMAgent {
    /// Create from configuration file (agent.yml)
    pub fn from_config_file(config_path: String) -> Result<Self, MoFaError> {
        let runtime =
            tokio::runtime::Runtime::new().map_err(|e| MoFaError::RuntimeError(e.to_string()))?;

        let agent = mofa_foundation::llm::agent_from_config(&config_path)
            .map_err(|e| MoFaError::ConfigError(e.to_string()))?;

        let agent_id = agent.config().agent_id.clone();
        let name = agent.config().name.clone();

        Ok(Self {
            agent_id,
            name,
            inner: Arc::new(RwLock::new(agent)),
            _inner: std::marker::PhantomData,
            runtime: Arc::new(runtime),
            _runtime: std::marker::PhantomData,
        })
    }

    /// Create from configuration dictionary
    pub fn from_config(
        config: LLMConfig,
        agent_id: String,
        name: String,
    ) -> Result<Self, MoFaError> {
        use mofa_foundation::llm::{
            AnthropicConfig, AnthropicProvider, GeminiConfig, GeminiProvider, LLMAgentBuilder,
            OpenAIConfig, OpenAIProvider,
        };

        let runtime =
            tokio::runtime::Runtime::new().map_err(|e| MoFaError::RuntimeError(e.to_string()))?;

        let mut builder = LLMAgentBuilder::new().with_id(&agent_id).with_name(&name);

        // Create provider based on config
        let provider: Arc<dyn mofa_foundation::llm::LLMProvider> = match config.provider {
            LLMProviderType::OpenAI => {
                let api_key = config
                    .api_key
                    .clone()
                    .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                    .ok_or_else(|| MoFaError::ConfigError("OpenAI API key not set".to_string()))?;

                let mut oai_config = OpenAIConfig::new(api_key);
                if let Some(model) = config.model.clone() {
                    oai_config = oai_config.with_model(model);
                }
                if let Some(base_url) = config.base_url.clone() {
                    oai_config = oai_config.with_base_url(base_url);
                }
                if let Some(temp) = config.temperature {
                    oai_config = oai_config.with_temperature(temp);
                }
                if let Some(tokens) = config.max_tokens {
                    oai_config = oai_config.with_max_tokens(tokens);
                }
                Arc::new(OpenAIProvider::with_config(oai_config))
            }
            LLMProviderType::Ollama => {
                let model = config.model.clone().unwrap_or_else(|| "llama2".to_string());
                Arc::new(OpenAIProvider::ollama(model))
            }
            LLMProviderType::Azure => {
                let endpoint = config
                    .base_url
                    .clone()
                    .ok_or_else(|| MoFaError::ConfigError("Azure endpoint not set".to_string()))?;
                let api_key = config
                    .api_key
                    .clone()
                    .ok_or_else(|| MoFaError::ConfigError("Azure API key not set".to_string()))?;
                let deployment = config
                    .deployment
                    .clone()
                    .or(config.model.clone())
                    .ok_or_else(|| {
                        MoFaError::ConfigError("Azure deployment not set".to_string())
                    })?;
                Arc::new(OpenAIProvider::azure(endpoint, api_key, deployment))
            }
            LLMProviderType::Compatible => {
                let base_url = config
                    .base_url
                    .clone()
                    .ok_or_else(|| MoFaError::ConfigError("base_url not set".to_string()))?;
                let model = config
                    .model
                    .clone()
                    .unwrap_or_else(|| "default".to_string());
                Arc::new(OpenAIProvider::local(base_url, model))
            }
            LLMProviderType::Anthropic => {
                let api_key = config
                    .api_key
                    .clone()
                    .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
                    .ok_or_else(|| {
                        MoFaError::ConfigError("Anthropic API key not set".to_string())
                    })?;

                let mut a_cfg = AnthropicConfig::new(api_key);
                if let Some(model) = config.model.clone() {
                    a_cfg = a_cfg.with_model(model);
                }
                if let Some(base_url) = config.base_url.clone() {
                    a_cfg = a_cfg.with_base_url(base_url);
                }
                if let Some(temp) = config.temperature {
                    a_cfg = a_cfg.with_temperature(temp);
                }
                if let Some(tokens) = config.max_tokens {
                    a_cfg = a_cfg.with_max_tokens(tokens);
                }

                Arc::new(AnthropicProvider::with_config(a_cfg))
            }
            LLMProviderType::Gemini => {
                let api_key = config
                    .api_key
                    .clone()
                    .or_else(|| std::env::var("GEMINI_API_KEY").ok())
                    .ok_or_else(|| MoFaError::ConfigError("Gemini API key not set".to_string()))?;

                let mut g_cfg = GeminiConfig::new(api_key);
                if let Some(model) = config.model.clone() {
                    g_cfg = g_cfg.with_model(model);
                }
                if let Some(base_url) = config.base_url.clone() {
                    g_cfg = g_cfg.with_base_url(base_url);
                }
                if let Some(temp) = config.temperature {
                    g_cfg = g_cfg.with_temperature(temp);
                }
                if let Some(tokens) = config.max_tokens {
                    g_cfg = g_cfg.with_max_tokens(tokens);
                }

                Arc::new(GeminiProvider::with_config(g_cfg))
            }
        };

        builder = builder.with_provider(provider);

        if let Some(temp) = config.temperature {
            builder = builder.with_temperature(temp);
        }
        if let Some(tokens) = config.max_tokens {
            builder = builder.with_max_tokens(tokens);
        }
        if let Some(prompt) = config.system_prompt {
            builder = builder.with_system_prompt(prompt);
        }

        let inner_agent = builder
            .try_build()
            .map_err(|e| MoFaError::ConfigError(e.to_string()))?;

        Ok(Self {
            agent_id,
            name,
            inner: Arc::new(RwLock::new(inner_agent)),
            _inner: std::marker::PhantomData,
            runtime: Arc::new(runtime),
            _runtime: std::marker::PhantomData,
        })
    }

    /// Get agent ID
    pub fn agent_id(&self) -> Result<String, MoFaError> {
        Ok(self.agent_id.clone())
    }

    /// Get agent name
    pub fn name(&self) -> Result<String, MoFaError> {
        Ok(self.name.clone())
    }

    /// Simple Q&A (no context retention)
    pub fn ask(&self, question: String) -> Result<String, MoFaError> {
        self.runtime.block_on(async {
            let agent = self.inner.read().await;
            agent
                .ask(&question)
                .await
                .map_err(|e| MoFaError::LLMError(e.to_string()))
        })
    }

    /// Multi-turn chat (with context retention)
    pub fn chat(&self, message: String) -> Result<String, MoFaError> {
        self.runtime.block_on(async {
            let agent = self.inner.read().await;
            agent
                .chat(&message)
                .await
                .map_err(|e| MoFaError::LLMError(e.to_string()))
        })
    }

    /// Clear conversation history
    pub fn clear_history(&self) {
        self.runtime.block_on(async {
            let agent = self.inner.read().await;
            agent.clear_history().await;
        });
    }

    /// Get conversation history
    pub fn get_history(&self) -> Vec<ChatMessage> {
        self.runtime.block_on(async {
            let agent = self.inner.read().await;
            agent
                .history()
                .await
                .iter()
                .map(|msg| {
                    let role = match msg.role {
                        mofa_foundation::llm::Role::System => ChatRole::System,
                        mofa_foundation::llm::Role::User => ChatRole::User,
                        mofa_foundation::llm::Role::Assistant => ChatRole::Assistant,
                        _ => ChatRole::User,
                    };
                    let content = msg
                        .content
                        .as_ref()
                        .map(|c| match c {
                            mofa_foundation::llm::MessageContent::Text(s) => s.clone(),
                            _ => String::new(),
                        })
                        .unwrap_or_default();
                    ChatMessage { role, content }
                })
                .collect()
        })
    }

    /// Get structured output info (placeholder until agent tracks last output)
    pub fn get_last_output(&self) -> Result<AgentOutputInfo, MoFaError> {
        Err(MoFaError::RuntimeError(
            "get_last_output is not yet supported: LLMAgent does not persist structured output in this API".to_string(),
        ))
    }
}

// =============================================================================
// LLM Agent Builder Implementation
// =============================================================================

/// Builder state for storing configuration
#[derive(Debug, Clone, Default)]
struct BuilderState {
    agent_id: Option<String>,
    name: Option<String>,
    system_prompt: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    session_id: Option<String>,
    user_id: Option<String>,
    tenant_id: Option<String>,
    context_window_size: Option<usize>,
    openai_api_key: Option<String>,
    openai_base_url: Option<String>,
    openai_model: Option<String>,
}

/// LLM Agent Builder - fluent builder for creating LLMAgent instances
///
/// This is the primary interface for building LLMAgent instances from Python,
/// Kotlin, Swift, and Java.
pub struct LLMAgentBuilder {
    state: Arc<StdMutex<BuilderState>>,
    runtime: Arc<Runtime>,
}

impl LLMAgentBuilder {
    /// Create a new builder
    pub(crate) fn create() -> Result<Arc<Self>, MoFaError> {
        let runtime =
            tokio::runtime::Runtime::new().map_err(|e| MoFaError::RuntimeError(e.to_string()))?;
        Ok(Arc::new(Self {
            state: Arc::new(StdMutex::new(BuilderState::default())),
            runtime: Arc::new(runtime),
        }))
    }

    /// Set agent ID
    pub fn set_id(self: Arc<Self>, id: String) -> Arc<Self> {
        let mut state = self.state.lock().unwrap();
        state.agent_id = Some(id);
        drop(state);
        self
    }

    /// Set agent name
    pub fn set_name(self: Arc<Self>, name: String) -> Arc<Self> {
        let mut state = self.state.lock().unwrap();
        state.name = Some(name);
        drop(state);
        self
    }

    /// Set system prompt
    pub fn set_system_prompt(self: Arc<Self>, prompt: String) -> Arc<Self> {
        let mut state = self.state.lock().unwrap();
        state.system_prompt = Some(prompt);
        drop(state);
        self
    }

    /// Set temperature
    pub fn set_temperature(self: Arc<Self>, temperature: f32) -> Arc<Self> {
        let mut state = self.state.lock().unwrap();
        state.temperature = Some(temperature);
        drop(state);
        self
    }

    /// Set max tokens
    pub fn set_max_tokens(self: Arc<Self>, max_tokens: u32) -> Arc<Self> {
        let mut state = self.state.lock().unwrap();
        state.max_tokens = Some(max_tokens);
        drop(state);
        self
    }

    /// Set initial session ID
    pub fn set_session_id(self: Arc<Self>, session_id: String) -> Arc<Self> {
        let mut state = self.state.lock().unwrap();
        state.session_id = Some(session_id);
        drop(state);
        self
    }

    /// Set user ID
    pub fn set_user_id(self: Arc<Self>, user_id: String) -> Arc<Self> {
        let mut state = self.state.lock().unwrap();
        state.user_id = Some(user_id);
        drop(state);
        self
    }

    /// Set tenant ID
    pub fn set_tenant_id(self: Arc<Self>, tenant_id: String) -> Arc<Self> {
        let mut state = self.state.lock().unwrap();
        state.tenant_id = Some(tenant_id);
        drop(state);
        self
    }

    /// Set context window size (in rounds)
    pub fn set_context_window_size(self: Arc<Self>, size: u32) -> Arc<Self> {
        let mut state = self.state.lock().unwrap();
        state.context_window_size = Some(size as usize);
        drop(state);
        self
    }

    /// Set OpenAI provider
    pub fn set_openai_provider(
        self: Arc<Self>,
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
    ) -> Result<Arc<Self>, MoFaError> {
        let mut state = self.state.lock().unwrap();
        state.openai_api_key = Some(api_key);
        state.openai_base_url = base_url;
        state.openai_model = model;
        drop(state);
        Ok(self)
    }

    /// Build the LLMAgent synchronously
    pub fn build(self: Arc<Self>) -> Result<Arc<LLMAgent>, MoFaError> {
        use mofa_foundation::llm::{LLMAgentBuilder, OpenAIConfig, OpenAIProvider};
        use std::sync::Arc as StdArc;

        let state = self.state.lock().unwrap();

        let agent_id = state
            .agent_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());

        let mut builder = LLMAgentBuilder::new().with_id(&agent_id);

        if let Some(ref name) = state.name {
            builder = builder.with_name(name);
        }
        if let Some(ref prompt) = state.system_prompt {
            builder = builder.with_system_prompt(prompt);
        }
        if let Some(temp) = state.temperature {
            builder = builder.with_temperature(temp);
        }
        if let Some(tokens) = state.max_tokens {
            builder = builder.with_max_tokens(tokens);
        }
        if let Some(ref session_id) = state.session_id {
            builder = builder.with_session_id(session_id);
        }
        if let Some(ref user_id) = state.user_id {
            builder = builder.with_user(user_id);
        }
        if let Some(ref tenant_id) = state.tenant_id {
            builder = builder.with_tenant(tenant_id);
        }
        if let Some(size) = state.context_window_size {
            builder = builder.with_sliding_window(size);
        }
        if let Some(ref api_key) = state.openai_api_key {
            let mut config = OpenAIConfig::new(api_key.clone());
            if let Some(ref base_url) = state.openai_base_url {
                config = config.with_base_url(base_url);
            }
            if let Some(ref model) = state.openai_model {
                config = config.with_model(model);
            }
            let provider = StdArc::new(OpenAIProvider::with_config(config));
            builder = builder.with_provider(provider);
        }

        drop(state);

        let inner_agent = builder
            .try_build()
            .map_err(|e| MoFaError::ConfigError(e.to_string()))?;

        let agent_id = inner_agent.config().agent_id.clone();
        let name = inner_agent.config().name.clone();

        Ok(Arc::new(LLMAgent {
            agent_id,
            name,
            inner: Arc::new(RwLock::new(inner_agent)),
            _inner: std::marker::PhantomData,
            runtime: self.runtime.clone(),
            _runtime: std::marker::PhantomData,
        }))
    }
}

// =============================================================================
// Session Implementation
// =============================================================================

/// A conversation session holding messages and metadata.
/// Wraps mofa_foundation::agent::session::Session for FFI.
pub struct Session {
    inner: StdMutex<mofa_foundation::agent::session::Session>,
}

impl Session {
    /// Create a new empty session
    pub fn new(key: String) -> Self {
        Self {
            inner: StdMutex::new(mofa_foundation::agent::session::Session::new(key)),
        }
    }

    /// Get the session key
    pub fn get_key(&self) -> String {
        self.inner.lock().unwrap().key.clone()
    }

    /// Add a message to the session
    pub fn add_message(&self, role: String, content: String) {
        self.inner.lock().unwrap().add_message(role, content);
    }

    /// Get message history (most recent N messages)
    pub fn get_history(&self, max_messages: u32) -> Vec<SessionMessageInfo> {
        let session = self.inner.lock().unwrap();
        session
            .get_history(max_messages as usize)
            .iter()
            .map(|msg| SessionMessageInfo {
                role: msg.role.clone(),
                content: msg.content.clone(),
                timestamp: msg.timestamp.to_rfc3339(),
            })
            .collect()
    }

    /// Clear all messages
    pub fn clear(&self) {
        self.inner.lock().unwrap().clear();
    }

    /// Get the number of messages
    pub fn message_count(&self) -> u32 {
        self.inner.lock().unwrap().len() as u32
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().is_empty()
    }

    /// Set metadata value (JSON string)
    pub fn set_metadata(&self, key: String, value_json: String) -> Result<(), MoFaError> {
        let value: serde_json::Value = serde_json::from_str(&value_json).map_err(|e| {
            MoFaError::InvalidArgument(format!("Invalid JSON for metadata value: {}", e))
        })?;
        self.inner.lock().unwrap().metadata.insert(key, value);
        Ok(())
    }

    /// Get metadata value as JSON string
    pub fn get_metadata(&self, key: String) -> Option<String> {
        self.inner
            .lock()
            .unwrap()
            .metadata
            .get(&key)
            .map(|v| v.to_string())
    }

    /// Convert to the inner foundation Session (for saving)
    fn to_inner(&self) -> mofa_foundation::agent::session::Session {
        self.inner.lock().unwrap().clone()
    }

    /// Create from an inner foundation Session
    fn from_inner(session: mofa_foundation::agent::session::Session) -> Arc<Self> {
        Arc::new(Self {
            inner: StdMutex::new(session),
        })
    }
}

// =============================================================================
// Session Manager Implementation
// =============================================================================

/// Manages conversation sessions with pluggable storage
pub struct SessionManager {
    inner: Arc<RwLock<mofa_foundation::agent::session::SessionManager>>,
    runtime: Arc<Runtime>,
}

impl SessionManager {
    /// Create a new in-memory session manager
    /// Internal fallible constructor
    pub(crate) fn try_new_in_memory() -> Result<Self, MoFaError> {
        let runtime =
            tokio::runtime::Runtime::new().map_err(|e| MoFaError::RuntimeError(e.to_string()))?;
        let storage = Box::new(mofa_foundation::agent::session::MemorySessionStorage::new());
        let manager = mofa_foundation::agent::session::SessionManager::with_storage(storage);
        Ok(Self {
            inner: Arc::new(RwLock::new(manager)),
            runtime: Arc::new(runtime),
        })
    }

    /// FFI-safe infallible constructor. Logs error and aborts if runtime creation fails.
    pub fn new_in_memory() -> Self {
        match Self::try_new_in_memory() {
            Ok(manager) => manager,
            Err(e) => {
                eprintln!(
                    "SessionManager::new_in_memory: failed to create in-memory session manager: {}",
                    e
                );
                std::process::abort();
            }
        }
    }

    /// Create a file-backed session manager (JSONL storage)
    pub fn new_with_storage(workspace_path: String) -> Result<Self, MoFaError> {
        let runtime =
            tokio::runtime::Runtime::new().map_err(|e| MoFaError::RuntimeError(e.to_string()))?;

        let manager = runtime
            .block_on(mofa_foundation::agent::session::SessionManager::with_jsonl(
                &workspace_path,
            ))
            .map_err(|e| MoFaError::SessionError(e.to_string()))?;

        Ok(Self {
            inner: Arc::new(RwLock::new(manager)),
            runtime: Arc::new(runtime),
        })
    }

    /// Get or create a session by key
    pub fn get_or_create(&self, key: String) -> Result<Arc<Session>, MoFaError> {
        let session = self.runtime.block_on(async {
            let manager = self.inner.read().await;
            manager.get_or_create(&key).await
        });
        Ok(Session::from_inner(session))
    }

    /// Get a session by key (returns None if not found)
    pub fn get_session(&self, key: String) -> Result<Option<Arc<Session>>, MoFaError> {
        let result = self.runtime.block_on(async {
            let manager = self.inner.read().await;
            manager.get(&key).await
        });
        match result {
            Ok(Some(session)) => Ok(Some(Session::from_inner(session))),
            Ok(None) => Ok(None),
            Err(e) => Err(MoFaError::SessionError(e.to_string())),
        }
    }

    /// Save a session to storage
    pub fn save_session(&self, session: Arc<Session>) -> Result<(), MoFaError> {
        let inner_session = session.to_inner();
        self.runtime
            .block_on(async {
                let manager = self.inner.read().await;
                manager.save(&inner_session).await
            })
            .map_err(|e| MoFaError::SessionError(e.to_string()))
    }

    /// Delete a session by key
    pub fn delete_session(&self, key: String) -> Result<bool, MoFaError> {
        self.runtime
            .block_on(async {
                let manager = self.inner.read().await;
                manager.delete(&key).await
            })
            .map_err(|e| MoFaError::SessionError(e.to_string()))
    }

    /// List all session keys
    pub fn list_sessions(&self) -> Result<Vec<String>, MoFaError> {
        self.runtime
            .block_on(async {
                let manager = self.inner.read().await;
                manager.list().await
            })
            .map_err(|e| MoFaError::SessionError(e.to_string()))
    }
}

// =============================================================================
// Tool Registry Implementation
// =============================================================================

/// Adapter that wraps a foreign FfiToolCallback into the kernel Tool trait
struct CallbackToolAdapter {
    callback: Box<dyn FfiToolCallback>,
    /// Cached name to avoid `Box::leak` on every `name()` call
    cached_name: String,
    /// Cached description to avoid `Box::leak` on every `description()` call
    cached_description: String,
}

impl CallbackToolAdapter {
    fn new(callback: Box<dyn FfiToolCallback>) -> Self {
        let cached_name = callback.name();
        let cached_description = callback.description();
        Self {
            callback,
            cached_name,
            cached_description,
        }
    }
}

#[async_trait::async_trait]
impl mofa_kernel::agent::components::tool::Tool for CallbackToolAdapter {
    fn name(&self) -> &str {
        &self.cached_name
    }

    fn description(&self) -> &str {
        &self.cached_description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        let json_str = self.callback.parameters_schema_json();
        serde_json::from_str(&json_str).unwrap_or(serde_json::Value::Object(Default::default()))
    }

    async fn execute(
        &self,
        input: mofa_kernel::agent::components::tool::ToolInput,
        _ctx: &mofa_kernel::agent::context::AgentContext,
    ) -> mofa_kernel::agent::components::tool::ToolResult {
        let arguments_json = input.arguments.to_string();
        let result = self.callback.execute(arguments_json);

        if result.success {
            let output = serde_json::from_str(&result.output_json)
                .unwrap_or(serde_json::Value::String(result.output_json));
            mofa_kernel::agent::components::tool::ToolResult::success(output)
        } else {
            mofa_kernel::agent::components::tool::ToolResult::failure(
                result
                    .error
                    .unwrap_or_else(|| "Unknown tool error".to_string()),
            )
        }
    }
}

/// Registry for managing tools that agents can invoke
pub struct ToolRegistry {
    inner: StdMutex<mofa_foundation::agent::components::tool::SimpleToolRegistry>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    /// Create a new empty tool registry
    pub fn new() -> Self {
        Self {
            inner: StdMutex::new(
                mofa_foundation::agent::components::tool::SimpleToolRegistry::new(),
            ),
        }
    }

    /// Register a foreign-language tool via callback
    pub fn register_tool(&self, tool: Box<dyn FfiToolCallback>) -> Result<(), MoFaError> {
        use mofa_kernel::agent::components::tool::{ToolExt, ToolRegistry as _};
        let adapter = CallbackToolAdapter::new(tool);
        let tool_arc = adapter.into_dynamic();
        self.inner
            .lock()
            .unwrap()
            .register(tool_arc)
            .map_err(|e: mofa_kernel::agent::error::AgentError| MoFaError::ToolError(e.to_string()))
    }

    /// Unregister a tool by name
    pub fn unregister_tool(&self, name: String) -> Result<bool, MoFaError> {
        use mofa_kernel::agent::components::tool::ToolRegistry as _;
        self.inner
            .lock()
            .unwrap()
            .unregister(&name)
            .map_err(|e: mofa_kernel::agent::error::AgentError| MoFaError::ToolError(e.to_string()))
    }

    /// List all registered tools
    pub fn list_tools(&self) -> Vec<ToolInfo> {
        use mofa_kernel::agent::components::tool::ToolRegistry as _;
        self.inner
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .map(|desc| ToolInfo {
                name: desc.name,
                description: desc.description,
                parameters_schema_json: desc.parameters_schema.to_string(),
            })
            .collect()
    }

    /// Get tool names
    pub fn list_tool_names(&self) -> Vec<String> {
        use mofa_kernel::agent::components::tool::ToolRegistry as _;
        self.inner.lock().unwrap().list_names()
    }

    /// Check if a tool exists
    pub fn has_tool(&self, name: String) -> bool {
        use mofa_kernel::agent::components::tool::ToolRegistry as _;
        self.inner.lock().unwrap().contains(&name)
    }

    /// Get the number of registered tools
    pub fn tool_count(&self) -> u32 {
        use mofa_kernel::agent::components::tool::ToolRegistry as _;
        self.inner.lock().unwrap().count() as u32
    }

    /// Execute a tool by name with JSON arguments
    pub fn execute_tool(
        &self,
        name: String,
        arguments_json: String,
    ) -> Result<FfiToolResult, MoFaError> {
        use mofa_kernel::agent::components::tool::ToolRegistry as _;

        let registry = self.inner.lock().unwrap();
        let tool = registry
            .get(&name)
            .ok_or_else(|| MoFaError::ToolError(format!("Tool not found: {}", name)))?;

        let arguments: serde_json::Value = serde_json::from_str(&arguments_json)
            .map_err(|e| MoFaError::InvalidArgument(format!("Invalid JSON arguments: {}", e)))?;

        // Execute synchronously using a runtime
        let runtime =
            tokio::runtime::Runtime::new().map_err(|e| MoFaError::RuntimeError(e.to_string()))?;

        let ctx = mofa_kernel::agent::context::AgentContext::new("ffi-execution");
        let result = runtime.block_on(tool.execute_dynamic(arguments, &ctx));

        match result {
            Ok(output) => Ok(FfiToolResult {
                success: true,
                output_json: output.to_string(),
                error: None,
            }),
            Err(e) => Ok(FfiToolResult {
                success: false,
                output_json: "{}".to_string(),
                error: Some(e.to_string()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_last_output_returns_explicit_runtime_error() {
        let config = LLMConfig {
            provider: LLMProviderType::Ollama,
            model: Some("llama2".to_string()),
            api_key: None,
            base_url: None,
            deployment: None,
            temperature: Some(0.2),
            max_tokens: Some(128),
            system_prompt: Some("You are a test agent".to_string()),
        };

        let agent = LLMAgent::from_config(
            config,
            "ffi-test-agent".to_string(),
            "FFI Test Agent".to_string(),
        )
        .expect("ollama config should build without network calls");

        let err = agent
            .get_last_output()
            .expect_err("get_last_output should return explicit unsupported error");

        let msg = err.to_string();
        assert!(msg.contains("Runtime error:"));
        assert!(msg.contains("get_last_output is not yet supported"));
    }

    #[test]
    fn udl_contract_includes_required_ffi_surface() {
        // Contract guard: CI should fail when critical UDL entries drift from
        // the implemented UniFFI
        let udl = include_str!("mofa.udl");

        for required in [
            "dictionary LLMConfig",
            "[Throws=MoFaError, Name=from_config_file]",
            "[Throws=MoFaError, Name=from_config]",
            "LLMAgentBuilder set_openai_provider(",
        ] {
            assert!(
                udl.contains(required),
                "missing required UDL contract marker: {required}"
            );
        }
    }
}
