//! Data types for the Cognitive Observatory protocol.

use serde::{Deserialize, Serialize};

/// A recorded span sent to the Observatory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanRecord {
    /// Unique span ID (hex string).
    pub span_id: String,
    /// Parent span ID, if any.
    pub parent_span_id: Option<String>,
    /// Trace ID (hex string).
    pub trace_id: String,
    /// The tracing target (module path), used as agent name.
    pub target: String,
    /// The span name.
    pub name: String,
    /// Unix timestamp (microseconds) when the span started.
    pub start_time_us: u64,
    /// Unix timestamp (microseconds) when the span ended. Zero if still open.
    pub end_time_us: u64,
    /// Latency in microseconds (end_time_us - start_time_us). Zero if still open.
    pub latency_us: u64,
    /// Structured fields recorded on the span or its events.
    pub fields: Vec<SpanField>,
    /// Token count extracted from `tokens` field, if present.
    pub tokens: Option<u64>,
    /// Model name extracted from `model` field, if present.
    pub model: Option<String>,
}

/// A key-value field recorded within a span.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanField {
    pub key: String,
    pub value: String,
}

/// Errors from the Cognitive Observatory integration.
#[derive(Debug, thiserror::Error)]
pub enum ObservatoryError {
    #[error("Failed to initialize tracing subscriber: {0}")]
    SubscriberInit(String),

    #[error("HTTP transport error: {0}")]
    Transport(String),

    #[error("Serialization error: {0}")]
    Serialization(String),
}
