//! Web-based Monitoring Dashboard Example
//!
//! This example demonstrates the comprehensive web-based monitoring dashboard
//! for the MoFA framework. It provides real-time monitoring of:
//! - Agents and their states
//! - Workflows and execution metrics
//! - Plugins and their health
//! - System resources
//!
//! Features:
//! - REST API for data access
//! - WebSocket for real-time updates
//! - Embedded web UI with charts
//!
//! Run with: cargo run --example monitoring_dashboard

use mofa_sdk::dashboard::{AgentMetrics, DashboardConfig, DashboardServer, LLMMetrics, MetricsCollector, PluginMetrics, WorkflowMetrics};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter("info,mofa=debug")
        .init();

    info!("╔══════════════════════════════════════════════════════════════╗");
    info!("║           MoFA Web Monitoring Dashboard Example             ║");
    info!("╚══════════════════════════════════════════════════════════════╝");
    // Create dashboard configuration
    let config = DashboardConfig::new()
        // Bind externally so Prometheus in Docker can scrape this example.
        .with_host("0.0.0.0")
        .with_port(8080)
        .with_cors(true)
        .with_ws_interval(Duration::from_secs(1));

    info!("📊 Starting dashboard server...");
    info!("   Host: {}", config.host);
    info!("   Port: {}", config.port);

    // Create dashboard server
    let mut server = DashboardServer::new(config);

    // Get the metrics collector to populate with demo data
    let collector = server.collector();

    // Start demo data generator in background
    let collector_clone = collector.clone();
    tokio::spawn(async move {
        generate_demo_data(collector_clone).await;
    });

    // Build the router (this also sets up WebSocket handler)
    let router = server.build_router();

    // Start metrics collection background task
    collector.clone().start_collection();

    // Ensure Prometheus exporter has an initial payload and keeps refreshing.
    if let Some(exporter) = server.prometheus_exporter() {
        exporter.refresh_once().await?;
        let _prometheus_worker = exporter.start();
    }

    // Get WebSocket handler for sending updates
    if let Some(ws_handler) = server.ws_handler() {
        let ws = ws_handler.clone();
        tokio::spawn(async move {
            // Periodically send alerts as demo
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            let mut count = 0;
            loop {
                interval.tick().await;
                count += 1;
                ws.send_alert(
                    "info",
                    &format!("Demo alert #{}: System operating normally", count),
                    "demo-generator",
                ).await;
            }
        });
    }

    info!("╔══════════════════════════════════════════════════════════════╗");
    info!("║  Dashboard is ready!                                         ║");
    info!("║                                                              ║");
    info!("║  🌐 Web UI:    http://127.0.0.1:8080                         ║");
    info!("║  📡 WebSocket: ws://127.0.0.1:8080/ws                        ║");
    info!("║  🔌 API:       http://127.0.0.1:8080/api                     ║");
    info!("║                                                              ║");
    info!("║  API Endpoints:                                              ║");
    info!("║    GET /api/overview    - Dashboard overview                 ║");
    info!("║    GET /api/metrics     - Current metrics                    ║");
    info!("║    GET /api/agents      - List all agents                    ║");
    info!("║    GET /api/agents/:id  - Get agent details                  ║");
    info!("║    GET /api/workflows   - List all workflows                 ║");
    info!("║    GET /api/plugins     - List all plugins                   ║");
    info!("║    GET /api/system      - System status                      ║");
    info!("║    GET /api/health      - Health check                       ║");
    info!("║                                                              ║");
    info!("║  Press Ctrl+C to stop                                        ║");
    info!("╚══════════════════════════════════════════════════════════════╝");

    // Start the server
    // Bind externally so both host and containers can reach the dashboard.
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!("🚀 Server listening on http://{}", addr);

    axum::serve(listener, router).await?;

    Ok(())
}

/// Generate demo data for the dashboard
async fn generate_demo_data(collector: Arc<MetricsCollector>) {
    info!("🎲 Starting demo data generator...");

    // Create demo agents
    let agents = [("agent-001", "Coordinator Agent", "running"),
        ("agent-002", "Research Agent", "running"),
        ("agent-003", "Writer Agent", "idle"),
        ("agent-004", "Review Agent", "running"),
        ("agent-005", "Deploy Agent", "idle")];

    // Create demo workflows
    let workflows = [("wf-001", "Content Pipeline", "running"),
        ("wf-002", "Data Processing", "idle"),
        ("wf-003", "Model Training", "running")];

    // Create demo plugins
    let plugins = [("plugin-001", "OpenAI LLM", "1.0.0", "running"),
        ("plugin-002", "Memory Store", "1.2.0", "running"),
        ("plugin-003", "Tool Executor", "0.9.0", "running"),
        ("plugin-004", "Vector DB", "2.0.0", "loaded")];

    // Counters for simulation
    let mut tick = 0u64;
    let mut interval = tokio::time::interval(Duration::from_millis(500));

    loop {
        interval.tick().await;
        tick += 1;

        // Update agent metrics
        for (i, (id, name, base_state)) in agents.iter().enumerate() {
            // Occasionally change state
            let state = if tick % 20 == (i as u64 * 3) % 20 {
                if *base_state == "running" { "idle" } else { "running" }
            } else {
                *base_state
            };

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let metrics = AgentMetrics {
                agent_id: id.to_string(),
                name: name.to_string(),
                state: state.to_string(),
                tasks_completed: tick * (i as u64 + 1) / 2,
                tasks_failed: tick / 50 * (i as u64 % 2),
                tasks_in_progress: if state == "running" { 1 + (i as u32 % 3) } else { 0 },
                messages_sent: tick * (i as u64 + 1),
                messages_received: tick * (i as u64 + 1) + i as u64 * 10,
                last_activity: now,
                avg_task_duration_ms: 150.0 + (i as f64 * 50.0),
            };

            collector.update_agent(metrics).await;
        }

        // Update workflow metrics
        for (i, (id, name, status)) in workflows.iter().enumerate() {
            let metrics = WorkflowMetrics {
                workflow_id: id.to_string(),
                name: name.to_string(),
                status: status.to_string(),
                total_executions: tick / 5 * (i as u64 + 1),
                successful_executions: tick / 5 * (i as u64 + 1) - tick / 100,
                failed_executions: tick / 100,
                running_instances: if *status == "running" { 1 + (i as u32 % 2) } else { 0 },
                avg_execution_time_ms: 500.0 + (i as f64 * 200.0),
                node_count: 5 + (i as u32 * 2),
            };

            collector.update_workflow(metrics).await;
        }

        // Update plugin metrics
        for (i, (id, name, version, state)) in plugins.iter().enumerate() {
            let metrics = PluginMetrics {
                plugin_id: id.to_string(),
                name: name.to_string(),
                version: version.to_string(),
                state: state.to_string(),
                call_count: tick * (i as u64 + 1) * 2,
                error_count: tick / 100 * (i as u64 % 2),
                avg_response_time_ms: 20.0 + (i as f64 * 10.0),
                last_reload: Some(std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()),
                reload_count: (tick / 500) as u32,
            };

        collector.update_plugin(metrics).await;
        }

        // Update LLM metrics
        let llm_models = [
            ("openai-gpt4", "OpenAI", "gpt-4"),
            ("openai-gpt35", "OpenAI", "gpt-3.5-turbo"),
            ("anthropic-claude", "Anthropic", "claude-3-opus"),
        ];

        for (i, (plugin_id, provider, model)) in llm_models.iter().enumerate() {
            let success_rate = 0.95 - (i as f64 * 0.05);
            let requests = tick * (i as u64 + 1) * 5;
            let successes = (requests as f64 * success_rate) as u64;
            let failures = requests - successes;

            let metrics = LLMMetrics {
                plugin_id: plugin_id.to_string(),
                provider_name: provider.to_string(),
                model_name: model.to_string(),
                state: "running".to_string(),
                total_requests: requests,
                successful_requests: successes,
                failed_requests: failures,
                total_tokens: requests * (200 + i as u64 * 100),
                prompt_tokens: requests * (120 + i as u64 * 60),
                completion_tokens: requests * (80 + i as u64 * 40),
                avg_latency_ms: 400.0 + (i as f64 * 150.0),
                tokens_per_second: Some(35.0 + i as f64 * 10.0),
                time_to_first_token_ms: Some(150.0 + i as f64 * 50.0),
                requests_per_minute: 60.0 + i as f64 * 20.0,
                error_rate: (1.0 - success_rate) * 100.0,
                last_request_timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };

            collector.update_llm(metrics).await;
        }

        // Log progress occasionally
        if tick % 60 == 0 {
            info!("📊 Demo data tick: {} (agents: {}, workflows: {}, plugins: {}, llm: {})",
                tick, agents.len(), workflows.len(), plugins.len(), llm_models.len());
        }
    }
}
