//! Cognitive Observatory — zero-config tracing subscriber for MoFA agents.
//!
//! Registers a global [`tracing_subscriber`] layer that forwards span events
//! to the Cognitive Observatory ingestion endpoint via HTTP.
//!
//! # Usage
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use mofa_monitoring::observatory::CognitiveObservatory;
//!
//! CognitiveObservatory::init("http://localhost:7070").await?;
//!
//! // Existing tracing macros are now automatically captured:
//! tracing::info!(tokens = 142, model = "gpt-4o", "LLM call completed");
//! # Ok(())
//! # }
//! ```

mod layer;
mod transport;
mod types;

pub use layer::ObservatoryLayer;
pub use transport::ObservatoryTransport;
pub use types::{ObservatoryError, SpanField, SpanRecord};

use tokio::sync::mpsc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Cognitive Observatory client.
///
/// Call [`CognitiveObservatory::init`] once at application startup to register
/// the global tracing subscriber layer.
pub struct CognitiveObservatory;

impl CognitiveObservatory {
    /// Initialize the Cognitive Observatory subscriber with default settings.
    ///
    /// Registers a global `tracing` subscriber that batches spans and POSTs them
    /// to `{endpoint}/v1/traces` every 100ms.
    ///
    /// # Errors
    ///
    /// Returns [`ObservatoryError`] if the subscriber has already been set or if
    /// the transport cannot be started.
    pub async fn init(endpoint: impl Into<String>) -> Result<(), ObservatoryError> {
        Self::init_with_config(ObservatoryConfig::new(endpoint)).await
    }

    /// Initialize with custom configuration.
    pub async fn init_with_config(config: ObservatoryConfig) -> Result<(), ObservatoryError> {
        let (tx, rx) = mpsc::channel(config.buffer_size);
        let transport = ObservatoryTransport::new(config.endpoint.clone(), rx);

        // Start the background flush task
        tokio::spawn(transport.run(config.flush_interval_ms));

        let layer = ObservatoryLayer::new(tx);

        tracing_subscriber::registry()
            .with(layer)
            .try_init()
            .map_err(|e| ObservatoryError::SubscriberInit(e.to_string()))
    }

    /// Build a layer that can be composed with an existing subscriber via `.with()`.
    ///
    /// Use this when you already have a tracing subscriber and want to add
    /// Observatory as an additional layer.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use mofa_monitoring::observatory::CognitiveObservatory;
    /// use tracing_subscriber::layer::SubscriberExt;
    /// use tracing_subscriber::util::SubscriberInitExt;
    ///
    /// # #[tokio::main]
    /// # async fn main() {
    /// let (layer, transport) = CognitiveObservatory::layer("http://localhost:7070").await;
    /// tokio::spawn(transport.run(100));
    ///
    /// tracing_subscriber::registry()
    ///     .with(layer)
    ///     .with(tracing_subscriber::fmt::layer())
    ///     .init();
    /// # }
    /// ```
    pub async fn layer(endpoint: impl Into<String>) -> (ObservatoryLayer, ObservatoryTransport) {
        let config = ObservatoryConfig::new(endpoint);
        let (tx, rx) = mpsc::channel(config.buffer_size);
        let transport = ObservatoryTransport::new(config.endpoint, rx);
        let layer = ObservatoryLayer::new(tx);
        (layer, transport)
    }
}

/// Configuration for the Cognitive Observatory subscriber.
#[derive(Debug, Clone)]
pub struct ObservatoryConfig {
    /// Base URL of the Cognitive Observatory server, e.g. `"http://localhost:7070"`.
    pub endpoint: String,
    /// How often to flush buffered spans to the server (milliseconds). Default: 100.
    pub flush_interval_ms: u64,
    /// Maximum number of spans to buffer before back-pressure. Default: 1024.
    pub buffer_size: usize,
}

impl ObservatoryConfig {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
            flush_interval_ms: 100,
            buffer_size: 1024,
        }
    }

    pub fn with_flush_interval(mut self, ms: u64) -> Self {
        self.flush_interval_ms = ms;
        self
    }

    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observatory::layer::ObservatoryLayer;
    use crate::observatory::types::SpanRecord;
    use tokio::sync::mpsc;
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn test_observatory_config_defaults() {
        let config = ObservatoryConfig::new("http://localhost:7070");
        assert_eq!(config.endpoint, "http://localhost:7070");
        assert_eq!(config.flush_interval_ms, 100);
        assert_eq!(config.buffer_size, 1024);
    }

    #[test]
    fn test_observatory_config_trailing_slash_stripped() {
        let config = ObservatoryConfig::new("http://localhost:7070/");
        assert_eq!(config.endpoint, "http://localhost:7070");
    }

    #[test]
    fn test_observatory_config_builder() {
        let config = ObservatoryConfig::new("http://localhost:7070")
            .with_flush_interval(50)
            .with_buffer_size(512);
        assert_eq!(config.flush_interval_ms, 50);
        assert_eq!(config.buffer_size, 512);
    }

    #[tokio::test]
    async fn test_layer_captures_span_on_close() {
        let (tx, mut rx) = mpsc::channel(32);
        let layer = ObservatoryLayer::new(tx);

        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = tracing::subscriber::set_default(subscriber);

        // Create a span and close it
        {
            let _span = tracing::info_span!("test_operation", agent = "my-agent").entered();
            // span closes on drop
        }

        // The closed span should be in the channel
        let record = rx.try_recv();
        assert!(record.is_ok(), "Expected a SpanRecord to be sent");
        let record = record.unwrap();
        assert_eq!(record.name, "test_operation");
        assert!(record.latency_us > 0);
    }

    #[tokio::test]
    async fn test_layer_captures_token_count_from_event() {
        let (tx, mut rx) = mpsc::channel(32);
        let layer = ObservatoryLayer::new(tx);

        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = tracing::subscriber::set_default(subscriber);

        {
            let _span = tracing::info_span!("llm_call").entered();
            tracing::info!(tokens = 142u64, model = "gpt-4o", "LLM call completed");
        }

        let record = rx.try_recv().unwrap();
        assert_eq!(record.name, "llm_call");
        assert_eq!(record.tokens, Some(142));
        assert_eq!(record.model.as_deref(), Some("gpt-4o"));
    }

    #[tokio::test]
    async fn test_layer_captures_parent_child_relationship() {
        let (tx, mut rx) = mpsc::channel(32);
        let layer = ObservatoryLayer::new(tx);

        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = tracing::subscriber::set_default(subscriber);

        {
            let parent = tracing::info_span!("parent_span").entered();
            {
                let _child = tracing::info_span!("child_span").entered();
            } // child closes first
            drop(parent);
        } // parent closes

        // Collect both records
        let mut records = Vec::new();
        while let Ok(r) = rx.try_recv() {
            records.push(r);
        }

        assert_eq!(records.len(), 2, "Expected 2 spans (child + parent)");
        let child = records.iter().find(|r| r.name == "child_span").unwrap();
        let parent = records.iter().find(|r| r.name == "parent_span").unwrap();
        assert_eq!(child.parent_span_id.as_ref(), Some(&parent.span_id));
    }

    #[tokio::test]
    async fn test_layer_target_is_captured() {
        let (tx, mut rx) = mpsc::channel(32);
        let layer = ObservatoryLayer::new(tx);

        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = tracing::subscriber::set_default(subscriber);

        {
            let _span = tracing::info_span!("my_op").entered();
        }

        let record = rx.try_recv().unwrap();
        // target should be the module path
        assert!(!record.target.is_empty());
    }

    #[tokio::test]
    async fn test_transport_flush_sends_http_request() {
        // This test verifies transport serialization, not the actual HTTP call.
        // A real HTTP test would need a mock server.
        let spans = vec![SpanRecord {
            span_id: "abc123".to_string(),
            parent_span_id: None,
            trace_id: "trace001".to_string(),
            target: "my_agent".to_string(),
            name: "test_span".to_string(),
            start_time_us: 1000,
            end_time_us: 2000,
            latency_us: 1000,
            fields: vec![],
            tokens: Some(10),
            model: Some("gpt-4o".to_string()),
        }];

        // Verify it serializes cleanly
        let json = serde_json::to_string(&spans).unwrap();
        assert!(json.contains("test_span"));
        assert!(json.contains("gpt-4o"));

        // Verify it deserializes back
        let decoded: Vec<SpanRecord> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].tokens, Some(10));
    }
}
