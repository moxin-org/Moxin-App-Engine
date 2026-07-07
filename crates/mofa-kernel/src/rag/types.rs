//! RAG core data types
//!
//! Types used across the vector store and document chunking interfaces.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A full document used in retrieval and generation stages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Unique identifier for the entire document.
    pub id: String,
    /// Text content of the entire document.
    pub text: String,
    /// Arbitrary metadata (source file, page number, section title, etc.)
    pub metadata: HashMap<String, String>,
}

impl Document {
    /// Create a new document.
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            metadata: HashMap::new(),
        }
    }

    /// Add a metadata entry.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Retrieved document plus ranking metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredDocument {
    /// Retrieved document.
    pub document: Document,
    /// Relevance score (higher is better).
    pub score: f32,
    /// Optional retrieval stage/source label (e.g. sparse, dense, hybrid).
    pub source: Option<String>,
}

impl ScoredDocument {
    /// Create a new scored document.
    pub fn new(document: Document, score: f32, source: Option<String>) -> Self {
        Self {
            document,
            score,
            source,
        }
    }
}

/// Input passed to generation stage.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenerateInput {
    /// User query.
    pub query: String,
    /// RAG context passed to generator.
    pub context: Vec<Document>,
    /// Additional generation metadata.
    pub metadata: HashMap<String, String>,
}

impl GenerateInput {
    /// Create a generation input with query + context.
    pub fn new(query: impl Into<String>, context: Vec<Document>) -> Self {
        Self {
            query: query.into(),
            context,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata entry.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// A chunk of a document with its embedding vector and metadata.
///
/// This is the basic unit stored in a vector store. Documents are split
/// into chunks, each chunk is embedded into a vector, and stored along
/// with its text content and metadata for later retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentChunk {
    /// Unique identifier for this chunk
    pub id: String,
    /// The text content of this chunk
    pub text: String,
    /// The embedding vector for this chunk
    pub embedding: Vec<f32>,
    /// Arbitrary metadata (source file, page number, section title, etc.)
    pub metadata: HashMap<String, String>,
}

impl DocumentChunk {
    /// Create a new document chunk
    pub fn new(id: impl Into<String>, text: impl Into<String>, embedding: Vec<f32>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            embedding,
            metadata: HashMap::new(),
        }
    }

    /// Add a metadata entry
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Result returned from a vector similarity search.
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The id of the matched chunk
    pub id: String,
    /// The text content of the matched chunk
    pub text: String,
    /// Similarity score (higher is more similar, range depends on metric)
    pub score: f32,
    /// Metadata from the matched chunk
    pub metadata: HashMap<String, String>,
}

impl SearchResult {
    /// Create a new search result
    pub fn new(id: impl Into<String>, text: impl Into<String>, score: f32) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            score,
            metadata: HashMap::new(),
        }
    }

    /// Create from a document chunk with a score
    pub fn from_chunk(chunk: &DocumentChunk, score: f32) -> Self {
        Self {
            id: chunk.id.clone(),
            text: chunk.text.clone(),
            score,
            metadata: chunk.metadata.clone(),
        }
    }
}

/// Similarity metric used for comparing embedding vectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub enum SimilarityMetric {
    /// Cosine similarity (measures angle between vectors, range 0.0 to 1.0 for normalized vectors)
    #[default]
    Cosine,
    /// Euclidean distance (L2 distance, lower is more similar)
    Euclidean,
    /// Dot product (higher is more similar)
    DotProduct,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_chunk_creation() {
        let chunk = DocumentChunk::new("chunk-1", "hello world", vec![0.1, 0.2, 0.3])
            .with_metadata("source", "test.txt")
            .with_metadata("page", "1");

        assert_eq!(chunk.id, "chunk-1");
        assert_eq!(chunk.text, "hello world");
        assert_eq!(chunk.embedding.len(), 3);
        assert_eq!(chunk.metadata.get("source").unwrap(), "test.txt");
        assert_eq!(chunk.metadata.get("page").unwrap(), "1");
    }

    #[test]
    fn test_search_result_from_chunk() {
        let chunk = DocumentChunk::new("chunk-1", "hello world", vec![0.1, 0.2, 0.3])
            .with_metadata("source", "test.txt");

        let result = SearchResult::from_chunk(&chunk, 0.95);
        assert_eq!(result.id, "chunk-1");
        assert_eq!(result.text, "hello world");
        assert_eq!(result.score, 0.95);
        assert_eq!(result.metadata.get("source").unwrap(), "test.txt");
    }

    #[test]
    fn test_similarity_metric_default() {
        assert_eq!(SimilarityMetric::default(), SimilarityMetric::Cosine);
    }

    #[test]
    fn test_generate_input_with_metadata() {
        let doc = Document::new("doc-1", "hello");
        let input = GenerateInput::new("what is this?", vec![doc]).with_metadata("language", "en");

        assert_eq!(input.query, "what is this?");
        assert_eq!(input.context.len(), 1);
        assert_eq!(input.metadata.get("language").unwrap(), "en");
    }
}
