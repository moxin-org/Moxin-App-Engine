# mofa-monitoring

MoFA Monitoring - Web-based dashboard and metrics collection

## Installation

```toml
[dependencies]
mofa-monitoring = "0.1"
```

## Features

- Web-based dashboard for monitoring agent execution
- Metrics collection and visualization
- Prometheus text exposition via `GET /metrics`
- Distributed tracing support with OpenTelemetry
- Real-time agent status monitoring
- Health checks and alerts
- HTTP server for dashboard UI
- Static file embedding for frontend assets

## Quick Start

```rust
use mofa_monitoring::tracing::{AgentTracer, TracerConfig, SamplingStrategy};
use mofa_monitoring::{DashboardServer, DashboardConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Start the monitoring dashboard on port 8080
    let server = DashboardServer::new(DashboardConfig::new().with_port(8080));
    tokio::spawn(async move { let _ = server.start().await; });

    // 2. Create a tracer for your agent
    let _tracer = AgentTracer::new(TracerConfig::new("my-agent")
        .with_sampling(SamplingStrategy::AlwaysOn));

    // 3. Use the tracer to instrument agent operations
    // (see API docs for AgentTracer methods)
    Ok(())
}
```

## Environment Variables (OTLP Export)

Enable the `otlp-metrics` feature and set these environment variables:

| Variable | Default | Description |
|---|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4317` | OTLP gRPC collector endpoint |
| `OTEL_EXPORTER_OTLP_HEADERS` | — | Comma-separated `key=value` auth headers |
| `OTEL_SERVICE_NAME` | `mofa-agent` | Service name label in all exported spans |
| `OTEL_RESOURCE_ATTRIBUTES` | — | Additional resource attributes (e.g. `env=prod`) |

## Prometheus Export

`DashboardServer` now exposes a Prometheus-compatible endpoint at `/metrics`.
The exporter uses a background cache worker so scrape requests stay read-only
and low-overhead under high concurrency.

Cardinality is guarded with configurable caps and an overflow `__other__`
series to protect TSDB backends from unbounded label growth.

## Optional OTLP Metrics Export

Enable the `otlp-metrics` feature to use the native OpenTelemetry OTLP
metrics push exporter:

```toml
[dependencies]
mofa-monitoring = { version = "0.1", features = ["otlp-metrics"] }
```

## Documentation

- [API Documentation](https://docs.rs/mofa-monitoring)
- [Main Repository](https://github.com/mofa-org/mofa)

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
