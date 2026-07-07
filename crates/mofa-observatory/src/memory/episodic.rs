use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use uuid::Uuid;

/// A single conversation turn stored in episodic memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: Uuid,
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    /// Role of the speaker: "user", "assistant", or "tool".
    pub role: String,
    pub content: String,
    pub metadata: HashMap<String, Value>,
}

/// Episodic memory backed by SQLite.
///
/// Stores conversation turns with timestamps. Supports retrieval by
/// session, importance scoring, and recency-based listing.
pub struct EpisodicMemory {
    pool: SqlitePool,
}

impl EpisodicMemory {
    pub async fn new(db_url: &str) -> Result<Self> {
        let pool = SqlitePool::connect(db_url).await?;
        sqlx::query(include_str!("../../migrations/001_init.sql"))
            .execute(&pool)
            .await?;
        Ok(Self { pool })
    }

    pub async fn in_memory() -> Result<Self> {
        Self::new("sqlite::memory:").await
    }

    pub async fn add(&self, ep: &Episode) -> Result<()> {
        let id = ep.id.to_string();
        let ts = ep.timestamp.to_rfc3339();
        let meta = serde_json::to_string(&ep.metadata)?;
        sqlx::query(
            "INSERT INTO episodes (id, session_id, timestamp, role, content, metadata, access_count, importance)
             VALUES (?, ?, ?, ?, ?, ?, 0, 0.5)",
        )
        .bind(&id)
        .bind(&ep.session_id)
        .bind(&ts)
        .bind(&ep.role)
        .bind(&ep.content)
        .bind(&meta)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Vec<Episode>> {
        let rows = sqlx::query(
            "SELECT id, session_id, timestamp, role, content, metadata
             FROM episodes WHERE session_id = ? ORDER BY timestamp ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_episode).collect()
    }

    pub async fn recent(&self, limit: i64) -> Result<Vec<Episode>> {
        let rows = sqlx::query(
            "SELECT id, session_id, timestamp, role, content, metadata
             FROM episodes ORDER BY timestamp DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_episode).collect()
    }
}

fn row_to_episode(row: &sqlx::sqlite::SqliteRow) -> Result<Episode> {
    let id_str: String = row.try_get("id")?;
    let ts_str: String = row.try_get("timestamp")?;
    let meta_str: String = row.try_get("metadata")?;

    Ok(Episode {
        id: Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()),
        session_id: row.try_get("session_id")?,
        timestamp: chrono::DateTime::parse_from_rfc3339(&ts_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        role: row.try_get("role")?,
        content: row.try_get("content")?,
        metadata: serde_json::from_str(&meta_str).unwrap_or_default(),
    })
}
