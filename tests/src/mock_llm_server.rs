//! OpenAI-compatible mock LLM server for tests.
//!
//! Provides preset responses, response sequences, and error injection
//! for `/v1/chat/completions` requests.

use axum::{
    extract::State,
    http::StatusCode,
    routing::get,
    routing::post,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, RwLock};

#[derive(Debug)]
pub struct MockLlmServer {
    base_url: String,
    addr: SocketAddr,
    state: Arc<RwLock<ServerState>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

#[derive(Debug, Default)]
struct ServerState {
    rules: Vec<Rule>,
    sequences: Vec<(String, VecDeque<RuleResponse>)>,
    default_response: String,
    history: Vec<RequestRecord>,
    default_delay_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct Rule {
    prompt_substring: String,
    response: RuleResponse,
    delay_ms: Option<u64>,
}

#[derive(Debug, Clone)]
enum RuleResponse {
    Text(String),
    ToolCall {
        content: Option<String>,
        tool_calls: Vec<ToolCallSpec>,
    },
    Error { status: u16, message: String },
}

#[derive(Debug, Clone)]
pub struct ToolCallSpec {
    pub name: String,
    pub arguments: serde_json::Value,
    pub id: Option<String>,
}

/// Builder for configuring mock LLM server rules in tests.
#[derive(Debug, Default)]
pub struct MockLlmServerBuilder {
    rules: Vec<Rule>,
    sequences: Vec<(String, VecDeque<RuleResponse>)>,
    default_response: Option<String>,
    default_delay_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestRecord {
    pub received_at: DateTime<Utc>,
    pub model: Option<String>,
    pub prompt: String,
    pub raw: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize)]
struct ChatCompletionRequest {
    model: Option<String>,
    messages: Vec<ChatMessage>,
    #[serde(default)]
    stream: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct ChatMessage {
    role: Option<String>,
    content: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatCompletionResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Serialize)]
struct ChatChoice {
    index: usize,
    message: ChatMessageOut,
    finish_reason: String,
}

#[derive(Debug, Serialize)]
struct ChatMessageOut {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCallOut>>,
}

#[derive(Debug, Serialize)]
struct ToolCallOut {
    id: String,
    r#type: String,
    function: ToolFunctionOut,
}

#[derive(Debug, Serialize)]
struct ToolFunctionOut {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    message: String,
    r#type: String,
}

#[derive(Debug, Serialize)]
struct ModelsResponse {
    object: String,
    data: Vec<ModelInfo>,
}

#[derive(Debug, Serialize)]
struct ModelInfo {
    id: String,
    object: String,
    created: u64,
    owned_by: String,
}

impl MockLlmServer {
    /// Start a mock LLM server on a random local port.
    pub async fn start() -> anyhow::Result<Self> {
        MockLlmServerBuilder::default().start().await
    }

    /// Start a mock LLM server using a builder configuration.
    pub async fn start_with_builder(builder: MockLlmServerBuilder) -> anyhow::Result<Self> {
        builder.start().await
    }

    /// Start a mock LLM server using the provided internal state.
    async fn start_with_state(state: ServerState) -> anyhow::Result<Self> {
        let state = Arc::new(RwLock::new(state));

        let app = Router::new()
            .route("/v1/chat/completions", post(handle_chat))
            .route("/v1/models", get(handle_models))
            .with_state(state.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let base_url = format!("http://{}", addr);

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });
        tokio::spawn(async move {
            let _ = server.await;
        });

        Ok(Self {
            base_url,
            addr,
            state,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// Base URL for HTTP clients (e.g., `http://127.0.0.1:PORT`).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Socket address the server is bound to.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Add a response rule matched by prompt substring.
    pub async fn add_response_rule(&self, prompt_substring: &str, response: &str) {
        let mut state = self.state.write().await;
        state.rules.push(Rule {
            prompt_substring: prompt_substring.to_string(),
            response: RuleResponse::Text(response.to_string()),
            delay_ms: None,
        });
    }

    /// Add an error rule matched by prompt substring.
    pub async fn add_error_rule(&self, prompt_substring: &str, status: u16, message: &str) {
        let mut state = self.state.write().await;
        state.rules.push(Rule {
            prompt_substring: prompt_substring.to_string(),
            response: RuleResponse::Error {
                status,
                message: message.to_string(),
            },
            delay_ms: None,
        });
    }

    /// Add a tool-call response rule matched by prompt substring.
    pub async fn add_tool_call_rule(
        &self,
        prompt_substring: &str,
        tool_name: &str,
        arguments: serde_json::Value,
        content: Option<&str>,
    ) {
        let mut state = self.state.write().await;
        state.rules.push(Rule {
            prompt_substring: prompt_substring.to_string(),
            response: RuleResponse::ToolCall {
                content: content.map(|s| s.to_string()),
                tool_calls: vec![ToolCallSpec {
                    name: tool_name.to_string(),
                    arguments,
                    id: None,
                }],
            },
            delay_ms: None,
        });
    }

    /// Add a sequence of responses for a prompt substring.
    /// Each matching call consumes the next response; the last repeats.
    pub async fn add_response_sequence(&self, prompt_substring: &str, responses: Vec<&str>) {
        let mut state = self.state.write().await;
        let deque = responses.into_iter().map(|s| RuleResponse::Text(s.to_string())).collect();
        state.sequences.push((prompt_substring.to_string(), deque));
    }

    /// Add a sequence of tool-call responses for a prompt substring.
    /// Each matching call consumes the next response; the last repeats.
    pub async fn add_tool_call_sequence(
        &self,
        prompt_substring: &str,
        tool_calls: Vec<ToolCallSpec>,
        content: Option<&str>,
    ) {
        let mut state = self.state.write().await;
        let mut deque = VecDeque::new();
        for call in tool_calls {
            deque.push_back(RuleResponse::ToolCall {
                content: content.map(|s| s.to_string()),
                tool_calls: vec![call],
            });
        }
        state.sequences.push((prompt_substring.to_string(), deque));
    }

    /// Add a response rule with a fixed delay.
    pub async fn add_response_rule_with_delay(
        &self,
        prompt_substring: &str,
        response: &str,
        delay_ms: u64,
    ) {
        let mut state = self.state.write().await;
        state.rules.push(Rule {
            prompt_substring: prompt_substring.to_string(),
            response: RuleResponse::Text(response.to_string()),
            delay_ms: Some(delay_ms),
        });
    }

    /// Set a default delay for all responses (unless a rule overrides it).
    pub async fn set_default_delay(&self, delay_ms: Option<u64>) {
        let mut state = self.state.write().await;
        state.default_delay_ms = delay_ms;
    }

    /// Set the default response when no rule matches.
    pub async fn set_default_response(&self, response: &str) {
        let mut state = self.state.write().await;
        state.default_response = response.to_string();
    }

    /// Retrieve a snapshot of request history.
    pub async fn history(&self) -> Vec<RequestRecord> {
        let state = self.state.read().await;
        state.history.clone()
    }

    /// Clear request history.
    pub async fn clear_history(&self) {
        let mut state = self.state.write().await;
        state.history.clear();
    }

    /// Shutdown the server.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl MockLlmServerBuilder {
    /// Create a new builder with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the default response when no rule matches.
    pub fn default_response(mut self, response: &str) -> Self {
        self.default_response = Some(response.to_string());
        self
    }

    /// Set a default delay for all responses.
    pub fn default_delay_ms(mut self, delay_ms: u64) -> Self {
        self.default_delay_ms = Some(delay_ms);
        self
    }

    /// Add a static response rule.
    pub fn response_rule(mut self, prompt_substring: &str, response: &str) -> Self {
        self.rules.push(Rule {
            prompt_substring: prompt_substring.to_string(),
            response: RuleResponse::Text(response.to_string()),
            delay_ms: None,
        });
        self
    }

    /// Add a static response rule with a delay.
    pub fn response_rule_with_delay(
        mut self,
        prompt_substring: &str,
        response: &str,
        delay_ms: u64,
    ) -> Self {
        self.rules.push(Rule {
            prompt_substring: prompt_substring.to_string(),
            response: RuleResponse::Text(response.to_string()),
            delay_ms: Some(delay_ms),
        });
        self
    }

    /// Add an error rule.
    pub fn error_rule(mut self, prompt_substring: &str, status: u16, message: &str) -> Self {
        self.rules.push(Rule {
            prompt_substring: prompt_substring.to_string(),
            response: RuleResponse::Error {
                status,
                message: message.to_string(),
            },
            delay_ms: None,
        });
        self
    }

    /// Add a tool-call rule.
    pub fn tool_call_rule(
        mut self,
        prompt_substring: &str,
        tool_name: &str,
        arguments: serde_json::Value,
        content: Option<&str>,
    ) -> Self {
        self.rules.push(Rule {
            prompt_substring: prompt_substring.to_string(),
            response: RuleResponse::ToolCall {
                content: content.map(|s| s.to_string()),
                tool_calls: vec![ToolCallSpec {
                    name: tool_name.to_string(),
                    arguments,
                    id: None,
                }],
            },
            delay_ms: None,
        });
        self
    }

    /// Add a sequence of text responses.
    pub fn response_sequence(mut self, prompt_substring: &str, responses: Vec<&str>) -> Self {
        let deque = responses.into_iter().map(|s| RuleResponse::Text(s.to_string())).collect();
        self.sequences.push((prompt_substring.to_string(), deque));
        self
    }

    /// Add a sequence of tool-call responses.
    pub fn tool_call_sequence(
        mut self,
        prompt_substring: &str,
        tool_calls: Vec<ToolCallSpec>,
        content: Option<&str>,
    ) -> Self {
        let mut deque = VecDeque::new();
        for call in tool_calls {
            deque.push_back(RuleResponse::ToolCall {
                content: content.map(|s| s.to_string()),
                tool_calls: vec![call],
            });
        }
        self.sequences.push((prompt_substring.to_string(), deque));
        self
    }

    /// Start the server with the configured rules.
    pub async fn start(self) -> anyhow::Result<MockLlmServer> {
        let state = ServerState {
            rules: self.rules,
            sequences: self.sequences,
            default_response: self
                .default_response
                .unwrap_or_else(|| "Mock fallback response.".to_string()),
            history: Vec::new(),
            default_delay_ms: self.default_delay_ms,
        };
        MockLlmServer::start_with_state(state).await
    }
}

async fn handle_chat(
    State(state): State<Arc<RwLock<ServerState>>>,
    Json(payload): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, (StatusCode, Json<ErrorResponse>)> {
    if payload.stream {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: ErrorBody {
                    message: "streaming not supported by MockLlmServer".to_string(),
                    r#type: "invalid_request_error".to_string(),
                },
            }),
        ));
    }

    // Basic request validation to keep tests deterministic.
    if payload.messages.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: ErrorBody {
                    message: "messages must be non-empty".to_string(),
                    r#type: "invalid_request_error".to_string(),
                },
            }),
        ));
    }

    // Require at least one non-empty message content.
    if payload
        .messages
        .iter()
        .all(|msg| msg.content.as_deref().unwrap_or("").is_empty())
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: ErrorBody {
                    message: "messages must include content".to_string(),
                    r#type: "invalid_request_error".to_string(),
                },
            }),
        ));
    }

    let prompt = build_prompt(&payload.messages);
    let raw = serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null);

    let (response, delay_ms) = {
        let mut guard = state.write().await;
        guard.history.push(RequestRecord {
            received_at: Utc::now(),
            model: payload.model.clone(),
            prompt: prompt.clone(),
            raw,
        });
        resolve_response(&mut guard, &prompt)
    };

    if let Some(delay) = delay_ms {
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
    }

    match response {
        RuleResponse::Text(text) => Ok(Json(ChatCompletionResponse {
            id: format!("mock-{}", uuid::Uuid::now_v7()),
            object: "chat.completion".to_string(),
            created: Utc::now().timestamp() as u64,
            model: payload
                .model
                .clone()
                .unwrap_or_else(|| "mock-model".to_string()),
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessageOut {
                    role: "assistant".to_string(),
                    content: text,
                    tool_calls: None,
                },
                finish_reason: "stop".to_string(),
            }],
        })),
        RuleResponse::ToolCall { content, tool_calls } => Ok(Json(ChatCompletionResponse {
            id: format!("mock-{}", uuid::Uuid::now_v7()),
            object: "chat.completion".to_string(),
            created: Utc::now().timestamp() as u64,
            model: payload
                .model
                .clone()
                .unwrap_or_else(|| "mock-model".to_string()),
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessageOut {
                    role: "assistant".to_string(),
                    content: content.unwrap_or_default(),
                    tool_calls: Some(
                        tool_calls
                            .into_iter()
                            .map(|spec| ToolCallOut {
                                id: spec.id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
                                r#type: "function".to_string(),
                                function: ToolFunctionOut {
                                    name: spec.name,
                                    arguments: serde_json::to_string(&spec.arguments)
                                        .unwrap_or_else(|_| "{}".to_string()),
                                },
                            })
                            .collect(),
                    ),
                },
                finish_reason: "tool_calls".to_string(),
            }],
        })),
        RuleResponse::Error { status, message } => Err((
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(ErrorResponse {
                error: ErrorBody {
                    message,
                    r#type: "mock_error".to_string(),
                },
            }),
        )),
    }
}

/// Handle OpenAI-compatible model listing requests.
async fn handle_models(
    State(state): State<Arc<RwLock<ServerState>>>,
) -> Json<ModelsResponse> {
    let model_id = {
        let guard = state.read().await;
        guard
            .history
            .last()
            .and_then(|record| record.model.clone())
            .unwrap_or_else(|| "mock-model".to_string())
    };

    Json(ModelsResponse {
        object: "list".to_string(),
        data: vec![ModelInfo {
            id: model_id,
            object: "model".to_string(),
            created: Utc::now().timestamp() as u64,
            owned_by: "mock-llm-server".to_string(),
        }],
    })
}

fn resolve_response(state: &mut ServerState, prompt: &str) -> (RuleResponse, Option<u64>) {
    for (key, deque) in state.sequences.iter_mut() {
        if prompt.contains(key.as_str()) {
            if deque.len() > 1 {
                let response = deque.pop_front().unwrap_or_else(|| {
                    RuleResponse::Text(state.default_response.clone())
                });
                return (response, state.default_delay_ms);
            }
            if let Some(last) = deque.front() {
                return (last.clone(), state.default_delay_ms);
            }
        }
    }

    for rule in state.rules.iter() {
        if prompt.contains(&rule.prompt_substring) {
            return (rule.response.clone(), rule.delay_ms.or(state.default_delay_ms));
        }
    }

    (
        RuleResponse::Text(state.default_response.clone()),
        state.default_delay_ms,
    )
}

fn build_prompt(messages: &[ChatMessage]) -> String {
    let mut parts = Vec::new();
    for msg in messages {
        let role = msg.role.as_deref().unwrap_or("unknown");
        let content = msg.content.as_deref().unwrap_or("");
        if !content.is_empty() {
            parts.push(format!("{}: {}", role, content));
        }
    }
    parts.join("\n")
}
