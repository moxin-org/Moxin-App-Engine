//! OpenAI Provider Implementation
//! OpenAI Provider Implementation
//!
//! 使用 `async-openai` crate 实现 OpenAI API 交互
//! Use the `async-openai` crate to implement OpenAI API interactions
//!
//! # 支持的服务
//! # Supported Services
//!
//! - OpenAI API (api.openai.com)
//! - Azure OpenAI
//! - 兼容 OpenAI API 的本地服务 (Ollama, vLLM, LocalAI 等)
//! - OpenAI-compatible local services (Ollama, vLLM, LocalAI, etc.)
//!
//! # 示例
//! # Examples
//!
//! ```rust,ignore
//! use mofa_foundation::llm::openai::{OpenAIProvider, OpenAIConfig};
//!
//! // 使用 OpenAI
//! // Use OpenAI
//! let provider = OpenAIProvider::new("sk-xxx");
//!
//! // 使用自定义 endpoint
//! // Use custom endpoint
//! let provider = OpenAIProvider::with_config(
//!     OpenAIConfig::new("sk-xxx")
//!         .with_base_url("http://localhost:11434/v1")
//!         .with_model("llama2")
//! );
//!
//! // 使用 Azure OpenAI
//! let provider = OpenAIProvider::azure("https://xxx.openai.azure.com", "api-key", "deployment");
//! ```

use super::provider::{ChatStream, LLMProvider, ModelCapabilities, ModelInfo};
use super::types::*;
use async_openai::{
    Client,
    config::OpenAIConfig as AsyncOpenAIConfig,
    types::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage,
        ChatCompletionRequestMessageContentPartAudio, ChatCompletionRequestMessageContentPartImage,
        ChatCompletionRequestMessageContentPartText, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestToolMessageArgs, ChatCompletionRequestUserMessageArgs,
        ChatCompletionRequestUserMessageContent, ChatCompletionRequestUserMessageContentPart,
        ChatCompletionToolArgs, ChatCompletionToolChoiceOption, ChatCompletionToolType,
        CreateChatCompletionRequestArgs, FunctionObjectArgs, ImageDetail as OpenAIImageDetail,
        ImageUrl as OpenAIImageUrl, InputAudio, InputAudioFormat,
    },
};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};

/// OpenAI Provider 配置
/// OpenAI Provider Configuration
#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    /// API Key
    /// API Key
    pub api_key: String,
    /// API 基础 URL
    /// API Base URL
    pub base_url: Option<String>,
    /// 组织 ID
    /// Organization ID
    pub org_id: Option<String>,
    /// 默认模型
    /// Default Model
    pub default_model: String,
    /// 默认温度
    /// Default Temperature
    pub default_temperature: f32,
    /// 默认最大 token 数
    /// Default Max Tokens
    pub default_max_tokens: u32,
    /// 请求超时（秒）
    /// Request Timeout (seconds)
    pub timeout_secs: u64,
}

impl Default for OpenAIConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: None,
            org_id: None,
            default_model: "gpt-4o".to_string(),
            default_temperature: 0.7,
            default_max_tokens: 4096,
            timeout_secs: 60,
        }
    }
}

impl OpenAIConfig {
    /// 创建新配置
    /// Create new configuration
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            ..Default::default()
        }
    }

    /// 从环境变量创建配置
    /// Create configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            base_url: std::env::var("OPENAI_BASE_URL").ok(),
            default_model: std::env::var("OPENAI_MODEL").unwrap_or_default(),
            ..Default::default()
        }
    }

    /// 设置 base URL
    /// Set base URL
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// 设置默认模型
    /// Set default model
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = model.into();
        self
    }

    /// 设置默认温度
    /// Set default temperature
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.default_temperature = temp;
        self
    }

    /// 设置默认最大 token 数
    /// Set default max tokens
    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.default_max_tokens = tokens;
        self
    }

    /// 设置组织 ID
    /// Set organization ID
    pub fn with_org_id(mut self, org_id: impl Into<String>) -> Self {
        self.org_id = Some(org_id.into());
        self
    }

    /// 设置超时
    /// Set timeout
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }
}

/// OpenAI LLM Provider
/// OpenAI LLM Provider
///
/// 支持 OpenAI API 及兼容服务
/// Supports OpenAI API and compatible services
pub struct OpenAIProvider {
    client: Client<AsyncOpenAIConfig>,
    config: OpenAIConfig,
}

impl OpenAIProvider {
    /// 使用 API Key 创建 Provider
    /// Create Provider using API Key
    pub fn new(api_key: impl Into<String>) -> Self {
        let config = OpenAIConfig::new(api_key);
        Self::with_config(config)
    }

    /// 从环境变量创建 Provider
    /// Create Provider from environment variables
    pub fn from_env() -> Self {
        Self::with_config(OpenAIConfig::from_env())
    }

    /// 使用配置创建 Provider
    /// Create Provider using configuration
    pub fn with_config(config: OpenAIConfig) -> Self {
        let mut openai_config = AsyncOpenAIConfig::new().with_api_key(&config.api_key);

        if let Some(ref base_url) = config.base_url {
            openai_config = openai_config.with_api_base(base_url);
        }

        if let Some(ref org_id) = config.org_id {
            openai_config = openai_config.with_org_id(org_id);
        }

        let client = Client::with_config(openai_config);

        Self { client, config }
    }

    /// 创建 Azure OpenAI Provider
    /// Create Azure OpenAI Provider
    pub fn azure(
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
        deployment: impl Into<String>,
    ) -> Self {
        let endpoint = endpoint.into();
        let deployment = deployment.into();

        // Azure OpenAI 使用不同的 URL 格式
        // Azure OpenAI uses a different URL format
        let base_url = format!(
            "{}/openai/deployments/{}",
            endpoint.trim_end_matches('/'),
            deployment
        );

        let config = OpenAIConfig::new(api_key)
            .with_base_url(base_url)
            .with_model(deployment);

        Self::with_config(config)
    }

    /// 创建 Ollama Provider (通过 OpenAI 兼容 API)
    /// Create Ollama Provider (via OpenAI compatible API)
    pub fn ollama(model: impl Into<String>) -> Self {
        Self::local("http://localhost:11434/v1", model)
    }

    /// 创建兼容 OpenAI API 的本地服务 Provider
    /// Create Provider for local services compatible with OpenAI API
    pub fn local(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        let config = OpenAIConfig::new("not-needed")
            .with_base_url(base_url)
            .with_model(model);

        Self::with_config(config)
    }

    /// 获取底层 async-openai 客户端
    /// Get the underlying async-openai client
    pub fn client(&self) -> &Client<AsyncOpenAIConfig> {
        &self.client
    }

    /// 获取配置
    /// Get configuration
    pub fn config(&self) -> &OpenAIConfig {
        &self.config
    }

    /// 转换消息格式
    /// Convert message format
    fn convert_messages(
        messages: &[ChatMessage],
    ) -> Result<Vec<ChatCompletionRequestMessage>, LLMError> {
        messages.iter().map(Self::convert_message).collect()
    }

    /// 转换单个消息
    /// Convert a single message
    fn convert_message(msg: &ChatMessage) -> Result<ChatCompletionRequestMessage, LLMError> {
        let text_only_content = msg
            .content
            .as_ref()
            .map(|c| match c {
                MessageContent::Text(s) => s.clone(),
                MessageContent::Parts(parts) => parts
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            })
            .unwrap_or_default();

        match msg.role {
            Role::System => Ok(ChatCompletionRequestSystemMessageArgs::default()
                .content(text_only_content)
                .build()
                .map_err(|e| LLMError::Other(e.to_string()))?
                .into()),
            Role::User => {
                let content = match msg.content.as_ref() {
                    Some(MessageContent::Text(s)) => {
                        ChatCompletionRequestUserMessageContent::Text(s.clone())
                    }
                    Some(MessageContent::Parts(parts)) => {
                        let mut out = Vec::new();
                        for part in parts {
                            match part {
                                ContentPart::Text { text } => {
                                    out.push(ChatCompletionRequestUserMessageContentPart::Text(
                                        ChatCompletionRequestMessageContentPartText {
                                            text: text.clone(),
                                        },
                                    ));
                                }
                                ContentPart::Image { image_url } => {
                                    let detail = image_url.detail.as_ref().map(|d| match d {
                                        ImageDetail::Auto => OpenAIImageDetail::Auto,
                                        ImageDetail::Low => OpenAIImageDetail::Low,
                                        ImageDetail::High => OpenAIImageDetail::High,
                                    });
                                    let image_part = ChatCompletionRequestMessageContentPartImage {
                                        image_url: OpenAIImageUrl {
                                            url: image_url.url.clone(),
                                            detail,
                                        },
                                    };
                                    out.push(
                                        ChatCompletionRequestUserMessageContentPart::ImageUrl(
                                            image_part,
                                        ),
                                    );
                                }
                                ContentPart::Audio { audio } => {
                                    let format = match audio.format.to_lowercase().as_str() {
                                        "wav" => InputAudioFormat::Wav,
                                        _ => InputAudioFormat::Mp3,
                                    };
                                    let audio_part = ChatCompletionRequestMessageContentPartAudio {
                                        input_audio: InputAudio {
                                            data: audio.data.clone(),
                                            format,
                                        },
                                    };
                                    out.push(
                                        ChatCompletionRequestUserMessageContentPart::InputAudio(
                                            audio_part,
                                        ),
                                    );
                                }
                                ContentPart::Video { video } => {
                                    // OpenAI API does not officially support pure video upload in standard completion context via generic content parts yet
                                    // We will gracefully log a warning and omit or fall back to an unsupported message.
                                    tracing::warn!(
                                        "OpenAI provider currently does not explicitly support raw Video parts in standard ChatCompletion. Ignoring video part format {}",
                                        video.format
                                    );
                                }
                            }
                        }
                        ChatCompletionRequestUserMessageContent::Array(out)
                    }
                    None => ChatCompletionRequestUserMessageContent::Text(String::new()),
                };

                Ok(ChatCompletionRequestUserMessageArgs::default()
                    .content(content)
                    .build()
                    .map_err(|e| LLMError::Other(e.to_string()))?
                    .into())
            }
            Role::Assistant => {
                let mut builder = ChatCompletionRequestAssistantMessageArgs::default();
                if !text_only_content.is_empty() {
                    builder.content(text_only_content);
                }

                // 处理工具调用
                // Handle tool calls
                if let Some(ref tool_calls) = msg.tool_calls {
                    let converted_calls: Vec<_> = tool_calls
                        .iter()
                        .map(|tc| async_openai::types::ChatCompletionMessageToolCall {
                            id: tc.id.clone(),
                            r#type: ChatCompletionToolType::Function,
                            function: async_openai::types::FunctionCall {
                                name: tc.function.name.clone(),
                                arguments: tc.function.arguments.clone(),
                            },
                        })
                        .collect();
                    builder.tool_calls(converted_calls);
                }

                Ok(builder
                    .build()
                    .map_err(|e| LLMError::Other(e.to_string()))?
                    .into())
            }
            Role::Tool => {
                let tool_call_id = msg
                    .tool_call_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());

                Ok(ChatCompletionRequestToolMessageArgs::default()
                    .tool_call_id(tool_call_id)
                    .content(text_only_content)
                    .build()
                    .map_err(|e| LLMError::Other(e.to_string()))?
                    .into())
            }
        }
    }

    /// 转换工具定义
    /// Convert tool definitions
    fn convert_tools(
        tools: &[Tool],
    ) -> Result<Vec<async_openai::types::ChatCompletionTool>, LLMError> {
        tools
            .iter()
            .map(|tool| {
                let function = FunctionObjectArgs::default()
                    .name(&tool.function.name)
                    .description(tool.function.description.clone().unwrap_or_default())
                    .parameters(
                        tool.function
                            .parameters
                            .clone()
                            .unwrap_or(serde_json::json!({})),
                    )
                    .build()
                    .map_err(|e| LLMError::Other(e.to_string()))?;

                ChatCompletionToolArgs::default()
                    .r#type(ChatCompletionToolType::Function)
                    .function(function)
                    .build()
                    .map_err(|e| LLMError::Other(e.to_string()))
            })
            .collect()
    }

    /// 转换响应
    /// Convert response
    fn convert_response(
        response: async_openai::types::CreateChatCompletionResponse,
    ) -> ChatCompletionResponse {
        let choices: Vec<Choice> = response
            .choices
            .into_iter()
            .map(|choice| {
                let message = Self::convert_response_message(choice.message);
                let finish_reason = choice.finish_reason.map(|r| match r {
                    async_openai::types::FinishReason::Stop => FinishReason::Stop,
                    async_openai::types::FinishReason::Length => FinishReason::Length,
                    async_openai::types::FinishReason::ToolCalls => FinishReason::ToolCalls,
                    async_openai::types::FinishReason::ContentFilter => FinishReason::ContentFilter,
                    async_openai::types::FinishReason::FunctionCall => FinishReason::ToolCalls,
                });

                Choice {
                    index: choice.index,
                    message,
                    finish_reason,
                    logprobs: None,
                }
            })
            .collect();

        let usage = response.usage.map(|u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        ChatCompletionResponse {
            id: response.id,
            object: response.object,
            created: response.created as u64,
            model: response.model,
            choices,
            usage,
            system_fingerprint: response.system_fingerprint,
        }
    }

    /// 转换响应消息
    /// Convert response message
    fn convert_response_message(
        msg: async_openai::types::ChatCompletionResponseMessage,
    ) -> ChatMessage {
        let content = msg.content.map(MessageContent::Text);

        let tool_calls = msg.tool_calls.map(|calls| {
            calls
                .into_iter()
                .map(|tc| ToolCall {
                    id: tc.id,
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: tc.function.name,
                        arguments: tc.function.arguments,
                    },
                })
                .collect()
        });

        ChatMessage {
            role: Role::Assistant,
            content,
            name: None,
            tool_calls,
            tool_call_id: None,
        }
    }

    fn should_fallback_for_compatible_deser_error(err: &async_openai::error::OpenAIError) -> bool {
        let msg = err.to_string();
        // Detect enum variants returned by compatible backends that async openai rejects.
        msg.contains("unknown variant `on_demand`")
            || msg.contains("failed to deserialize api response")
                && msg.contains("service_tier")
    }

    fn strip_incompatible_service_tier(value: &mut serde_json::Value) -> bool {
        value
            .as_object_mut()
            .and_then(|obj| obj.remove("service_tier"))
            .is_some()
    }

    fn chat_completions_url(&self) -> String {
        let base = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1");
        format!("{}/chat/completions", base.trim_end_matches('/'))
    }

    async fn chat_with_raw_json_fallback(
        &self,
        request_json: &serde_json::Value,
    ) -> LLMResult<ChatCompletionResponse> {
        let mut headers = HeaderMap::new();
        let auth_value = HeaderValue::from_str(&format!("Bearer {}", self.config.api_key))
            .map_err(|e| LLMError::ConfigError(e.to_string()))?;
        headers.insert(AUTHORIZATION, auth_value);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if let Some(org_id) = &self.config.org_id {
            let org_header = HeaderValue::from_str(org_id)
                .map_err(|e| LLMError::ConfigError(e.to_string()))?;
            headers.insert("OpenAI-Organization", org_header);
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.config.timeout_secs))
            .default_headers(headers)
            .build()
            .map_err(|e| LLMError::NetworkError(e.to_string()))?;

        let response = client
            .post(self.chat_completions_url())
            .json(request_json)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    LLMError::Timeout(e.to_string())
                } else {
                    LLMError::NetworkError(e.to_string())
                }
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(LLMError::ApiError {
                code: Some(status.as_u16().to_string()),
                message: body,
            });
        }

        let mut value: serde_json::Value = response
            .json()
            .await
            .map_err(|e| LLMError::Other(format!("failed to decode fallback response json: {e}")))?;

        // Strip only the known incompatible field and reuse normal conversion.
        Self::strip_incompatible_service_tier(&mut value);

        let parsed: async_openai::types::CreateChatCompletionResponse =
            serde_json::from_value(value).map_err(|e| {
                LLMError::Other(format!("failed to deserialize fallback api response: {e}"))
            })?;

        Ok(Self::convert_response(parsed))
    }
}

#[async_trait]
impl LLMProvider for OpenAIProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn default_model(&self) -> &str {
        &self.config.default_model
    }

    fn supported_models(&self) -> Vec<&str> {
        vec![
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4-turbo",
            "gpt-4",
            "gpt-3.5-turbo",
            "o1",
            "o1-mini",
            "o1-preview",
        ]
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_vision(&self) -> bool {
        true
    }

    fn supports_embedding(&self) -> bool {
        true
    }

    async fn chat(&self, request: ChatCompletionRequest) -> LLMResult<ChatCompletionResponse> {
        let messages = Self::convert_messages(&request.messages)?;

        let model = if request.model.is_empty() {
            self.config.default_model.clone()
        } else {
            request.model.clone()
        };

        let mut builder = CreateChatCompletionRequestArgs::default();
        builder.model(&model).messages(messages);

        // 设置可选参数
        // Set optional parameters
        if let Some(temp) = request.temperature {
            builder.temperature(temp);
        } else {
            builder.temperature(self.config.default_temperature);
        }

        if let Some(max_tokens) = request.max_tokens {
            builder.max_tokens(max_tokens);
        }

        if let Some(top_p) = request.top_p {
            builder.top_p(top_p);
        }

        if let Some(ref stop) = request.stop {
            builder.stop(stop.clone());
        }

        if let Some(freq_penalty) = request.frequency_penalty {
            builder.frequency_penalty(freq_penalty);
        }

        if let Some(pres_penalty) = request.presence_penalty {
            builder.presence_penalty(pres_penalty);
        }

        if let Some(ref user) = request.user {
            builder.user(user);
        }

        // 设置工具
        // Set tools
        if let Some(ref tools) = request.tools
            && !tools.is_empty()
        {
            let converted_tools = Self::convert_tools(tools)?;
            builder.tools(converted_tools);

            // 设置 tool_choice
            // Set tool_choice
            if let Some(ref choice) = request.tool_choice {
                let tc = match choice {
                    ToolChoice::Auto => ChatCompletionToolChoiceOption::Auto,
                    ToolChoice::None => ChatCompletionToolChoiceOption::None,
                    ToolChoice::Required => ChatCompletionToolChoiceOption::Required,
                    ToolChoice::Specific { function, .. } => ChatCompletionToolChoiceOption::Named(
                        async_openai::types::ChatCompletionNamedToolChoice {
                            r#type: ChatCompletionToolType::Function,
                            function: async_openai::types::FunctionName {
                                name: function.name.clone(),
                            },
                        },
                    ),
                };
                builder.tool_choice(tc);
            }
        }

        // 设置响应格式
        // Set response format
        if let Some(ref format) = request.response_format
            && format.format_type == "json_object"
        {
            builder.response_format(async_openai::types::ResponseFormat::JsonObject);
        }

        let openai_request = builder
            .build()
            .map_err(|e| LLMError::ConfigError(e.to_string()))?;

        // Preserve the exact request for the raw JSON fallback path.
        let request_json = serde_json::to_value(&openai_request)
            .map_err(|e| LLMError::ConfigError(e.to_string()))?;

        let response = match self.client.chat().create(openai_request).await {
            Ok(response) => response,
            Err(err) if Self::should_fallback_for_compatible_deser_error(&err) => {
                tracing::warn!(
                    "OpenAI-compatible response deserialization failed; retrying via raw JSON fallback: {}",
                    err
                );
                return self.chat_with_raw_json_fallback(&request_json).await;
            }
            Err(err) => return Err(Self::convert_error(err)),
        };

        Ok(Self::convert_response(response))
    }

    async fn chat_stream(&self, request: ChatCompletionRequest) -> LLMResult<ChatStream> {
        let messages = Self::convert_messages(&request.messages)?;

        let model = if request.model.is_empty() {
            self.config.default_model.clone()
        } else {
            request.model.clone()
        };

        let mut builder = CreateChatCompletionRequestArgs::default();
        builder.model(&model).messages(messages).stream(true);

        if let Some(temp) = request.temperature {
            builder.temperature(temp);
        }

        if let Some(max_tokens) = request.max_tokens {
            builder.max_tokens(max_tokens);
        }

        // 设置工具
        // Set tools
        if let Some(ref tools) = request.tools
            && !tools.is_empty()
        {
            let converted_tools = Self::convert_tools(tools)?;
            builder.tools(converted_tools);
        }

        let openai_request = builder
            .build()
            .map_err(|e| LLMError::ConfigError(e.to_string()))?;

        let stream = self
            .client
            .chat()
            .create_stream(openai_request)
            .await
            .map_err(Self::convert_error)?;

        // 转换流，过滤掉 UTF-8 错误（某些 OpenAI 兼容 API 可能返回无效的 UTF-8 数据）
        // Convert stream, filtering UTF-8 errors (some compatible APIs may return invalid data)
        let converted_stream = stream
            .filter_map(|result| async move {
                match result {
                    Ok(chunk) => Some(Ok(Self::convert_chunk(chunk))),
                    Err(e) => {
                        let err_str = e.to_string();
                        // 过滤掉 UTF-8 错误，记录日志但继续处理流
                        // Filter UTF-8 errors, log them but continue processing the stream
                        if err_str.contains("stream did not contain valid UTF-8") || err_str.contains("utf8") {
                            tracing::warn!("Skipping invalid UTF-8 chunk from stream (may happen with some OpenAI-compatible APIs)");
                            None
                        } else {
                            Some(Err(Self::convert_error(e)))
                        }
                    }
                }
            });

        Ok(Box::pin(converted_stream))
    }

    async fn embedding(&self, request: EmbeddingRequest) -> LLMResult<EmbeddingResponse> {
        use async_openai::types::CreateEmbeddingRequestArgs;

        let input = match request.input {
            EmbeddingInput::Single(s) => vec![s],
            EmbeddingInput::Multiple(v) => v,
        };

        let openai_request = CreateEmbeddingRequestArgs::default()
            .model(&request.model)
            .input(input)
            .build()
            .map_err(|e| LLMError::ConfigError(e.to_string()))?;

        let response = self
            .client
            .embeddings()
            .create(openai_request)
            .await
            .map_err(Self::convert_error)?;

        let data: Vec<EmbeddingData> = response
            .data
            .into_iter()
            .map(|d| EmbeddingData {
                object: "embedding".to_string(),
                index: d.index,
                embedding: d.embedding,
            })
            .collect();

        Ok(EmbeddingResponse {
            object: "list".to_string(),
            model: response.model,
            data,
            usage: EmbeddingUsage {
                prompt_tokens: response.usage.prompt_tokens,
                total_tokens: response.usage.total_tokens,
            },
        })
    }

    async fn health_check(&self) -> LLMResult<bool> {
        // 发送一个简单请求来检查连接
        // Send a simple request to check the connection
        let request = ChatCompletionRequest::new(&self.config.default_model)
            .system("Say 'ok'")
            .max_tokens(5);

        match self.chat(request).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn get_model_info(&self, model: &str) -> LLMResult<ModelInfo> {
        // OpenAI 没有公开的模型信息 API，返回预定义信息
        // OpenAI has no public info API, return predefined information
        let info = match model {
            "gpt-4o" => ModelInfo {
                id: "gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                description: Some("Most capable GPT-4 model with vision".to_string()),
                context_window: Some(128000),
                max_output_tokens: Some(16384),
                training_cutoff: Some("2023-10".to_string()),
                capabilities: ModelCapabilities {
                    streaming: true,
                    tools: true,
                    vision: true,
                    json_mode: true,
                    json_schema: true,
                },
            },
            "gpt-4o-mini" => ModelInfo {
                id: "gpt-4o-mini".to_string(),
                name: "GPT-4o Mini".to_string(),
                description: Some("Smaller, faster GPT-4o".to_string()),
                context_window: Some(128000),
                max_output_tokens: Some(16384),
                training_cutoff: Some("2023-10".to_string()),
                capabilities: ModelCapabilities {
                    streaming: true,
                    tools: true,
                    vision: true,
                    json_mode: true,
                    json_schema: true,
                },
            },
            "gpt-4-turbo" => ModelInfo {
                id: "gpt-4-turbo".to_string(),
                name: "GPT-4 Turbo".to_string(),
                description: Some("GPT-4 Turbo with vision".to_string()),
                context_window: Some(128000),
                max_output_tokens: Some(4096),
                training_cutoff: Some("2023-12".to_string()),
                capabilities: ModelCapabilities {
                    streaming: true,
                    tools: true,
                    vision: true,
                    json_mode: true,
                    json_schema: false,
                },
            },
            "gpt-3.5-turbo" => ModelInfo {
                id: "gpt-3.5-turbo".to_string(),
                name: "GPT-3.5 Turbo".to_string(),
                description: Some("Fast and cost-effective".to_string()),
                context_window: Some(16385),
                max_output_tokens: Some(4096),
                training_cutoff: Some("2021-09".to_string()),
                capabilities: ModelCapabilities {
                    streaming: true,
                    tools: true,
                    vision: false,
                    json_mode: true,
                    json_schema: false,
                },
            },
            _ => ModelInfo {
                id: model.to_string(),
                name: model.to_string(),
                description: None,
                context_window: None,
                max_output_tokens: None,
                training_cutoff: None,
                capabilities: ModelCapabilities::default(),
            },
        };

        Ok(info)
    }
}

impl OpenAIProvider {
    /// 转换流式响应块
    /// Convert streaming response chunk
    fn convert_chunk(
        chunk: async_openai::types::CreateChatCompletionStreamResponse,
    ) -> ChatCompletionChunk {
        let choices: Vec<ChunkChoice> = chunk
            .choices
            .into_iter()
            .map(|choice| {
                let delta = ChunkDelta {
                    role: choice.delta.role.map(|_| Role::Assistant),
                    content: choice.delta.content,
                    tool_calls: choice.delta.tool_calls.map(|calls| {
                        calls
                            .into_iter()
                            .map(|tc| ToolCallDelta {
                                index: tc.index,
                                id: tc.id,
                                call_type: Some("function".to_string()),
                                function: tc.function.map(|f| FunctionCallDelta {
                                    name: f.name,
                                    arguments: f.arguments,
                                }),
                            })
                            .collect()
                    }),
                };

                let finish_reason = choice.finish_reason.map(|r| match r {
                    async_openai::types::FinishReason::Stop => FinishReason::Stop,
                    async_openai::types::FinishReason::Length => FinishReason::Length,
                    async_openai::types::FinishReason::ToolCalls => FinishReason::ToolCalls,
                    async_openai::types::FinishReason::ContentFilter => FinishReason::ContentFilter,
                    async_openai::types::FinishReason::FunctionCall => FinishReason::ToolCalls,
                });

                ChunkChoice {
                    index: choice.index,
                    delta,
                    finish_reason,
                }
            })
            .collect();

        ChatCompletionChunk {
            id: chunk.id,
            object: "chat.completion.chunk".to_string(),
            created: chunk.created as u64,
            model: chunk.model,
            choices,
            usage: chunk.usage.map(|u| Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
        }
    }

    /// 转换错误
    /// Convert error
    fn convert_error(err: async_openai::error::OpenAIError) -> LLMError {
        match err {
            async_openai::error::OpenAIError::ApiError(api_err) => {
                let code = api_err.code.clone();
                let message = api_err.message.clone();

                // 根据错误类型分类
                // Categorize by error type
                if message.contains("rate limit") {
                    LLMError::RateLimited(message)
                } else if message.contains("quota") || message.contains("billing") {
                    LLMError::QuotaExceeded(message)
                } else if message.contains("model") && message.contains("not found") {
                    LLMError::ModelNotFound(message)
                } else if message.contains("context") || message.contains("tokens") {
                    LLMError::ContextLengthExceeded(message)
                } else if message.contains("content") && message.contains("filter") {
                    LLMError::ContentFiltered(message)
                } else {
                    LLMError::ApiError { code, message }
                }
            }
            async_openai::error::OpenAIError::Reqwest(e) => {
                if e.is_timeout() {
                    LLMError::Timeout(e.to_string())
                } else {
                    LLMError::NetworkError(e.to_string())
                }
            }
            async_openai::error::OpenAIError::InvalidArgument(msg) => LLMError::ConfigError(msg),
            _ => LLMError::Other(err.to_string()),
        }
    }

    /// 快速创建 OpenAI Provider
    /// Quickly create OpenAI Provider
    pub fn openai(api_key: impl Into<String>) -> OpenAIProvider {
        OpenAIProvider::new(api_key)
    }

    /// 快速创建兼容 OpenAI API 的本地 Provider
    /// Quickly create local OpenAI compatible Provider
    pub fn openai_compatible(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> OpenAIProvider {
        let config = OpenAIConfig::new(api_key)
            .with_base_url(base_url)
            .with_model(model);
        OpenAIProvider::with_config(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_incompatible_service_tier() {
        let mut value = serde_json::json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1,
            "model": "llama-3.3-70b-versatile",
            "service_tier": "on_demand",
            "choices": [],
            "usage": null
        });

        assert!(OpenAIProvider::strip_incompatible_service_tier(&mut value));
        assert!(value.get("service_tier").is_none());
    }

    #[test]
    fn test_config_builder() {
        let config = OpenAIConfig::new("sk-test")
            .with_base_url("http://localhost:8080")
            .with_model("gpt-4")
            .with_temperature(0.5)
            .with_max_tokens(2048);

        assert_eq!(config.api_key, "sk-test");
        assert_eq!(config.base_url, Some("http://localhost:8080".to_string()));
        assert_eq!(config.default_model, "gpt-4");
        assert_eq!(config.default_temperature, 0.5);
        assert_eq!(config.default_max_tokens, 2048);
    }

    #[test]
    fn test_provider_name() {
        let provider = OpenAIProvider::new("test-key");
        assert_eq!(provider.name(), "openai");
    }
}
