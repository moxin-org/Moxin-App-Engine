# Monitoring & Observability

Examples demonstrating monitoring dashboards and observability features.

## Web Monitoring Dashboard

Real-time web dashboard for agent monitoring.

**Location:** `examples/monitoring_dashboard/`

```rust
use mofa_sdk::dashboard::{DashboardConfig, DashboardServer, MetricsCollector};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Configure dashboard
    let config = DashboardConfig::new()
        .with_host("127.0.0.1")
        .with_port(8080)
        .with_cors(true)
        .with_require_auth(false)
        .with_ws_interval(Duration::from_secs(1));

    // Create dashboard server
    let mut server = DashboardServer::new(config);

    // Get metrics collector
    let collector = server.collector();

    // Start demo data generator (in real app, use actual agent metrics)
    tokio::spawn(async move {
        generate_demo_data(collector).await;
    });

    // Build router and start server
    let router = server.build_router();
    let addr: SocketAddr = "127.0.0.1:8080".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("Dashboard running at http://{}", addr);
    axum::serve(listener, router).await?;

    Ok(())
}
```

## Updating Metrics

Push metrics to the dashboard:

```rust
use mofa_sdk::dashboard::{AgentMetrics, WorkflowMetrics, PluginMetrics};

// Update agent metrics
let agent_metrics = AgentMetrics {
    agent_id: "agent-001".to_string(),
    name: "Research Agent".to_string(),
    state: "running".to_string(),
    tasks_completed: 42,
    tasks_failed: 2,
    tasks_in_progress: 3,
    messages_sent: 150,
    messages_received: 148,
    last_activity: now(),
    avg_task_duration_ms: 250.0,
};

collector.update_agent(agent_metrics).await;

// Update workflow metrics
let workflow_metrics = WorkflowMetrics {
    workflow_id: "wf-001".to_string(),
    name: "Content Pipeline".to_string(),
    status: "running".to_string(),
    total_executions: 100,
    successful_executions: 95,
    failed_executions: 5,
    running_instances: 2,
    avg_execution_time_ms: 5000.0,
    node_count: 5,
};

collector.update_workflow(workflow_metrics).await;

// Update plugin metrics
let plugin_metrics = PluginMetrics {
    plugin_id: "plugin-001".to_string(),
    name: "OpenAI LLM".to_string(),
    version: "1.0.0".to_string(),
    state: "running".to_string(),
    call_count: 1000,
    error_count: 5,
    avg_response_time_ms: 150.0,
    last_reload: Some(now()),
    reload_count: 3,
};

collector.update_plugin(plugin_metrics).await;
```

## WebSocket Real-Time Updates

The dashboard provides WebSocket for real-time updates:

```rust
// Get WebSocket handler
if let Some(ws_handler) = server.ws_handler() {
    let ws = ws_handler.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            ws.send_alert(
                "info",
                "System operating normally",
                "health-check",
            ).await;
        }
    });
}
```

## API Endpoints

The dashboard exposes REST API endpoints:

| Endpoint | Description |
|----------|-------------|
| `GET /api/overview` | Dashboard overview |
| `GET /api/metrics` | Current metrics snapshot |
| `GET /api/agents` | List all agents |
| `GET /api/agents/:id` | Get agent details |
| `GET /api/workflows` | List all workflows |
| `GET /api/plugins` | List all plugins |
| `GET /api/system` | System status |
| `GET /api/health` | Health check |

## Accessing the Dashboard

```bash
# Start the dashboard
cargo run -p monitoring_dashboard

# Open in browser
open http://127.0.0.1:8080

# WebSocket endpoint
ws://127.0.0.1:8080/ws

# API base URL
http://127.0.0.1:8080/api
```

## Integration with Agents

Connect your agents to the dashboard:

```rust
use mofa_sdk::monitoring::MetricsEmitter;

// Create emitter connected to dashboard
let emitter = MetricsEmitter::new("http://127.0.0.1:8080/api");

// In agent execution
async fn execute(&mut self, input: AgentInput, ctx: &AgentContext) -> AgentResult<AgentOutput> {
    let start = Instant::now();

    // ... do work ...

    // Emit metrics
    emitter.emit_task_completed(
        self.id(),
        start.elapsed().as_millis() as f64,
    ).await;

    Ok(output)
}
```

## Running Examples

```bash
# Start monitoring dashboard
cargo run -p monitoring_dashboard

# Access at http://127.0.0.1:8080
```

## Available Examples

| Example | Description |
|---------|-------------|
| `monitoring_dashboard` | Web-based monitoring dashboard |

## See Also

- [Monitoring Guide](../guides/monitoring.md) — Monitoring best practices
- [Production Deployment](../advanced/production.md) — Production setup
