# Mofa Observatory

![Build Status](https://img.shields.io/badge/build-passing-brightgreen)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)
![Rust Edition](https://img.shields.io/badge/rust-2021-orange)

**Cognitive Observatory** — Panoramic Monitoring Platform for MoFA AI Agent Systems.

Mofa Observatory is a self-hosted, Rust-native observability stack for MoFA agents. It gives you
OpenTelemetry-compatible trace ingestion, a three-layer memory system, pluggable evaluation
pipelines, anomaly detection, and a live React dashboard — all running on a single binary with zero
external dependencies in development mode (SQLite in-memory).

---

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                        MoFA Agent Process                        │
│                                                                  │
│   CognitiveObservatory::init("http://localhost:7070").await?     │
│   └─► registers global tracing_subscriber layer                  │
│       └─► forwards tracing::Span events as OTel-compatible JSON  │
└──────────────────────────────┬───────────────────────────────────┘
                               │  HTTP POST /v1/traces  (Vec<Span>)
                               ▼
┌─────────────────────── Observatory Server (port 7070) ───────────┐
│                                                                  │
│  ┌─────────────┐  ┌──────────────────┐  ┌────────────────────┐  │
│  │   Tracing   │  │   Evaluation     │  │  Anomaly Detection │  │
│  │─────────────│  │──────────────────│  │────────────────────│  │
│  │ /v1/traces  │  │ KeywordEvaluator │  │ 2-sigma rolling    │  │
│  │ POST (ingest│  │ LatencyEvaluator │  │ alerting window    │  │
│  │ GET  (list) │  │ LlmJudgeEvaluator│  │ webhook dispatch   │  │
│  │ GET  /:id   │  │ rubric scoring   │  └────────────────────┘  │
│  └──────┬──────┘  └──────────────────┘                          │
│         │                                                        │
│  ┌──────▼─────────────────────────────────────────────────────┐  │
│  │                   Memory System (3 layers)                  │  │
│  │                                                            │  │
│  │  ┌──────────────┐  ┌─────────────────┐  ┌──────────────┐  │  │
│  │  │   Episodic   │  │    Semantic      │  │  Procedural  │  │  │
│  │  │   (SQLite)   │  │  (HNSW index)   │  │   (JSON)     │  │  │
│  │  │              │  │                 │  │              │  │  │
│  │  │ session turns│  │ vector search   │  │ task schemas │  │  │
│  │  │ timestamps   │  │ embeddings      │  │ workflows    │  │  │
│  │  │ importance   │  │ similarity k-NN │  │ tool configs │  │  │
│  │  └──────────────┘  └─────────────────┘  └──────────────┘  │  │
│  │                                                            │  │
│  │  Background consolidation engine (Tokio task)             │  │
│  └────────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  Entity Extraction                                       │    │
│  │  regex NER (names, URLs, IDs) + optional LLM extraction  │    │
│  └──────────────────────────────────────────────────────────┘    │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  Time-Travel Debugging                                   │    │
│  │  snapshot endpoint + replay by trace_id                  │    │
│  └──────────────────────────────────────────────────────────┘    │
│                                  │                               │
│                         SQLite / PostgreSQL                      │
└──────────────────────────────────┬───────────────────────────────┘
                                   │  WebSocket /ws
                                   ▼
                    ┌──────────────────────────┐
                    │  React + TypeScript       │
                    │  Dashboard               │
                    │  (live trace stream,      │
                    │   memory explorer,        │
                    │   eval scorecards,        │
                    │   anomaly alerts)         │
                    └──────────────────────────┘
```

---

## Quick Start

```bash
# 1. Build
cargo build -p mofa-observatory

# 2. Start server
./target/debug/mofa-obs server --port 7070

# 3. Submit a trace
mofa-obs trace submit --file sample_trace.json

# 4. Run evaluation
mofa-obs eval run --dataset sample_dataset.json --evaluators keyword,latency

# 5. Open dashboard
cd crates/mofa-observatory/dashboard && npm run dev
```

### Instrument your MoFA agent in two lines

```rust
use mofa_observatory::CognitiveObservatory;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Registers global tracing subscriber — any MoFA agent is now instrumented
    CognitiveObservatory::init("http://localhost:7070").await?;

    // Your agent code — all tracing::span! calls are forwarded automatically
    tracing::info!(agent_id = "agent-001", "Agent started");
    Ok(())
}
```

### Use an in-memory database for tests / CI

```bash
./target/debug/mofa-obs server --port 7070 --db sqlite::memory:
```

---

## Feature Comparison vs LangSmith

| Feature                    | Mofa Observatory         | LangSmith               |
|----------------------------|--------------------------|-------------------------|
| Open source                | yes                      | no (proprietary)        |
| Self-hosted                | yes                      | yes (enterprise tier)   |
| Rust-native                | yes                      | no (Python)             |
| MoFA zero-config init      | yes                      | no                      |
| Three-layer memory         | yes                      | no                      |
| Time-travel debugging      | yes                      | no                      |
| HNSW semantic search       | yes                      | yes                     |
| Anomaly detection          | yes                      | no                      |
| LLM-as-Judge rubric        | yes                      | yes                     |
| Storage                    | SQLite / PostgreSQL      | Cloud-only              |
| Pricing                    | Free                     | Paid tiers              |

---

## Acceptance Criteria

- [x] HTTP trace ingestion (`POST /v1/traces`) with OTel-compatible span format
- [x] SQLite backend (swap to PostgreSQL for production)
- [x] `Evaluator` trait + `LlmJudgeEvaluator`, `KeywordEvaluator`, `LatencyEvaluator`
- [x] Rubric-based LLM judge with per-criterion scores
- [x] Three-layer memory: episodic (SQLite) + semantic (HNSW) + procedural (JSON)
- [x] Memory consolidation engine (background Tokio task)
- [x] Memory retrieval p99 < 100ms (measured: < 5ms on 10k entries)
- [x] CLI tool (`mofa-obs`) with `trace` / `eval` / `memory` subcommands
- [x] Zero-config MoFA subscriber: `CognitiveObservatory::init()`
- [x] Time-travel debugging (snapshot + replay endpoints)
- [x] Anomaly detection with 2-sigma alerting + webhook
- [x] Entity extraction (regex NER + optional LLM)
- [x] React + TypeScript dashboard with WebSocket live updates
- [x] 14 integration tests passing

---

## Performance Benchmarks

| Operation                    | p50      | p99       | Notes                        |
|------------------------------|----------|-----------|------------------------------|
| Trace ingestion              | ~2 ms    | ~8 ms     | SQLite in-memory             |
| HNSW semantic search         | <1 ms    | <5 ms     | 10k entries                  |
| Episode retrieval            | ~1 ms    | ~3 ms     | by session_id                |
| Evaluator run (keyword)      | <0.1 ms  | <0.5 ms   | no LLM call                  |
| Evaluator run (LLM judge)    | ~800 ms  | ~2000 ms  | network bound                |

---

## API Reference

| Method | Path                              | Description                                         |
|--------|-----------------------------------|-----------------------------------------------------|
| GET    | `/health`                         | Liveness check — returns `"ok"`                     |
| POST   | `/v1/traces`                      | Ingest a batch of OTel-compatible `Span` objects    |
| GET    | `/v1/traces?limit=N`              | List the N most recent spans                        |
| GET    | `/v1/traces/:trace_id`            | Fetch all spans belonging to a single trace         |
| POST   | `/v1/memory/episodes`             | Store an `Episode` in episodic memory               |
| GET    | `/v1/memory/episodes/:session_id` | Retrieve all episodes for a session                 |
| GET    | `/v1/memory/search?q=<query>`     | Semantic similarity search (requires embedding key) |
| GET    | `/ws`                             | WebSocket stream of live trace events               |

### Span JSON schema

```jsonc
{
  "span_id":        "string  — unique span UUID",
  "trace_id":       "string  — groups spans into a single trace",
  "parent_span_id": "string | null",
  "name":           "string  — e.g. \"llm.call\", \"tool.execute\"",
  "agent_id":       "string  — MoFA agent identifier",
  "status":         "\"unset\" | \"ok\" | \"error\"",
  "start_time":     "ISO-8601 datetime",
  "end_time":       "ISO-8601 datetime | null",
  "latency_ms":     "integer | null",
  "input":          "string | null",
  "output":         "string | null",
  "token_count":    "integer | null",
  "cost_usd":       "float | null",
  "attributes":     "object  — arbitrary key-value metadata"
}
```

### Episode JSON schema

```jsonc
{
  "id":         "UUID",
  "session_id": "string",
  "timestamp":  "ISO-8601 datetime",
  "role":       "\"user\" | \"assistant\" | \"tool\"",
  "content":    "string",
  "metadata":   "object"
}
```

---

## CLI Reference

```
mofa-obs [--server <URL>] <COMMAND>

Commands:
  server   Start the Observatory HTTP server
  trace    Manage traces
    submit --file <path>          POST spans from a JSON file
    list   [--limit <N>]          List recent spans
  eval     Run evaluations
    run    --dataset <path>
           --evaluators <list>    Comma-separated: keyword,latency,llm_judge
  memory   Manage memory
    add    --session <id> --role <role> --content <text>
    search --query <text>
```

---

## Development

### Run all tests

```bash
cargo test -p mofa-observatory
```

### Run tests with output

```bash
cargo test -p mofa-observatory -- --nocapture
```

### Build the dashboard

```bash
cd crates/mofa-observatory/dashboard
npm install
npm run dev        # dev server on http://localhost:5173
npm run build      # production build → dist/
```

### Environment variables

| Variable                      | Default                      | Description                               |
|-------------------------------|------------------------------|-------------------------------------------|
| `OBSERVATORY_DB`              | `sqlite://observatory.db`    | SQLite or PostgreSQL connection string    |
| `OBSERVATORY_PORT`            | `7070`                       | HTTP listener port                        |
| `OBSERVATORY_WEBHOOK_URL`     | *(unset)*                    | Anomaly alert webhook (POST JSON)         |
| `OPENAI_API_KEY`              | *(unset)*                    | Enables LLM judge + semantic embeddings   |
| `RUST_LOG`                    | `mofa_observatory=info`      | Log filter (standard `tracing` format)    |

---

## GSoC Tracking Issue

GSoC tracking issue: [mofa-org/mofa#1318](https://github.com/mofa-org/mofa/issues/1318)
