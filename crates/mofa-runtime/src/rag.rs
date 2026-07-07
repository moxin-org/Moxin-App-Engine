//! Runtime-level RAG indexing and query hooks.
//!
//! This module provides a concrete ingestion-to-retrieval path that can be
//! reused by CLI and higher-level runtime integrations without binding the
//! runtime crate to a specific vector database provider.

use async_trait::async_trait;
use mofa_kernel::agent::types::error::{GlobalError, GlobalResult};
use mofa_kernel::rag::{Document, DocumentChunk, ScoredDocument, SearchResult, VectorStore};
use std::collections::HashMap;

/// Embedding provider abstraction used by runtime RAG hooks.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embeds a batch of input texts.
    async fn embed(&self, inputs: &[String]) -> GlobalResult<Vec<Vec<f32>>>;

    /// Returns embedding dimensionality.
    fn dimensions(&self) -> usize;
}

/// Deterministic local embedder for tests/dev and predictable CI behavior.
#[derive(Debug, Clone)]
pub struct DeterministicEmbeddingProvider {
    dimensions: usize,
}

impl DeterministicEmbeddingProvider {
    /// Creates a deterministic embedder with the given dimensions.
    pub fn new(dimensions: usize) -> GlobalResult<Self> {
        if dimensions == 0 {
            return Err(GlobalError::Runtime(
                "embedding dimensions must be greater than 0".to_string(),
            ));
        }
        Ok(Self { dimensions })
    }

    fn embed_one(&self, text: &str) -> Vec<f32> {
        let mut embedding = vec![0.0_f32; self.dimensions];
        for (index, byte) in text.bytes().enumerate() {
            embedding[index % self.dimensions] += byte as f32 / 255.0;
        }

        let norm = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for value in &mut embedding {
                *value /= norm;
            }
        }
        embedding
    }
}

#[async_trait]
impl EmbeddingProvider for DeterministicEmbeddingProvider {
    async fn embed(&self, inputs: &[String]) -> GlobalResult<Vec<Vec<f32>>> {
        Ok(inputs.iter().map(|input| self.embed_one(input)).collect())
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

/// Chunking strategy used during document ingestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkingStrategy {
    /// Character-window chunking with overlap.
    Characters,
    /// Sentence-oriented chunking.
    Sentences,
}

/// Runtime RAG ingestion options.
#[derive(Debug, Clone)]
pub struct RagIngestionConfig {
    /// Max chunk size in characters.
    pub chunk_size: usize,
    /// Overlap between adjacent chunks in characters.
    pub chunk_overlap: usize,
    /// Chunking strategy.
    pub chunking: ChunkingStrategy,
}

impl Default for RagIngestionConfig {
    fn default() -> Self {
        Self {
            chunk_size: 512,
            chunk_overlap: 64,
            chunking: ChunkingStrategy::Characters,
        }
    }
}

/// Index documents into the provided vector store.
///
/// Returns the chunks generated and upserted, so callers can persist/inspect
/// the indexed units when needed.
pub async fn index_documents<S, E>(
    store: &mut S,
    documents: &[Document],
    embedder: &E,
    config: &RagIngestionConfig,
) -> GlobalResult<Vec<DocumentChunk>>
where
    S: VectorStore,
    E: EmbeddingProvider,
{
    if config.chunk_size == 0 {
        return Err(GlobalError::Runtime(
            "chunk_size must be greater than 0".to_string(),
        ));
    }

    let mut pending = Vec::new();
    let mut texts = Vec::new();

    for document in documents {
        let chunks = chunk_text(&document.text, config);
        for (chunk_index, chunk_text) in chunks.into_iter().enumerate() {
            let mut metadata = document.metadata.clone();
            metadata.insert("source_doc_id".to_string(), document.id.clone());
            metadata.insert("chunk_index".to_string(), chunk_index.to_string());

            pending.push((format!("{}::chunk-{}", document.id, chunk_index), metadata));
            texts.push(chunk_text);
        }
    }

    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let embeddings = embedder.embed(&texts).await?;
    if embeddings.len() != texts.len() {
        return Err(GlobalError::Runtime(format!(
            "embedding count mismatch: expected {}, got {}",
            texts.len(),
            embeddings.len()
        )));
    }

    let expected_dim = embedder.dimensions();
    let mut chunks = Vec::with_capacity(texts.len());
    for ((id, metadata), (text, embedding)) in pending
        .into_iter()
        .zip(texts.into_iter().zip(embeddings.into_iter()))
    {
        if embedding.len() != expected_dim {
            return Err(GlobalError::Runtime(format!(
                "embedding dimension mismatch: expected {}, got {}",
                expected_dim,
                embedding.len()
            )));
        }

        chunks.push(DocumentChunk {
            id,
            text,
            embedding,
            metadata,
        });
    }

    store.upsert_batch(chunks.clone()).await?;
    Ok(chunks)
}

/// Query a vector store by embedding the query with the supplied provider.
pub async fn query_store<S, E>(
    store: &S,
    embedder: &E,
    query: &str,
    top_k: usize,
    threshold: Option<f32>,
) -> GlobalResult<Vec<SearchResult>>
where
    S: VectorStore,
    E: EmbeddingProvider,
{
    if top_k == 0 {
        return Err(GlobalError::Runtime(
            "top_k must be greater than 0".to_string(),
        ));
    }

    let query = query.trim();
    if query.is_empty() {
        return Err(GlobalError::Runtime("query must not be empty".to_string()));
    }

    let query_embeddings = embedder.embed(&[query.to_string()]).await?;
    let query_embedding = query_embeddings.first().ok_or_else(|| {
        GlobalError::Runtime("embedding provider returned no vectors".to_string())
    })?;
    if query_embedding.len() != embedder.dimensions() {
        return Err(GlobalError::Runtime(format!(
            "query embedding dimension mismatch: expected {}, got {}",
            embedder.dimensions(),
            query_embedding.len()
        )));
    }

    let results = store.search(query_embedding, top_k, threshold).await?;
    Ok(results)
}

/// Convert search results into scored documents for `RagPipeline` retrieval.
pub fn to_scored_documents(results: Vec<SearchResult>, source: &str) -> Vec<ScoredDocument> {
    results
        .into_iter()
        .map(|result| ScoredDocument {
            document: Document {
                id: result.id,
                text: result.text,
                metadata: result.metadata,
            },
            score: result.score,
            source: Some(source.to_string()),
        })
        .collect()
}

/// Parses a PDF file and extracts its readable text.
pub fn parse_pdf_document(path: &std::path::Path) -> GlobalResult<String> {
    let bytes = std::fs::read(path).map_err(|e| {
        GlobalError::Runtime(format!("failed to read pdf {}: {}", path.display(), e))
    })?;
    pdf_extract::extract_text_from_mem(&bytes)
        .map_err(|e| GlobalError::Runtime(format!("failed to parse pdf {}: {}", path.display(), e)))
}

/// Parses an HTML file and extracts its readable text.
pub fn parse_html_document(path: &std::path::Path) -> GlobalResult<String> {
    let html_bytes = std::fs::read(path).map_err(|e| {
        GlobalError::Runtime(format!("failed to read html {}: {}", path.display(), e))
    })?;
    html2text::from_read(html_bytes.as_slice(), 10000).map_err(|e| {
        GlobalError::Runtime(format!("failed to parse html {}: {:?}", path.display(), e))
    })
}

fn chunk_text(text: &str, config: &RagIngestionConfig) -> Vec<String> {
    match config.chunking {
        ChunkingStrategy::Characters => {
            chunk_by_chars(text, config.chunk_size, config.chunk_overlap)
        }
        ChunkingStrategy::Sentences => chunk_by_sentences(text, config.chunk_size),
    }
}

fn chunk_by_chars(text: &str, chunk_size: usize, chunk_overlap: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    if chars.len() <= chunk_size {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let step = chunk_size.saturating_sub(chunk_overlap).max(1);
    let mut start = 0;

    while start < chars.len() {
        let end = (start + chunk_size).min(chars.len());
        chunks.push(chars[start..end].iter().collect::<String>());
        if end >= chars.len() {
            break;
        }
        start += step;
    }

    chunks
}

fn chunk_by_sentences(text: &str, chunk_size: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    if text.len() <= chunk_size {
        return vec![text.to_string()];
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();

    for (index, character) in chars.iter().enumerate() {
        current.push(*character);
        let is_end = (*character == '.' || *character == '?' || *character == '!')
            && (index + 1 == chars.len() || chars[index + 1].is_whitespace());
        if is_end {
            segments.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }

    let mut chunks = Vec::new();
    let mut current_chunk = String::new();
    for segment in segments {
        if current_chunk.is_empty() {
            current_chunk = segment;
            continue;
        }
        if current_chunk.len() + segment.len() <= chunk_size {
            current_chunk.push_str(&segment);
        } else {
            chunks.push(current_chunk.trim().to_string());
            current_chunk = segment;
        }
    }
    if !current_chunk.is_empty() {
        chunks.push(current_chunk.trim().to_string());
    }

    chunks
}

/// Merge chunks by id, using later entries as replacements.
pub fn merge_chunks(
    existing: Vec<DocumentChunk>,
    new_chunks: Vec<DocumentChunk>,
) -> Vec<DocumentChunk> {
    let mut merged = HashMap::new();
    for chunk in existing {
        merged.insert(chunk.id.clone(), chunk);
    }
    for chunk in new_chunks {
        merged.insert(chunk.id.clone(), chunk);
    }

    let mut values: Vec<DocumentChunk> = merged.into_values().collect();
    values.sort_by(|a, b| a.id.cmp(&b.id));
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use mofa_kernel::agent::error::{AgentError, AgentResult};
    use mofa_kernel::rag::SimilarityMetric;

    struct TestStore {
        chunks: HashMap<String, DocumentChunk>,
        dimensions: Option<usize>,
    }

    impl TestStore {
        fn new() -> Self {
            Self {
                chunks: HashMap::new(),
                dimensions: None,
            }
        }
    }

    #[async_trait]
    impl VectorStore for TestStore {
        async fn upsert(&mut self, chunk: DocumentChunk) -> AgentResult<()> {
            if let Some(dim) = self.dimensions {
                if chunk.embedding.len() != dim {
                    return Err(AgentError::InvalidInput(format!(
                        "dimension mismatch: expected {}, got {}",
                        dim,
                        chunk.embedding.len()
                    )));
                }
            } else {
                self.dimensions = Some(chunk.embedding.len());
            }

            self.chunks.insert(chunk.id.clone(), chunk);
            Ok(())
        }

        async fn search(
            &self,
            query_embedding: &[f32],
            top_k: usize,
            threshold: Option<f32>,
        ) -> AgentResult<Vec<SearchResult>> {
            let mut results = self
                .chunks
                .values()
                .map(|chunk| {
                    let score = chunk
                        .embedding
                        .iter()
                        .zip(query_embedding.iter())
                        .map(|(lhs, rhs)| lhs * rhs)
                        .sum::<f32>();
                    SearchResult::from_chunk(chunk, score)
                })
                .filter(|result| threshold.map(|min| result.score >= min).unwrap_or(true))
                .collect::<Vec<_>>();

            results.sort_by(|lhs, rhs| {
                rhs.score
                    .partial_cmp(&lhs.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            results.truncate(top_k);
            Ok(results)
        }

        async fn delete(&mut self, id: &str) -> AgentResult<bool> {
            Ok(self.chunks.remove(id).is_some())
        }

        async fn clear(&mut self) -> AgentResult<()> {
            self.chunks.clear();
            self.dimensions = None;
            Ok(())
        }

        async fn count(&self) -> AgentResult<usize> {
            Ok(self.chunks.len())
        }

        fn similarity_metric(&self) -> SimilarityMetric {
            SimilarityMetric::DotProduct
        }
    }

    fn doc(id: &str, text: &str) -> Document {
        Document {
            id: id.to_string(),
            text: text.to_string(),
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn index_and_query_roundtrip() {
        let mut store = TestStore::new();
        let embedder = DeterministicEmbeddingProvider::new(32).unwrap();
        let config = RagIngestionConfig::default();

        let indexed = index_documents(
            &mut store,
            &[doc("doc-rust", "Rust microkernel architecture for agents.")],
            &embedder,
            &config,
        )
        .await
        .unwrap();

        assert!(!indexed.is_empty());
        assert_eq!(store.count().await.unwrap(), indexed.len());

        let results = query_store(&store, &embedder, "microkernel agents", 3, None)
            .await
            .unwrap();
        assert!(!results.is_empty());
        assert!(
            results
                .iter()
                .any(|result| result.id.starts_with("doc-rust"))
        );
    }

    #[tokio::test]
    async fn sentence_chunking_generates_multiple_chunks() {
        let mut store = TestStore::new();
        let embedder = DeterministicEmbeddingProvider::new(16).unwrap();
        let config = RagIngestionConfig {
            chunk_size: 30,
            chunk_overlap: 0,
            chunking: ChunkingStrategy::Sentences,
        };

        let indexed = index_documents(
            &mut store,
            &[doc(
                "doc-1",
                "First sentence. Second sentence. Third sentence.",
            )],
            &embedder,
            &config,
        )
        .await
        .unwrap();

        assert!(indexed.len() >= 2);
    }

    #[tokio::test]
    async fn query_top_k_zero_is_rejected() {
        let store = TestStore::new();
        let embedder = DeterministicEmbeddingProvider::new(8).unwrap();

        let err = query_store(&store, &embedder, "test", 0, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("top_k must be greater than 0"));
    }

    #[test]
    fn merge_chunks_prefers_new_versions() {
        let old = DocumentChunk::new("a", "old", vec![0.1, 0.2]);
        let new = DocumentChunk::new("a", "new", vec![0.3, 0.4]);

        let merged = merge_chunks(vec![old], vec![new]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "new");
    }

    #[test]
    fn parse_html_document_extracts_text() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "test_doc_{}.html",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, r#"<html><body><h1>Hello HTML</h1><p>Test <a href="http://a.com">link</a></p><script>ignore this</script></body></html>"#).unwrap();

        let text = super::parse_html_document(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        println!("EXTRACTED HTML:\n{}", text);
        assert!(text.contains("Hello HTML"));
        assert!(text.contains("Test"));
        assert!(text.contains("link"));
        assert!(!text.contains("ignore this"));
    }

    #[test]
    fn parse_pdf_document_fails_gracefully_on_invalid_pdf() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "test_doc_{}.pdf",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"not a valid pdf file").unwrap();

        let result = super::parse_pdf_document(&path);
        let _ = std::fs::remove_file(&path);

        // We just want to ensure it doesn't panic and returns a GlobalError
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("failed to parse pdf")
        );
    }
}
