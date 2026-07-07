#![allow(dead_code, unused_imports, improper_ctypes_definitions)]
//! Plugin mechanism module
//!
//! Provides a complete plugin system, supporting:
//! - Plugin lifecycle management
//! - Various plugin types (LLM, Tool, Storage, Memory, etc.)
//! - Plugin registration and discovery
//! - Inter-plugin communication and dependency management
//! - Event hook mechanism
//! - Agent Skills support

// Unified error conversions (GlobalError <-> plugin errors)
pub mod error_conversions;

pub mod asr;
pub mod hot_reload;
pub mod skill;
pub mod tool;
pub mod tools;
pub mod tts;
pub mod wasm_runtime;

pub use asr::{ASREngine, ASRPlugin, ASRPluginConfig, MockASREngine};

pub use mofa_kernel::{
    AgentPlugin, PluginConfig, PluginContext, PluginError, PluginEvent, PluginMetadata,
    PluginResult, PluginState, PluginType,
};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
// ============================================================================
// LLM 插件
// LLM Plugin
// ============================================================================

/// LLM 客户端 trait
/// LLM Client trait
#[async_trait::async_trait]
pub trait LLMClient: Send + Sync {
    /// 生成文本
    /// Generate text
    async fn generate(&self, prompt: &str) -> PluginResult<String>;

    /// 流式生成
    /// Stream generation
    async fn generate_stream(
        &self,
        prompt: &str,
        callback: Box<dyn Fn(String) + Send + Sync>,
    ) -> PluginResult<String>;

    /// 聊天完成
    /// Chat completion
    async fn chat(&self, messages: Vec<ChatMessage>) -> PluginResult<String>;

    /// 获取嵌入向量
    /// Get embedding vector
    async fn embedding(&self, text: &str) -> PluginResult<Vec<f32>>;
}

/// 聊天消息
/// Chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: &str) -> Self {
        Self {
            role: "system".to_string(),
            content: content.to_string(),
        }
    }

    pub fn user(content: &str) -> Self {
        Self {
            role: "user".to_string(),
            content: content.to_string(),
        }
    }

    pub fn assistant(content: &str) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.to_string(),
        }
    }
}

/// LLM 插件配置
/// LLM plugin configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMPluginConfig {
    /// 模型名称
    /// Model name
    pub model: String,
    /// API 密钥
    /// API key
    pub api_key: Option<String>,
    /// API 基础 URL
    /// API base URL
    pub base_url: Option<String>,
    /// 最大 token 数
    /// Max tokens
    pub max_tokens: usize,
    /// 温度参数
    /// Temperature parameter
    pub temperature: f32,
    /// 超时时间（秒）
    /// Timeout (seconds)
    pub timeout_secs: u64,
}

impl Default for LLMPluginConfig {
    fn default() -> Self {
        Self {
            model: "gpt-3.5-turbo".to_string(),
            api_key: None,
            base_url: None,
            max_tokens: 2048,
            temperature: 0.7,
            timeout_secs: 30,
        }
    }
}

/// OpenAI 客户端实现
/// OpenAI client implementation
pub struct OpenAIClient {
    config: LLMPluginConfig,
}

impl OpenAIClient {
    pub fn new(config: LLMPluginConfig) -> Self {
        Self { config }
    }
}

#[async_trait::async_trait]
impl LLMClient for OpenAIClient {
    async fn generate(&self, prompt: &str) -> PluginResult<String> {
        // 模拟实现，实际应调用 OpenAI API
        // Mock implementation, should call OpenAI API in reality
        debug!(
            "OpenAI generating response for prompt: {}...",
            &prompt[..prompt.len().min(50)]
        );
        Ok(format!(
            "[{}] Generated response to: {}",
            self.config.model, prompt
        ))
    }

    async fn generate_stream(
        &self,
        prompt: &str,
        callback: Box<dyn Fn(String) + Send + Sync>,
    ) -> PluginResult<String> {
        // 模拟流式生成 TODO
        // Mock stream generation TODO
        // Stub: simulates word-level streaming with inter-token spacing.
        // Replace with a real SSE/delta streaming call (e.g. via `async-openai`)
        // once live credentials and an HTTP client are wired in.
        let response = format!("[{}] Stream response to: {}", self.config.model, prompt);
        let words: Vec<&str> = response.split_whitespace().collect();
        let len = words.len();
        for (i, word) in words.iter().enumerate() {
            let chunk = if i + 1 < len {
                format!("{} ", word) // preserve trailing space between tokens
            } else {
                word.to_string()
            };
            callback(chunk);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        Ok(response)
    }

    async fn chat(&self, messages: Vec<ChatMessage>) -> PluginResult<String> {
        let last_message = messages.last().map(|m| m.content.as_str()).unwrap_or("");
        debug!("OpenAI chat with {} messages", messages.len());
        Ok(format!(
            "[{}] Chat response to: {}",
            self.config.model, last_message
        ))
    }

    async fn embedding(&self, text: &str) -> PluginResult<Vec<f32>> {
        // 模拟嵌入向量
        // Mock embedding vector
        debug!(
            "OpenAI generating embedding for text: {}...",
            &text[..text.len().min(50)]
        );
        Ok(vec![0.1, 0.2, 0.3, 0.4, 0.5])
    }
}

/// LLM 能力插件
/// LLM capability plugin
pub struct LLMPlugin {
    metadata: PluginMetadata,
    state: PluginState,
    config: LLMPluginConfig,
    client: Option<Arc<dyn LLMClient>>,
    call_count: u64,
    total_tokens: u64,
}

impl LLMPlugin {
    pub fn new(plugin_id: &str) -> Self {
        let metadata = PluginMetadata::new(plugin_id, "LLM Plugin", PluginType::LLM)
            .with_description("Large Language Model integration plugin")
            .with_capability("text_generation")
            .with_capability("chat")
            .with_capability("embedding");

        Self {
            metadata,
            state: PluginState::Unloaded,
            config: LLMPluginConfig::default(),
            client: None,
            call_count: 0,
            total_tokens: 0,
        }
    }

    pub fn with_config(mut self, config: LLMPluginConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_client<C: LLMClient + 'static>(mut self, client: C) -> Self {
        self.client = Some(Arc::new(client));
        self
    }

    /// 获取 LLM 客户端
    /// Get LLM client
    pub fn client(&self) -> Option<&Arc<dyn LLMClient>> {
        self.client.as_ref()
    }

    /// 聊天接口
    /// Chat interface
    pub async fn chat(&mut self, messages: Vec<ChatMessage>) -> PluginResult<String> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| PluginError::InitFailed("LLM client not initialized".into()))?;
        self.call_count += 1;
        client.chat(messages).await
    }

    /// 生成嵌入向量
    /// Generate embedding vector
    pub async fn embedding(&self, text: &str) -> PluginResult<Vec<f32>> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| PluginError::InitFailed("LLM client not initialized".into()))?;
        client.embedding(text).await
    }
}

#[async_trait::async_trait]
impl AgentPlugin for LLMPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    fn state(&self) -> PluginState {
        self.state.clone()
    }

    async fn load(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        self.state = PluginState::Loading;
        info!("Loading LLM plugin: {}", self.metadata.id);

        // 从上下文配置加载设置
        // Load settings from context config
        if let Some(model) = ctx.config.get_string("model") {
            self.config.model = model;
        }
        if let Some(api_key) = ctx.config.get_string("api_key") {
            self.config.api_key = Some(api_key);
        }

        self.state = PluginState::Loaded;
        Ok(())
    }

    async fn init_plugin(&mut self) -> PluginResult<()> {
        info!("Initializing LLM plugin: {}", self.metadata.id);

        // 初始化 LLM 客户端
        // Initialize LLM client
        if self.client.is_none() {
            self.client = Some(Arc::new(OpenAIClient::new(self.config.clone())));
        }

        Ok(())
    }

    async fn start(&mut self) -> PluginResult<()> {
        self.state = PluginState::Running;
        info!("LLM plugin {} started", self.metadata.id);
        Ok(())
    }

    async fn stop(&mut self) -> PluginResult<()> {
        self.state = PluginState::Loaded;
        info!("LLM plugin {} stopped", self.metadata.id);
        Ok(())
    }

    async fn unload(&mut self) -> PluginResult<()> {
        self.client = None;
        self.state = PluginState::Unloaded;
        info!("LLM plugin {} unloaded", self.metadata.id);
        Ok(())
    }

    async fn execute(&mut self, input: String) -> PluginResult<String> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| PluginError::InitFailed("LLM client not initialized".into()))?;
        self.call_count += 1;
        client.generate(&input).await
    }

    fn stats(&self) -> HashMap<String, serde_json::Value> {
        let mut stats = HashMap::new();
        stats.insert("call_count".to_string(), serde_json::json!(self.call_count));
        stats.insert(
            "total_tokens".to_string(),
            serde_json::json!(self.total_tokens),
        );
        stats.insert("model".to_string(), serde_json::json!(self.config.model));
        stats
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

// ============================================================================
// 工具插件
// Tool Plugin
// ============================================================================

pub mod rhai_runtime;

pub use rhai_runtime::*;
pub use tool::*;
pub use tools::*;
pub use tts::{
    AudioPlaybackConfig, MockTTSEngine, TTSCommand, TTSEngine, TTSPlugin, TTSPluginConfig,
    TextToSpeechTool, VoiceInfo, cache::ModelCache, model_downloader::HFHubClient, play_audio,
    play_audio_async,
};

// Re-export KokoroTTSWrapper when kokoro feature is enabled
#[cfg(feature = "kokoro")]
pub use tts::kokoro_wrapper::KokoroTTS;

/// 工具定义
/// Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// 工具名称
    /// Tool name
    pub name: String,
    /// 工具描述
    /// Tool description
    pub description: String,
    /// 参数 JSON Schema
    /// Parameters JSON Schema
    pub parameters: serde_json::Value,
    /// 是否需要确认
    /// Whether confirmation is required
    pub requires_confirmation: bool,
}

/// 工具调用请求
/// Tool call request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// 工具名称
    /// Tool name
    pub name: String,
    /// 调用参数
    /// Call arguments
    pub arguments: serde_json::Value,
    /// 调用 ID
    /// Call ID
    pub call_id: String,
}

/// 工具调用结果
/// Tool call result
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// 调用 ID
    /// Call ID
    pub call_id: String,
    /// 是否成功
    /// Whether successful
    pub success: bool,
    /// 结果数据
    /// Result data
    pub result: serde_json::Value,
    /// 错误信息
    /// Error message
    pub error: Option<String>,
}

/// 工具执行器 trait
/// Tool executor trait
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    /// 获取工具定义
    /// Get tool definition
    fn definition(&self) -> &ToolDefinition;

    /// 执行工具
    /// Execute tool
    async fn execute(&self, arguments: serde_json::Value) -> PluginResult<serde_json::Value>;

    /// 验证参数
    /// Validate arguments
    fn validate(&self, arguments: &serde_json::Value) -> PluginResult<()> {
        let _ = arguments;
        Ok(())
    }
}

/// 工具插件
/// Tool plugin
pub struct ToolPlugin {
    metadata: PluginMetadata,
    state: PluginState,
    tools: HashMap<String, Box<dyn ToolExecutor>>,
    call_history: Vec<ToolCall>,
}

impl ToolPlugin {
    pub fn new(plugin_id: &str) -> Self {
        let metadata = PluginMetadata::new(plugin_id, "Tool Plugin", PluginType::Tool)
            .with_description("Tool calling and execution plugin")
            .with_capability("tool_call")
            .with_capability("function_call");

        Self {
            metadata,
            state: PluginState::Unloaded,
            tools: HashMap::new(),
            call_history: Vec::new(),
        }
    }

    /// 注册工具
    /// Register tool
    pub fn register_tool<T: ToolExecutor + 'static>(&mut self, tool: T) {
        let name = tool.definition().name.clone();
        self.tools.insert(name.clone(), Box::new(tool));
        info!("Registered tool: {}", name);
    }

    /// 获取所有工具定义
    /// List all tool definitions
    pub fn list_tools(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|t| t.definition().clone())
            .collect()
    }

    /// 调用工具
    /// Call tool
    pub async fn call_tool(&mut self, call: ToolCall) -> PluginResult<ToolResult> {
        let tool = self.tools.get(&call.name).ok_or_else(|| {
            PluginError::ExecutionFailed(format!("Tool not found: {}", call.name))
        })?;

        // 验证参数
        // Validate arguments
        tool.validate(&call.arguments)?;

        // 记录调用
        // Record call
        self.call_history.push(call.clone());

        // 执行工具
        // Execute tool
        match tool.execute(call.arguments).await {
            Ok(result) => Ok(ToolResult {
                call_id: call.call_id,
                success: true,
                result,
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                call_id: call.call_id,
                success: false,
                result: serde_json::Value::Null,
                error: Some(e.to_string()),
            }),
        }
    }
}

#[async_trait::async_trait]
impl AgentPlugin for ToolPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    fn state(&self) -> PluginState {
        self.state.clone()
    }

    async fn load(&mut self, _ctx: &PluginContext) -> PluginResult<()> {
        self.state = PluginState::Loading;
        info!("Loading Tool plugin: {}", self.metadata.id);
        self.state = PluginState::Loaded;
        Ok(())
    }

    async fn init_plugin(&mut self) -> PluginResult<()> {
        info!("Initializing Tool plugin: {}", self.metadata.id);
        Ok(())
    }

    async fn start(&mut self) -> PluginResult<()> {
        self.state = PluginState::Running;
        info!(
            "Tool plugin {} started with {} tools",
            self.metadata.id,
            self.tools.len()
        );
        Ok(())
    }

    async fn stop(&mut self) -> PluginResult<()> {
        self.state = PluginState::Loaded;
        info!("Tool plugin {} stopped", self.metadata.id);
        Ok(())
    }

    async fn unload(&mut self) -> PluginResult<()> {
        self.tools.clear();
        self.state = PluginState::Unloaded;
        info!("Tool plugin {} unloaded", self.metadata.id);
        Ok(())
    }

    async fn execute(&mut self, input: String) -> PluginResult<String> {
        // 解析输入为工具调用
        // Parse input as tool call
        let call: ToolCall = serde_json::from_str(&input).map_err(|e| {
            PluginError::ExecutionFailed(format!("Invalid tool call format: {}", e))
        })?;
        let result = self.call_tool(call).await?;
        serde_json::to_string(&result)
            .map_err(|e| PluginError::ExecutionFailed(format!("Failed to serialize result: {}", e)))
    }

    fn stats(&self) -> HashMap<String, serde_json::Value> {
        let mut stats = HashMap::new();
        stats.insert(
            "tool_count".to_string(),
            serde_json::json!(self.tools.len()),
        );
        stats.insert(
            "call_count".to_string(),
            serde_json::json!(self.call_history.len()),
        );
        stats
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

// ============================================================================
// 存储插件
// Storage Plugin
// ============================================================================

/// 存储后端 trait
/// Storage backend trait
#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    /// 获取值
    /// Get value
    async fn get(&self, key: &str) -> PluginResult<Option<Vec<u8>>>;

    /// 设置值
    /// Set value
    async fn set(&self, key: &str, value: Vec<u8>) -> PluginResult<()>;

    /// 删除值
    /// Delete value
    async fn delete(&self, key: &str) -> PluginResult<bool>;

    /// 检查键是否存在
    /// Check if key exists
    async fn exists(&self, key: &str) -> PluginResult<bool>;

    /// 列出所有键
    /// List all keys
    async fn keys(&self, prefix: Option<&str>) -> PluginResult<Vec<String>>;
}

/// 内存存储后端
/// In-memory storage backend
pub struct MemoryStorage {
    data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl StorageBackend for MemoryStorage {
    async fn get(&self, key: &str) -> PluginResult<Option<Vec<u8>>> {
        let data = self.data.read().await;
        Ok(data.get(key).cloned())
    }

    async fn set(&self, key: &str, value: Vec<u8>) -> PluginResult<()> {
        let mut data = self.data.write().await;
        data.insert(key.to_string(), value);
        Ok(())
    }

    async fn delete(&self, key: &str) -> PluginResult<bool> {
        let mut data = self.data.write().await;
        Ok(data.remove(key).is_some())
    }

    async fn exists(&self, key: &str) -> PluginResult<bool> {
        let data = self.data.read().await;
        Ok(data.contains_key(key))
    }

    async fn keys(&self, prefix: Option<&str>) -> PluginResult<Vec<String>> {
        let data = self.data.read().await;
        Ok(match prefix {
            Some(p) => data.keys().filter(|k| k.starts_with(p)).cloned().collect(),
            None => data.keys().cloned().collect(),
        })
    }
}

/// 存储插件
/// Storage plugin
pub struct StoragePlugin {
    metadata: PluginMetadata,
    state: PluginState,
    backend: Option<Arc<dyn StorageBackend>>,
    read_count: u64,
    write_count: u64,
}

impl StoragePlugin {
    pub fn new(plugin_id: &str) -> Self {
        let metadata = PluginMetadata::new(plugin_id, "Storage Plugin", PluginType::Storage)
            .with_description("Key-value storage plugin")
            .with_capability("storage")
            .with_capability("persistence");

        Self {
            metadata,
            state: PluginState::Unloaded,
            backend: None,
            read_count: 0,
            write_count: 0,
        }
    }

    pub fn with_backend<B: StorageBackend + 'static>(mut self, backend: B) -> Self {
        self.backend = Some(Arc::new(backend));
        self
    }

    /// 获取值
    /// Get value
    pub async fn get(&mut self, key: &str) -> PluginResult<Option<Vec<u8>>> {
        let backend = self
            .backend
            .as_ref()
            .ok_or_else(|| PluginError::InitFailed("Storage backend not initialized".into()))?;
        self.read_count += 1;
        backend.get(key).await
    }

    /// 设置值
    /// Set value
    pub async fn set(&mut self, key: &str, value: Vec<u8>) -> PluginResult<()> {
        let backend = self
            .backend
            .as_ref()
            .ok_or_else(|| PluginError::InitFailed("Storage backend not initialized".into()))?;
        self.write_count += 1;
        backend.set(key, value).await
    }

    /// 删除值
    /// Delete value
    pub async fn delete(&mut self, key: &str) -> PluginResult<bool> {
        let backend = self
            .backend
            .as_ref()
            .ok_or_else(|| PluginError::InitFailed("Storage backend not initialized".into()))?;
        self.write_count += 1;
        backend.delete(key).await
    }

    /// 获取字符串值
    /// Get string value
    pub async fn get_string(&mut self, key: &str) -> PluginResult<Option<String>> {
        let data = self.get(key).await?;
        Ok(data.map(|d| String::from_utf8_lossy(&d).to_string()))
    }

    /// 设置字符串值
    /// Set string value
    pub async fn set_string(&mut self, key: &str, value: &str) -> PluginResult<()> {
        self.set(key, value.as_bytes().to_vec()).await
    }
}

#[async_trait::async_trait]
impl AgentPlugin for StoragePlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    fn state(&self) -> PluginState {
        self.state.clone()
    }

    async fn load(&mut self, _ctx: &PluginContext) -> PluginResult<()> {
        self.state = PluginState::Loading;
        info!("Loading Storage plugin: {}", self.metadata.id);
        self.state = PluginState::Loaded;
        Ok(())
    }

    async fn init_plugin(&mut self) -> PluginResult<()> {
        info!("Initializing Storage plugin: {}", self.metadata.id);
        if self.backend.is_none() {
            self.backend = Some(Arc::new(MemoryStorage::new()));
        }
        Ok(())
    }

    async fn start(&mut self) -> PluginResult<()> {
        self.state = PluginState::Running;
        info!("Storage plugin {} started", self.metadata.id);
        Ok(())
    }

    async fn stop(&mut self) -> PluginResult<()> {
        self.state = PluginState::Loaded;
        info!("Storage plugin {} stopped", self.metadata.id);
        Ok(())
    }

    async fn unload(&mut self) -> PluginResult<()> {
        self.backend = None;
        self.state = PluginState::Unloaded;
        info!("Storage plugin {} unloaded", self.metadata.id);
        Ok(())
    }

    async fn execute(&mut self, input: String) -> PluginResult<String> {
        // 简单的 get/set 命令解析
        // Simple get/set command parsing
        let parts: Vec<&str> = input.as_str().splitn(3, ' ').collect();
        match parts.as_slice() {
            ["get", key] => {
                let value = self.get_string(key).await?;
                Ok(value.unwrap_or_else(|| "null".to_string()))
            }
            ["set", key, value] => {
                self.set_string(key, value).await?;
                Ok("OK".to_string())
            }
            ["delete", key] => {
                let deleted = self.delete(key).await?;
                Ok(if deleted { "1" } else { "0" }.to_string())
            }
            _ => Err(PluginError::ExecutionFailed(
                "Invalid command. Use: get <key>, set <key> <value>, delete <key>".into(),
            )),
        }
    }

    fn stats(&self) -> HashMap<String, serde_json::Value> {
        let mut stats = HashMap::new();
        stats.insert("read_count".to_string(), serde_json::json!(self.read_count));
        stats.insert(
            "write_count".to_string(),
            serde_json::json!(self.write_count),
        );
        stats
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

// ============================================================================
// 记忆插件
// Memory Plugin
// ============================================================================

/// 记忆条目
/// Memory entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// 记忆 ID
    /// Memory ID
    pub id: String,
    /// 内容
    /// Content
    pub content: String,
    /// 嵌入向量
    /// Embedding vector
    pub embedding: Option<Vec<f32>>,
    /// 创建时间
    /// Creation time
    pub created_at: u64,
    /// 访问次数
    /// Access count
    pub access_count: u32,
    /// 重要性分数
    /// Importance score
    pub importance: f32,
    /// 元数据
    /// Metadata
    pub metadata: HashMap<String, String>,
}

/// 记忆插件
/// Memory plugin
pub struct MemoryPlugin {
    metadata: PluginMetadata,
    state: PluginState,
    memories: Vec<MemoryEntry>,
    max_memories: usize,
}

impl MemoryPlugin {
    pub fn new(plugin_id: &str) -> Self {
        let metadata = PluginMetadata::new(plugin_id, "Memory Plugin", PluginType::Memory)
            .with_description("Agent memory management plugin")
            .with_capability("short_term_memory")
            .with_capability("long_term_memory")
            .with_capability("memory_retrieval");

        Self {
            metadata,
            state: PluginState::Unloaded,
            memories: Vec::new(),
            max_memories: 1000,
        }
    }

    pub fn with_max_memories(mut self, max: usize) -> Self {
        self.max_memories = max;
        self
    }

    /// 添加记忆
    /// Add memory
    pub fn add_memory(&mut self, content: &str, importance: f32) -> String {
        let id = uuid::Uuid::now_v7().to_string();
        let entry = MemoryEntry {
            id: id.clone(),
            content: content.to_string(),
            embedding: None,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            access_count: 0,
            importance,
            metadata: HashMap::new(),
        };
        self.memories.push(entry);

        // 如果超过最大数量，移除最不重要的记忆
        // If maximum count exceeded, remove least important memories
        if self.memories.len() > self.max_memories {
            // 按重要性降序排序
            // Sort by importance descending
            self.memories
                .sort_by(|a, b| b.importance.total_cmp(&a.importance));
            // 截断保留最重要的记忆
            // Truncate to keep most important ones
            self.memories.truncate(self.max_memories);
        }

        id
    }

    /// 检索记忆
    /// Retrieve memory
    pub fn retrieve(&mut self, query: &str, limit: usize) -> Vec<&MemoryEntry> {
        // 简单的关键词匹配，实际应用中应使用向量相似度
        // Simple keyword matching, should use vector similarity in practice
        let mut results: Vec<&mut MemoryEntry> = self
            .memories
            .iter_mut()
            .filter(|m| m.content.contains(query))
            .collect();

        // 更新访问次数
        // Update access count
        for entry in &mut results {
            entry.access_count += 1;
        }

        results.into_iter().map(|m| &*m).take(limit).collect()
    }

    /// 获取所有记忆
    /// Get all memories
    pub fn all_memories(&self) -> &[MemoryEntry] {
        &self.memories
    }

    /// 清除记忆
    /// Clear memories
    pub fn clear(&mut self) {
        self.memories.clear();
    }
}

#[async_trait::async_trait]
impl AgentPlugin for MemoryPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    fn state(&self) -> PluginState {
        self.state.clone()
    }

    async fn load(&mut self, _ctx: &PluginContext) -> PluginResult<()> {
        self.state = PluginState::Loading;
        info!("Loading Memory plugin: {}", self.metadata.id);
        self.state = PluginState::Loaded;
        Ok(())
    }

    async fn init_plugin(&mut self) -> PluginResult<()> {
        info!("Initializing Memory plugin: {}", self.metadata.id);
        Ok(())
    }

    async fn start(&mut self) -> PluginResult<()> {
        self.state = PluginState::Running;
        info!("Memory plugin {} started", self.metadata.id);
        Ok(())
    }

    async fn stop(&mut self) -> PluginResult<()> {
        self.state = PluginState::Paused;
        info!("Memory plugin {} stopped", self.metadata.id);
        Ok(())
    }

    async fn unload(&mut self) -> PluginResult<()> {
        self.memories.clear();
        self.state = PluginState::Unloaded;
        info!("Memory plugin {} unloaded", self.metadata.id);
        Ok(())
    }

    async fn execute(&mut self, input: String) -> PluginResult<String> {
        let parts: Vec<&str> = input.as_str().splitn(3, ' ').collect();
        match parts.as_slice() {
            ["add", content] => {
                let id = self.add_memory(content, 0.5);
                Ok(format!("Added memory: {}", id))
            }
            ["add", content, importance] => {
                let imp: f32 = importance.parse().unwrap_or(0.5);
                let id = self.add_memory(content, imp);
                Ok(format!("Added memory: {}", id))
            }
            ["search", query] => {
                let results = self.retrieve(query, 5);
                let contents: Vec<&str> = results.iter().map(|m| m.content.as_str()).collect();
                let json = serde_json::to_string(&contents)
                    .map_err(|e| PluginError::ExecutionFailed(e.to_string()))?;
                Ok(json)
            }
            ["count"] => Ok(self.memories.len().to_string()),
            ["clear"] => {
                self.clear();
                Ok("Cleared".to_string())
            }
            _ => Err(PluginError::ExecutionFailed("Invalid command".into())),
        }
    }

    fn stats(&self) -> HashMap<String, serde_json::Value> {
        let mut stats = HashMap::new();
        stats.insert(
            "memory_count".to_string(),
            serde_json::json!(self.memories.len()),
        );
        stats.insert(
            "max_memories".to_string(),
            serde_json::json!(self.max_memories),
        );
        stats
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

// ============================================================================
// 插件管理器
// Plugin Manager
// ============================================================================

/// 插件注册表条目
/// Plugin registry entry
struct PluginEntry {
    plugin: Box<dyn AgentPlugin>,
    config: PluginConfig,
}

/// 插件管理器
/// Plugin Manager
pub struct PluginManager {
    /// 已注册的插件
    /// Registered plugins
    plugins: Arc<RwLock<HashMap<String, PluginEntry>>>,
    /// 插件执行上下文
    /// Plugin execution context
    context: PluginContext,
    /// 事件接收器
    /// Event receiver
    event_rx: Option<tokio::sync::mpsc::Receiver<PluginEvent>>,
    /// 事件发送器（用于克隆给插件）
    /// Event sender (for cloning to plugins)
    event_tx: tokio::sync::mpsc::Sender<PluginEvent>,
}

impl PluginManager {
    /// 创建新的插件管理器
    /// Create a new plugin manager
    pub fn new(agent_id: &str) -> Self {
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(256);
        let context = PluginContext::new(agent_id).with_event_sender(event_tx.clone());

        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
            context,
            event_rx: Some(event_rx),
            event_tx,
        }
    }

    /// 获取插件上下文
    /// Get plugin context
    pub fn context(&self) -> &PluginContext {
        &self.context
    }

    /// 注册插件
    /// Register plugin
    pub async fn register<P: AgentPlugin + 'static>(&self, plugin: P) -> PluginResult<()> {
        self.register_with_config(plugin, PluginConfig::new()).await
    }

    /// 使用配置注册插件
    /// Register plugin with configuration
    pub async fn register_with_config<P: AgentPlugin + 'static>(
        &self,
        plugin: P,
        config: PluginConfig,
    ) -> PluginResult<()> {
        let plugin_id = plugin.plugin_id().to_string();
        let mut plugins = self.plugins.write().await;

        if plugins.contains_key(&plugin_id) {
            return Err(PluginError::Other(format!(
                "Plugin {} already registered",
                plugin_id
            )));
        }

        let entry = PluginEntry {
            plugin: Box::new(plugin),
            config,
        };
        plugins.insert(plugin_id.clone(), entry);

        info!("Plugin {} registered", plugin_id);
        Ok(())
    }

    /// 卸载插件
    /// Unregister plugin
    pub async fn unregister(&self, plugin_id: &str) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;
        if let Some(mut entry) = plugins.remove(plugin_id) {
            entry.plugin.unload().await?;
            info!("Plugin {} unregistered", plugin_id);
        }
        Ok(())
    }

    /// 获取插件
    /// Get plugin
    pub async fn get(
        &self,
        plugin_id: &str,
    ) -> Option<impl std::ops::Deref<Target = Box<dyn AgentPlugin>> + '_> {
        let plugins = self.plugins.read().await;
        if plugins.contains_key(plugin_id) {
            Some(tokio::sync::RwLockReadGuard::map(plugins, |p| {
                &p.get(plugin_id).unwrap().plugin
            }))
        } else {
            None
        }
    }

    /// 获取可变插件引用
    /// Get mutable plugin reference
    pub async fn get_mut(
        &self,
        plugin_id: &str,
    ) -> Option<impl std::ops::DerefMut<Target = Box<dyn AgentPlugin>> + '_> {
        let plugins = self.plugins.write().await;
        if plugins.contains_key(plugin_id) {
            Some(tokio::sync::RwLockWriteGuard::map(plugins, |p| {
                &mut p.get_mut(plugin_id).unwrap().plugin
            }))
        } else {
            None
        }
    }

    /// 获取指定类型的插件
    /// Get plugins of a specific type
    pub async fn get_by_type(&self, plugin_type: PluginType) -> Vec<String> {
        let plugins = self.plugins.read().await;
        plugins
            .iter()
            .filter(|(_, entry)| entry.plugin.plugin_type() == plugin_type)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// 加载所有插件
    /// Load all plugins
    pub async fn load_all(&self) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;
        for (id, entry) in plugins.iter_mut() {
            let ctx = self.context.clone().with_config(entry.config.clone());
            if let Err(e) = entry.plugin.load(&ctx).await {
                error!("Failed to load plugin {}: {}", id, e);
                return Err(e);
            }
        }
        info!("All plugins loaded");
        Ok(())
    }

    /// 初始化所有插件
    /// Initialize all plugins
    pub async fn init_all(&self) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;

        // 按优先级排序
        // Sort by priority
        let mut sorted: Vec<_> = plugins.iter_mut().collect();
        sorted.sort_by(|a, b| {
            b.1.plugin
                .metadata()
                .priority
                .cmp(&a.1.plugin.metadata().priority)
        });

        for (id, entry) in sorted {
            if let Err(e) = entry.plugin.init_plugin().await {
                error!("Failed to initialize plugin {}: {}", id, e);
                return Err(e);
            }
        }
        info!("All plugins initialized");
        Ok(())
    }

    /// 启动所有插件
    /// Start all plugins
    pub async fn start_all(&self) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;
        for (id, entry) in plugins.iter_mut() {
            if entry.config.auto_start
                && entry.config.enabled
                && let Err(e) = entry.plugin.start().await
            {
                error!("Failed to start plugin {}: {}", id, e);
                return Err(e);
            }
        }
        info!("All auto-start plugins started");
        Ok(())
    }

    /// 停止所有插件
    /// Stop all plugins
    pub async fn stop_all(&self) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;
        for (id, entry) in plugins.iter_mut() {
            if let Err(e) = entry.plugin.stop().await {
                warn!("Failed to stop plugin {}: {}", id, e);
            }
        }
        info!("All plugins stopped");
        Ok(())
    }

    /// 卸载所有插件
    /// Unload all plugins
    pub async fn unload_all(&self) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;
        for (id, entry) in plugins.iter_mut() {
            if let Err(e) = entry.plugin.unload().await {
                warn!("Failed to unload plugin {}: {}", id, e);
            }
        }
        plugins.clear();
        info!("All plugins unloaded");
        Ok(())
    }

    /// 执行插件
    /// Execute plugin
    pub async fn execute(&self, plugin_id: &str, input: String) -> PluginResult<String> {
        let mut plugins = self.plugins.write().await;
        let entry = plugins
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::Other(format!("Plugin {} not found", plugin_id)))?;
        entry.plugin.execute(input).await
    }

    /// 获取所有插件 ID
    /// Get all plugin IDs
    pub async fn plugin_ids(&self) -> Vec<String> {
        let plugins = self.plugins.read().await;
        plugins.keys().cloned().collect()
    }

    /// 获取所有插件元数据
    /// List all plugin metadata
    pub async fn list_plugins(&self) -> Vec<PluginMetadata> {
        let plugins = self.plugins.read().await;
        plugins
            .values()
            .map(|e| e.plugin.metadata().clone())
            .collect()
    }

    /// 获取插件统计信息
    /// Get plugin statistics
    pub async fn stats(&self, plugin_id: &str) -> Option<HashMap<String, serde_json::Value>> {
        let plugins = self.plugins.read().await;
        plugins.get(plugin_id).map(|e| e.plugin.stats())
    }

    /// 健康检查所有插件
    /// Health check all plugins
    pub async fn health_check_all(&self) -> HashMap<String, bool> {
        let plugins = self.plugins.read().await;
        let mut results = HashMap::new();
        for (id, entry) in plugins.iter() {
            let healthy = entry.plugin.health_check().await.unwrap_or(false);
            results.insert(id.clone(), healthy);
        }
        results
    }

    /// 获取事件接收器
    /// Take event receiver
    pub fn take_event_receiver(&mut self) -> Option<tokio::sync::mpsc::Receiver<PluginEvent>> {
        self.event_rx.take()
    }
}

// ============================================================================
// 测试
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_plugin_manager() {
        let manager = PluginManager::new("test_agent");

        // 注册 LLM 插件
        // Register LLM plugin
        let llm = LLMPlugin::new("llm_001");
        manager.register(llm).await.unwrap();

        // 注册存储插件
        // Register storage plugin
        let storage = StoragePlugin::new("storage_001");
        manager.register(storage).await.unwrap();

        // 注册记忆插件
        // Register memory plugin
        let memory = MemoryPlugin::new("memory_001");
        manager.register(memory).await.unwrap();

        // 获取所有插件
        // Get all plugins
        let ids = manager.plugin_ids().await;
        assert_eq!(ids.len(), 3);

        // 加载和初始化
        // Load and initialize
        manager.load_all().await.unwrap();
        manager.init_all().await.unwrap();
        manager.start_all().await.unwrap();

        // 执行 LLM 插件
        // Execute LLM plugin
        let result = manager
            .execute("llm_001", "Hello".to_string())
            .await
            .unwrap();
        assert!(result.contains("Hello"));

        // 执行存储插件
        // Execute storage plugin
        manager
            .execute("storage_001", "set foo bar".to_string())
            .await
            .unwrap();
        let value = manager
            .execute("storage_001", "get foo".to_string())
            .await
            .unwrap();
        assert_eq!(value, "bar");

        // 停止和卸载
        // Stop and unload
        manager.stop_all().await.unwrap();
        manager.unload_all().await.unwrap();

        let ids = manager.plugin_ids().await;
        assert_eq!(ids.len(), 0);
    }

    #[tokio::test]
    async fn test_llm_plugin() {
        let mut llm = LLMPlugin::new("llm_test");
        let ctx = PluginContext::new("test_agent");

        llm.load(&ctx).await.unwrap();
        llm.init_plugin().await.unwrap();
        llm.start().await.unwrap();

        assert_eq!(llm.state(), PluginState::Running);

        let response = llm.execute("Test prompt".to_string()).await.unwrap();
        assert!(response.contains("Test prompt"));

        llm.stop().await.unwrap();
        llm.unload().await.unwrap();

        assert_eq!(llm.state(), PluginState::Unloaded);
    }

    // ---- State-machine transition tests (fixes #448) -------------------------

    /// Verifies the full lifecycle state sequence for LLMPlugin:
    /// Unloaded → Loading → Loaded → Running → Loaded (stop) → Unloaded
    #[tokio::test]
    async fn test_llm_plugin_stop_sets_loaded_state() {
        let mut llm = LLMPlugin::new("llm_state_test");
        let ctx = PluginContext::new("test_agent");

        // Initial state
        assert_eq!(llm.state(), PluginState::Unloaded);

        // After load: Loaded
        llm.load(&ctx).await.unwrap();
        assert_eq!(llm.state(), PluginState::Loaded);

        // After init + start: Running
        llm.init_plugin().await.unwrap();
        llm.start().await.unwrap();
        assert_eq!(llm.state(), PluginState::Running);

        // After stop: must be Loaded, NOT Paused
        llm.stop().await.unwrap();
        assert_eq!(
            llm.state(),
            PluginState::Loaded,
            "stop() must transition to Loaded, not Paused (see issue #448)"
        );
        assert_ne!(
            llm.state(),
            PluginState::Paused,
            "stop() must NOT set Paused — that is reserved for pause()"
        );

        // After unload: Unloaded
        llm.unload().await.unwrap();
        assert_eq!(llm.state(), PluginState::Unloaded);
    }

    /// Verifies the full lifecycle state sequence for ToolPlugin:
    /// Unloaded → Loading → Loaded → Running → Loaded (stop) → Unloaded
    #[tokio::test]
    async fn test_tool_plugin_stop_sets_loaded_state() {
        let mut tool = ToolPlugin::new("tool_state_test");
        let ctx = PluginContext::new("test_agent");

        // Initial state
        assert_eq!(tool.state(), PluginState::Unloaded);

        // After load: Loaded
        tool.load(&ctx).await.unwrap();
        assert_eq!(tool.state(), PluginState::Loaded);

        // After init + start: Running
        tool.init_plugin().await.unwrap();
        tool.start().await.unwrap();
        assert_eq!(tool.state(), PluginState::Running);

        // After stop: must be Loaded, NOT Paused
        tool.stop().await.unwrap();
        assert_eq!(
            tool.state(),
            PluginState::Loaded,
            "stop() must transition to Loaded, not Paused (see issue #448)"
        );
        assert_ne!(
            tool.state(),
            PluginState::Paused,
            "stop() must NOT set Paused — that is reserved for pause()"
        );

        // After unload: Unloaded
        tool.unload().await.unwrap();
        assert_eq!(tool.state(), PluginState::Unloaded);
    }

    /// Verifies the full lifecycle state sequence for StoragePlugin:
    /// Unloaded → Loading → Loaded → Running → Loaded (stop) → Unloaded
    #[tokio::test]
    async fn test_storage_plugin_stop_sets_loaded_state() {
        let mut storage = StoragePlugin::new("storage_state_test");
        let ctx = PluginContext::new("test_agent");

        // Initial state
        assert_eq!(storage.state(), PluginState::Unloaded);

        // After load: Loaded
        storage.load(&ctx).await.unwrap();
        assert_eq!(storage.state(), PluginState::Loaded);

        // After init + start: Running
        storage.init_plugin().await.unwrap();
        storage.start().await.unwrap();
        assert_eq!(storage.state(), PluginState::Running);

        // After stop: must be Loaded, NOT Paused
        storage.stop().await.unwrap();
        assert_eq!(
            storage.state(),
            PluginState::Loaded,
            "stop() must transition to Loaded, not Paused (see issue #448)"
        );
        assert_ne!(
            storage.state(),
            PluginState::Paused,
            "stop() must NOT set Paused — that is reserved for pause()"
        );

        // After unload: Unloaded
        storage.unload().await.unwrap();
        assert_eq!(storage.state(), PluginState::Unloaded);
    }

    /// Verifies a plugin can be restarted after stop (start → stop → start cycle).
    /// This is only valid if stop() correctly returns to Loaded (not Paused).
    #[tokio::test]
    async fn test_llm_plugin_can_restart_after_stop() {
        let mut llm = LLMPlugin::new("llm_restart_test");
        let ctx = PluginContext::new("test_agent");

        llm.load(&ctx).await.unwrap();
        llm.init_plugin().await.unwrap();

        // First start → stop cycle
        llm.start().await.unwrap();
        assert_eq!(llm.state(), PluginState::Running);
        llm.stop().await.unwrap();
        assert_eq!(llm.state(), PluginState::Loaded);

        // Second start: only possible if stop() returned to Loaded (not Paused or Unloaded)
        llm.start().await.unwrap();
        assert_eq!(llm.state(), PluginState::Running);

        llm.stop().await.unwrap();
        llm.unload().await.unwrap();
        assert_eq!(llm.state(), PluginState::Unloaded);
    }

    #[tokio::test]
    async fn test_storage_plugin() {
        let mut storage = StoragePlugin::new("storage_test").with_backend(MemoryStorage::new());
        let ctx = PluginContext::new("test_agent");

        storage.load(&ctx).await.unwrap();
        storage.init_plugin().await.unwrap();
        storage.start().await.unwrap();

        // 测试存储操作
        // Test storage operations
        storage.set_string("key1", "value1").await.unwrap();
        let value = storage.get_string("key1").await.unwrap();
        assert_eq!(value, Some("value1".to_string()));

        // 测试删除
        // Test deletion
        let deleted = storage.delete("key1").await.unwrap();
        assert!(deleted);

        let value = storage.get_string("key1").await.unwrap();
        assert!(value.is_none());

        storage.stop().await.unwrap();
        storage.unload().await.unwrap();
    }

    #[tokio::test]
    async fn test_memory_plugin() {
        let mut memory = MemoryPlugin::new("memory_test").with_max_memories(100);
        let ctx = PluginContext::new("test_agent");

        memory.load(&ctx).await.unwrap();
        memory.init_plugin().await.unwrap();
        memory.start().await.unwrap();

        // 添加记忆
        // Add memory
        let id1 = memory.add_memory("Important meeting tomorrow", 0.9);
        let id2 = memory.add_memory("Buy groceries", 0.3);

        assert!(!id1.is_empty());
        assert!(!id2.is_empty());

        // 检索记忆
        // Retrieve memory
        let results = memory.retrieve("meeting", 10);
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("meeting"));

        // 检查计数
        // Check count
        assert_eq!(memory.all_memories().len(), 2);

        memory.stop().await.unwrap();
        memory.unload().await.unwrap();
    }

    #[tokio::test]
    async fn test_plugin_context() {
        let ctx = PluginContext::new("test_agent");

        // 测试共享状态
        // Test shared state
        ctx.set_state("counter", 42i32).await;
        let value: Option<i32> = ctx.get_state("counter").await;
        assert_eq!(value, Some(42));

        // 测试配置
        // Test configuration
        let mut config = PluginConfig::new();
        config.set("timeout", 30);
        config.set("enabled", true);

        assert_eq!(config.get_i64("timeout"), Some(30));
        assert_eq!(config.get_bool("enabled"), Some(true));
    }
}
