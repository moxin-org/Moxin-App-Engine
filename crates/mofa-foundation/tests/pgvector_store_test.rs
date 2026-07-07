//! Tests for PgVectorStore
//!
//! These tests require a PostgreSQL instance with pgvector extension.
//! Run with: cargo test -p mofa-foundation --features pgvector pgvector_store_test

#[cfg(test)]
mod tests {
    use mofa_foundation::rag::{DocumentChunk, PgVectorConfig, PgVectorStore, VectorStore};
    use std::collections::HashMap;

    /// Integration test for inserting and searching documents.
    /// Requires: PostgreSQL with pgvector extension running on localhost:5432
    /// User: postgres, Password: postgres, DB: vectordb
    #[tokio::test]
    #[ignore] // Requires PostgreSQL with pgvector
    async fn test_pgvector_insert_and_search() {
        // Setup: Create config and store
        let config = PgVectorConfig::new(
            "host=localhost port=5432 user=postgres password=postgres dbname=vectordb",
            "test_embeddings",
            3, // Using 3-dimensional vectors for simplicity
        )
        .with_create_table(true);

        let mut store = PgVectorStore::new(config).await.expect("Failed to create store");

        // Insert test documents with known embeddings
        // These are simple 3D vectors for testing
        let doc1 = DocumentChunk::new(
            "550e8400-e29b-41d4-a716-446655440000",
            "The quick brown fox jumps over the lazy dog",
            vec![0.1, 0.2, 0.3],
        )
        .with_metadata("source", "test1.txt");

        let doc2 = DocumentChunk::new(
            "550e8400-e29b-41d4-a716-446655440001",
            "A fast canine leaps over a sleeping canine",
            vec![0.2, 0.3, 0.4], // Similar to doc1
        )
        .with_metadata("source", "test2.txt");

        let doc3 = DocumentChunk::new(
            "550e8400-e29b-41d4-a716-446655440002",
            "Machine learning is a subset of artificial intelligence",
            vec![0.9, 0.8, 0.7], // Different from doc1 and doc2
        )
        .with_metadata("source", "test3.txt");

        // Insert documents
        store.upsert(doc1).await.expect("Failed to insert doc1");
        store.upsert(doc2).await.expect("Failed to insert doc2");
        store.upsert(doc3).await.expect("Failed to insert doc3");

        // Verify count
        let count = store.count().await.expect("Failed to count");
        assert_eq!(count, 3, "Should have 3 documents");

        // Search with a query vector similar to doc1 and doc2
        let query = vec![0.15, 0.25, 0.35];
        let results = store
            .search(&query, 2, None)
            .await
            .expect("Search failed");

        // Verify we got results
        assert!(!results.is_empty(), "Should have search results");
        assert_eq!(results.len(), 2, "Should return top 2 results");

        // Verify top result is one of the similar documents (doc1 or doc2)
        // The closest should be doc1 or doc2 since the query is similar
        let top_result = &results[0];
        assert!(
            top_result.id == "550e8400-e29b-41d4-a716-446655440000"
                || top_result.id == "550e8400-e29b-41d4-a716-446655440001",
            "Top result should be doc1 or doc2, got: {}",
            top_result.id
        );

        // Verify the top result has high similarity
        assert!(
            top_result.score > 0.9,
            "Top result should have high similarity, got: {}",
            top_result.score
        );
    }

    /// Test for batch insert functionality.
    #[tokio::test]
    #[ignore] // Requires PostgreSQL with pgvector
    async fn test_pgvector_batch_insert() {
        let config = PgVectorConfig::new(
            "host=localhost port=5432 user=postgres password=postgres dbname=vectordb",
            "test_batch_embeddings",
            4,
        )
        .with_create_table(true);

        let mut store = PgVectorStore::new(config).await.expect("Failed to create store");

        // Create multiple documents
        let docs: Vec<DocumentChunk> = (0..10)
            .map(|i| {
                DocumentChunk::new(
                    format!("doc-{:03}", i),
                    format!("Document number {}", i),
                    vec![0.1 * i as f32, 0.2 * i as f32, 0.3 * i as f32, 0.4 * i as f32],
                )
            })
            .collect();

        // Batch insert
        store
            .upsert_batch(docs)
            .await
            .expect("Batch insert failed");

        // Verify count
        let count = store.count().await.expect("Failed to count");
        assert_eq!(count, 10, "Should have 10 documents");
    }

    /// Test for delete functionality.
    #[tokio::test]
    #[ignore] // Requires PostgreSQL with pgvector
    async fn test_pgvector_delete() {
        let config = PgVectorConfig::new(
            "host=localhost port=5432 user=postgres password=postgres dbname=vectordb",
            "test_delete_embeddings",
            3,
        )
        .with_create_table(true);

        let mut store = PgVectorStore::new(config).await.expect("Failed to create store");

        // Insert a document
        let doc = DocumentChunk::new(
            "delete-test-doc",
            "This document will be deleted",
            vec![0.1, 0.2, 0.3],
        );
        store.upsert(doc).await.expect("Failed to insert");

        // Verify it exists
        let count = store.count().await.expect("Failed to count");
        assert_eq!(count, 1);

        // Delete it
        let deleted = store
            .delete("delete-test-doc")
            .await
            .expect("Delete failed");
        assert!(deleted, "Should have deleted the document");

        // Verify it's gone
        let count = store.count().await.expect("Failed to count");
        assert_eq!(count, 0, "Should have 0 documents");
    }

    /// Test for clear functionality.
    #[tokio::test]
    #[ignore] // Requires PostgreSQL with pgvector
    async fn test_pgvector_clear() {
        let config = PgVectorConfig::new(
            "host=localhost port=5432 user=postgres password=postgres dbname=vectordb",
            "test_clear_embeddings",
            3,
        )
        .with_create_table(true);

        let mut store = PgVectorStore::new(config).await.expect("Failed to create store");

        // Insert some documents
        for i in 0..5 {
            let doc = DocumentChunk::new(
                format!("doc-{}", i),
                format!("Document {}", i),
                vec![0.1, 0.2, 0.3],
            );
            store.upsert(doc).await.expect("Failed to insert");
        }

        // Verify count
        let count = store.count().await.expect("Failed to count");
        assert_eq!(count, 5);

        // Clear all
        store.clear().await.expect("Clear failed");

        // Verify empty
        let count = store.count().await.expect("Failed to count");
        assert_eq!(count, 0, "Should have 0 documents after clear");
    }

    /// Test for similarity metric.
    #[tokio::test]
    #[ignore] // Requires PostgreSQL with pgvector
    async fn test_pgvector_similarity_metric() {
        let config = PgVectorConfig::new(
            "host=localhost port=5432 user=postgres password=postgres dbname=vectordb",
            "test_metric_embeddings",
            3,
        )
        .with_create_table(true);

        let store = PgVectorStore::new(config).await.expect("Failed to create store");

        // Verify cosine similarity is used
        assert_eq!(
            store.similarity_metric(),
            mofa_kernel::rag::SimilarityMetric::Cosine,
            "Should use Cosine similarity"
        );
    }
}