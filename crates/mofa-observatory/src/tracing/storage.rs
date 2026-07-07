use super::span::{Span, SpanStatus};
use anyhow::Result;
use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};

pub struct TraceStorage {
    pub(crate) pool: SqlitePool,
}

impl TraceStorage {
    pub async fn new(db_url: &str) -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(db_url)
            .await?;
        // Run embedded migrations
        sqlx::query(include_str!("../../migrations/001_init.sql"))
            .execute(&pool)
            .await?;
        Ok(Self { pool })
    }

    pub async fn in_memory() -> Result<Self> {
        Self::new("sqlite::memory:").await
    }

    pub async fn insert_span(&self, span: &Span) -> Result<()> {
        let status = match span.status {
            SpanStatus::Unset => "unset",
            SpanStatus::Ok => "ok",
            SpanStatus::Error => "error",
        };
        let attrs = serde_json::to_string(&span.attributes)?;
        let start = span.start_time.to_rfc3339();
        let end = span.end_time.map(|t| t.to_rfc3339());

        sqlx::query(
            "INSERT OR REPLACE INTO spans
             (id, trace_id, parent_id, name, agent_id, status,
              start_time, end_time, latency_ms, input, output,
              token_count, cost_usd, attributes)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&span.span_id)
        .bind(&span.trace_id)
        .bind(&span.parent_span_id)
        .bind(&span.name)
        .bind(&span.agent_id)
        .bind(status)
        .bind(&start)
        .bind(&end)
        .bind(span.latency_ms)
        .bind(&span.input)
        .bind(&span.output)
        .bind(span.token_count)
        .bind(span.cost_usd)
        .bind(&attrs)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_spans(&self, limit: i64, offset: i64) -> Result<Vec<Span>> {
        let rows = sqlx::query(
            "SELECT id, trace_id, parent_id, name, agent_id, status,
                    start_time, end_time, latency_ms, input, output,
                    token_count, cost_usd, attributes
             FROM spans ORDER BY start_time DESC LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_span).collect()
    }

    pub async fn get_span(&self, span_id: &str) -> Result<Option<Span>> {
        let row = sqlx::query(
            "SELECT id, trace_id, parent_id, name, agent_id, status,
                    start_time, end_time, latency_ms, input, output,
                    token_count, cost_usd, attributes
             FROM spans WHERE id = ?",
        )
        .bind(span_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|r| row_to_span(&r)).transpose()
    }

    pub async fn get_trace(&self, trace_id: &str) -> Result<Vec<Span>> {
        let rows = sqlx::query(
            "SELECT id, trace_id, parent_id, name, agent_id, status,
                    start_time, end_time, latency_ms, input, output,
                    token_count, cost_usd, attributes
             FROM spans WHERE trace_id = ? ORDER BY start_time ASC",
        )
        .bind(trace_id)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_span).collect()
    }
}

fn row_to_span(row: &sqlx::sqlite::SqliteRow) -> Result<Span> {
    use std::collections::HashMap;
    let status_str: String = row.try_get("status")?;
    let status = match status_str.as_str() {
        "ok" => SpanStatus::Ok,
        "error" => SpanStatus::Error,
        _ => SpanStatus::Unset,
    };
    let start_str: String = row.try_get("start_time")?;
    let end_str: Option<String> = row.try_get("end_time")?;
    let attrs_str: String = row.try_get("attributes")?;

    Ok(Span {
        span_id: row.try_get("id")?,
        trace_id: row.try_get("trace_id")?,
        parent_span_id: row.try_get("parent_id")?,
        name: row.try_get("name")?,
        agent_id: row.try_get("agent_id")?,
        status,
        start_time: chrono::DateTime::parse_from_rfc3339(&start_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now()),
        end_time: end_str.and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .ok()
        }),
        latency_ms: row.try_get("latency_ms")?,
        input: row.try_get("input")?,
        output: row.try_get("output")?,
        token_count: row.try_get("token_count")?,
        cost_usd: row.try_get("cost_usd")?,
        attributes: serde_json::from_str(&attrs_str).unwrap_or_else(|_| HashMap::new()),
    })
}
