//! Web-based Monitoring Dashboard (real telemetry source)
//!
//! This binary keeps the existing dashboard UI but sources LLM telemetry
//! from persisted API-call records instead of synthetic demo generators.
//!
//! Run with:
//!   MOFA_SQLITE_METRICS_URL='sqlite://./mofa_metrics.db?mode=rwc' \
//!   cargo run --manifest-path examples/monitoring_dashboard/Cargo.toml --bin persisted_telemetry

use mofa_sdk::dashboard::{DashboardConfig, DashboardServer};
use mofa_sdk::persistence::{DynApiCallStore, PersistenceMetricsSource, SqliteStore};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

fn normalize_sqlite_url(input: &str) -> String {
    // Preserve explicit sqlite mode options provided by the caller.
    if input == "sqlite::memory:" || input.contains("mode=") {
        return input.to_string();
    }

    // Ensure file URLs are writable/creatable by default.
    if input.starts_with("sqlite://") {
        if input.contains('?') {
            return format!("{input}&mode=rwc");
        }
        return format!("{input}?mode=rwc");
    }

    // Normalize legacy `sqlite:...` forms into SQLx-compatible URLs.
    if let Some(path) = input.strip_prefix("sqlite:") {
        if path.starts_with('/') {
            return format!("sqlite://{path}?mode=rwc");
        }
        return format!("sqlite:///{path}?mode=rwc");
    }

    input.to_string()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter("info,mofa=debug")
        .init();

    let sqlite_url_raw = std::env::var("MOFA_SQLITE_METRICS_URL")
        .unwrap_or_else(|_| "sqlite://./mofa_metrics.db?mode=rwc".to_string());
    let sqlite_url = normalize_sqlite_url(&sqlite_url_raw);
    let provider_name =
        std::env::var("MOFA_LLM_PROVIDER_NAME").unwrap_or_else(|_| "persistence".to_string());
    let host = std::env::var("MOFA_MONITOR_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("MOFA_MONITOR_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(8080);

    info!("╔══════════════════════════════════════════════════════════════╗");
    info!("║    MoFA Monitoring Dashboard (Persisted Telemetry Source)   ║");
    info!("╚══════════════════════════════════════════════════════════════╝");
    info!("📊 Connecting persistence metrics source...");
    info!("   SQLite URL: {}", sqlite_url);
    info!("   Provider:   {}", provider_name);

    // Back the LLM metrics source with persisted API-call statistics.
    let sqlite_store = SqliteStore::connect(&sqlite_url).await?;
    let api_store: DynApiCallStore = Arc::new(sqlite_store);
    let metrics_source = Arc::new(PersistenceMetricsSource::new(api_store));

    let config = DashboardConfig::new()
        .with_host(&host)
        .with_port(port)
        .with_cors(true)
        .with_ws_interval(Duration::from_secs(1));

    let mut server = DashboardServer::new(config)
        .with_llm_metrics_source(metrics_source, provider_name);

    let router = server.build_router();
    let collector = server.collector();
    // Prime the first snapshot so /metrics has data on initial scrape.
    collector.collect().await;
    collector.start_collection();

    if let Some(exporter) = server.prometheus_exporter() {
        exporter.refresh_once().await?;
        let _prometheus_worker = exporter.start();
    }

    let bind_addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;

    info!("🌐 Web UI:    http://{}", bind_addr);
    info!("📡 Metrics:   http://{}/metrics", bind_addr);
    info!("🚀 Server listening on http://{}", bind_addr);
    info!("Press Ctrl+C to stop");

    axum::serve(listener, router).await?;
    Ok(())
}
