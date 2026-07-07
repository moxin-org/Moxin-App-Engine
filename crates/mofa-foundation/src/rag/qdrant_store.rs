//! Qdrant-backed vector store implementation
//!
//! Provides a production-grade VectorStore backed by Qdrant vector database.
//! Suitable for large-scale RAG pipelines with persistent storage.

use async_trait::async_trait;
use mofa_kernel::agent::error::{AgentError, AgentResult};
use mofa_kernel::rag::{DocumentChunk, SearchResult, SimilarityMetric, VectorStore};
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    CountPointsBuilder, CreateCollectionBuilder, DeletePointsBuilder, Distance, PointStruct,
    PointsIdsList, QueryPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Reserved payload keys for internal storage.
const PAYLOAD_KEY_ORIGINAL_ID: &str = "_original_id";
const PAYLOAD_KEY_TEXT: &str = "_text";
const PAYLOAD_KEY_METADATA_PREFIX: &str = "meta_";

/// Configuration for connecting to a Qdrant instance.
pub struct QdrantConfig {
    /// Qdrant server URL (e.g., "http://localhost:6334")
    pub url: String,
    /// Optional API key for Qdrant Cloud or authenticated instances
    pub api_key: Option<String>,
    /// Name of the collection to use
    pub collection_name: String,
    /// Dimensionality of embedding vectors
    pub vector_dimensions: u64,
    /// Similarity metric to use for the collection
    pub metric: SimilarityMetric,
    /// Whether to create the collection if it does not exist
    pub create_collection: bool,
}

/// Qdrant-backed vector store.
///
/// Stores document chunks as Qdrant points with embeddings as vectors
/// and text/metadata as payload fields. String IDs from DocumentChunk
/// are mapped to u64 using a deterministic hash, with the original
/// string ID preserved in the payload for lossless retrieval.
pub struct QdrantVectorStore {
    client: Qdrant,
    collection_name: String,
    vector_dimensions: u64,
    metric: SimilarityMetric,
}

/// Convert a string ID to a u64 point ID for Qdrant.
///
/// Uses DefaultHasher for a deterministic mapping. The original string
/// ID is always stored in the point payload so retrieval is lossless.
fn string_id_to_u64(id: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
    let digest = Sha256::digest(id.as_bytes());
    u64::from_le_bytes(
        digest[..8]
            .try_into()
            .expect("SHA-256 output is at least 8 bytes"),
    )
}

/// Extract a string value from a Qdrant payload Value.
fn extract_string(val: &qdrant_client::qdrant::Value) -> Option<&str> {
    match &val.kind {
        Some(qdrant_client::qdrant::value::Kind::StringValue(s)) => Some(s.as_str()),
        _ => None,
    }
}

impl QdrantVectorStore {
    /// Create a new QdrantVectorStore from the given configuration.
    ///
    /// Connects to the Qdrant instance and optionally creates the collection
    /// if `create_collection` is true and the collection does not exist.
    pub async fn new(config: QdrantConfig) -> AgentResult<Self> {
        let mut builder = Qdrant::from_url(&config.url);
        if let Some(api_key) = config.api_key {
            builder = builder.api_key(api_key);
        }
        let client = builder.build().map_err(|e| {
            AgentError::InitializationFailed(format!("Qdrant connection failed: {e}"))
        })?;

        let store = Self {
            client,
            collection_name: config.collection_name,
            vector_dimensions: config.vector_dimensions,
            metric: config.metric,
        };

        if config.create_collection {
            store.ensure_collection_exists().await?;
        }

        Ok(store)
    }

    /// Convert SimilarityMetric to Qdrant's Distance enum.
    fn to_qdrant_distance(metric: SimilarityMetric) -> Distance {
        match metric {
            SimilarityMetric::Cosine => Distance::Cosine,
            SimilarityMetric::Euclidean => Distance::Euclid,
            SimilarityMetric::DotProduct => Distance::Dot,
            // Default to Cosine for any future variants
            _ => Distance::Cosine,
        }
    }

    /// Ensure the collection exists, creating it if it does not.
    async fn ensure_collection_exists(&self) -> AgentResult<()> {
        let exists = self
            .client
            .collection_exists(&self.collection_name)
            .await
            .map_err(|e| AgentError::Internal(format!("Qdrant collection check failed: {e}")))?;

        if !exists {
            let distance = Self::to_qdrant_distance(self.metric);
            self.client
                .create_collection(
                    CreateCollectionBuilder::new(&self.collection_name)
                        .vectors_config(VectorParamsBuilder::new(self.vector_dimensions, distance)),
                )
                .await
                .map_err(|e| {
                    AgentError::InitializationFailed(format!(
                        "Failed to create Qdrant collection '{}': {e}",
                        self.collection_name
                    ))
                })?;
        }

        Ok(())
    }

    /// Convert a DocumentChunk into a Qdrant PointStruct.
    fn chunk_to_point(chunk: &DocumentChunk) -> PointStruct {
        let point_id = string_id_to_u64(&chunk.id);

        let mut payload: HashMap<String, qdrant_client::qdrant::Value> = HashMap::new();
        payload.insert(PAYLOAD_KEY_ORIGINAL_ID.to_string(), chunk.id.clone().into());
        payload.insert(PAYLOAD_KEY_TEXT.to_string(), chunk.text.clone().into());

        for (key, value) in &chunk.metadata {
            payload.insert(
                format!("{PAYLOAD_KEY_METADATA_PREFIX}{key}"),
                value.clone().into(),
            );
        }

        PointStruct::new(point_id, chunk.embedding.clone(), payload)
    }

    /// Extract a SearchResult from a Qdrant ScoredPoint.
    fn scored_point_to_result(point: &qdrant_client::qdrant::ScoredPoint) -> SearchResult {
        let payload = &point.payload;

        let id = payload
            .get(PAYLOAD_KEY_ORIGINAL_ID)
            .and_then(extract_string)
            .unwrap_or_default()
            .to_string();

        let text = payload
            .get(PAYLOAD_KEY_TEXT)
            .and_then(extract_string)
            .unwrap_or_default()
            .to_string();

        let mut metadata = HashMap::new();
        for (key, val) in payload {
            if let Some(meta_key) = key.strip_prefix(PAYLOAD_KEY_METADATA_PREFIX)
                && let Some(s) = extract_string(val)
            {
                metadata.insert(meta_key.to_string(), s.to_string());
            }
        }

        SearchResult {
            id,
            text,
            score: point.score,
            metadata,
        }
    }
}

#[async_trait]
impl VectorStore for QdrantVectorStore {
    async fn upsert(&mut self, chunk: DocumentChunk) -> AgentResult<()> {
        let point = Self::chunk_to_point(&chunk);
        self.client
            .upsert_points(UpsertPointsBuilder::new(&self.collection_name, vec![point]).wait(true))
            .await
            .map_err(|e| AgentError::Internal(format!("Qdrant upsert failed: {e}")))?;
        Ok(())
    }

    async fn upsert_batch(&mut self, chunks: Vec<DocumentChunk>) -> AgentResult<()> {
        if chunks.is_empty() {
            return Ok(());
        }
        let points: Vec<PointStruct> = chunks.iter().map(Self::chunk_to_point).collect();
        self.client
            .upsert_points(UpsertPointsBuilder::new(&self.collection_name, points).wait(true))
            .await
            .map_err(|e| AgentError::Internal(format!("Qdrant batch upsert failed: {e}")))?;
        Ok(())
    }

    async fn search(
        &self,
        query_embedding: &[f32],
        top_k: usize,
        threshold: Option<f32>,
    ) -> AgentResult<Vec<SearchResult>> {
        // Request extra results when using threshold filtering since
        // Qdrant QueryPoints does not support score thresholds natively.
        let limit = if threshold.is_some() {
            (top_k * 2).clamp(100, 1000) as u64
        } else {
            top_k as u64
        };

        let response = self
            .client
            .query(
                QueryPointsBuilder::new(&self.collection_name)
                    .query(query_embedding.to_vec())
                    .limit(limit)
                    .with_payload(true),
            )
            .await
            .map_err(|e| AgentError::Internal(format!("Qdrant search failed: {e}")))?;

        let mut results: Vec<SearchResult> = response
            .result
            .iter()
            .map(Self::scored_point_to_result)
            .collect();

        if let Some(t) = threshold {
            results.retain(|r| r.score >= t);
        }

        results.truncate(top_k);
        Ok(results)
    }

    async fn delete(&mut self, id: &str) -> AgentResult<bool> {
        let point_id = string_id_to_u64(id);
        self.client
            .delete_points(
                DeletePointsBuilder::new(&self.collection_name)
                    .points(PointsIdsList {
                        ids: vec![point_id.into()],
                    })
                    .wait(true),
            )
            .await
            .map_err(|e| AgentError::Internal(format!("Qdrant delete failed: {e}")))?;
        Ok(true)
    }

    async fn clear(&mut self) -> AgentResult<()> {
        // Delete and recreate the collection to clear all points.
        let _ = self.client.delete_collection(&self.collection_name).await;

        self.ensure_collection_exists().await?;
        Ok(())
    }

    async fn count(&self) -> AgentResult<usize> {
        let result = self
            .client
            .count(CountPointsBuilder::new(&self.collection_name))
            .await
            .map_err(|e| AgentError::Internal(format!("Qdrant count failed: {e}")))?;
        Ok(result.result.map(|c| c.count as usize).unwrap_or(0))
    }

    fn similarity_metric(&self) -> SimilarityMetric {
        self.metric
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_id_to_u64_deterministic() {
        let id = "test-document-1";
        assert_eq!(string_id_to_u64(id), string_id_to_u64(id));
    }

    #[test]
    fn test_string_id_to_u64_different_ids() {
        assert_ne!(string_id_to_u64("a"), string_id_to_u64("b"));
    }

    #[test]
    fn test_to_qdrant_distance() {
        assert_eq!(
            QdrantVectorStore::to_qdrant_distance(SimilarityMetric::Cosine),
            Distance::Cosine
        );
        assert_eq!(
            QdrantVectorStore::to_qdrant_distance(SimilarityMetric::Euclidean),
            Distance::Euclid
        );
        assert_eq!(
            QdrantVectorStore::to_qdrant_distance(SimilarityMetric::DotProduct),
            Distance::Dot
        );
    }

    #[test]
    fn test_chunk_to_point_preserves_data() {
        let chunk = DocumentChunk::new("my-id", "hello world", vec![1.0, 2.0, 3.0])
            .with_metadata("source", "test.txt");
        let point = QdrantVectorStore::chunk_to_point(&chunk);

        // Verify point ID is the hash of the string ID
        assert_eq!(
            match point
                .id
                .as_ref()
                .unwrap()
                .point_id_options
                .as_ref()
                .unwrap()
            {
                qdrant_client::qdrant::point_id::PointIdOptions::Num(n) => *n,
                _ => 0,
            },
            string_id_to_u64("my-id")
        );

        // Verify payload has original ID
        let original_id = point.payload.get(PAYLOAD_KEY_ORIGINAL_ID);
        assert!(original_id.is_some());

        // Verify payload has text
        let text = point.payload.get(PAYLOAD_KEY_TEXT);
        assert!(text.is_some());

        // Verify metadata is stored with prefix
        let source = point.payload.get("meta_source");
        assert!(source.is_some());
    }

    #[test]
    fn test_qdrant_config_creation() {
        let config = QdrantConfig {
            url: "http://localhost:6334".to_string(),
            api_key: None,
            collection_name: "test_collection".to_string(),
            vector_dimensions: 384,
            metric: SimilarityMetric::Cosine,
            create_collection: true,
        };
        assert_eq!(config.vector_dimensions, 384);
        assert_eq!(config.collection_name, "test_collection");
    }
}
