//! OpenAI-compatible request/response types for the MoFA inference gateway.
//!
//! All types follow the OpenAI Chat Completions API spec so that any
//! OpenAI-SDK-compatible client can target the MoFA gateway with zero
//! code changes.

#![allow(missing_docs)]

use serde::{Deserialize, Serialize};

use mofa_foundation::inference::types::{InferenceRequest, RequestPriority};
use mofa_foundation::inference::{OrchestratorConfig, RoutingPolicy};

// ──────────────────────────────────────────────────────────────────────────────
// Request types
// ──────────────────────────────────────────────────────────────────────────────

/// A single message in the conversation, following the OpenAI role/content model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Role of the message sender. One of `"system"`, `"user"`, `"assistant"`.
    pub role: String,
    /// The text content of the message.
    pub content: String,
}

/// The body of a `POST /v1/chat/completions` request.
///
/// Compatible with the OpenAI Chat Completions API. The optional `priority`
/// field is a MoFA extension that maps to [`RequestPriority`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    /// Model identifier — used as the routing key.
    ///
    /// Use a local model id (e.g., `"qwen3-local"`) to prefer on-device
    /// inference, or an upstream model id (e.g., `"gpt-4o"`) to route to
    /// the configured cloud provider.
    pub model: String,

    /// Conversation history, in chronological order.
    pub messages: Vec<ChatMessage>,

    /// If `true`, the response is streamed as Server-Sent Events.
    #[serde(default)]
    pub stream: bool,

    /// Maximum number of tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Sampling temperature (0.0–2.0). Passed through to the backend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// **MoFA extension**: request priority for admission control.
    ///
    /// Defaults to `Normal` if omitted. See [`RequestPriority`] for semantics.
    #[serde(default)]
    pub priority: RequestPriorityParam,
}

impl ChatCompletionRequest {
    /// Extract the prompt by concatenating all messages (user + system).
    pub fn to_prompt(&self) -> String {
        self.messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Convert the MoFA priority extension to an internal [`RequestPriority`].
    pub fn priority(&self) -> RequestPriority {
        match self.priority {
            RequestPriorityParam::Low => RequestPriority::Low,
            RequestPriorityParam::Normal => RequestPriority::Normal,
            RequestPriorityParam::High => RequestPriority::High,
            RequestPriorityParam::Critical => RequestPriority::Critical,
            // Future-proof: treat unknown variants as Normal.
            _ => RequestPriority::Normal,
        }
    }

    /// Convert API request into an internal inference request.
    ///
    /// Carries generation controls (`max_tokens`, `temperature`) forward so
    /// downstream routing/providers can consume them instead of silently
    /// dropping user-specified parameters.
    pub fn to_inference_request(&self, required_memory_mb: usize) -> InferenceRequest {
        let mut req = InferenceRequest::new(&self.model, self.to_prompt(), required_memory_mb)
            .with_priority(self.priority());

        if let Some(max_tokens) = self.max_tokens {
            req = req.with_max_tokens(max_tokens);
        }
        if let Some(temperature) = self.temperature {
            req = req.with_temperature(temperature);
        }

        req
    }
}

/// Serializable counterpart to [`RequestPriority`] for JSON deserialization.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum RequestPriorityParam {
    /// Low priority — may be deferred when memory is tight.
    Low,
    /// Normal (default) priority.
    #[default]
    Normal,
    /// High priority — bypasses the defer band.
    High,
    /// Critical priority — bypasses the defer band; eviction may be attempted.
    Critical,
}

// ──────────────────────────────────────────────────────────────────────────────
// Non-streaming response types
// ──────────────────────────────────────────────────────────────────────────────

/// A fully-formed chat completion response, following the OpenAI spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    /// Unique identifier for this completion (e.g., `"chatcmpl-abc123"`).
    pub id: String,
    /// Always `"chat.completion"`.
    pub object: String,
    /// Unix timestamp of when the completion was created.
    pub created: u64,
    /// The model that was ultimately used (may differ from requested if routed).
    pub model: String,
    /// One or more completion choices.
    pub choices: Vec<Choice>,
    /// Token usage statistics.
    pub usage: Usage,
}

/// A single completion choice in a non-streaming response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    /// Zero-based index of this choice.
    pub index: u32,
    /// The generated message.
    pub message: ChatMessage,
    /// Why the model stopped generating.
    pub finish_reason: String,
}

/// Token usage for the request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

// ──────────────────────────────────────────────────────────────────────────────
// Streaming response types (SSE)
// ──────────────────────────────────────────────────────────────────────────────

/// A single Server-Sent Event chunk for streamed completions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    /// Always `"chat.completion.chunk"`.
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

/// A streaming choice delta.
#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: Delta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// The partial content in a streaming chunk.
#[allow(missing_docs)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Error type
// ──────────────────────────────────────────────────────────────────────────────

/// Gateway error body, following the OpenAI error envelope convention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayErrorBody {
    /// The error detail envelope.
    pub error: GatewayErrorDetail,
}

/// Inner error detail within a [`GatewayErrorBody`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayErrorDetail {
    /// Human-readable error message.
    pub message: String,
    /// Machine-readable error type (e.g., `"rate_limit_error"`).
    pub r#type: String,
    /// Optional error code for finer-grained programmatic handling.
    pub code: Option<String>,
}

impl GatewayErrorBody {
    /// Create a new error body with the given message and type.
    pub fn new(message: impl Into<String>, err_type: impl Into<String>) -> Self {
        Self {
            error: GatewayErrorDetail {
                message: message.into(),
                r#type: err_type.into(),
                code: None,
            },
        }
    }

    /// Construct a rate-limit error body.
    pub fn rate_limited() -> Self {
        Self::new(
            "Rate limit exceeded. Please slow down your requests.",
            "rate_limit_error",
        )
    }

    /// Construct an invalid-request error body.
    pub fn invalid_request(msg: impl Into<String>) -> Self {
        Self::new(msg, "invalid_request_error")
    }

    /// Construct a server error body.
    pub fn server_error(msg: impl Into<String>) -> Self {
        Self::new(msg, "server_error")
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Models list
// ──────────────────────────────────────────────────────────────────────────────

/// Response for `GET /v1/models`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelListResponse {
    /// Always `"list"`.
    pub object: String,
    /// The list of available model objects.
    pub data: Vec<ModelObject>,
}

/// An entry in the models list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelObject {
    /// Model identifier.
    pub id: String,
    /// Always `"model"`.
    pub object: String,
    /// Unix timestamp of model creation.
    pub created: u64,
    /// Owner of the model.
    pub owned_by: String,
}

// ──────────────────────────────────────────────────────────────────────────────
// Gateway configuration
// ──────────────────────────────────────────────────────────────────────────────

/// Configuration for the MoFA inference gateway server.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// Hostname or IP to bind to (default: `"127.0.0.1"`)
    pub host: String,
    /// Port to listen on (default: `8080`)
    pub port: u16,
    /// Maximum requests per minute per client IP (default: `60`)
    pub rate_limit_rpm: u32,
    /// Underlying orchestrator configuration.
    pub orchestrator_config: OrchestratorConfig,
    /// Models to advertise on the `/v1/models` endpoint.
    pub available_models: Vec<String>,
    /// Optional API key for static bearer token authentication.
    pub api_key: Option<String>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            rate_limit_rpm: 60,
            orchestrator_config: OrchestratorConfig::default(),
            available_models: vec!["mofa-local".to_string()],
            api_key: None,
        }
    }
}

impl GatewayConfig {
    /// Create a new config with the given port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set rate limit in requests-per-minute.
    pub fn with_rpm(mut self, rpm: u32) -> Self {
        self.rate_limit_rpm = rpm;
        self
    }

    /// Set the routing policy on the underlying orchestrator.
    pub fn with_routing_policy(mut self, policy: RoutingPolicy) -> Self {
        self.orchestrator_config.routing_policy = policy;
        self
    }

    /// Set available models list.
    pub fn with_models(mut self, models: Vec<String>) -> Self {
        self.available_models = models;
        self
    }

    /// Set the API key for authentication.
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Typed gateway error
// ──────────────────────────────────────────────────────────────────────────────

/// Typed error for [`GatewayServer::serve`](super::server::GatewayServer::serve).
///
/// Uses distinct variants so callers can match on specific failure modes
/// instead of dealing with an opaque `Box<dyn Error>`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OpenAiGatewayError {
    /// The configured `host:port` string could not be parsed as a [`SocketAddr`](std::net::SocketAddr).
    #[error("invalid listen address: {0}")]
    AddrParse(#[from] std::net::AddrParseError),

    /// TCP bind failed (e.g., port already in use).
    #[error("failed to bind TCP listener: {0}")]
    Bind(std::io::Error),

    /// The axum server returned an error.
    #[error("server error: {0}")]
    Serve(std::io::Error),
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_request_serialization() {
        let json = r#"{
            "model": "qwen3-local",
            "messages": [
                {"role": "user", "content": "Hello!"}
            ],
            "stream": false
        }"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.model, "qwen3-local");
        assert_eq!(req.messages.len(), 1);
        assert!(!req.stream);
    }

    #[test]
    fn test_priority_defaults_to_normal() {
        let json = r#"{"model":"m","messages":[]}"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req.priority, RequestPriorityParam::Normal));
    }

    #[test]
    fn test_to_prompt() {
        let req = ChatCompletionRequest {
            model: "m".into(),
            messages: vec![
                ChatMessage {
                    role: "system".into(),
                    content: "Be helpful.".into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: "Hi".into(),
                },
            ],
            stream: false,
            max_tokens: None,
            temperature: None,
            priority: RequestPriorityParam::Normal,
        };
        let prompt = req.to_prompt();
        assert!(prompt.contains("system: Be helpful."));
        assert!(prompt.contains("user: Hi"));
    }

    #[test]
    fn test_to_inference_request_propagates_generation_params() {
        let req = ChatCompletionRequest {
            model: "mofa-local".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "Generate".into(),
            }],
            stream: false,
            max_tokens: Some(128),
            temperature: Some(0.2),
            priority: RequestPriorityParam::High,
        };

        let internal = req.to_inference_request(7168);
        assert_eq!(internal.model_id, "mofa-local");
        assert_eq!(internal.required_memory_mb, 7168);
        assert_eq!(internal.max_tokens, Some(128));
        assert_eq!(internal.temperature, Some(0.2));
        assert_eq!(internal.priority, RequestPriority::High);
    }

    #[test]
    fn test_gateway_error_body_rate_limited() {
        let err = GatewayErrorBody::rate_limited();
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("rate_limit_error"));
    }

    #[test]
    fn test_response_serializes() {
        let resp = ChatCompletionResponse {
            id: "chatcmpl-test".into(),
            object: "chat.completion".into(),
            created: 1_000_000,
            model: "mofa-local".into(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: "Hello!".into(),
                },
                finish_reason: "stop".into(),
            }],
            usage: Usage {
                prompt_tokens: 5,
                completion_tokens: 3,
                total_tokens: 8,
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("chat.completion"));
        assert!(json.contains("assistant"));
    }
}
