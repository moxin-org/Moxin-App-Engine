use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
#[non_exhaustive]
pub enum Role {
    System,
    #[default]
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ContentPart {
    Text { text: String },
    Image { image_url: ImageUrl },
    Audio { audio: AudioData },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<ImageDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ImageDetail {
    Low,
    High,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioData {
    pub data: String,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<MessageContent>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}
impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: Some(MessageContent::Text(content.into())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(MessageContent::Text(content.into())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user_with_content(content: MessageContent) -> Self {
        Self {
            role: Role::User,
            content: Some(content),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user_with_parts(parts: Vec<ContentPart>) -> Self {
        Self::user_with_content(MessageContent::Parts(parts))
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: Some(MessageContent::Text(content.into())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant_with_tool_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: None,
            name: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(MessageContent::Text(content.into())),
            name: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    pub fn user_with_image(text: impl Into<String>, image_url: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(MessageContent::Parts(vec![
                ContentPart::Text { text: text.into() },
                ContentPart::Image {
                    image_url: ImageUrl {
                        url: image_url.into(),
                        detail: None,
                    },
                },
            ])),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn text_content(&self) -> Option<&str> {
        match &self.content {
            Some(MessageContent::Text(s)) => Some(s),
            Some(MessageContent::Parts(parts)) => {
                for part in parts {
                    if let ContentPart::Text { text } = part {
                        return Some(text);
                    }
                }
                None
            }
            None => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

impl Tool {
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: name.into(),
                description: Some(description.into()),
                parameters: Some(parameters),
                strict: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Specific {
        #[serde(rename = "type")]
        choice_type: String,
        function: ToolChoiceFunction,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolChoiceFunction {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatCompletionRequest {
    pub model: String,

    #[serde(default)]
    pub messages: Vec<ChatMessage>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
}

impl ChatCompletionRequest {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            ..Default::default()
        }
    }
    pub fn message(mut self, message: ChatMessage) -> Self {
        self.messages.push(message);
        self
    }

    pub fn system(mut self, content: impl Into<String>) -> Self {
        self.messages.push(ChatMessage::system(content));
        self
    }

    pub fn user(mut self, content: impl Into<String>) -> Self {
        self.messages.push(ChatMessage::user(content));
        self
    }

    pub fn temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    pub fn max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = Some(tokens);
        self
    }

    pub fn tool(mut self, tool: Tool) -> Self {
        self.tools.get_or_insert_with(Vec::new).push(tool);
        self
    }

    pub fn tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn stream(mut self) -> Self {
        self.stream = Some(true);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<serde_json::Value>,
}

impl ResponseFormat {
    pub fn text() -> Self {
        Self {
            format_type: "text".to_string(),
            json_schema: None,
        }
    }

    pub fn json() -> Self {
        Self {
            format_type: "json_object".to_string(),
            json_schema: None,
        }
    }

    pub fn json_schema(schema: serde_json::Value) -> Self {
        Self {
            format_type: "json_schema".to_string(),
            json_schema: Some(schema),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub choices: Vec<Choice>,
}

impl ChatCompletionResponse {
    pub fn content(&self) -> Option<&str> {
        self.choices.first()?.message.text_content()
    }

    pub fn tool_calls(&self) -> Option<&Vec<ToolCall>> {
        self.choices.first()?.message.tool_calls.as_ref()
    }

    pub fn has_tool_calls(&self) -> bool {
        self.tool_calls().map(|t| !t.is_empty()).unwrap_or(false)
    }

    pub fn finish_reason(&self) -> Option<&FinishReason> {
        self.choices.first()?.finish_reason.as_ref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: ChatMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    pub choices: Vec<ChunkChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: ChunkDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallDelta {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<FunctionCallDelta>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: EmbeddingInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    pub data: Vec<EmbeddingData>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<EmbeddingUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum EmbeddingInput {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingData {
    pub object: String,
    pub index: u32,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingUsage {
    pub prompt_tokens: u32,
    pub total_tokens: u32,
}

// ============================================================================
// Protocol Version Negotiation
// ============================================================================

/// The protocol version carried by a [`MessageEnvelope`].
///
/// This discriminant allows receivers to detect messages produced by a newer
/// protocol revision and handle the mismatch explicitly, rather than failing
/// silently with a generic deserialization error.
///
/// # Versioning Policy
///
/// - **`V1`** is the current stable revision of the MoFA inference protocol.
/// - New variants (e.g. `V2`) will be added in future releases.
/// - Receivers **MUST** call [`MessageEnvelope::check_version`] and handle
///   [`crate::agent::AgentError::ProtocolVersionMismatch`] rather than
///   silently processing messages whose version they do not recognise.
///
/// # Wire Format
///
/// `ProtocolVersion` serialises as a compact numeric string so the JSON
/// remains human-readable:
///
/// ```json
/// { "version": "1", "payload": { … } }
/// ```
///
/// Receivers compiled against an older build that do not recognise a future
/// version tag (`"2"`, `"3"`, …) will deserialise it as [`Unknown`] instead
/// of returning a hard deserialization error.  The caller must then decide
/// how to handle the unknown version — typically via
/// [`MessageEnvelope::check_version`].
///
/// [`Unknown`]: ProtocolVersion::Unknown
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProtocolVersion {
    /// Version 1 — the first stable revision of the MoFA inference protocol.
    #[serde(rename = "1")]
    V1,

    /// Catch-all for any version tag not recognised by this build.
    ///
    /// This variant exists solely as a serde deserialization fallback.  Do
    /// **not** construct it directly; call [`MessageEnvelope::check_version`]
    /// to surface a [`crate::agent::AgentError::ProtocolVersionMismatch`]
    /// error when this variant is encountered.
    #[doc(hidden)]
    #[serde(other)]
    Unknown,
}

impl Default for ProtocolVersion {
    /// Returns [`ProtocolVersion::V1`], the current stable version.
    fn default() -> Self {
        Self::V1
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V1 => write!(f, "1"),
            Self::Unknown => write!(f, "<unknown>"),
        }
    }
}

/// A protocol-version-aware wrapper for any inference message payload.
///
/// `MessageEnvelope<T>` pairs a serialisable payload `T` with a
/// [`ProtocolVersion`] discriminant, allowing receivers to guard against
/// protocol drift *before* deserialising the payload.  It also carries an
/// optional `trace_id` for distributed tracing integration.
///
/// # Backward Compatibility
///
/// `version` uses `#[serde(default)]`, so messages produced before this
/// envelope was introduced — which therefore lack a `"version"` key —
/// deserialise cleanly as [`ProtocolVersion::V1`].  No migration step is
/// required for pre-existing callers.
///
/// # Example
///
/// ```rust
/// use mofa_kernel::llm::{MessageEnvelope, ProtocolVersion};
///
/// // Wrap a payload.
/// let env = MessageEnvelope::new("hello".to_string())
///     .with_trace_id("req-abc-123");
///
/// assert_eq!(env.version, ProtocolVersion::V1);
/// assert!(env.check_version().is_ok());
///
/// // Round-trip through JSON.
/// let json = serde_json::to_string(&env).unwrap();
/// let back: MessageEnvelope<String> = serde_json::from_str(&json).unwrap();
/// assert_eq!(env.payload, back.payload);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope<T> {
    /// The protocol revision this message was produced under.
    ///
    /// Defaults to [`ProtocolVersion::V1`] when the field is absent in the
    /// serialised form (backward compatibility with pre-envelope messages).
    #[serde(default)]
    pub version: ProtocolVersion,

    /// Application-level payload.
    pub payload: T,

    /// Optional distributed trace identifier.
    ///
    /// When present, propagate this value across service boundaries so that
    /// observability tooling can correlate the full inference request chain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
}

impl<T> MessageEnvelope<T> {
    /// Wrap `payload` in a [`ProtocolVersion::V1`] envelope with no trace ID.
    ///
    /// # Example
    ///
    /// ```rust
    /// use mofa_kernel::llm::MessageEnvelope;
    ///
    /// let env = MessageEnvelope::new(42u32);
    /// assert_eq!(env.payload, 42u32);
    /// assert!(env.trace_id.is_none());
    /// ```
    pub fn new(payload: T) -> Self {
        Self {
            version: ProtocolVersion::V1,
            payload,
            trace_id: None,
        }
    }

    /// Attach a distributed trace identifier to this envelope.
    ///
    /// Accepts anything that converts into a [`String`] (`&str`, `String`,
    /// `Arc<str>`, …) for ergonomic call sites.
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    /// Verify that this envelope's version is supported by the current build.
    ///
    /// Returns `Ok(())` for [`ProtocolVersion::V1`].
    /// Returns [`Err`] wrapping
    /// [`crate::agent::AgentError::ProtocolVersionMismatch`] for any
    /// unrecognised version tag, including [`ProtocolVersion::Unknown`].
    ///
    /// Callers **must** invoke this before processing the payload whenever
    /// the envelope arrived from an untrusted or potentially newer sender.
    ///
    /// # Errors
    ///
    /// - [`crate::agent::AgentError::ProtocolVersionMismatch`] — the sender
    ///   used a protocol version not supported by this receiver.
    ///
    /// # Example
    ///
    /// ```rust
    /// use mofa_kernel::llm::MessageEnvelope;
    ///
    /// let env = MessageEnvelope::new("ping".to_string());
    /// assert!(env.check_version().is_ok());
    /// ```
    pub fn check_version(&self) -> crate::agent::AgentResult<()> {
        match self.version {
            ProtocolVersion::V1 => Ok(()),
            _ => Err(crate::agent::AgentError::ProtocolVersionMismatch {
                received: self.version.to_string(),
                supported: ProtocolVersion::V1.to_string(),
            }),
        }
    }

    /// Transform the payload, preserving `version` and `trace_id`.
    ///
    /// Useful for converting between typed payload representations without
    /// discarding envelope metadata.
    ///
    /// # Example
    ///
    /// ```rust
    /// use mofa_kernel::llm::MessageEnvelope;
    ///
    /// let env = MessageEnvelope::new(10u32).with_trace_id("t");
    /// let mapped: MessageEnvelope<String> = env.map(|n| n.to_string());
    /// assert_eq!(mapped.payload, "10");
    /// assert_eq!(mapped.trace_id.as_deref(), Some("t"));
    /// ```
    pub fn map<U, F>(self, f: F) -> MessageEnvelope<U>
    where
        F: FnOnce(T) -> U,
    {
        MessageEnvelope {
            version: self.version,
            payload: f(self.payload),
            trace_id: self.trace_id,
        }
    }
}

#[cfg(test)]
mod protocol_version_tests {
    use super::*;
    use crate::agent::AgentError;

    // -----------------------------------------------------------------------
    // ProtocolVersion
    // -----------------------------------------------------------------------

    #[test]
    fn protocol_version_default_is_v1() {
        assert_eq!(ProtocolVersion::default(), ProtocolVersion::V1);
    }

    #[test]
    fn protocol_version_display_v1() {
        assert_eq!(ProtocolVersion::V1.to_string(), "1");
    }

    #[test]
    fn protocol_version_display_unknown() {
        assert_eq!(ProtocolVersion::Unknown.to_string(), "<unknown>");
    }

    #[test]
    fn protocol_version_v1_serialises_as_string_one() {
        let json = serde_json::to_string(&ProtocolVersion::V1).unwrap();
        assert_eq!(json, r#""1""#);
    }

    #[test]
    fn protocol_version_v1_deserialises_from_string_one() {
        let v: ProtocolVersion = serde_json::from_str(r#""1""#).unwrap();
        assert_eq!(v, ProtocolVersion::V1);
    }

    /// Any version tag not present in the enum deserialises to `Unknown`
    /// rather than causing a hard deserialization error.
    #[test]
    fn protocol_version_unknown_tag_deserialises_to_unknown_variant() {
        let v: ProtocolVersion = serde_json::from_str(r#""999""#).unwrap();
        assert_eq!(v, ProtocolVersion::Unknown);
    }

    #[test]
    fn protocol_version_future_tag_is_not_v1() {
        let v: ProtocolVersion = serde_json::from_str(r#""2""#).unwrap();
        assert_ne!(v, ProtocolVersion::V1);
    }

    // -----------------------------------------------------------------------
    // MessageEnvelope construction
    // -----------------------------------------------------------------------

    #[test]
    fn message_envelope_new_sets_v1_and_no_trace_id() {
        let env = MessageEnvelope::new(42u32);
        assert_eq!(env.version, ProtocolVersion::V1);
        assert_eq!(env.payload, 42u32);
        assert!(env.trace_id.is_none());
    }

    #[test]
    fn message_envelope_with_trace_id_sets_field() {
        let env = MessageEnvelope::new("hello".to_string())
            .with_trace_id("trace-abc-123");
        assert_eq!(env.trace_id.as_deref(), Some("trace-abc-123"));
    }

    // -----------------------------------------------------------------------
    // Serialisation round-trips
    // -----------------------------------------------------------------------

    #[test]
    fn message_envelope_round_trip_with_string_payload() {
        let original = MessageEnvelope::new("round-trip".to_string())
            .with_trace_id("tid-001");

        let json = serde_json::to_string(&original).unwrap();
        let restored: MessageEnvelope<String> = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.version, ProtocolVersion::V1);
        assert_eq!(restored.payload, "round-trip");
        assert_eq!(restored.trace_id.as_deref(), Some("tid-001"));
    }

    #[test]
    fn message_envelope_round_trip_with_numeric_payload() {
        let original = MessageEnvelope::new(9001u64);
        let json = serde_json::to_string(&original).unwrap();
        let restored: MessageEnvelope<u64> = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.payload, 9001u64);
    }

    /// A message serialised without a `"version"` field (e.g. produced before
    /// the envelope type was introduced) must deserialise successfully and
    /// default to V1.
    #[test]
    fn message_envelope_missing_version_field_defaults_to_v1() {
        let legacy_json = r#"{"payload":"legacy"}"#;
        let env: MessageEnvelope<String> = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(env.version, ProtocolVersion::V1);
        assert_eq!(env.payload, "legacy");
    }

    #[test]
    fn message_envelope_trace_id_omitted_from_json_when_none() {
        let env = MessageEnvelope::new(1u32);
        let json = serde_json::to_string(&env).unwrap();
        // `trace_id` must NOT appear in the JSON when it is None.
        assert!(!json.contains("trace_id"), "unexpected trace_id in: {json}");
    }

    #[test]
    fn message_envelope_trace_id_present_in_json_when_some() {
        let env = MessageEnvelope::new(1u32).with_trace_id("t-xyz");
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("trace_id"), "trace_id missing from: {json}");
        assert!(json.contains("t-xyz"));
    }

    // -----------------------------------------------------------------------
    // check_version — happy path
    // -----------------------------------------------------------------------

    #[test]
    fn check_version_returns_ok_for_v1() {
        let env = MessageEnvelope::new("payload".to_string());
        assert!(env.check_version().is_ok());
    }

    // -----------------------------------------------------------------------
    // check_version — version mismatch path
    // -----------------------------------------------------------------------

    /// Receiving a message with an unknown version tag MUST surface a typed
    /// `ProtocolVersionMismatch` error — not a generic deserialization panic
    /// or an unrelated error variant.
    #[test]
    fn check_version_returns_protocol_version_mismatch_for_unknown_tag() {
        // Step 1: an envelope with a future/unknown version deserialises OK.
        let future_json = r#"{"version":"99","payload":"from the future"}"#;
        let env: MessageEnvelope<String> = serde_json::from_str(future_json).unwrap();
        assert_eq!(env.version, ProtocolVersion::Unknown);

        // Step 2: check_version() is the guard that surfaces the error.
        let result = env.check_version();
        assert!(result.is_err(), "expected Err, got Ok");

        match result.unwrap_err() {
            AgentError::ProtocolVersionMismatch { received, supported } => {
                // The supported side must clearly state what this build accepts.
                assert_eq!(supported, "1");
                // The received side must be non-empty (the display of Unknown).
                assert!(!received.is_empty());
            }
            other => panic!("expected ProtocolVersionMismatch, got: {other:?}"),
        }
    }

    #[test]
    fn check_version_error_message_contains_supported_version() {
        let future_json = r#"{"version":"42","payload":0}"#;
        let env: MessageEnvelope<u32> = serde_json::from_str(future_json).unwrap();
        let err = env.check_version().unwrap_err();
        // The human-readable error must mention "1" so operators know what to
        // upgrade to.
        assert!(
            err.to_string().contains('1'),
            "error message should mention supported version \"1\": {err}"
        );
    }

    // -----------------------------------------------------------------------
    // map() combinator
    // -----------------------------------------------------------------------

    #[test]
    fn map_transforms_payload_and_preserves_envelope_metadata() {
        let env = MessageEnvelope::new(10u32).with_trace_id("t-map");
        let mapped: MessageEnvelope<String> = env.map(|n| n.to_string());

        assert_eq!(mapped.payload, "10");
        assert_eq!(mapped.version, ProtocolVersion::V1);
        assert_eq!(mapped.trace_id.as_deref(), Some("t-map"));
    }

    #[test]
    fn map_on_envelope_with_unknown_version_preserves_unknown() {
        let future_json = r#"{"version":"5","payload":0}"#;
        let env: MessageEnvelope<u32> = serde_json::from_str(future_json).unwrap();
        let mapped: MessageEnvelope<String> = env.map(|n| n.to_string());
        assert_eq!(mapped.version, ProtocolVersion::Unknown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- Role ---

    #[test]
    fn role_default_is_user() {
        assert_eq!(Role::default(), Role::User);
    }

    #[test]
    fn role_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Role::System).unwrap(), "\"system\"");
        assert_eq!(serde_json::to_string(&Role::User).unwrap(), "\"user\"");
        assert_eq!(
            serde_json::to_string(&Role::Assistant).unwrap(),
            "\"assistant\""
        );
        assert_eq!(serde_json::to_string(&Role::Tool).unwrap(), "\"tool\"");
    }

    #[test]
    fn role_roundtrip() {
        for role in [Role::System, Role::User, Role::Assistant, Role::Tool] {
            let json = serde_json::to_string(&role).unwrap();
            let back: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(role, back);
        }
    }

    // --- ChatMessage constructors ---

    #[test]
    fn system_message_sets_role_and_content() {
        let msg = ChatMessage::system("You are helpful.");
        assert_eq!(msg.role, Role::System);
        assert_eq!(msg.text_content(), Some("You are helpful."));
        assert!(msg.tool_calls.is_none());
        assert!(msg.tool_call_id.is_none());
    }

    #[test]
    fn user_message_sets_role_and_content() {
        let msg = ChatMessage::user("Hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.text_content(), Some("Hello"));
    }

    #[test]
    fn assistant_message_sets_role_and_content() {
        let msg = ChatMessage::assistant("Hi there");
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.text_content(), Some("Hi there"));
    }

    #[test]
    fn tool_result_sets_role_and_id() {
        let msg = ChatMessage::tool_result("call_123", "result data");
        assert_eq!(msg.role, Role::Tool);
        assert_eq!(msg.text_content(), Some("result data"));
        assert_eq!(msg.tool_call_id.as_deref(), Some("call_123"));
    }

    #[test]
    fn assistant_with_tool_calls_has_no_content() {
        let tc = ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "get_weather".into(),
                arguments: r#"{"city":"NYC"}"#.into(),
            },
        };
        let msg = ChatMessage::assistant_with_tool_calls(vec![tc]);
        assert_eq!(msg.role, Role::Assistant);
        assert!(msg.content.is_none());
        assert_eq!(msg.tool_calls.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn user_with_image_creates_multipart() {
        let msg = ChatMessage::user_with_image("describe this", "https://img.example.com/a.png");
        assert_eq!(msg.role, Role::User);
        if let Some(MessageContent::Parts(parts)) = &msg.content {
            assert_eq!(parts.len(), 2);
            assert!(matches!(&parts[0], ContentPart::Text { text } if text == "describe this"));
            assert!(
                matches!(&parts[1], ContentPart::Image { image_url } if image_url.url == "https://img.example.com/a.png")
            );
        } else {
            panic!("expected Parts content");
        }
    }

    #[test]
    fn user_with_parts_delegates_correctly() {
        let parts = vec![ContentPart::Text {
            text: "hello".into(),
        }];
        let msg = ChatMessage::user_with_parts(parts);
        assert_eq!(msg.role, Role::User);
        assert!(matches!(msg.content, Some(MessageContent::Parts(_))));
    }

    // --- text_content extraction ---

    #[test]
    fn text_content_from_text_variant() {
        let msg = ChatMessage::user("simple text");
        assert_eq!(msg.text_content(), Some("simple text"));
    }

    #[test]
    fn text_content_from_parts_returns_first_text() {
        let msg = ChatMessage::user_with_image("first text", "https://example.com/img.png");
        assert_eq!(msg.text_content(), Some("first text"));
    }

    #[test]
    fn text_content_returns_none_when_no_content() {
        let msg = ChatMessage::assistant_with_tool_calls(vec![]);
        assert!(msg.text_content().is_none());
    }

    #[test]
    fn text_content_returns_none_for_image_only_parts() {
        let msg = ChatMessage {
            role: Role::User,
            content: Some(MessageContent::Parts(vec![ContentPart::Image {
                image_url: ImageUrl {
                    url: "https://example.com/img.png".into(),
                    detail: None,
                },
            }])),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        };
        assert!(msg.text_content().is_none());
    }

    // --- ChatMessage serialization ---

    #[test]
    fn chat_message_roundtrip_json() {
        let msg = ChatMessage::user("roundtrip test");
        let json = serde_json::to_string(&msg).unwrap();
        let back: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, Role::User);
        assert_eq!(back.text_content(), Some("roundtrip test"));
    }

    #[test]
    fn chat_message_skips_none_fields() {
        let msg = ChatMessage::user("hi");
        let val: serde_json::Value = serde_json::to_value(&msg).unwrap();
        assert!(val.get("tool_calls").is_none());
        assert!(val.get("tool_call_id").is_none());
        assert!(val.get("name").is_none());
    }

    // --- Tool / FunctionDefinition ---

    #[test]
    fn tool_function_constructor() {
        let t = Tool::function(
            "search",
            "Search the web",
            json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        );
        assert_eq!(t.tool_type, "function");
        assert_eq!(t.function.name, "search");
        assert_eq!(t.function.description.as_deref(), Some("Search the web"));
        assert!(t.function.parameters.is_some());
        assert!(t.function.strict.is_none());
    }

    // --- ChatCompletionRequest builder ---

    #[test]
    fn request_builder_chain() {
        let req = ChatCompletionRequest::new("gpt-4")
            .system("Be helpful")
            .user("Hello")
            .temperature(0.7)
            .max_tokens(100)
            .stream();

        assert_eq!(req.model, "gpt-4");
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.temperature, Some(0.7));
        assert_eq!(req.max_tokens, Some(100));
        assert_eq!(req.stream, Some(true));
    }

    #[test]
    fn request_builder_tool_appends() {
        let t = Tool::function("a", "desc", json!({}));
        let req = ChatCompletionRequest::new("model").tool(t);
        assert_eq!(req.tools.as_ref().unwrap().len(), 1);

        let t2 = Tool::function("b", "desc2", json!({}));
        let req = req.tool(t2);
        assert_eq!(req.tools.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn request_builder_tools_replaces() {
        let t1 = Tool::function("a", "d", json!({}));
        let t2 = Tool::function("b", "d", json!({}));
        let req = ChatCompletionRequest::new("m").tool(t1).tools(vec![t2]);
        assert_eq!(req.tools.as_ref().unwrap().len(), 1);
        assert_eq!(req.tools.as_ref().unwrap()[0].function.name, "b");
    }

    // --- ResponseFormat ---

    #[test]
    fn response_format_text() {
        let f = ResponseFormat::text();
        assert_eq!(f.format_type, "text");
        assert!(f.json_schema.is_none());
    }

    #[test]
    fn response_format_json() {
        let f = ResponseFormat::json();
        assert_eq!(f.format_type, "json_object");
    }

    #[test]
    fn response_format_json_schema() {
        let schema = json!({"type": "object"});
        let f = ResponseFormat::json_schema(schema.clone());
        assert_eq!(f.format_type, "json_schema");
        assert_eq!(f.json_schema.unwrap(), schema);
    }

    // --- ChatCompletionResponse ---

    #[test]
    fn response_content_extracts_first_choice() {
        let resp = ChatCompletionResponse {
            choices: vec![Choice {
                index: 0,
                message: ChatMessage::assistant("answer"),
                finish_reason: Some(FinishReason::Stop),
                logprobs: None,
            }],
        };
        assert_eq!(resp.content(), Some("answer"));
        assert_eq!(resp.finish_reason(), Some(&FinishReason::Stop));
        assert!(!resp.has_tool_calls());
    }

    #[test]
    fn response_content_none_when_empty_choices() {
        let resp = ChatCompletionResponse { choices: vec![] };
        assert!(resp.content().is_none());
        assert!(resp.finish_reason().is_none());
        assert!(!resp.has_tool_calls());
    }

    #[test]
    fn response_has_tool_calls_when_present() {
        let tc = ToolCall {
            id: "c1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "f".into(),
                arguments: "{}".into(),
            },
        };
        let resp = ChatCompletionResponse {
            choices: vec![Choice {
                index: 0,
                message: ChatMessage::assistant_with_tool_calls(vec![tc]),
                finish_reason: Some(FinishReason::ToolCalls),
                logprobs: None,
            }],
        };
        assert!(resp.has_tool_calls());
        assert_eq!(resp.tool_calls().unwrap().len(), 1);
    }

    // --- FinishReason serialization ---

    #[test]
    fn finish_reason_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&FinishReason::Stop).unwrap(),
            "\"stop\""
        );
        assert_eq!(
            serde_json::to_string(&FinishReason::ToolCalls).unwrap(),
            "\"tool_calls\""
        );
        assert_eq!(
            serde_json::to_string(&FinishReason::ContentFilter).unwrap(),
            "\"content_filter\""
        );
    }

    // --- EmbeddingInput ---

    #[test]
    fn embedding_input_single_roundtrip() {
        let input = EmbeddingInput::Single("hello".into());
        let json = serde_json::to_string(&input).unwrap();
        let back: EmbeddingInput = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, EmbeddingInput::Single(s) if s == "hello"));
    }

    #[test]
    fn embedding_input_multiple_roundtrip() {
        let input = EmbeddingInput::Multiple(vec!["a".into(), "b".into()]);
        let json = serde_json::to_string(&input).unwrap();
        let back: EmbeddingInput = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, EmbeddingInput::Multiple(v) if v.len() == 2));
    }

    // --- ImageDetail ---

    #[test]
    fn image_detail_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&ImageDetail::Low).unwrap(), "\"low\"");
        assert_eq!(
            serde_json::to_string(&ImageDetail::High).unwrap(),
            "\"high\""
        );
        assert_eq!(
            serde_json::to_string(&ImageDetail::Auto).unwrap(),
            "\"auto\""
        );
    }

    // --- ChunkDelta defaults ---

    #[test]
    fn chunk_delta_default_all_none() {
        let d = ChunkDelta::default();
        assert!(d.role.is_none());
        assert!(d.content.is_none());
        assert!(d.tool_calls.is_none());
    }
}
