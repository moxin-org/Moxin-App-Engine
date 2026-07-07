-- Spans table: stores OpenTelemetry-compatible span data
CREATE TABLE IF NOT EXISTS spans (
    id          TEXT PRIMARY KEY,
    trace_id    TEXT NOT NULL,
    parent_id   TEXT,
    name        TEXT NOT NULL,
    agent_id    TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'unset',
    start_time  TEXT NOT NULL,
    end_time    TEXT,
    latency_ms  INTEGER,
    input       TEXT,
    output      TEXT,
    token_count INTEGER,
    cost_usd    REAL,
    attributes  TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_spans_trace   ON spans(trace_id);
CREATE INDEX IF NOT EXISTS idx_spans_agent   ON spans(agent_id);
CREATE INDEX IF NOT EXISTS idx_spans_start   ON spans(start_time DESC);

-- Session snapshots: for time-travel debugging
CREATE TABLE IF NOT EXISTS session_snapshots (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT NOT NULL,
    step        INTEGER NOT NULL,
    state_json  TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_snapshots_session ON session_snapshots(session_id, step);

-- Episodic memory: conversation turns
CREATE TABLE IF NOT EXISTS episodes (
    id           TEXT PRIMARY KEY,
    session_id   TEXT NOT NULL,
    timestamp    TEXT NOT NULL,
    role         TEXT NOT NULL,
    content      TEXT NOT NULL,
    metadata     TEXT NOT NULL DEFAULT '{}',
    access_count INTEGER NOT NULL DEFAULT 0,
    importance   REAL NOT NULL DEFAULT 0.5
);

CREATE INDEX IF NOT EXISTS idx_episodes_session ON episodes(session_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_episodes_ts      ON episodes(timestamp DESC);

-- Entities extracted from spans
CREATE TABLE IF NOT EXISTS entities (
    id         TEXT PRIMARY KEY,
    span_id    TEXT NOT NULL,
    kind       TEXT NOT NULL,
    value      TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_entities_span ON entities(span_id);
CREATE INDEX IF NOT EXISTS idx_entities_kind ON entities(kind);
