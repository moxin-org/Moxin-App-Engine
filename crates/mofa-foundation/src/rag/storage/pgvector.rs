//! Pgvector-backed vector store implementation
//!
//! Provides a production-grade VectorStore backed by PostgreSQL with the pgvector extension.
//! Suitable for RAG pipelines requiring persistent vector storage with PostgreSQL.

use async_trait::async_trait;
use mofa_kernel::agent::error::{AgentError, AgentResult};
use mofa_kernel::rag::{DocumentChunk, SearchResult, SimilarityMetric, VectorStore};
use std::collections::HashMap;

/// Configuration for connecting to a PostgreSQL instance with pgvector.
#[derive(Debug, Clone)]
pub struct PgVectorConfig {
    /// PostgreSQL connection string (e.g., "host=localhost port=5432 user=postgres password=secret dbname=vectordb")
    pub connection_string: String,
    /// Name of the table to use for storing embeddings
    pub table_name: String,
    /// Dimensionality of embedding vectors
    pub vector_dimensions: usize,
    /// Whether to create the table if it does not exist
    pub create_table: bool,
}

impl PgVectorConfig {
    /// Create a new PgVectorConfig with the given connection string.
    pub fn new(connection_string: impl Into<String>, table_name: impl Into<String>, vector_dimensions: usize) -> Self {
        Self {
            connection_string: connection_string.into(),
            table_name: table_name.into(),
            vector_dimensions,
            create_table: true,
        }
    }

    /// Set whether to create the table if it does not exist.
    pub fn with_create_table(mut self, create: bool) -> Self {
        self.create_table = create;
        self
    }
}

/// Pgvector-backed vector store.
///
/// Stores document chunks as rows in a PostgreSQL table with the pgvector extension.
/// Uses cosine similarity for vector search.
pub struct PgVectorStore {
    client: tokio_postgres::Client,
    table_name: String,
    vector_dimensions: usize,
    metric: SimilarityMetric,
}

impl PgVectorStore {
    /// Create a new PgVectorStore from the given configuration.
    ///
    /// Connects to the PostgreSQL instance and optionally creates the table
    /// if `create_table` is true and the table does not exist.
    pub async fn new(config: PgVectorConfig) -> AgentResult<Self> {
        let (client, connection) = tokio_postgres::connect(&config.connection_string, tokio_postgres::NoTls)
            .await
            .map_err(|e| AgentError::InitializationFailed(format!("Failed to connect to PostgreSQL: {e}")))?;

        // Spawn the connection handler
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("PostgreSQL connection error: {}", e);
            }
        });

        let store = Self {
            client,
            table_name: config.table_name,
            vector_dimensions: config.vector_dimensions,
            metric: SimilarityMetric::Cosine,
        };

        if config.create_table {
            store.ensure_table_exists().await?;
        }

        Ok(store)
    }

    /// Ensure the table exists, creating it if it does not.
    async fn ensure_table_exists(&self) -> AgentResult<()> {
        // Create the pgvector extension if not exists
        self.client.execute("CREATE EXTENSION IF NOT EXISTS vector;", &[])
            .await
            .map_err(|e| AgentError::InitializationFailed(format!("Failed to create pgvector extension: {e}")))?;

        // Create the table if not exists - use TEXT for id instead of UUID for simpler handling
        let query = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                embedding vector({}) NOT NULL,
                metadata TEXT DEFAULT '{{}}',
                created_at TIMESTAMP DEFAULT NOW()
            );",
            self.table_name, self.vector_dimensions
        );

        self.client.execute(&query, &[])
            .await
            .map_err(|e| AgentError::InitializationFailed(format!("Failed to create table: {e}")))?;

        // Create HNSW index for vector search (more accurate than IVFFlat for small datasets)
        // Note: For production with large datasets, consider tuning m and ef_construction parameters
        let index_name = format!("{}_embedding_idx", self.table_name);
        let index_query = format!(
            "CREATE INDEX IF NOT EXISTS {} ON {} USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 64);",
            index_name, self.table_name
        );
        let _ = self.client.execute(&index_query, &[]).await;

        Ok(())
    }

    /// Convert a DocumentChunk into a database row.
    fn chunk_to_row(&self, chunk: &DocumentChunk) -> AgentResult<(String, String, String, String)> {
        // Use the chunk ID directly as string
        let id = chunk.id.clone();

        // Convert embedding to pgvector text format: [0.1, 0.2, 0.3]
        let embedding = format!("[{}]", chunk.embedding.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", "));

        // Store metadata as JSON string
        let metadata = serde_json::to_string(&chunk.metadata)
            .unwrap_or_else(|_| "{}".to_string());

        Ok((id, chunk.text.clone(), embedding, metadata))
    }
}

/// Convert a f32 vector to pgvector-compatible bytes.
fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for &val in embedding {
        // pgvector stores f32 in little-endian format
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

/// Parse pgvector bytes back to f32 vector.
#[allow(dead_code)]
fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    let mut embedding = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let val = f32::from_le_bytes(chunk.try_into().unwrap());
        embedding.push(val);
    }
    embedding
}

#[async_trait]
impl VectorStore for PgVectorStore {
    async fn upsert(&mut self, chunk: DocumentChunk) -> AgentResult<()> {
        let len = chunk.embedding.len();
        if len != self.vector_dimensions {
            return Err(AgentError::InvalidInput(format!(
                "chunk embedding length {} does not match store dimension {}",
                len, self.vector_dimensions
            )));
        }

        let (id, content, embedding, metadata) = self.chunk_to_row(&chunk)?;

        // Use string interpolation for embedding since tokio-postgres can't serialize vector type
        let query = format!(
            "INSERT INTO {} (id, content, embedding, metadata) VALUES ('{}', '{}', '{}'::vector({}), '{}')
             ON CONFLICT (id) DO UPDATE SET content = EXCLUDED.content, embedding = EXCLUDED.embedding, metadata = EXCLUDED.metadata",
            self.table_name,
            id.replace("'", "''"),
            content.replace("'", "''"),
            embedding.replace("'", "''"),
            self.vector_dimensions,
            metadata.replace("'", "''")
        );

        self.client.execute(&query, &[]).await
            .map_err(|e| AgentError::Internal(format!("Pgvector upsert failed: {e}")))?;

        Ok(())
    }

    async fn upsert_batch(&mut self, chunks: Vec<DocumentChunk>) -> AgentResult<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        for chunk in &chunks {
            let len = chunk.embedding.len();
            if len != self.vector_dimensions {
                return Err(AgentError::InvalidInput(format!(
                    "chunk embedding length {} does not match store dimension {}",
                    len, self.vector_dimensions
                )));
            }
        }

        // Process each chunk
        for chunk in chunks {
            let (id, content, embedding, metadata) = self.chunk_to_row(&chunk)?;

            // Use string interpolation for embedding since tokio-postgres can't serialize vector type
            let query = format!(
                "INSERT INTO {} (id, content, embedding, metadata) VALUES ('{}', '{}', '{}'::vector({}), '{}')
                 ON CONFLICT (id) DO UPDATE SET content = EXCLUDED.content, embedding = EXCLUDED.embedding, metadata = EXCLUDED.metadata",
                self.table_name,
                id.replace("'", "''"),
                content.replace("'", "''"),
                embedding.replace("'", "''"),
                self.vector_dimensions,
                metadata.replace("'", "''")
            );

            self.client.execute(&query, &[]).await
                .map_err(|e| AgentError::Internal(format!("Pgvector batch upsert failed: {e}")))?;
        }

        Ok(())
    }

    async fn search(
        &self,
        query_embedding: &[f32],
        top_k: usize,
        _threshold: Option<f32>,
    ) -> AgentResult<Vec<SearchResult>> {
        if query_embedding.len() != self.vector_dimensions {
            return Err(AgentError::InvalidInput(format!(
                "query embedding length {} does not match store dimension {}",
                query_embedding.len(), self.vector_dimensions
            )));
        }

        // Convert embedding to pgvector text format: [0.1, 0.2, 0.3]
        let query_vector = format!("[{}]", query_embedding.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", "));

        // Use euclidean distance for search using the <-> operator
        // Use string interpolation since tokio-postgres can't serialize vector type
        let query = format!(
            "SELECT id, content, metadata, (embedding <=> '{}'::vector({})) as distance
             FROM {}
             ORDER BY embedding <=> '{}'::vector({})
             LIMIT {}",
            query_vector,
            self.vector_dimensions,
            self.table_name,
            query_vector,
            self.vector_dimensions,
            top_k
        );

        let rows = self.client.query(&query, &[]).await
            .map_err(|e| AgentError::Internal(format!("Pgvector search failed: {e}")))?;

        let mut results = Vec::new();
        for row in rows {
            let id: String = row.get(0);
            let content: String = row.get(1);
            let metadata_str: Option<String> = row.get(2);
            let distance: f64 = row.get(3);

            // Convert distance to similarity score (1 - distance for cosine)
            let score = 1.0 - distance;

            let metadata: HashMap<String, String> = metadata_str
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

            results.push(SearchResult {
                id,
                text: content,
                score: score as f32,
                metadata,
            });
        }

        Ok(results)
    }

    async fn delete(&mut self, id: &str) -> AgentResult<bool> {
        let query = format!("DELETE FROM {} WHERE id = $1", self.table_name);

        let affected = self.client.execute(&query, &[&id])
            .await
            .map_err(|e| AgentError::Internal(format!("Pgvector delete failed: {e}")))?;

        Ok(affected > 0)
    }

    async fn clear(&mut self) -> AgentResult<()> {
        let query = format!("DELETE FROM {}", self.table_name);

        self.client.execute(&query, &[])
            .await
            .map_err(|e| AgentError::Internal(format!("Pgvector clear failed: {e}")))?;

        Ok(())
    }

    async fn count(&self) -> AgentResult<usize> {
        let query = format!("SELECT COUNT(*) FROM {}", self.table_name);

        let count: i64 = self.client.query_one(&query, &[])
            .await
            .map_err(|e| AgentError::Internal(format!("Pgvector count failed: {e}")))?
            .get(0);

        Ok(count as usize)
    }

    fn similarity_metric(&self) -> SimilarityMetric {
        self.metric
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pgvector_config_creation() {
        let config = PgVectorConfig::new(
            "host=localhost user=postgres",
            "test_embeddings",
            1536,
        );
        assert_eq!(config.vector_dimensions, 1536);
        assert_eq!(config.table_name, "test_embeddings");
        assert!(config.create_table);
    }

    #[test]
    fn test_pgvector_config_with_create_table() {
        let config = PgVectorConfig::new(
            "host=localhost user=postgres",
            "test_embeddings",
            1536,
        ).with_create_table(false);
        
        assert!(!config.create_table);
    }

    #[test]
    fn test_embedding_to_bytes() {
        let embedding = vec![0.1, 0.2, 0.3];
        let bytes = embedding_to_bytes(&embedding);
        assert_eq!(bytes.len(), 12);
        
        // Check that it round-trips correctly
        let recovered = bytes_to_embedding(&bytes);
        assert_eq!(recovered.len(), 3);
        assert!((recovered[0] - 0.1).abs() < 0.001);
        assert!((recovered[1] - 0.2).abs() < 0.001);
        assert!((recovered[2] - 0.3).abs() < 0.001);
    }
}