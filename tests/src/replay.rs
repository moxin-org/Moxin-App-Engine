//! Replay tape support for deterministic DSL-backed runs.

use async_trait::async_trait;
use mofa_kernel::agent::error::{AgentError, AgentResult};
use mofa_kernel::agent::types::{
    ChatCompletionRequest, ChatCompletionResponse, LLMProvider, ToolCall, TokenUsage,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TapeInteraction {
    #[serde(default)]
    pub request: Option<TapeRequest>,
    pub response: String,
    #[serde(default)]
    pub tool_calls: Vec<TapeToolCall>,
    #[serde(default)]
    pub usage: Option<TapeTokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tape {
    pub version: u32,
    pub case_name: String,
    pub interactions: Vec<TapeInteraction>,
}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("failed to read tape file: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse tape JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("tape has no interactions")]
    EmptyTape,

    #[error("replay exhausted after {0} interactions")]
    ReplayExhausted(usize),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TapeRequest {
    pub messages: Vec<TapeMessage>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub tool_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TapeMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<TapeToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TapeToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TapeTokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

pub struct RecordingLLMProvider {
    name: String,
    inner: Arc<dyn LLMProvider>,
    case_name: String,
    output_path: PathBuf,
    tape: RwLock<Tape>,
    last_request: RwLock<Option<ChatCompletionRequest>>,
    last_response: RwLock<Option<ChatCompletionResponse>>,
}

pub struct ReplayLLMProvider {
    name: String,
    tape: Tape,
    cursor: RwLock<usize>,
    last_request: RwLock<Option<ChatCompletionRequest>>,
    last_response: RwLock<Option<ChatCompletionResponse>>,
}

impl Tape {
    // Start a new tape for the given case.
    pub fn new(case_name: impl Into<String>) -> Self {
        Self {
            version: 1,
            case_name: case_name.into(),
            interactions: Vec::new(),
        }
    }

    // Load a tape from disk.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ReplayError> {
        let body = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&body)?)
    }

    // Persist the tape back to disk.
    pub fn to_file(&self, path: impl AsRef<Path>) -> Result<(), ReplayError> {
        let body = serde_json::to_string_pretty(self)?;
        std::fs::write(path, body)?;
        Ok(())
    }

    // Extract the recorded responses for quick replay validation.
    pub fn responses(&self) -> Result<Vec<String>, ReplayError> {
        if self.interactions.is_empty() {
            return Err(ReplayError::EmptyTape);
        }
        Ok(self
            .interactions
            .iter()
            .map(|interaction| interaction.response.clone())
            .collect())
    }

    // Append a new interaction to the tape.
    pub fn push_interaction(&mut self, request: ChatCompletionRequest, response: &ChatCompletionResponse) {
        self.interactions.push(TapeInteraction {
            request: Some(TapeRequest::from_request(&request)),
            response: response.content.clone().unwrap_or_default(),
            tool_calls: response
                .tool_calls
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(TapeToolCall::from_tool_call)
                .collect(),
            usage: response.usage.clone().map(TapeTokenUsage::from_usage),
        });
    }
}

impl TapeRequest {
    // Snapshot a chat request into the tape schema.
    fn from_request(request: &ChatCompletionRequest) -> Self {
        Self {
            messages: request
                .messages
                .iter()
                .map(|message| TapeMessage {
                    role: message.role.clone(),
                    content: message.content.clone(),
                    tool_call_id: message.tool_call_id.clone(),
                    tool_calls: message
                        .tool_calls
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .map(TapeToolCall::from_tool_call)
                        .collect(),
                })
                .collect(),
            model: request.model.clone(),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            tool_names: request
                .tools
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|tool| tool.name)
                .collect(),
        }
    }
}

impl TapeToolCall {
    // Encode a real tool call for replay.
    fn from_tool_call(tool_call: ToolCall) -> Self {
        Self {
            id: tool_call.id,
            name: tool_call.name,
            arguments: tool_call.arguments,
        }
    }

    // Decode the tape entry back into a kernel tool call.
    fn into_tool_call(self) -> ToolCall {
        ToolCall {
            id: self.id,
            name: self.name,
            arguments: self.arguments,
        }
    }
}

impl TapeTokenUsage {
    // Capture token usage for later inspection.
    fn from_usage(usage: TokenUsage) -> Self {
        Self {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        }
    }

    // Rehydrate the token usage for replay responses.
    fn into_usage(self) -> TokenUsage {
        TokenUsage {
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            total_tokens: self.total_tokens,
        }
    }
}

impl RecordingLLMProvider {
    // Wrap a live provider so every response is recorded to tape.
    pub fn new(
        name: impl Into<String>,
        inner: Arc<dyn LLMProvider>,
        case_name: impl Into<String>,
        output_path: impl Into<PathBuf>,
    ) -> Self {
        let case_name = case_name.into();
        Self {
            name: name.into(),
            inner,
            tape: RwLock::new(Tape::new(case_name.clone())),
            case_name,
            output_path: output_path.into(),
            last_request: RwLock::new(None),
            last_response: RwLock::new(None),
        }
    }

    // Return the most recent request issued through this recording provider.
    pub async fn last_request(&self) -> Option<ChatCompletionRequest> {
        self.last_request.read().await.clone()
    }

    // Return the latest response captured by the recording provider.
    pub async fn last_response(&self) -> Option<ChatCompletionResponse> {
        self.last_response.read().await.clone()
    }
}

#[async_trait]
impl LLMProvider for RecordingLLMProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat(&self, request: ChatCompletionRequest) -> AgentResult<ChatCompletionResponse> {
        // Forward the call, cache the interaction, and persist the tape.
        *self.last_request.write().await = Some(request.clone());
        let response = self.inner.chat(request.clone()).await?;
        *self.last_response.write().await = Some(response.clone());

        let mut tape = self.tape.write().await;
        if tape.case_name.is_empty() {
            tape.case_name = self.case_name.clone();
        }
        tape.push_interaction(request, &response);
        tape.to_file(&self.output_path)
            .map_err(|err| AgentError::ExecutionFailed(err.to_string()))?;

        Ok(response)
    }
}

impl ReplayLLMProvider {
    // Create a replay provider that steps through a pre-recorded tape.
    pub fn new(name: impl Into<String>, tape: Tape) -> Self {
        Self {
            name: name.into(),
            tape,
            cursor: RwLock::new(0),
            last_request: RwLock::new(None),
            last_response: RwLock::new(None),
        }
    }

    // Return the last request seen by the replay provider.
    pub async fn last_request(&self) -> Option<ChatCompletionRequest> {
        self.last_request.read().await.clone()
    }

    // Return the last response handed out during replay.
    pub async fn last_response(&self) -> Option<ChatCompletionResponse> {
        self.last_response.read().await.clone()
    }
}

#[async_trait]
impl LLMProvider for ReplayLLMProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat(&self, request: ChatCompletionRequest) -> AgentResult<ChatCompletionResponse> {
        // Serve the next recorded interaction instead of calling a live LLM.
        *self.last_request.write().await = Some(request);
        let mut cursor = self.cursor.write().await;
        let interaction = self
            .tape
            .interactions
            .get(*cursor)
            .cloned()
            .or_else(|| self.tape.interactions.last().cloned())
            .ok_or_else(|| AgentError::ExecutionFailed(ReplayError::EmptyTape.to_string()))?;
        if *cursor < self.tape.interactions.len() {
            *cursor += 1;
        }

        let response = ChatCompletionResponse {
            content: Some(interaction.response),
            tool_calls: Some(
                interaction
                    .tool_calls
                    .into_iter()
                    .map(TapeToolCall::into_tool_call)
                    .collect(),
            ),
            usage: interaction.usage.map(TapeTokenUsage::into_usage),
        };
        *self.last_response.write().await = Some(response.clone());
        Ok(response)
    }
}
