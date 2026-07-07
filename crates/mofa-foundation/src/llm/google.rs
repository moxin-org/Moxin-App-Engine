//! Google Gemini Provider
//!
//! Implements Gemini Pro via Generative Language API v1beta with streaming support.
//! Parses Gemini SSE events and maps them to `ChatCompletionChunk`.

use super::provider::{ChatStream, LLMProvider, ModelCapabilities, ModelInfo};
use super::types::*;
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Gemini provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiConfig {
    /// API key
    pub api_key: String,
    /// Base URL (default: https://generativelanguage.googleapis.com)
    pub base_url: String,
    /// Default model id, e.g., gemini-1.5-pro-latest
    pub default_model: String,
    /// Default temperature
    pub default_temperature: f32,
    /// Default max output tokens
    pub default_max_tokens: u32,
    /// Request timeout
    pub timeout_secs: u64,
}

impl Default for GeminiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            default_model: "gemini-1.5-pro-latest".to_string(),
            default_temperature: 0.7,
            default_max_tokens: 2048,
            timeout_secs: 60,
        }
    }
}

impl GeminiConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            ..Default::default()
        }
    }

    pub fn from_env() -> Self {
        let mut cfg = Self {
            api_key: std::env::var("GEMINI_API_KEY").unwrap_or_default(),
            ..Default::default()
        };

        if let Ok(model) = std::env::var("GEMINI_MODEL") {
            cfg.default_model = model;
        }
        if let Ok(base_url) = std::env::var("GEMINI_BASE_URL") {
            cfg.base_url = base_url;
        }
        cfg
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = model.into();
        self
    }

    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.default_temperature = temp;
        self
    }

    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.default_max_tokens = tokens;
        self
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }
}

/// Gemini provider with streaming support
pub struct GeminiProvider {
    client: reqwest::Client,
    config: GeminiConfig,
}

impl GeminiProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_config(GeminiConfig::new(api_key))
    }

    pub fn from_env() -> Self {
        Self::with_config(GeminiConfig::from_env())
    }

    pub fn with_config(config: GeminiConfig) -> Self {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
        {
            Ok(client) => client,
            Err(e) => {
                tracing::error!(
                    "Failed to build Gemini reqwest client (timeout_secs={}): {}. Falling back to default client.",
                    config.timeout_secs,
                    e
                );
                reqwest::Client::new()
            }
        };
        Self { client, config }
    }

    fn convert_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<serde_json::Value>) {
        let mut system_parts = Vec::new();
        let mut contents = Vec::new();

        for msg in messages {
            match msg.role {
                Role::System => {
                    if let Some(text) = msg.text_content() {
                        system_parts.push(text.to_string());
                    }
                }
                Role::User | Role::Assistant | Role::Tool => {
                    let role = match msg.role {
                        Role::User => "user",
                        Role::Assistant => "model",
                        Role::Tool => "user",
                        Role::System => unreachable!(),
                    };

                    match &msg.content {
                        Some(MessageContent::Text(t)) => {
                            contents.push(serde_json::json!({
                                "role": role,
                                "parts": [{"text": t.clone()}],
                            }));
                        }
                        Some(MessageContent::Parts(parts)) => {
                            let mut gemini_parts = Vec::new();
                            for part in parts {
                                match part {
                                    ContentPart::Text { text } => {
                                        gemini_parts
                                            .push(serde_json::json!({"text": text.clone()}));
                                    }
                                    ContentPart::Image { image_url } => {
                                        let mime_type = if image_url.url.contains("data:image/jpeg")
                                        {
                                            "image/jpeg"
                                        } else if image_url.url.contains("data:image/png") {
                                            "image/png"
                                        } else if image_url.url.contains("data:image/webp") {
                                            "image/webp"
                                        } else {
                                            "image/jpeg"
                                        };
                                        let data = image_url
                                            .url
                                            .split(',')
                                            .next_back()
                                            .unwrap_or(&image_url.url);
                                        gemini_parts.push(serde_json::json!({
                                            "inlineData": {
                                                "mimeType": mime_type,
                                                "data": data,
                                            }
                                        }));
                                    }
                                    ContentPart::Audio { audio } => {
                                        let mime_type =
                                            format!("audio/{}", audio.format.to_lowercase());
                                        let data = audio
                                            .data
                                            .split(',')
                                            .next_back()
                                            .unwrap_or(&audio.data);
                                        gemini_parts.push(serde_json::json!({
                                            "inlineData": {
                                                "mimeType": mime_type,
                                                "data": data,
                                            }
                                        }));
                                    }
                                    ContentPart::Video { video } => {
                                        let mime_type =
                                            format!("video/{}", video.format.to_lowercase());
                                        let data = video
                                            .data
                                            .split(',')
                                            .next_back()
                                            .unwrap_or(&video.data);
                                        gemini_parts.push(serde_json::json!({
                                            "inlineData": {
                                                "mimeType": mime_type,
                                                "data": data,
                                            }
                                        }));
                                    }
                                }
                            }
                            contents.push(serde_json::json!({
                                "role": role,
                                "parts": gemini_parts,
                            }));
                        }
                        None => {}
                    }
                }
            }
        }

        let system = if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n"))
        };

        (system, contents)
    }

    /// Build the JSON request body (shared between chat and chat_stream)
    fn build_request_body(&self, request: &ChatCompletionRequest) -> serde_json::Value {
        let (system, contents) = Self::convert_messages(&request.messages);

        let mut body = serde_json::json!({
            "contents": contents,
            "generationConfig": {
                "temperature": request
                    .temperature
                    .unwrap_or(self.config.default_temperature),
                "maxOutputTokens": request
                    .max_tokens
                    .unwrap_or(self.config.default_max_tokens),
            }
        });

        if let Some(sys) = system {
            body["systemInstruction"] = serde_json::json!({"parts": [{"text": sys}]});
        }

        if let Some(tp) = request.top_p {
            body["generationConfig"]["topP"] = serde_json::json!(tp);
        }

        if let Some(stop) = request.stop.clone() {
            body["generationConfig"]["stopSequences"] = serde_json::json!(stop);
        }

        body
    }

    fn map_error(err: reqwest::Error) -> LLMError {
        if err.is_timeout() {
            LLMError::Timeout(err.to_string())
        } else if err.is_connect() || err.is_request() {
            LLMError::NetworkError(err.to_string())
        } else {
            LLMError::Other(err.to_string())
        }
    }
}

// ============================================================================
// Gemini response types (shared between non-streaming and streaming)
// ============================================================================

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize)]
struct GeminiPart {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiUsage {
    #[serde(rename = "promptTokenCount", default)]
    prompt_tokens: u32,
    #[serde(rename = "candidatesTokenCount", default)]
    candidates_tokens: u32,
    #[serde(rename = "totalTokenCount", default)]
    total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata")]
    usage: Option<GeminiUsage>,
    model_version: Option<String>,
}

/// A single streaming chunk from the Gemini API.
///
/// Gemini's `streamGenerateContent?alt=sse` endpoint sends newline-delimited
/// JSON objects on `data:` lines. Each object has the same shape as the
/// non-streaming response but contains partial content.
#[derive(Debug, Deserialize)]
struct GeminiStreamChunk {
    candidates: Option<Vec<GeminiCandidate>>,
    #[serde(rename = "usageMetadata")]
    usage: Option<GeminiUsage>,
}

// ============================================================================
// Gemini SSE streaming parser
// ============================================================================

/// Map Gemini finish reason strings to `FinishReason`
fn map_gemini_finish_reason(reason: &str) -> Option<FinishReason> {
    match reason {
        "STOP" => Some(FinishReason::Stop),
        "MAX_TOKENS" => Some(FinishReason::Length),
        "SAFETY" | "RECITATION" | "OTHER" => Some(FinishReason::ContentFilter),
        _ => None,
    }
}

/// Convert a `GeminiStreamChunk` into a `ChatCompletionChunk`.
///
/// `is_first` controls whether the role field is populated (only on the first chunk).
fn gemini_chunk_to_completion(
    chunk: &GeminiStreamChunk,
    model: &str,
    is_first: bool,
) -> ChatCompletionChunk {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let (content, finish_reason) = chunk
        .candidates
        .as_ref()
        .and_then(|cs| cs.first())
        .map(|c| {
            let text = c
                .content
                .as_ref()
                .and_then(|cont| cont.parts.iter().filter_map(|p| p.text.clone()).next());
            let fr = c
                .finish_reason
                .as_deref()
                .and_then(map_gemini_finish_reason);
            (text, fr)
        })
        .unwrap_or((None, None));

    let usage = chunk.usage.as_ref().map(|u| Usage {
        prompt_tokens: u.prompt_tokens,
        completion_tokens: u.candidates_tokens,
        total_tokens: u.total_tokens,
    });

    ChatCompletionChunk {
        id: String::new(),
        object: "chat.completion.chunk".to_string(),
        created: now,
        model: model.to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: if is_first {
                    Some(Role::Assistant)
                } else {
                    None
                },
                content,
                tool_calls: None,
            },
            finish_reason,
        }],
        usage,
    }
}

/// Parse raw SSE lines from a Gemini streaming response into `ChatCompletionChunk` items.
///
/// Uses `futures::stream::unfold` with byte-level buffering, identical in
/// structure to `parse_anthropic_sse` in `anthropic.rs`.
fn parse_gemini_sse(resp: reqwest::Response, model: String) -> ChatStream {
    let stream = futures::stream::unfold(
        (resp, String::new(), model, true),
        |(mut resp, mut buf, model, mut is_first)| async move {
            loop {
                // Try to extract a complete line from the buffer.
                if let Some(newline_pos) = buf.find('\n') {
                    let line = buf[..newline_pos].trim_end_matches('\r').to_string();
                    buf = buf[newline_pos + 1..].to_string();

                    if line.is_empty() {
                        continue;
                    }

                    if let Some(json_str) = line.strip_prefix("data: ") {
                        // Gemini sends `data: [DONE]` as the sentinel.
                        if json_str.trim() == "[DONE]" {
                            return None;
                        }

                        match serde_json::from_str::<GeminiStreamChunk>(json_str) {
                            Ok(chunk) => {
                                let completion =
                                    gemini_chunk_to_completion(&chunk, &model, is_first);
                                is_first = false;
                                return Some((Ok(completion), (resp, buf, model, is_first)));
                            }
                            Err(e) => {
                                tracing::warn!("Skipping unparseable Gemini SSE chunk: {}", e);
                                continue;
                            }
                        }
                    }

                    // Skip non-data lines (e.g., comments, event types)
                    continue;
                }

                // Need more bytes from the network.
                match resp.chunk().await {
                    Ok(Some(bytes)) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Ok(None) => {
                        // End of stream. If there is leftover data, try to parse it.
                        if !buf.trim().is_empty()
                            && let Some(json_str) = buf.trim().strip_prefix("data: ")
                            && json_str.trim() != "[DONE]"
                            && let Ok(chunk) = serde_json::from_str::<GeminiStreamChunk>(json_str)
                        {
                            let completion = gemini_chunk_to_completion(&chunk, &model, is_first);
                            return Some((Ok(completion), (resp, String::new(), model, false)));
                        }
                        return None;
                    }
                    Err(e) => {
                        let err = LLMError::NetworkError(e.to_string());
                        return Some((Err(err), (resp, buf, model, is_first)));
                    }
                }
            }
        },
    );

    Box::pin(stream)
}

#[async_trait]
impl LLMProvider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    fn default_model(&self) -> &str {
        &self.config.default_model
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tools(&self) -> bool {
        false
    }

    fn supports_vision(&self) -> bool {
        false
    }

    async fn chat(&self, request: ChatCompletionRequest) -> LLMResult<ChatCompletionResponse> {
        let model = if request.model.is_empty() {
            self.config.default_model.clone()
        } else {
            request.model.clone()
        };

        let body = self.build_request_body(&request);

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.config.base_url.trim_end_matches('/'),
            model,
            self.config.api_key
        );

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(Self::map_error)?;

        let status = resp.status();
        let text = resp.text().await.map_err(Self::map_error)?;

        if !status.is_success() {
            return Err(LLMError::ApiError {
                code: Some(status.as_u16().to_string()),
                message: text,
            });
        }

        let parsed: GeminiResponse =
            serde_json::from_str(&text).map_err(|e| LLMError::SerializationError(e.to_string()))?;

        let first_candidate = parsed.candidates.first();

        let first = first_candidate
            .and_then(|c| c.content.as_ref())
            .and_then(|c| c.parts.iter().find_map(|p| p.text.clone()))
            .unwrap_or_default();

        let finish_reason = first_candidate
            .and_then(|c| c.finish_reason.as_deref())
            .and_then(map_gemini_finish_reason);

        let usage = parsed.usage.map(|u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.candidates_tokens,
            total_tokens: u.total_tokens,
        });

        let choice = Choice {
            index: 0,
            message: ChatMessage {
                role: Role::Assistant,
                content: Some(MessageContent::Text(first)),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason,
            logprobs: None,
        };

        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs();

        Ok(ChatCompletionResponse {
            id: "".to_string(),
            object: "chat.completion".to_string(),
            created,
            model,
            choices: vec![choice],
            usage,
            system_fingerprint: parsed.model_version,
        })
    }

    async fn chat_stream(&self, request: ChatCompletionRequest) -> LLMResult<ChatStream> {
        let model = if request.model.is_empty() {
            self.config.default_model.clone()
        } else {
            request.model.clone()
        };

        let body = self.build_request_body(&request);

        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            self.config.base_url.trim_end_matches('/'),
            model,
            self.config.api_key
        );

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(Self::map_error)?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.map_err(Self::map_error)?;
            return Err(LLMError::ApiError {
                code: Some(status.as_u16().to_string()),
                message: text,
            });
        }

        Ok(parse_gemini_sse(resp, model))
    }

    async fn health_check(&self) -> LLMResult<bool> {
        let req = ChatCompletionRequest::new(self.default_model())
            .system("Say 'ok'")
            .max_tokens(4);
        self.chat(req).await.map(|_| true).or(Ok(false))
    }

    async fn get_model_info(&self, model: &str) -> LLMResult<ModelInfo> {
        Ok(ModelInfo {
            id: model.to_string(),
            name: model.to_string(),
            description: None,
            context_window: None,
            max_output_tokens: Some(self.config.default_max_tokens),
            training_cutoff: None,
            capabilities: ModelCapabilities {
                streaming: true,
                tools: false,
                vision: false,
                json_mode: true,
                json_schema: false,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: process raw SSE text and collect `ChatCompletionChunk` items.
    fn process_gemini_sse_text(raw: &str, model: &str) -> Vec<ChatCompletionChunk> {
        let mut chunks = Vec::new();
        let mut is_first = true;

        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(json_str) = line.strip_prefix("data: ") {
                if json_str.trim() == "[DONE]" {
                    break;
                }
                if let Ok(chunk) = serde_json::from_str::<GeminiStreamChunk>(json_str) {
                    chunks.push(gemini_chunk_to_completion(&chunk, model, is_first));
                    is_first = false;
                }
            }
        }
        chunks
    }

    #[test]
    fn test_convert_messages_with_media() {
        let messages = vec![ChatMessage::user_with_parts(vec![
            ContentPart::Image {
                image_url: ImageUrl {
                    url: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==".to_string(),
                    detail: None,
                },
            },
            ContentPart::Audio {
                audio: AudioData {
                    data: "data:audio/wav;base64,UklGRiQAAABXQVZFZm10IBAAAAABAAEAQB8AAEAfAAABAAgAZGF0YQAAAAA=".to_string(),
                    format: "wav".to_string(),
                },
            },
            ContentPart::Video {
                video: VideoData {
                    data: "data:video/mp4;base64,AAAAIGZ0eXBtcDQyAAAAAG1wNDJpc29tYXZjMQAAADhmoW9v...".to_string(),
                    format: "mp4".to_string(),
                },
            },
        ])];

        let (_, contents) = GeminiProvider::convert_messages(&messages);
        assert_eq!(contents.len(), 1);
        let parts = contents[0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 3);

        assert_eq!(parts[0]["inlineData"]["mimeType"], "image/png");
        assert_eq!(
            parts[0]["inlineData"]["data"],
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
        );

        assert_eq!(parts[1]["inlineData"]["mimeType"], "audio/wav");
        assert_eq!(
            parts[1]["inlineData"]["data"],
            "UklGRiQAAABXQVZFZm10IBAAAAABAAEAQB8AAEAfAAABAAgAZGF0YQAAAAA="
        );

        assert_eq!(parts[2]["inlineData"]["mimeType"], "video/mp4");
        assert_eq!(
            parts[2]["inlineData"]["data"],
            "AAAAIGZ0eXBtcDQyAAAAAG1wNDJpc29tYXZjMQAAADhmoW9v..."
        );
    }

    #[test]
    fn test_config_defaults() {
        let config = GeminiConfig::default();
        assert_eq!(config.base_url, "https://generativelanguage.googleapis.com");
        assert_eq!(config.default_model, "gemini-1.5-pro-latest");
        assert_eq!(config.default_max_tokens, 2048);
        assert!((config.default_temperature - 0.7).abs() < f32::EPSILON);
        assert_eq!(config.timeout_secs, 60);
    }

    #[test]
    fn test_config_builder() {
        let config = GeminiConfig::new("test-key")
            .with_model("gemini-2.0-flash")
            .with_temperature(0.5)
            .with_max_tokens(1024)
            .with_base_url("https://custom.api.com")
            .with_timeout(120);

        assert_eq!(config.api_key, "test-key");
        assert_eq!(config.default_model, "gemini-2.0-flash");
        assert!((config.default_temperature - 0.5).abs() < f32::EPSILON);
        assert_eq!(config.default_max_tokens, 1024);
        assert_eq!(config.base_url, "https://custom.api.com");
        assert_eq!(config.timeout_secs, 120);
    }

    #[test]
    fn test_supports_streaming() {
        let provider = GeminiProvider::new("test-key");
        assert_eq!(provider.name(), "gemini");
        assert!(provider.supports_streaming());
        assert!(!provider.supports_tools());
        assert!(!provider.supports_vision());
    }

    #[test]
    fn test_parse_gemini_sse_text_deltas() {
        let sse_data = r#"data: {"candidates":[{"content":{"parts":[{"text":"Hello"}],"role":"model"},"finishReason":null}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":1,"totalTokenCount":11}}

data: {"candidates":[{"content":{"parts":[{"text":" world"}],"role":"model"},"finishReason":null}]}

data: {"candidates":[{"content":{"parts":[{"text":"!"}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":3,"totalTokenCount":13}}

data: [DONE]"#;

        let chunks = process_gemini_sse_text(sse_data, "gemini-1.5-pro-latest");
        assert_eq!(chunks.len(), 3);

        // First chunk: has role, content "Hello", and usage
        assert_eq!(chunks[0].choices[0].delta.role, Some(Role::Assistant));
        assert_eq!(chunks[0].choices[0].delta.content.as_deref(), Some("Hello"));
        assert!(chunks[0].usage.is_some());
        let usage = chunks[0].usage.as_ref().unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 1);
        assert_eq!(usage.total_tokens, 11);

        // Second chunk: no role, content " world", no usage
        assert_eq!(chunks[1].choices[0].delta.role, None);
        assert_eq!(
            chunks[1].choices[0].delta.content.as_deref(),
            Some(" world")
        );
        assert!(chunks[1].usage.is_none());

        // Third chunk: content "!", finish_reason STOP, usage
        assert_eq!(chunks[2].choices[0].delta.content.as_deref(), Some("!"));
        assert_eq!(chunks[2].choices[0].finish_reason, Some(FinishReason::Stop));
        assert!(chunks[2].usage.is_some());
    }

    #[test]
    fn test_parse_gemini_sse_finish_reasons() {
        assert_eq!(map_gemini_finish_reason("STOP"), Some(FinishReason::Stop));
        assert_eq!(
            map_gemini_finish_reason("MAX_TOKENS"),
            Some(FinishReason::Length)
        );
        assert_eq!(
            map_gemini_finish_reason("SAFETY"),
            Some(FinishReason::ContentFilter)
        );
        assert_eq!(
            map_gemini_finish_reason("RECITATION"),
            Some(FinishReason::ContentFilter)
        );
        assert_eq!(map_gemini_finish_reason("UNKNOWN"), None);
    }

    #[test]
    fn test_parse_gemini_sse_usage_metadata() {
        let sse_data = r#"data: {"candidates":[{"content":{"parts":[{"text":"Hi"}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":2,"totalTokenCount":7}}"#;

        let chunks = process_gemini_sse_text(sse_data, "gemini-2.0-flash");
        assert_eq!(chunks.len(), 1);

        let usage = chunks[0].usage.as_ref().expect("usage should be present");
        assert_eq!(usage.prompt_tokens, 5);
        assert_eq!(usage.completion_tokens, 2);
        assert_eq!(usage.total_tokens, 7);
        assert_eq!(chunks[0].model, "gemini-2.0-flash");
    }
}
