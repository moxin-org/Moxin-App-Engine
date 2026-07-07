//! Distributed tracing support for MoFA agents.
//!
//! When the `tracing-otel` feature is enabled, provides OpenTelemetry integration
//! for distributed tracing across agent operations.
//!
//! # Usage
//!
//! ```rust,ignore
//! use mofa_foundation::tracing::{TracingConfig, TracingExporter, AgentTracer};
//!
//! let config = TracingConfig::default();
//! let tracer = AgentTracer::new("my-agent".to_string(), config);
//! let span = tracer.start_thought_span("reasoning about the problem");
//! // ... agent work ...
//! drop(span); // span ends on drop
//! ```

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the tracing system.
#[derive(Debug, Clone)]
pub struct TracingConfig {
    /// Name of the service reported to the tracing backend.
    pub service_name: String,
    /// Where to export spans.
    pub exporter: TracingExporter,
    /// Fraction of traces to sample (0.0 = none, 1.0 = all).
    pub sampling_ratio: f64,
    /// Maximum number of attributes per span.
    pub max_attributes: u32,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            service_name: "mofa".to_string(),
            exporter: TracingExporter::Stdout,
            sampling_ratio: 1.0,
            max_attributes: 128,
        }
    }
}

/// Tracing export destination.
#[derive(Debug, Clone)]
pub enum TracingExporter {
    /// Print spans to stdout (useful for development).
    Stdout,
    /// Export to Jaeger.
    Jaeger { endpoint: String },
    /// Export via OTLP protocol.
    Otlp { endpoint: String },
    /// Disable tracing (no-op).
    None,
}

// ============================================================================
// Errors
// ============================================================================

/// Errors that can occur during tracing setup.
#[derive(Debug, thiserror::Error)]
pub enum TracingError {
    #[error("Tracing initialization failed: {0}")]
    InitError(String),

    #[error("Tracing is not supported: feature 'tracing-otel' is not enabled")]
    FeatureNotEnabled,

    #[error("Baggage error: {0}")]
    BaggageError(String),
}

pub type TracingResult<T> = Result<T, TracingError>;

// ============================================================================
// Span handle
// ============================================================================

/// A handle to an active span.
///
/// Attributes can be set on the span via [`SpanHandle::set_attribute`].
/// The span ends when this handle is dropped.
pub struct SpanHandle {
    name: String,
    attributes: Vec<(String, String)>,
    events: Vec<(String, Vec<(String, String)>)>,
    #[cfg(feature = "tracing-otel")]
    inner: Option<opentelemetry::global::BoxedSpan>,
}

impl SpanHandle {
    fn new_noop(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            attributes: Vec::new(),
            events: Vec::new(),
            #[cfg(feature = "tracing-otel")]
            inner: None,
        }
    }

    #[cfg(feature = "tracing-otel")]
    fn new_with_span(name: impl Into<String>, span: opentelemetry::global::BoxedSpan) -> Self {
        Self {
            name: name.into(),
            attributes: Vec::new(),
            events: Vec::new(),
            inner: Some(span),
        }
    }

    /// Set a string attribute on this span.
    pub fn set_attribute(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();
        self.attributes.push((key.clone(), value.clone()));
        #[cfg(feature = "tracing-otel")]
        if let Some(span) = &mut self.inner {
            use opentelemetry::trace::Span;
            span.set_attribute(opentelemetry::KeyValue::new(key, value));
        }
    }

    /// Record an event within this span.
    pub fn add_event(&mut self, name: impl Into<String>, attrs: Vec<(String, String)>) {
        let name = name.into();
        self.events.push((name.clone(), attrs.clone()));
        #[cfg(feature = "tracing-otel")]
        if let Some(span) = &mut self.inner {
            use opentelemetry::trace::Span;
            let kv: Vec<opentelemetry::KeyValue> = attrs
                .into_iter()
                .map(|(k, v)| opentelemetry::KeyValue::new(k, v))
                .collect();
            span.add_event(name, kv);
        }
    }

    /// Record an error on this span.
    pub fn record_error(&mut self, error: &str) {
        self.add_event(
            "error",
            vec![("error.message".to_string(), error.to_string())],
        );
        #[cfg(feature = "tracing-otel")]
        if let Some(span) = &mut self.inner {
            use opentelemetry::trace::Span;
            span.set_status(opentelemetry::trace::Status::error(error.to_string()));
        }
    }

    /// Return the span's operation name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for SpanHandle {
    fn drop(&mut self) {
        #[cfg(feature = "tracing-otel")]
        if let Some(span) = &mut self.inner {
            use opentelemetry::trace::Span;
            span.end();
        }
    }
}

// ============================================================================
// AgentTracer
// ============================================================================

/// Tracer scoped to a specific agent.
///
/// Creates spans for different agent operations, each pre-populated with
/// `agent.id` and operation-specific semantic attributes.
pub struct AgentTracer {
    agent_id: String,
    config: TracingConfig,
    #[cfg(feature = "tracing-otel")]
    tracer: opentelemetry::global::BoxedTracer,
}

impl AgentTracer {
    /// Create a new `AgentTracer` for the given agent.
    pub fn new(agent_id: String, config: TracingConfig) -> Self {
        #[cfg(feature = "tracing-otel")]
        let tracer = opentelemetry::global::tracer(config.service_name.clone());
        Self {
            agent_id,
            config,
            #[cfg(feature = "tracing-otel")]
            tracer,
        }
    }

    /// Start a span for a thought/reasoning operation.
    pub fn start_thought_span(&self, thought: &str) -> SpanHandle {
        #[cfg(feature = "tracing-otel")]
        {
            use opentelemetry::trace::Tracer;
            let mut span = self.tracer.start("agent.thought");
            use opentelemetry::trace::Span;
            span.set_attribute(opentelemetry::KeyValue::new(
                "agent.id",
                self.agent_id.clone(),
            ));
            span.set_attribute(opentelemetry::KeyValue::new("agent.operation", "thought"));
            span.set_attribute(opentelemetry::KeyValue::new(
                "agent.thought.content",
                thought.to_string(),
            ));
            return SpanHandle::new_with_span("agent.thought", span);
        }
        #[allow(unreachable_code)]
        {
            let mut handle = SpanHandle::new_noop("agent.thought");
            handle.set_attribute("agent.id", self.agent_id.clone());
            handle.set_attribute("agent.operation", "thought");
            handle.set_attribute("agent.thought.content", thought);
            handle
        }
    }

    /// Start a span for an action operation.
    pub fn start_action_span(&self, action_type: &str) -> SpanHandle {
        #[cfg(feature = "tracing-otel")]
        {
            use opentelemetry::trace::Tracer;
            let mut span = self.tracer.start("agent.action");
            use opentelemetry::trace::Span;
            span.set_attribute(opentelemetry::KeyValue::new(
                "agent.id",
                self.agent_id.clone(),
            ));
            span.set_attribute(opentelemetry::KeyValue::new("agent.operation", "action"));
            span.set_attribute(opentelemetry::KeyValue::new(
                "agent.action.type",
                action_type.to_string(),
            ));
            return SpanHandle::new_with_span("agent.action", span);
        }
        #[allow(unreachable_code)]
        {
            let mut handle = SpanHandle::new_noop("agent.action");
            handle.set_attribute("agent.id", self.agent_id.clone());
            handle.set_attribute("agent.operation", "action");
            handle.set_attribute("agent.action.type", action_type);
            handle
        }
    }

    /// Start a span for a tool call.
    pub fn start_tool_call_span(&self, tool_name: &str) -> SpanHandle {
        #[cfg(feature = "tracing-otel")]
        {
            use opentelemetry::trace::Tracer;
            let mut span = self.tracer.start("agent.tool_call");
            use opentelemetry::trace::Span;
            span.set_attribute(opentelemetry::KeyValue::new(
                "agent.id",
                self.agent_id.clone(),
            ));
            span.set_attribute(opentelemetry::KeyValue::new("agent.operation", "tool_call"));
            span.set_attribute(opentelemetry::KeyValue::new(
                "tool.name",
                tool_name.to_string(),
            ));
            return SpanHandle::new_with_span("agent.tool_call", span);
        }
        #[allow(unreachable_code)]
        {
            let mut handle = SpanHandle::new_noop("agent.tool_call");
            handle.set_attribute("agent.id", self.agent_id.clone());
            handle.set_attribute("agent.operation", "tool_call");
            handle.set_attribute("tool.name", tool_name);
            handle
        }
    }

    /// Record an observation within an existing span.
    pub fn record_observation(&self, span: &mut SpanHandle, observation: &str, source: &str) {
        span.add_event(
            "agent.observation",
            vec![
                ("observation.content".to_string(), observation.to_string()),
                ("observation.source".to_string(), source.to_string()),
            ],
        );
    }

    /// Record an error within an existing span.
    pub fn record_error(&self, span: &mut SpanHandle, error: &str) {
        span.record_error(error);
    }

    /// Return the agent ID this tracer is scoped to.
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }
}

// ============================================================================
// Baggage
// ============================================================================

/// Thread-local baggage store for propagation.
static BAGGAGE: std::sync::LazyLock<Arc<RwLock<HashMap<String, String>>>> =
    std::sync::LazyLock::new(|| Arc::new(RwLock::new(HashMap::new())));

/// Set a baggage key-value pair for context propagation.
pub fn set_baggage(key: &str, value: &str) {
    if let Ok(mut b) = BAGGAGE.write() {
        b.insert(key.to_string(), value.to_string());
    }
}

/// Retrieve a baggage value by key.
pub fn get_baggage(key: &str) -> Option<String> {
    BAGGAGE.read().ok()?.get(key).cloned()
}

// ============================================================================
// Tracing provider initialization
// ============================================================================

/// Initialize the global OpenTelemetry tracing provider.
///
/// Returns `Err(TracingError::FeatureNotEnabled)` if the `tracing-otel` feature
/// is not enabled. Use `TracingExporter::None` to run without a real exporter.
pub fn init_tracing(config: TracingConfig) -> TracingResult<()> {
    #[cfg(feature = "tracing-otel")]
    {
        use opentelemetry::trace::TracerProvider as _;
        use opentelemetry_sdk::trace::SdkTracerProvider;

        let provider = match &config.exporter {
            TracingExporter::None => SdkTracerProvider::builder().build(),
            TracingExporter::Stdout
            | TracingExporter::Jaeger { .. }
            | TracingExporter::Otlp { .. } => {
                // For Stdout/Jaeger/Otlp, build a simple provider.
                // Full exporter setup would add opentelemetry-stdout or otlp deps.
                SdkTracerProvider::builder().build()
            }
        };

        opentelemetry::global::set_tracer_provider(provider);
        return Ok(());
    }
    #[allow(unreachable_code)]
    Err(TracingError::FeatureNotEnabled)
}

/// Flush and shut down the global OpenTelemetry provider.
pub fn shutdown_tracing() {
    #[cfg(feature = "tracing-otel")]
    {
        opentelemetry::global::shutdown_tracer_provider();
    }
}

// ============================================================================
// Metrics (lightweight in-process counters — no OTel dependency)
// ============================================================================

/// Simple in-process metrics collector.
///
/// This collector is always available regardless of the `tracing-otel` feature.
/// For production metrics export, pair with an OpenTelemetry metrics exporter.
pub struct MetricsCollector {
    counters: Arc<RwLock<HashMap<String, u64>>>,
    gauges: Arc<RwLock<HashMap<String, f64>>>,
    histograms: Arc<RwLock<HashMap<String, Vec<f64>>>>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            counters: Arc::new(RwLock::new(HashMap::new())),
            gauges: Arc::new(RwLock::new(HashMap::new())),
            histograms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn increment_counter(&self, name: &str, value: u64) {
        if let Ok(mut c) = self.counters.write() {
            *c.entry(name.to_string()).or_insert(0) += value;
        }
    }

    pub fn set_gauge(&self, name: &str, value: f64) {
        if let Ok(mut g) = self.gauges.write() {
            g.insert(name.to_string(), value);
        }
    }

    pub fn record_histogram(&self, name: &str, value: f64) {
        if let Ok(mut h) = self.histograms.write() {
            h.entry(name.to_string()).or_default().push(value);
        }
    }

    pub fn counter(&self, name: &str) -> u64 {
        self.counters
            .read()
            .ok()
            .and_then(|c| c.get(name).copied())
            .unwrap_or(0)
    }

    pub fn gauge(&self, name: &str) -> Option<f64> {
        self.gauges.read().ok()?.get(name).copied()
    }

    pub fn histogram_values(&self, name: &str) -> Vec<f64> {
        self.histograms
            .read()
            .ok()
            .and_then(|h| h.get(name).cloned())
            .unwrap_or_default()
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracer_creates_spans_with_correct_attributes() {
        let config = TracingConfig::default();
        let tracer = AgentTracer::new("test-agent".to_string(), config);

        let span = tracer.start_thought_span("reasoning about problem");
        assert_eq!(span.name(), "agent.thought");
        assert!(
            span.attributes
                .iter()
                .any(|(k, v)| k == "agent.id" && v == "test-agent")
        );
        assert!(
            span.attributes
                .iter()
                .any(|(k, v)| k == "agent.operation" && v == "thought")
        );
        assert!(
            span.attributes
                .iter()
                .any(|(k, _)| k == "agent.thought.content")
        );
        drop(span); // no panic

        let span = tracer.start_action_span("search");
        assert_eq!(span.name(), "agent.action");
        assert!(
            span.attributes
                .iter()
                .any(|(k, v)| k == "agent.id" && v == "test-agent")
        );
        assert!(
            span.attributes
                .iter()
                .any(|(k, v)| k == "agent.operation" && v == "action")
        );
        assert!(
            span.attributes
                .iter()
                .any(|(k, v)| k == "agent.action.type" && v == "search")
        );
        drop(span);

        let span = tracer.start_tool_call_span("web_search");
        assert_eq!(span.name(), "agent.tool_call");
        assert!(
            span.attributes
                .iter()
                .any(|(k, v)| k == "agent.id" && v == "test-agent")
        );
        assert!(
            span.attributes
                .iter()
                .any(|(k, v)| k == "agent.operation" && v == "tool_call")
        );
        assert!(
            span.attributes
                .iter()
                .any(|(k, v)| k == "tool.name" && v == "web_search")
        );
        drop(span);
    }

    #[test]
    fn test_baggage_propagation() {
        set_baggage("request-id", "abc-123");
        set_baggage("user-id", "user-42");

        assert_eq!(get_baggage("request-id").as_deref(), Some("abc-123"));
        assert_eq!(get_baggage("user-id").as_deref(), Some("user-42"));
        assert_eq!(get_baggage("nonexistent"), None);
    }

    #[test]
    fn test_tracing_config_defaults() {
        let config = TracingConfig::default();
        assert_eq!(config.service_name, "mofa");
        assert_eq!(config.sampling_ratio, 1.0);
        assert_eq!(config.max_attributes, 128);
        assert!(matches!(config.exporter, TracingExporter::Stdout));
    }

    #[test]
    fn test_record_observation_and_error() {
        let config = TracingConfig::default();
        let tracer = AgentTracer::new("agent-obs".to_string(), config);
        let mut span = tracer.start_thought_span("observing");

        tracer.record_observation(&mut span, "saw a result", "tool");
        tracer.record_error(&mut span, "something went wrong");

        // verify events were recorded
        assert!(
            span.events
                .iter()
                .any(|(name, _)| name == "agent.observation")
        );
        assert!(span.events.iter().any(|(name, _)| name == "error"));
    }

    #[test]
    fn test_metrics_collector() {
        let m = MetricsCollector::new();
        m.increment_counter("requests", 1);
        m.increment_counter("requests", 2);
        assert_eq!(m.counter("requests"), 3);

        m.set_gauge("memory_mb", 512.0);
        assert_eq!(m.gauge("memory_mb"), Some(512.0));

        m.record_histogram("latency_ms", 10.0);
        m.record_histogram("latency_ms", 20.0);
        let vals = m.histogram_values("latency_ms");
        assert_eq!(vals.len(), 2);
    }

    #[test]
    fn test_init_tracing_without_feature() {
        // Without the tracing-otel feature, init_tracing returns FeatureNotEnabled
        let result = init_tracing(TracingConfig::default());
        #[cfg(not(feature = "tracing-otel"))]
        assert!(matches!(result, Err(TracingError::FeatureNotEnabled)));
        #[cfg(feature = "tracing-otel")]
        assert!(result.is_ok());
    }
}
