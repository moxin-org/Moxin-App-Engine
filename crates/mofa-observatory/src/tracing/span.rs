use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Status of a span in the trace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, sqlx::Type)]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum SpanStatus {
    Unset,
    Ok,
    Error,
}

impl Default for SpanStatus {
    fn default() -> Self {
        Self::Unset
    }
}

/// An OpenTelemetry-compatible span representing a unit of work in a distributed trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    /// Unique identifier for this span.
    pub span_id: String,
    /// Identifier shared across all spans in the same trace.
    pub trace_id: String,
    /// Parent span's ID, or `None` for root spans.
    pub parent_span_id: Option<String>,
    /// Human-readable operation name (e.g. "llm.call", "tool.execute").
    pub name: String,
    /// ID of the MoFA agent that produced this span.
    pub agent_id: String,
    /// Outcome of the operation.
    pub status: SpanStatus,
    /// Wall-clock time when the operation started.
    pub start_time: DateTime<Utc>,
    /// Wall-clock time when the operation ended (`None` if still in flight).
    pub end_time: Option<DateTime<Utc>>,
    /// Duration in milliseconds (computed on ingestion if absent).
    pub latency_ms: Option<i64>,
    /// Serialized input to the operation (e.g. user prompt).
    pub input: Option<String>,
    /// Serialized output from the operation (e.g. LLM response).
    pub output: Option<String>,
    /// Total tokens consumed across input + output.
    pub token_count: Option<i64>,
    /// Estimated cost in USD.
    pub cost_usd: Option<f64>,
    /// Arbitrary key-value attributes (tool calls, model name, etc.).
    pub attributes: HashMap<String, serde_json::Value>,
}

impl Span {
    /// Create a new root span (no parent) for an agent operation.
    pub fn new_root(name: impl Into<String>, agent_id: impl Into<String>) -> Self {
        Self {
            span_id: Uuid::new_v4().to_string(),
            trace_id: Uuid::new_v4().to_string(),
            parent_span_id: None,
            name: name.into(),
            agent_id: agent_id.into(),
            status: SpanStatus::default(),
            start_time: Utc::now(),
            end_time: None,
            latency_ms: None,
            input: None,
            output: None,
            token_count: None,
            cost_usd: None,
            attributes: HashMap::new(),
        }
    }
}
