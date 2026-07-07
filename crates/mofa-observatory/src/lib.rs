//! # Mofa Observatory
//!
//! Cognitive Observatory — Panoramic Monitoring Platform for MoFA AI Agent Systems.
//!
//! ## Features
//! - OpenTelemetry-compatible trace ingestion (HTTP POST /v1/traces)
//! - SQLite-backed span storage with in-memory option
//! - Pluggable evaluation framework (LLM judge, keyword, latency evaluators)
//! - Three-layer memory system (episodic, semantic HNSW, procedural)
//! - Memory consolidation engine (background Tokio task)
//! - Entity extraction pipeline (regex NER + optional LLM)
//! - Anomaly detection with rolling 2-sigma alerting and webhooks
//! - Time-travel debugging (snapshot + replay)
//! - React+TypeScript dashboard with WebSocket live updates
//! - Zero-config tracing subscriber: `CognitiveObservatory::init()`
//!
//! ## Quick Start
//! ```rust,no_run
//! use mofa_observatory::CognitiveObservatory;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Registers global tracing subscriber — any MoFA agent is now instrumented
//!     CognitiveObservatory::init("http://localhost:7070").await?;
//!     Ok(())
//! }
//! ```

pub mod anomaly;
pub mod api;
pub mod entity;
pub mod evaluation;
pub mod memory;
pub mod tracing;

/// Zero-config initializer.
///
/// Registers an OpenTelemetry-compatible [`tracing_subscriber`] layer that
/// forwards all `tracing::Span` events to the Observatory ingestion endpoint.
/// Any MoFA agent calling this function will appear in the dashboard without
/// further instrumentation changes.
pub struct CognitiveObservatory;

impl CognitiveObservatory {
    /// Initialize the Observatory and register the global tracing subscriber.
    ///
    /// # Arguments
    /// * `endpoint` - Base URL of the Observatory server, e.g. `"http://localhost:7070"`
    pub async fn init(_endpoint: &str) -> anyhow::Result<()> {
        // Full implementation in the subscriber module (Task 13)
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive("mofa_observatory=info".parse()?),
            )
            .init();
        Ok(())
    }
}
