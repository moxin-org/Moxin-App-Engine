//! Minimal OpenAI-compatible provider for record-mode DSL runs.

use async_trait::async_trait;
use mofa_kernel::agent::error::{AgentError, AgentResult};
use mofa_kernel::agent::types::{
    ChatCompletionRequest, ChatCompletionResponse, LLMProvider, ToolCall, ToolDefinition,
    TokenUsage,
};
use serde::{Deserialize, Serialize};
use std::error::Error as StdError;

#[derive(Debug, Clone)]
pub struct OpenAiCompatProviderConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

#[derive(Debug)]
pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    config: OpenAiCompatProviderConfig,
}

impl OpenAiCompatProvider {
    // Create a new provider backed by a fresh HTTP client.
    pub fn new(config: OpenAiCompatProviderConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    // Normalize the configured base URL into a completions endpoint.
    fn completions_url(&self) -> String {
        let trimmed = self.config.base_url.trim_end_matches('/');
        if trimmed.ends_with("/chat/completions") || trimmed.ends_with("/completions") {
            trimmed.to_string()
        } else {
            format!("{trimmed}/chat/completions")
        }
    }
}

#[async_trait]
impl LLMProvider for OpenAiCompatProvider {
    // Provider identifier exposed to the kernel.
    fn name(&self) -> &str {
        "openai-compatible"
    }

    async fn chat(&self, request: ChatCompletionRequest) -> AgentResult<ChatCompletionResponse> {
        // Translate the kernel chat request, send, and convert the response.
        let request_body = OpenAiCompatRequest {
            model: request
                .model
                .clone()
                .unwrap_or_else(|| self.config.model.clone()),
            messages: request
                .messages
                .iter()
                .map(OpenAiCompatMessage::from_kernel)
                .collect(),
            tools: request.tools.as_ref().map(|tools| {
                tools
                    .iter()
                    .map(OpenAiCompatToolDefinition::from_kernel)
                    .collect()
            }),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
        };

        eprintln!("OpenAI-compatible request = {:?}", request_body);
        let mut builder = self.client.post(self.completions_url()).json(&request_body);
        if let Some(api_key) = &self.config.api_key {
            builder = builder.bearer_auth(api_key);
        }

        let response = match builder.send().await {
            Ok(response) => response,
            Err(err) => {
        eprintln!("Provider send error: {}", err);
                let mut current: Option<&dyn StdError> = err.source();
                while let Some(source) = current {
            eprintln!("Provider send error source: {}", source);
                    current = source.source();
                }
                return Err(AgentError::ExecutionFailed(err.to_string()));
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            eprintln!("Provider error status={} body={}", status, body);
            return Err(AgentError::ExecutionFailed(format!(
                "provider returned {}: {}",
                status, body
            )));
        }

        let body: OpenAiCompatResponse = response
            .json()
            .await
            .map_err(|err| AgentError::SerializationError(err.to_string()))?;
        eprintln!("OpenAI-compatible response = {:?}", body);
        let message = body
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| AgentError::ExecutionFailed("provider returned no choices".to_string()))?
            .message;

        Ok(ChatCompletionResponse {
            content: message.content,
            tool_calls: Some(
                message
                    .tool_calls
                    .unwrap_or_default()
                    .into_iter()
                    .map(OpenAiCompatToolCall::into_kernel)
                    .collect(),
            ),
            usage: body.usage.map(OpenAiCompatUsage::into_kernel),
        })
    }
}

#[derive(Debug, Serialize)]
struct OpenAiCompatRequest {
    model: String,
    messages: Vec<OpenAiCompatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiCompatToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiCompatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiCompatToolCall>>,
}

impl OpenAiCompatMessage {
    // Translate a kernel chat message into the OpenAI-compatible wire format.
    fn from_kernel(message: &mofa_kernel::agent::types::ChatMessage) -> Self {
        Self {
            role: message.role.clone(),
            content: message.content.clone(),
            tool_call_id: message.tool_call_id.clone(),
            tool_calls: message.tool_calls.as_ref().map(|tool_calls| {
                tool_calls
                    .iter()
                    .cloned()
                    .map(OpenAiCompatToolCall::from_kernel)
                    .collect()
            }),
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiCompatToolDefinition {
    #[serde(rename = "type")]
    kind: &'static str,
    function: OpenAiCompatFunctionDefinition,
}

impl OpenAiCompatToolDefinition {
    // Map kernel tool metadata into the OpenAI-compatible representation.
    fn from_kernel(tool: &ToolDefinition) -> Self {
        Self {
            kind: "function",
            function: OpenAiCompatFunctionDefinition {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiCompatFunctionDefinition {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiCompatResponse {
    choices: Vec<OpenAiCompatChoice>,
    #[serde(default)]
    usage: Option<OpenAiCompatUsage>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiCompatChoice {
    message: OpenAiCompatMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiCompatToolCall {
    id: String,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    function: OpenAiCompatFunctionCall,
}

impl OpenAiCompatToolCall {
    // Serialize a kernel tool call into the cached tape format.
    fn from_kernel(tool_call: ToolCall) -> Self {
        Self {
            id: tool_call.id,
            kind: Some("function".to_string()),
            function: OpenAiCompatFunctionCall {
                name: tool_call.name,
                arguments: tool_call.arguments.to_string(),
            },
        }
    }

    // Restore the cached tool call to the kernel model.
    fn into_kernel(self) -> ToolCall {
        let arguments =
            serde_json::from_str(&self.function.arguments).unwrap_or(serde_json::Value::String(
                self.function.arguments,
            ));
        ToolCall {
            id: self.id,
            name: self.function.name,
            arguments,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiCompatFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiCompatUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

impl OpenAiCompatUsage {
    // Convert recorded usage into the kernel token usage struct.
    fn into_kernel(self) -> TokenUsage {
        TokenUsage {
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            total_tokens: self.total_tokens,
        }
    }
}
