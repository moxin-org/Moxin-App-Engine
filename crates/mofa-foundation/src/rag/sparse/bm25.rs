//! BM25 Retriever implementation
//!
//! Provides a Retriever implementation using BM25 sparse retrieval.

use async_trait::async_trait;
use mofa_kernel::agent::error::AgentResult;
use mofa_kernel::rag::{Retriever, ScoredDocument};

use super::index::Bm25Index;

/// BM25 Retriever for sparse document retrieval.
///
/// This retriever uses the BM25 ranking algorithm to find relevant documents
/// based on keyword matching.
#[derive(Clone)]
pub struct Bm25Retriever {
    /// The underlying BM25 index
    index: Bm25Index,
}

impl Bm25Retriever {
    /// Create a new BM25 retriever with default parameters.
    pub fn new() -> Self {
        Self {
            index: Bm25Index::new(),
        }
    }

    /// Create a new BM25 retriever with custom BM25 parameters.
    pub fn with_params(k1: f64, b: f64) -> Self {
        Self {
            index: Bm25Index::with_params(k1, b),
        }
    }

    /// Index documents for retrieval.
    pub fn index_documents(
        &mut self,
        documents: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) {
        self.index.index_documents(documents);
    }

    /// Index a single document.
    pub fn index_document(&mut self, id: impl Into<String>, text: impl Into<String>) {
        self.index.index_document(id, text);
    }

    /// Get the number of indexed documents.
    pub fn num_docs(&self) -> usize {
        self.index.num_docs()
    }

    /// Clear all indexed documents.
    pub fn clear(&mut self) {
        self.index.clear();
    }

    /// Retrieve documents matching the query using BM25 scoring.
    fn retrieve_sync(&self, query: &str, top_k: usize) -> Vec<ScoredDocument> {
        if self.index.num_docs() == 0 || query.trim().is_empty() {
            return Vec::new();
        }

        // Score all documents against the query
        let mut scored_docs: Vec<ScoredDocument> = self
            .index
            .document_ids()
            .iter()
            .filter_map(|id| {
                let score = self.index.score(id, query);
                let text = self.index.get_document(id)?.clone();
                Some(ScoredDocument::new(
                    mofa_kernel::rag::Document::new((*id).clone(), text),
                    score as f32,
                    Some("bm25".to_string()),
                ))
            })
            .collect();

        // Sort by score in descending order
        scored_docs.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Return top_k results
        scored_docs.truncate(top_k);
        scored_docs
    }
}

impl Default for Bm25Retriever {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Retriever for Bm25Retriever {
    /// Retrieve the top-k most relevant documents for the given query.
    async fn retrieve(&self, query: &str, top_k: usize) -> AgentResult<Vec<ScoredDocument>> {
        // Use the synchronous version directly
        Ok(self.retrieve_sync(query, top_k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_retrieve_empty_index() {
        let retriever = Bm25Retriever::new();
        let results = retriever.retrieve("query", 5).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_retrieve_with_documents() {
        let mut retriever = Bm25Retriever::new();
        retriever.index_documents(vec![
            ("doc1", "Rust is a systems programming language"),
            ("doc2", "Python is great for machine learning"),
        ]);

        let results = retriever.retrieve("systems programming", 2).await.unwrap();

        assert_eq!(results.len(), 2);
        // doc1 should rank higher for "systems programming"
        assert_eq!(results[0].document.id, "doc1");
    }

    #[tokio::test]
    async fn test_top_k_filtering() {
        let mut retriever = Bm25Retriever::new();
        retriever.index_documents(vec![
            ("doc1", "Rust programming"),
            ("doc2", "Python programming"),
            ("doc3", "JavaScript programming"),
            ("doc4", "Go programming"),
        ]);

        let results = retriever.retrieve("programming", 2).await.unwrap();

        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_empty_query() {
        let mut retriever = Bm25Retriever::new();
        retriever.index_document("doc1", "Some text");

        let results = retriever.retrieve("", 5).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_no_matching_documents() {
        let mut retriever = Bm25Retriever::new();
        retriever.index_document("doc1", "Rust programming language");

        let results = retriever
            .retrieve("python machine learning", 5)
            .await
            .unwrap();
        // Documents are returned with score 0 when no match
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 0.0);
    }

    #[tokio::test]
    async fn test_ranking_correctness() {
        let mut retriever = Bm25Retriever::new();
        retriever.index_documents(vec![
            ("doc1", "Rust is a systems programming language"),
            ("doc2", "Python is great for machine learning"),
        ]);

        let results = retriever.retrieve("systems programming", 2).await.unwrap();

        // Verify that doc1 ranks higher for this query
        assert_eq!(results.len(), 2);
        assert!(results[0].score >= results[1].score);
        assert_eq!(results[0].document.id, "doc1");
    }
}
