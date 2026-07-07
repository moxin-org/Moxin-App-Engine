//! Dashboard web server
//!
//! Main server component that serves the dashboard

use axum::{
    Router,
    extract::Path,
    http::{Method, StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

use mofa_kernel::workflow::telemetry::{DebugEvent, SessionRecorder};

use super::api::create_api_router;
use super::assets::{INDEX_HTML, serve_asset};
use super::auth::{AuthProvider, NoopAuthProvider};
use super::metrics::{MetricsCollector, MetricsConfig};
use super::prometheus::{PrometheusExportConfig, PrometheusExporter};
use super::websocket::{WebSocketHandler, create_websocket_handler};
#[cfg(feature = "otlp-metrics")]
use crate::tracing::{OtlpExporterHandles, OtlpMetricsExporter, OtlpMetricsExporterConfig};
use tokio::sync::mpsc;

/// Dashboard server configuration
#[derive(Clone)]
pub struct DashboardConfig {
    /// Server host
    pub host: String,
    /// Server port
    pub port: u16,
    /// Enable CORS
    pub enable_cors: bool,
    /// Metrics configuration
    pub metrics_config: MetricsConfig,
    /// WebSocket update interval
    pub ws_update_interval: Duration,
    /// Enable request tracing
    pub enable_tracing: bool,
    /// Require a real auth provider before the dashboard server can start.
    pub require_auth: bool,
    /// WebSocket authentication provider.
    ///
    /// Defaults to [`NoopAuthProvider`], but server startup requires either a
    /// real provider via [`DashboardConfig::with_auth`] or an explicit
    /// [`DashboardConfig::with_require_auth(false)`] opt-out.
    pub auth_provider: Arc<dyn AuthProvider>,
    /// Prometheus export configuration
    pub prometheus_export_config: PrometheusExportConfig,
    /// Optional OTLP metrics exporter configuration
    #[cfg(feature = "otlp-metrics")]
    pub otlp_metrics_exporter: Option<OtlpMetricsExporterConfig>,
}

impl std::fmt::Debug for DashboardConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DashboardConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("enable_cors", &self.enable_cors)
            .field("ws_update_interval", &self.ws_update_interval)
            .field("enable_tracing", &self.enable_tracing)
            .field("require_auth", &self.require_auth)
            .field("auth_enabled", &self.auth_provider.is_enabled())
            .field("prometheus_export_config", &self.prometheus_export_config)
            .finish()
    }
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
            enable_cors: true,
            metrics_config: MetricsConfig::default(),
            ws_update_interval: Duration::from_secs(1),
            enable_tracing: true,
            require_auth: true,
            auth_provider: Arc::new(NoopAuthProvider),
            prometheus_export_config: PrometheusExportConfig::default(),
            #[cfg(feature = "otlp-metrics")]
            otlp_metrics_exporter: None,
        }
    }
}

impl DashboardConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_host(mut self, host: &str) -> Self {
        self.host = host.to_string();
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_cors(mut self, enable: bool) -> Self {
        self.enable_cors = enable;
        self
    }

    pub fn with_metrics_config(mut self, config: MetricsConfig) -> Self {
        self.metrics_config = config;
        self
    }

    pub fn with_ws_interval(mut self, interval: Duration) -> Self {
        self.ws_update_interval = interval;
        self
    }

    /// Require or explicitly disable dashboard authentication.
    ///
    /// `true` is the secure default. Set this to `false` only for trusted
    /// local development or test environments.
    pub fn with_require_auth(mut self, require_auth: bool) -> Self {
        self.require_auth = require_auth;
        self
    }

    /// Set the WebSocket authentication provider.
    pub fn with_auth(mut self, provider: Arc<dyn AuthProvider>) -> Self {
        self.auth_provider = provider;
        self
    }

    #[cfg(feature = "otlp-metrics")]
    pub fn with_otlp_metrics_exporter(mut self, config: OtlpMetricsExporterConfig) -> Self {
        self.otlp_metrics_exporter = Some(config);
        self
    }

    pub fn socket_addr(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port)
            .parse()
            .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], self.port)))
    }
}

/// Server state shared across handlers
pub struct ServerState {
    pub collector: Arc<MetricsCollector>,
    pub ws_handler: Arc<WebSocketHandler>,
    /// Optional session recorder for debug sessions
    pub session_recorder: Option<Arc<dyn SessionRecorder>>,
}

/// Dashboard server
pub struct DashboardServer {
    config: DashboardConfig,
    collector: Arc<MetricsCollector>,
    prometheus_export_config: PrometheusExportConfig,
    ws_handler: Option<Arc<WebSocketHandler>>,
    prometheus_exporter: Option<Arc<PrometheusExporter>>,
    prometheus_worker: Option<tokio::task::JoinHandle<()>>,
    #[cfg(feature = "otlp-metrics")]
    otlp_metrics_exporter: Option<Arc<OtlpMetricsExporter>>,
    #[cfg(feature = "otlp-metrics")]
    otlp_metrics_handles: Option<OtlpExporterHandles>,
    session_recorder: Option<Arc<dyn SessionRecorder>>,
    debug_event_rx: Option<mpsc::Receiver<DebugEvent>>,
}

impl DashboardServer {
    /// Create a new dashboard server
    pub fn new(config: DashboardConfig) -> Self {
        if config.require_auth && !config.auth_provider.is_enabled() {
            panic!(
                "Dashboard authentication is required by default. Configure \
                 DashboardConfig::with_auth(...) or explicitly opt out with \
                 DashboardConfig::with_require_auth(false) for trusted local development."
            );
        }

        if !config.require_auth && !config.auth_provider.is_enabled() {
            warn!(
                "Dashboard authentication is disabled; REST and WebSocket \
                 endpoints are publicly accessible. Only disable auth for \
                 trusted local development or tests."
            );
        }

        let collector = Arc::new(MetricsCollector::new(config.metrics_config.clone()));
        let prometheus_export_config = config.prometheus_export_config.clone();

        Self {
            config,
            collector,
            prometheus_export_config,
            ws_handler: None,
            prometheus_exporter: None,
            prometheus_worker: None,
            #[cfg(feature = "otlp-metrics")]
            otlp_metrics_exporter: None,
            #[cfg(feature = "otlp-metrics")]
            otlp_metrics_handles: None,
            session_recorder: None,
            debug_event_rx: None,
        }
    }

    /// Get the metrics collector
    pub fn collector(&self) -> Arc<MetricsCollector> {
        self.collector.clone()
    }

    /// Get the WebSocket handler (if started)
    pub fn ws_handler(&self) -> Option<Arc<WebSocketHandler>> {
        self.ws_handler.clone()
    }

    /// Override Prometheus exporter settings.
    pub fn with_prometheus_export_config(mut self, config: PrometheusExportConfig) -> Self {
        self.prometheus_export_config = config;
        self
    }

    /// Get the Prometheus exporter (if initialized)
    pub fn prometheus_exporter(&self) -> Option<Arc<PrometheusExporter>> {
        self.prometheus_exporter.clone()
    }

    #[cfg(feature = "otlp-metrics")]
    pub fn otlp_metrics_exporter(&self) -> Option<Arc<OtlpMetricsExporter>> {
        self.otlp_metrics_exporter.clone()
    }

    /// Get the session recorder (if configured)
    pub fn session_recorder(&self) -> Option<Arc<dyn SessionRecorder>> {
        self.session_recorder.clone()
    }

    /// Attach a session recorder for debug sessions
    pub fn with_session_recorder(mut self, recorder: Arc<dyn SessionRecorder>) -> Self {
        self.session_recorder = Some(recorder);
        self
    }

    /// Attach a channel receiver for debug events to stream to WebSocket clients.
    ///
    /// This enables real-time debugging by forwarding `DebugEvent`s from the
    /// provided receiver to all WebSocket clients subscribed to the "debug" topic.
    ///
    /// # Arguments
    /// * `rx` - The receiver for debug events to stream to clients
    ///
    /// # Returns
    /// The updated `DashboardServer` instance
    pub fn with_debug_events(mut self, rx: mpsc::Receiver<DebugEvent>) -> Self {
        self.debug_event_rx = Some(rx);
        self
    }

    /// Build the router
    pub fn build_router(&mut self) -> Router {
        // Create WebSocket handler
        let (ws_handler, ws_route) =
            create_websocket_handler(self.collector.clone(), self.config.auth_provider.clone());
        self.ws_handler = Some(ws_handler.clone());

        // API routes
        let api_router = create_api_router(self.collector.clone(), self.session_recorder.clone());
        let prometheus_exporter = if let Some(exporter) = &self.prometheus_exporter {
            exporter.clone()
        } else {
            let exporter = Arc::new(PrometheusExporter::new(
                self.collector.clone(),
                self.prometheus_export_config.clone(),
            ));
            self.prometheus_exporter = Some(exporter.clone());
            exporter
        };

        // Build main router
        let mut router = Router::new()
            // Static assets
            .route("/", get(serve_index))
            .route("/index.html", get(serve_index))
            .route("/debugger", get(serve_debugger))
            .route("/debugger.html", get(serve_debugger))
            .route("/styles.css", get(serve_styles))
            .route("/app.js", get(serve_app_js))
            .route("/debugger.js", get(serve_debugger_js))
            .route("/assets/*path", get(serve_static))
            // Prometheus endpoint
            .route(
                "/metrics",
                get({
                    let exporter = prometheus_exporter.clone();
                    move || {
                        let exporter = exporter.clone();
                        async move { serve_prometheus_metrics(exporter).await }
                    }
                }),
            )
            // API routes
            .nest("/api", api_router)
            // WebSocket
            .route("/ws", ws_route);

        // Add middleware
        let _service_builder = ServiceBuilder::new();

        if self.config.enable_tracing {
            router = router.layer(TraceLayer::new_for_http());
        }

        if self.config.enable_cors {
            let cors = CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers(Any);
            router = router.layer(cors);
        }

        router
    }

    /// Start the server
    pub async fn start(mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let addr = self.config.socket_addr();
        let _ws_interval = self.config.ws_update_interval;

        info!("Building dashboard server...");
        let router = self.build_router();

        // Start metrics collection
        let collector = self.collector.clone();
        tokio::spawn(async move {
            collector.start_collection();
        });
        if let Some(exporter) = &self.prometheus_exporter {
            if let Err(err) = exporter.refresh_once().await {
                warn!("initial /metrics cache refresh failed: {}", err);
            }
            if let Some(handle) = self.prometheus_worker.take() {
                handle.abort();
            }
            self.prometheus_worker = Some(exporter.clone().start());
        }

        #[cfg(feature = "otlp-metrics")]
        if let Some(config) = &self.config.otlp_metrics_exporter {
            let otlp_exporter = Arc::new(OtlpMetricsExporter::new(
                self.collector.clone(),
                config.clone(),
            ));
            match otlp_exporter.clone().start().await {
                Ok(handles) => {
                    self.otlp_metrics_exporter = Some(otlp_exporter);
                    self.otlp_metrics_handles = Some(handles);
                }
                Err(err) => warn!("failed to start OTLP metrics exporter: {}", err),
            }
        }
        // Start WebSocket updates
        if let Some(ws_handler) = &self.ws_handler {
            let handler_for_task = Arc::clone(ws_handler);
            tokio::spawn(async move {
                handler_for_task.start_updates();
            });

            // Start debug event forwarder if debug events receiver is provided
            if let Some(debug_rx) = self.debug_event_rx.take() {
                let debug_handler = Arc::clone(ws_handler);
                tokio::spawn(async move {
                    debug_handler.start_debug_event_forwarder(debug_rx);
                });
            }
        }

        info!("Starting dashboard server on {}", addr);
        info!("Dashboard URL: http://{}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, router).await?;

        Ok(())
    }

    /// Start the server in background
    pub fn start_background(
        self,
    ) -> tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>> {
        tokio::spawn(async move { self.start().await })
    }
}

impl Drop for DashboardServer {
    fn drop(&mut self) {
        if let Some(handle) = self.prometheus_worker.take() {
            handle.abort();
        }

        #[cfg(feature = "otlp-metrics")]
        if let Some(handles) = self.otlp_metrics_handles.take() {
            handles.sampler.abort();
            handles.exporter.abort();
        }
    }
}

/// Serve index.html
async fn serve_index() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html")],
        INDEX_HTML,
    )
}

/// Serve styles.css
async fn serve_styles() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/css")],
        super::assets::STYLES_CSS,
    )
}

/// Serve app.js
async fn serve_app_js() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/javascript")],
        super::assets::APP_JS,
    )
}

/// Serve debugger page
async fn serve_debugger() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html")],
        super::assets::DEBUGGER_HTML,
    )
}

/// Serve debugger.js
async fn serve_debugger_js() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/javascript")],
        super::assets::DEBUGGER_JS,
    )
}

/// Serve static assets
async fn serve_static(Path(path): Path<String>) -> impl IntoResponse {
    serve_asset(path).await
}

/// Serve Prometheus metrics text exposition.
async fn serve_prometheus_metrics(exporter: Arc<PrometheusExporter>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        exporter.render_cached().await,
    )
}

/// Create a simple dashboard server for local development without auth.
pub fn create_dashboard(port: u16) -> DashboardServer {
    let config = DashboardConfig::new()
        .with_port(port)
        .with_require_auth(false);
    DashboardServer::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dashboard_config_default() {
        let config = DashboardConfig::default();
        assert_eq!(config.port, 8080);
        assert!(config.enable_cors);
        assert!(config.enable_tracing);
        assert!(config.require_auth);
    }

    #[test]
    fn test_dashboard_config_builder() {
        let config = DashboardConfig::new()
            .with_host("127.0.0.1")
            .with_port(3000)
            .with_cors(false)
            .with_require_auth(false);

        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 3000);
        assert!(!config.enable_cors);
        assert!(!config.require_auth);
    }

    #[test]
    fn test_socket_addr() {
        let config = DashboardConfig::new()
            .with_host("127.0.0.1")
            .with_port(8080);

        let addr = config.socket_addr();
        assert_eq!(addr.port(), 8080);
    }

    #[test]
    #[should_panic(expected = "Dashboard authentication is required by default")]
    fn test_dashboard_server_new_requires_auth_by_default() {
        let config = DashboardConfig::default();
        let _server = DashboardServer::new(config);
    }

    #[tokio::test]
    async fn test_dashboard_server_new_allows_explicit_unauthenticated_mode() {
        let config = DashboardConfig::default().with_require_auth(false);
        let server = DashboardServer::new(config);

        assert!(server.ws_handler.is_none());
        assert!(server.prometheus_exporter.is_none());
        assert!(server.prometheus_worker.is_none());
        assert!(server.session_recorder.is_none());
    }
}
