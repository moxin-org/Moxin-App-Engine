#![allow(dead_code, unused_imports)]
//! MoFA Monitoring - Web-based dashboard and metrics collection
//!
//! This crate provides monitoring capabilities for MoFA agents:
//! - Web dashboard for real-time visualization
//! - Metrics collection and aggregation
//! - REST API for integration
//! - WebSocket for live updates
//!
//! # Example
//!
//! ```rust,no_run
//! use mofa_monitoring::{DashboardServer, DashboardConfig};
//!
//! # #[tokio::main]
//! # async fn main() {
//! let config = DashboardConfig::new()
//!     .with_port(8080)
//!     // Explicit local-dev opt-out. Production deployments should configure
//!     // a real auth provider via `.with_auth(...)`.
//!     .with_require_auth(false);
//!
//! let server = DashboardServer::new(config);
//! // Start the server (this would block)
//! // server.start().await.unwrap();
//! # }
//! ```

mod dashboard;
pub mod tracing;

pub use dashboard::{
    AgentMetrics, AgentStatus, ApiError, ApiResponse, AuthInfo, AuthProvider, CardinalityLimits,
    DashboardConfig, DashboardServer, Gauge, Histogram, LLMMetrics, LLMStatus, LLMSummary,
    MetricType, MetricValue, MetricsCollector, MetricsConfig, MetricsRegistry, MetricsSnapshot,
    NoopAuthProvider, PluginMetrics, PluginStatus, PrometheusExportConfig, PrometheusExportError,
    PrometheusExporter, ServerState, SystemMetrics, SystemStatus, TokenAuthProvider,
    WebSocketClient, WebSocketHandler, WebSocketMessage, WorkflowMetrics,
};
