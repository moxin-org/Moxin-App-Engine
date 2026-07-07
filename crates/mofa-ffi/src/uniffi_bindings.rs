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

/// MoFA error type for UniFFI.
///
/// Intentionally NOT `#[non_exhaustive]` — UniFFI generates exhaustive matches
/// across the FFI boundary and requires all variants to be known at compile time.
///
/// At the FFI boundary every `error_stack::Report<*>` from internal code is
/// downcast to the closest `MoFaError` category via [`From`] impls below.
#[derive(Debug, thiserror::Error)]
pub enum MoFaError {
    #[error("Configuration error: {0}")]
    ConfigError(String),
    #[error("Runtime error: {0}")]
    RuntimeError(String),
    #[error("LLM error: {0}")]
    LLMError(String),
    #[error("IO error: {0}")]
    IoError(String),
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    #[error("Tool error: {0}")]
    ToolError(String),
    #[error("Session error: {0}")]
    SessionError(String),
}

/// Convenience result alias for UniFFI-exposed functions.
///
/// Always `Result<T, MoFaError>` — `error_stack::Report` cannot cross the FFI
/// boundary. Use the [`From`] impls below to convert internal reports here.
pub type MoFaResult<T> = Result<T, MoFaError>;

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

/// Typed FFI value kind for tool schemas, arguments, and outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolValueKind {
    Null,
    Bool,
    Int,
    Float,
    String,
    List,
    Object,
}

/// Object entry for a typed FFI value.
#[derive(Debug, Clone)]
pub struct ToolObjectEntry {
    pub key: String,
    pub value: ToolValue,
}

/// Typed FFI value used across the UniFFI tool contract.
#[derive(Debug, Clone)]
pub struct ToolValue {
    pub kind: ToolValueKind,
    pub bool_value: Option<bool>,
    pub int_value: Option<i64>,
    pub float_value: Option<f64>,
    pub string_value: Option<String>,
    pub list_value: Option<Vec<ToolValue>>,
    pub object_entries: Option<Vec<ToolObjectEntry>>,
}

impl ToolValue {
    fn null() -> Self {
        Self {
            kind: ToolValueKind::Null,
            bool_value: None,
            int_value: None,
            float_value: None,
            string_value: None,
            list_value: None,
            object_entries: None,
        }
    }

    fn from_json_value(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self::null(),
            serde_json::Value::Bool(v) => Self {
                kind: ToolValueKind::Bool,
                bool_value: Some(v),
                int_value: None,
                float_value: None,
                string_value: None,
                list_value: None,
                object_entries: None,
            },
            serde_json::Value::Number(v) => {
                if let Some(i) = v.as_i64() {
                    Self {
                        kind: ToolValueKind::Int,
                        bool_value: None,
                        int_value: Some(i),
                        float_value: None,
                        string_value: None,
                        list_value: None,
                        object_entries: None,
                    }
                } else if let Some(u) = v.as_u64() {
                    if let Ok(i) = i64::try_from(u) {
                        Self {
                            kind: ToolValueKind::Int,
                            bool_value: None,
                            int_value: Some(i),
                            float_value: None,
                            string_value: None,
                            list_value: None,
                            object_entries: None,
                        }
                    } else {
                        Self {
                            kind: ToolValueKind::Float,
                            bool_value: None,
                            int_value: None,
                            float_value: Some(u as f64),
                            string_value: None,
                            list_value: None,
                            object_entries: None,
                        }
                    }
                } else {
                    Self {
                        kind: ToolValueKind::Float,
                        bool_value: None,
                        int_value: None,
                        float_value: v.as_f64(),
                        string_value: None,
                        list_value: None,
                        object_entries: None,
                    }
                }
            }
            serde_json::Value::String(v) => Self {
                kind: ToolValueKind::String,
                bool_value: None,
                int_value: None,
                float_value: None,
                string_value: Some(v),
                list_value: None,
                object_entries: None,
            },
            serde_json::Value::Array(values) => Self {
                kind: ToolValueKind::List,
                bool_value: None,
                int_value: None,
                float_value: None,
                string_value: None,
                list_value: Some(values.into_iter().map(Self::from_json_value).collect()),
                object_entries: None,
            },
            serde_json::Value::Object(values) => Self {
                kind: ToolValueKind::Object,
                bool_value: None,
                int_value: None,
                float_value: None,
                string_value: None,
                list_value: None,
                object_entries: Some(
                    values
                        .into_iter()
                        .map(|(key, value)| ToolObjectEntry {
                            key,
                            value: Self::from_json_value(value),
                        })
                        .collect(),
                ),
            },
        }
    }

    fn to_json_value(&self) -> MoFaResult<serde_json::Value> {
        match self.kind {
            ToolValueKind::Null => Ok(serde_json::Value::Null),
            ToolValueKind::Bool => self.bool_value.map(serde_json::Value::Bool).ok_or_else(|| {
                MoFaError::InvalidArgument("ToolValue.bool_value is required".to_string())
            }),
            ToolValueKind::Int => self.int_value.map(serde_json::Value::from).ok_or_else(|| {
                MoFaError::InvalidArgument("ToolValue.int_value is required".to_string())
            }),
            ToolValueKind::Float => {
                let value = self.float_value.ok_or_else(|| {
                    MoFaError::InvalidArgument("ToolValue.float_value is required".to_string())
                })?;
                let number = serde_json::Number::from_f64(value).ok_or_else(|| {
                    MoFaError::InvalidArgument(format!(
                        "ToolValue.float_value must be finite, got {}",
                        value
                    ))
                })?;
                Ok(serde_json::Value::Number(number))
            }
            ToolValueKind::String => self
                .string_value
                .clone()
                .map(serde_json::Value::String)
                .ok_or_else(|| {
                    MoFaError::InvalidArgument("ToolValue.string_value is required".to_string())
                }),
            ToolValueKind::List => {
                let values = self.list_value.as_ref().ok_or_else(|| {
                    MoFaError::InvalidArgument("ToolValue.list_value is required".to_string())
                })?;
                Ok(serde_json::Value::Array(
                    values
                        .iter()
                        .map(|value| value.to_json_value())
                        .collect::<Result<Vec<_>, _>>()?,
                ))
            }
            ToolValueKind::Object => {
                let entries = self.object_entries.as_ref().ok_or_else(|| {
                    MoFaError::InvalidArgument("ToolValue.object_entries is required".to_string())
                })?;
                let mut map = serde_json::Map::with_capacity(entries.len());
                for entry in entries {
                    map.insert(entry.key.clone(), entry.value.to_json_value()?);
                }
                Ok(serde_json::Value::Object(map))
            }
        }
    }
}

/// Typed schema format for tool descriptors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSchemaFormat {
    JsonSchema,
}

/// Typed schema descriptor for foreign-language tools.
#[derive(Debug, Clone)]
pub struct TypedToolSchema {
    pub format: ToolSchemaFormat,
    pub schema: ToolValue,
}

/// Typed tool input wrapper for foreign-language tools.
#[derive(Debug, Clone)]
pub struct TypedToolInput {
    pub arguments: ToolValue,
    pub raw_input: Option<String>,
}

/// Structured error kind for typed FFI tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiToolErrorKind {
    Validation,
    Execution,
    Serialization,
    Unknown,
}

/// Structured error payload for typed FFI tool execution.
#[derive(Debug, Clone)]
pub struct FfiToolError {
    pub kind: FfiToolErrorKind,
    pub message: String,
}

impl FfiToolError {
    fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: FfiToolErrorKind::Validation,
            message: message.into(),
        }
    }

    fn execution(message: impl Into<String>) -> Self {
        Self {
            kind: FfiToolErrorKind::Execution,
            message: message.into(),
        }
    }
}

fn normalize_typed_tool_result(result: TypedFfiToolResult) -> TypedFfiToolResult {
    if result.success && result.output.is_none() {
        TypedFfiToolResult {
            success: false,
            output: None,
            error: Some(FfiToolError {
                kind: FfiToolErrorKind::Validation,
                message: "Typed FFI tool reported success without an output payload".to_string(),
            }),
        }
    } else if !result.success && result.error.is_none() {
        TypedFfiToolResult {
            success: false,
            output: None,
            error: Some(FfiToolError {
                kind: FfiToolErrorKind::Unknown,
                message: "Typed FFI tool failed without an error payload".to_string(),
            }),
        }
    } else {
        result
    }
}

/// Tool description for listing through the legacy JSON-string contract.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub parameters_schema_json: String,
}

/// Tool description for listing through the typed FFI contract.
#[derive(Debug, Clone)]
pub struct TypedToolInfo {
    pub name: String,
    pub description: String,
    pub parameters_schema: TypedToolSchema,
}

/// FFI tool execution result for the legacy JSON-string contract.
#[derive(Debug, Clone)]
pub struct FfiToolResult {
    pub success: bool,
    pub output_json: String,
    pub error: Option<String>,
}

/// Typed FFI tool execution result.
#[derive(Debug, Clone)]
pub struct TypedFfiToolResult {
    pub success: bool,
    pub output: Option<ToolValue>,
    pub error: Option<FfiToolError>,
}

/// Callback interface for foreign-language tool implementations.
/// This is the legacy JSON-string contract kept for backward compatibility.
pub trait FfiToolCallback: Send + Sync {
    fn name(&self) -> String;
    fn description(&self) -> String;
    fn parameters_schema_json(&self) -> String;
    fn execute(&self, arguments_json: String) -> FfiToolResult;
}

/// Typed callback interface for foreign-language tool implementations.
pub trait TypedFfiToolCallback: Send + Sync {
    fn name(&self) -> String;
    fn description(&self) -> String;
    fn parameters_schema(&self) -> TypedToolSchema;
    fn execute(&self, input: TypedToolInput) -> TypedFfiToolResult;
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

/// Adapter that wraps a typed foreign callback into the kernel Tool trait.
struct TypedCallbackToolAdapter {
    callback: Arc<dyn TypedFfiToolCallback>,
    cached_name: String,
    cached_description: String,
}

impl TypedCallbackToolAdapter {
    fn new(callback: Arc<dyn TypedFfiToolCallback>) -> Self {
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

#[async_trait::async_trait]
impl mofa_kernel::agent::components::tool::Tool for TypedCallbackToolAdapter {
    fn name(&self) -> &str {
        &self.cached_name
    }

    fn description(&self) -> &str {
        &self.cached_description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        let schema = self.callback.parameters_schema();
        match schema.schema.to_json_value() {
            Ok(value) => value,
            Err(_) => serde_json::Value::Object(Default::default()),
        }
    }

    async fn execute(
        &self,
        input: mofa_kernel::agent::components::tool::ToolInput,
        _ctx: &mofa_kernel::agent::context::AgentContext,
    ) -> mofa_kernel::agent::components::tool::ToolResult {
        let typed_input = TypedToolInput {
            arguments: ToolValue::from_json_value(input.arguments),
            raw_input: input.raw_input,
        };
        let result = normalize_typed_tool_result(self.callback.execute(typed_input));

        if result.success {
            match result.output {
                Some(output) => match output.to_json_value() {
                    Ok(value) => mofa_kernel::agent::components::tool::ToolResult::success(value),
                    Err(err) => {
                        mofa_kernel::agent::components::tool::ToolResult::failure(err.to_string())
                    }
                },
                None => mofa_kernel::agent::components::tool::ToolResult::failure(
                    "Typed FFI tool reported success without an output payload",
                ),
            }
        } else {
            mofa_kernel::agent::components::tool::ToolResult::failure(
                result
                    .error
                    .map(|err| err.message)
                    .unwrap_or_else(|| "Unknown typed tool error".to_string()),
            )
        }
    }
}

/// Registry for managing tools that agents can invoke
pub struct ToolRegistry {
    inner: StdMutex<mofa_foundation::agent::components::tool::SimpleToolRegistry>,
    typed_callbacks: StdMutex<HashMap<String, Arc<dyn TypedFfiToolCallback>>>,
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
            typed_callbacks: StdMutex::new(HashMap::new()),
        }
    }

    /// Register a foreign-language tool via the legacy JSON-string callback contract.
    pub fn register_tool(&self, tool: Box<dyn FfiToolCallback>) -> Result<(), MoFaError> {
        use mofa_kernel::agent::components::tool::{ToolExt, ToolRegistry as _};
        let adapter = CallbackToolAdapter::new(tool);
        let tool_name = adapter.cached_name.clone();
        let tool_arc = adapter.into_dynamic();
        self.inner.lock().unwrap().register(tool_arc).map_err(
            |e: mofa_kernel::agent::error::AgentError| MoFaError::ToolError(e.to_string()),
        )?;
        self.typed_callbacks.lock().unwrap().remove(&tool_name);
        Ok(())
    }

    /// Register a foreign-language tool via the typed callback contract.
    pub fn register_typed_tool(
        &self,
        tool: Box<dyn TypedFfiToolCallback>,
    ) -> Result<(), MoFaError> {
        use mofa_kernel::agent::components::tool::{ToolExt, ToolRegistry as _};
        let callback: Arc<dyn TypedFfiToolCallback> = Arc::from(tool);
        let adapter = TypedCallbackToolAdapter::new(callback.clone());
        let tool_arc = adapter.into_dynamic();
        self.inner.lock().unwrap().register(tool_arc).map_err(
            |e: mofa_kernel::agent::error::AgentError| MoFaError::ToolError(e.to_string()),
        )?;
        self.typed_callbacks
            .lock()
            .unwrap()
            .insert(callback.name(), callback);
        Ok(())
    }

    /// Unregister a tool by name
    pub fn unregister_tool(&self, name: String) -> Result<bool, MoFaError> {
        use mofa_kernel::agent::components::tool::ToolRegistry as _;
        let removed = self.inner.lock().unwrap().unregister(&name).map_err(
            |e: mofa_kernel::agent::error::AgentError| MoFaError::ToolError(e.to_string()),
        )?;
        if removed {
            self.typed_callbacks.lock().unwrap().remove(&name);
        }
        Ok(removed)
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

    /// List all registered tools through the typed FFI contract.
    pub fn list_typed_tools(&self) -> Vec<TypedToolInfo> {
        use mofa_kernel::agent::components::tool::ToolRegistry as _;
        self.inner
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .map(|desc| TypedToolInfo {
                name: desc.name,
                description: desc.description,
                parameters_schema: TypedToolSchema {
                    format: ToolSchemaFormat::JsonSchema,
                    schema: ToolValue::from_json_value(desc.parameters_schema),
                },
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

    /// Execute a tool by name through the typed FFI contract.
    pub fn execute_typed_tool(
        &self,
        name: String,
        input: TypedToolInput,
    ) -> Result<TypedFfiToolResult, MoFaError> {
        if let Some(callback) = self.typed_callbacks.lock().unwrap().get(&name).cloned() {
            return Ok(normalize_typed_tool_result(callback.execute(input)));
        }

        use mofa_kernel::agent::components::tool::ToolRegistry as _;

        let registry = self.inner.lock().unwrap();
        let tool = registry
            .get(&name)
            .ok_or_else(|| MoFaError::ToolError(format!("Tool not found: {}", name)))?;

        let arguments = match input.arguments.to_json_value() {
            Ok(value) => value,
            Err(err) => {
                return Ok(TypedFfiToolResult {
                    success: false,
                    output: None,
                    error: Some(FfiToolError::validation(format!(
                        "Invalid typed tool arguments: {}",
                        err
                    ))),
                });
            }
        };

        let runtime =
            tokio::runtime::Runtime::new().map_err(|e| MoFaError::RuntimeError(e.to_string()))?;

        let ctx = mofa_kernel::agent::context::AgentContext::new("ffi-execution");
        let result = runtime.block_on(tool.execute_dynamic(arguments, &ctx));

        match result {
            Ok(output) => Ok(TypedFfiToolResult {
                success: true,
                output: Some(ToolValue::from_json_value(output)),
                error: None,
            }),
            Err(e) => Ok(TypedFfiToolResult {
                success: false,
                output: None,
                error: Some(FfiToolError::execution(e.to_string())),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoTypedTool;

    struct SuccessWithoutOutputTypedTool;

    struct FailureWithoutErrorTypedTool;

    struct EchoLegacyTool;

    impl TypedFfiToolCallback for EchoTypedTool {
        fn name(&self) -> String {
            "echo_typed".to_string()
        }

        fn description(&self) -> String {
            "Echo typed payload".to_string()
        }

        fn parameters_schema(&self) -> TypedToolSchema {
            TypedToolSchema {
                format: ToolSchemaFormat::JsonSchema,
                schema: ToolValue::from_json_value(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" }
                    },
                    "required": ["message"]
                })),
            }
        }

        fn execute(&self, input: TypedToolInput) -> TypedFfiToolResult {
            TypedFfiToolResult {
                success: true,
                output: Some(input.arguments),
                error: None,
            }
        }
    }

    impl TypedFfiToolCallback for SuccessWithoutOutputTypedTool {
        fn name(&self) -> String {
            "success_without_output".to_string()
        }

        fn description(&self) -> String {
            "Returns success without output".to_string()
        }

        fn parameters_schema(&self) -> TypedToolSchema {
            TypedToolSchema {
                format: ToolSchemaFormat::JsonSchema,
                schema: ToolValue::from_json_value(serde_json::json!({
                    "type": "object"
                })),
            }
        }

        fn execute(&self, _input: TypedToolInput) -> TypedFfiToolResult {
            TypedFfiToolResult {
                success: true,
                output: None,
                error: None,
            }
        }
    }

    impl TypedFfiToolCallback for FailureWithoutErrorTypedTool {
        fn name(&self) -> String {
            "failure_without_error".to_string()
        }

        fn description(&self) -> String {
            "Returns failure without an error payload".to_string()
        }

        fn parameters_schema(&self) -> TypedToolSchema {
            TypedToolSchema {
                format: ToolSchemaFormat::JsonSchema,
                schema: ToolValue::from_json_value(serde_json::json!({
                    "type": "object"
                })),
            }
        }

        fn execute(&self, _input: TypedToolInput) -> TypedFfiToolResult {
            TypedFfiToolResult {
                success: false,
                output: None,
                error: None,
            }
        }
    }

    impl FfiToolCallback for EchoLegacyTool {
        fn name(&self) -> String {
            "echo_legacy".to_string()
        }

        fn description(&self) -> String {
            "Echo legacy JSON payload".to_string()
        }

        fn parameters_schema_json(&self) -> String {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                }
            })
            .to_string()
        }

        fn execute(&self, arguments_json: String) -> FfiToolResult {
            FfiToolResult {
                success: true,
                output_json: arguments_json,
                error: None,
            }
        }
    }

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
            "dictionary ToolValue",
            "callback interface TypedFfiToolCallback",
            "void register_typed_tool(TypedFfiToolCallback tool);",
            "sequence<TypedToolInfo> list_typed_tools();",
            "TypedFfiToolResult execute_typed_tool(string name, TypedToolInput input);",
        ] {
            assert!(
                udl.contains(required),
                "missing required UDL contract marker: {required}"
            );
        }
    }

    #[test]
    fn tool_value_roundtrip_preserves_nested_json() {
        let json = serde_json::json!({
            "message": "hello",
            "count": 3,
            "flags": [true, false],
            "meta": {
                "nested": "value"
            }
        });

        let value = ToolValue::from_json_value(json.clone());
        let roundtrip = value.to_json_value().expect("tool value should roundtrip");

        assert_eq!(roundtrip, json);
    }

    #[test]
    fn execute_typed_tool_uses_typed_contract_end_to_end() {
        let registry = ToolRegistry::new();
        registry
            .register_typed_tool(Box::new(EchoTypedTool))
            .expect("typed tool should register");

        let tools = registry.list_typed_tools();
        assert!(tools.iter().any(|tool| tool.name == "echo_typed"));

        let input = TypedToolInput {
            arguments: ToolValue::from_json_value(serde_json::json!({
                "message": "hello"
            })),
            raw_input: Some("hello".to_string()),
        };

        let result = registry
            .execute_typed_tool("echo_typed".to_string(), input)
            .expect("typed execution should succeed");

        assert!(result.success);
        let output = result
            .output
            .as_ref()
            .expect("typed tool should return an output");
        let output_json = output
            .to_json_value()
            .expect("output should convert to json");
        assert_eq!(output_json, serde_json::json!({ "message": "hello" }));
    }

    #[test]
    fn execute_typed_tool_normalizes_success_without_output() {
        let registry = ToolRegistry::new();
        registry
            .register_typed_tool(Box::new(SuccessWithoutOutputTypedTool))
            .expect("typed tool should register");

        let result = registry
            .execute_typed_tool(
                "success_without_output".to_string(),
                TypedToolInput {
                    arguments: ToolValue::from_json_value(serde_json::json!({})),
                    raw_input: None,
                },
            )
            .expect("typed execution should return a normalized result");

        assert!(!result.success);
        let error = result
            .error
            .expect("normalized result should contain error");
        assert_eq!(error.kind, FfiToolErrorKind::Validation);
        assert!(error.message.contains("without an output payload"));
    }

    #[test]
    fn execute_typed_tool_normalizes_failure_without_error() {
        let registry = ToolRegistry::new();
        registry
            .register_typed_tool(Box::new(FailureWithoutErrorTypedTool))
            .expect("typed tool should register");

        let result = registry
            .execute_typed_tool(
                "failure_without_error".to_string(),
                TypedToolInput {
                    arguments: ToolValue::from_json_value(serde_json::json!({})),
                    raw_input: None,
                },
            )
            .expect("typed execution should return a normalized result");

        assert!(!result.success);
        let error = result
            .error
            .expect("normalized result should contain error");
        assert_eq!(error.kind, FfiToolErrorKind::Unknown);
        assert!(error.message.contains("without an error payload"));
    }

    #[test]
    fn execute_typed_tool_rejects_invalid_typed_arguments() {
        let registry = ToolRegistry::new();
        registry
            .register_tool(Box::new(EchoLegacyTool))
            .expect("legacy tool should register");

        let result = registry
            .execute_typed_tool(
                "echo_legacy".to_string(),
                TypedToolInput {
                    arguments: ToolValue {
                        kind: ToolValueKind::Object,
                        bool_value: None,
                        int_value: None,
                        float_value: None,
                        string_value: None,
                        list_value: None,
                        object_entries: None,
                    },
                    raw_input: None,
                },
            )
            .expect("typed execution should return a validation result");

        assert!(!result.success);
        let error = result
            .error
            .expect("invalid typed args should report error");
        assert_eq!(error.kind, FfiToolErrorKind::Validation);
        assert!(error.message.contains("Invalid typed tool arguments"));
    }

    #[test]
    fn unregister_tool_removes_typed_callback_execution_path() {
        let registry = ToolRegistry::new();
        registry
            .register_typed_tool(Box::new(EchoTypedTool))
            .expect("typed tool should register");

        let removed = registry
            .unregister_tool("echo_typed".to_string())
            .expect("unregister should succeed");
        assert!(removed);

        let err = registry
            .execute_typed_tool(
                "echo_typed".to_string(),
                TypedToolInput {
                    arguments: ToolValue::from_json_value(serde_json::json!({
                        "message": "hello"
                    })),
                    raw_input: None,
                },
            )
            .expect_err("unregistered typed tool should not execute");

        assert!(err.to_string().contains("Tool not found"));
    }
}
