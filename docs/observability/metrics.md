# Metrics Export Pipeline

MoFA monitoring now supports two export paths:

- `GET /metrics`: Prometheus text exposition
- Optional native OTLP metrics push (`otlp-metrics` feature)

## Prometheus Endpoint

The dashboard server exposes Prometheus data at:

```text
GET /metrics
Content-Type: text/plain; version=0.0.4; charset=utf-8
```

The exporter uses a background cache worker (default refresh: `1s`).
Scrapes read cached output, so request-time work is minimal even under high
concurrency.

## Cardinality Controls

Default hard limits:

- `agent_id`: 100
- `workflow_id`: 100
- `plugin_or_tool`: 100
- distinct `(provider, model)` pairs: 50

When a limit is exceeded, overflow series keep their original label key(s)
but replace value(s) with `__other__` (for example
`agent_id="__other__"` or `provider="__other__",model="__other__"`).

Exporter self-metrics:

- `mofa_exporter_render_duration_seconds`
- `mofa_exporter_dropped_series_total{label=...}`
- `mofa_exporter_cache_age_seconds`
- `mofa_exporter_refresh_failures_total`

## OTLP Push Export

Enable feature:

```toml
mofa-monitoring = { version = "0.1", features = ["otlp-metrics"] }
```

Use `OtlpMetricsExporter` to periodically sample `MetricsCollector` snapshots
and record them via native OpenTelemetry metric instruments exported through
the OTLP SDK pipeline.

Backpressure is enforced with a bounded queue (`max_queue_size`), and dropped
samples are counted.

## Local Verification

```bash
cargo check -p mofa-monitoring --offline
cargo check -p mofa-monitoring --features otlp-metrics --offline
cargo test -p mofa-monitoring dashboard::prometheus --offline
cargo test -p mofa-monitoring --features otlp-metrics tracing::metrics_exporter --offline
```

## Swarm Distributed Tracing (SwarmTraceReporter)

PR #1490 adds `SwarmTraceReporter` to `mofa-smith`. Every `mofa swarm run` execution now produces a distributed trace with one root span per goal and one child span per task, emitted as OpenTelemetry spans with `gen_ai.agent.*` semantic attributes.

### Starting the reporter

```rust
use mofa_smith::{SwarmTraceReporter, OtlpBackend, LogBackend};

// For production: send to Jaeger or Grafana Tempo via OTLP
let backend = OtlpBackend::new("http://localhost:4318");
let (reporter, handle) = SwarmTraceReporter::new(backend);
tokio::spawn(reporter.run());

// For local dev: print spans as JSON to stdout
let (reporter, handle) = SwarmTraceReporter::new(LogBackend::default());
```

### Span attributes

Every span carries these `gen_ai.agent.*` attributes from the OTel GenAI semantic conventions:

| Attribute | Value |
|-----------|-------|
| `gen_ai.agent.id` | Agent UUID |
| `gen_ai.agent.task` | Task description string |
| `gen_ai.agent.risk_level` | "low" / "medium" / "high" / "critical" |
| `gen_ai.agent.duration_ms` | Wall-clock duration |
| `gen_ai.agent.outcome` | "success" / "failure" / "rejected" |

### Pluggable backend

Implement `TraceBackend` to send spans to any destination:

```rust
#[async_trait::async_trait]
impl TraceBackend for MyBackend {
    async fn record_span(&self, span: SpanEvent) -> Result<(), TraceError> {
        // forward to Datadog, Zipkin, or any custom sink
        Ok(())
    }
}
```
