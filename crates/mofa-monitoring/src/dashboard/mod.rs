//! Web-based monitoring dashboard module
//!
//! Provides a web dashboard for monitoring MoFA:
//! - Real-time metrics visualization
//! - Agent status monitoring
//! - Workflow execution tracking
//! - Plugin health monitoring
//! - LLM model inference metrics (per-model monitoring for dashboard)
//! - System resource usage
//! - REST API for integration
//! - WebSocket for live updates
//!
//! Authentication is required by default. For trusted local demos and tests,
//! explicitly opt out with [`DashboardConfig::with_require_auth(false)`].

mod api;
mod assets;
pub mod auth;
mod metrics;
mod prometheus;
mod server;
mod websocket;

pub use api::{
    AgentStatus, ApiError, ApiResponse, DebugSessionResponse, LLMStatus, LLMSummary, PluginStatus,
    SystemStatus,
};
pub use auth::{AuthInfo, AuthProvider, NoopAuthProvider, TokenAuthProvider};
pub use metrics::{
    AgentMetrics, Gauge, Histogram, LLMMetrics, MetricType, MetricValue, MetricsCollector,
    MetricsConfig, MetricsRegistry, MetricsSnapshot, PluginMetrics, SystemMetrics, WorkflowMetrics,
};
pub use prometheus::{
    CardinalityLimits, PrometheusExportConfig, PrometheusExportError, PrometheusExporter,
};
pub use server::{DashboardConfig, DashboardServer, ServerState};
pub use websocket::{WebSocketClient, WebSocketHandler, WebSocketMessage};
