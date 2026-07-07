//! `mofa rag` command implementation.

use crate::CliError;
use mofa_foundation::rag::InMemoryVectorStore;
#[cfg(feature = "qdrant")]
use mofa_foundation::rag::{QdrantConfig, QdrantVectorStore};
use mofa_kernel::rag::{Document, DocumentChunk, SearchResult, SimilarityMetric, VectorStore};
use mofa_runtime::rag::{
    ChunkingStrategy, DeterministicEmbeddingProvider, LLMEmbeddingProvider, RagIngestionConfig, index_documents,
    merge_chunks, query_store, EmbeddingProvider
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use mofa_foundation::llm::{LLMClient, openai::{OpenAIProvider, OpenAIConfig}, ollama::{OllamaProvider, OllamaConfig}};

enum CliEmbeddingProvider {
    Deterministic(DeterministicEmbeddingProvider),
    Llm(LLMEmbeddingProvider),
}

#[async_trait::async_trait]
impl EmbeddingProvider for CliEmbeddingProvider {
    async fn embed(&self, inputs: &[String]) -> mofa_kernel::agent::types::error::GlobalResult<Vec<Vec<f32>>> {
        match self {
            Self::Deterministic(p) => p.embed(inputs).await,
            Self::Llm(p) => p.embed(inputs).await,
        }
    }

    fn dimensions(&self) -> usize {
        match self {
            Self::Deterministic(p) => p.dimensions(),
            Self::Llm(p) => p.dimensions(),
        }
    }
}

fn build_embedder(
    provider: &str,
    dimensions: usize,
    api_base: Option<&str>,
    api_key: Option<&str>,
    model: Option<&str>,
) -> Result<CliEmbeddingProvider, CliError> {
    match provider {
        "openai" => {
            let key = api_key.unwrap_or_default();
            let model_name = model.unwrap_or("text-embedding-ada-002");
            let mut ocfg = OpenAIConfig::new(key).with_model(model_name);
            if let Some(base) = api_base {
                ocfg = ocfg.with_base_url(base);
            }
            let provider_impl = OpenAIProvider::with_config(ocfg);
            let client = LLMClient::new(Arc::new(provider_impl));
            let embedder = LLMEmbeddingProvider::new(Arc::new(client), dimensions)
                .map_err(map_global_error)?;
            Ok(CliEmbeddingProvider::Llm(embedder))
        }
        "ollama" => {
            let model_name = model.unwrap_or("nomic-embed-text");
            let mut ocfg = OllamaConfig::new().with_model(model_name);
            if let Some(base) = api_base {
                ocfg = ocfg.with_base_url(base);
            }
            let provider_impl = OllamaProvider::with_config(ocfg);
            let client = LLMClient::new(Arc::new(provider_impl));
            let embedder = LLMEmbeddingProvider::new(Arc::new(client), dimensions)
                .map_err(map_global_error)?;
            Ok(CliEmbeddingProvider::Llm(embedder))
        }
        _ => {
            let embedder = DeterministicEmbeddingProvider::new(dimensions).map_err(map_global_error)?;
            Ok(CliEmbeddingProvider::Deterministic(embedder))
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct LocalRagIndex {
    dimensions: usize,
    chunks: Vec<DocumentChunk>,
}

#[allow(clippy::too_many_arguments)]
pub async fn run_index(
    input: Vec<PathBuf>,
    backend: &str,
    index_file: &Path,
    dimensions: usize,
    chunk_size: usize,
    chunk_overlap: usize,
    sentence_chunks: bool,
    qdrant_url: Option<&str>,
    qdrant_api_key: Option<&str>,
    qdrant_collection: &str,
    embedding_provider: &str,
    embedding_api_base: Option<&str>,
    embedding_api_key: Option<&str>,
    embedding_model: Option<&str>,
) -> Result<(), CliError> {
    let documents = load_documents(&input)?;
    if documents.is_empty() {
        return Err(CliError::Other("no input documents supplied".to_string()));
    }

    let embedder = build_embedder(embedding_provider, dimensions, embedding_api_base, embedding_api_key, embedding_model)?;
    let ingestion = RagIngestionConfig {
        chunk_size,
        chunk_overlap,
        chunking: if sentence_chunks {
            ChunkingStrategy::Sentences
        } else {
            ChunkingStrategy::Characters
        },
    };

    match backend {
        "in-memory" => {
            let mut index = load_local_index(index_file)?;
            if index.dimensions != 0 && index.dimensions != dimensions {
                return Err(CliError::Other(format!(
                    "index file dimensions ({}) do not match requested dimensions ({})",
                    index.dimensions, dimensions
                )));
            }

            let mut store = InMemoryVectorStore::cosine();
            if !index.chunks.is_empty() {
                store
                    .upsert_batch(index.chunks.clone())
                    .await
                    .map_err(map_agent_error)?;
            }

            let new_chunks = index_documents(&mut store, &documents, &embedder, &ingestion)
                .await
                .map_err(map_global_error)?;
            let new_count = new_chunks.len();

            index.dimensions = dimensions;
            index.chunks = merge_chunks(index.chunks, new_chunks);
            save_local_index(index_file, &index)?;

            println!(
                "Indexed {} chunks from {} documents into local index {} (total chunks: {}).",
                new_count,
                documents.len(),
                index_file.display(),
                index.chunks.len()
            );
        }
        "qdrant" => {
            let indexed = index_qdrant(
                &documents,
                &embedder,
                &ingestion,
                dimensions,
                qdrant_url,
                qdrant_api_key,
                qdrant_collection,
            )
            .await?;
            println!(
                "Indexed {} chunks from {} documents into Qdrant collection '{}'.",
                indexed,
                documents.len(),
                qdrant_collection
            );
        }
        other => {
            return Err(CliError::Other(format!(
                "unsupported backend '{}'; expected 'in-memory' or 'qdrant'",
                other
            )));
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_query(
    query: &str,
    backend: &str,
    index_file: &Path,
    dimensions: usize,
    top_k: usize,
    threshold: Option<f32>,
    qdrant_url: Option<&str>,
    qdrant_api_key: Option<&str>,
    qdrant_collection: &str,
    embedding_provider: &str,
    embedding_api_base: Option<&str>,
    embedding_api_key: Option<&str>,
    embedding_model: Option<&str>,
) -> Result<(), CliError> {
    let results = match backend {
        "in-memory" => {
            let index = load_local_index(index_file)?;
            if index.chunks.is_empty() {
                return Err(CliError::Other(format!(
                    "no indexed chunks found in {}; run 'mofa rag index' first",
                    index_file.display()
                )));
            }

            let mut store = InMemoryVectorStore::cosine();
            store
                .upsert_batch(index.chunks)
                .await
                .map_err(map_agent_error)?;

            let embedder = build_embedder(embedding_provider, index.dimensions, embedding_api_base, embedding_api_key, embedding_model)?;
            query_store(&store, &embedder, query, top_k, threshold)
                .await
                .map_err(map_global_error)?
        }
        "qdrant" => {
            let embedder = build_embedder(embedding_provider, dimensions, embedding_api_base, embedding_api_key, embedding_model)?;
            query_qdrant(
                query,
                &embedder,
                top_k,
                threshold,
                dimensions,
                qdrant_url,
                qdrant_api_key,
                qdrant_collection,
            )
            .await?
        }
        other => {
            return Err(CliError::Other(format!(
                "unsupported backend '{}'; expected 'in-memory' or 'qdrant'",
                other
            )));
        }
    };

    print_results(query, &results);
    Ok(())
}

#[cfg(feature = "qdrant")]
#[allow(clippy::too_many_arguments)]
async fn index_qdrant(
    documents: &[Document],
    embedder: &CliEmbeddingProvider,
    ingestion: &RagIngestionConfig,
    dimensions: usize,
    qdrant_url: Option<&str>,
    qdrant_api_key: Option<&str>,
    qdrant_collection: &str,
) -> Result<usize, CliError> {
    let url = qdrant_url
        .ok_or_else(|| CliError::Other("qdrant backend requires --qdrant-url".to_string()))?;
    let mut store = QdrantVectorStore::new(QdrantConfig {
        url: url.to_string(),
        api_key: qdrant_api_key.map(str::to_string),
        collection_name: qdrant_collection.to_string(),
        vector_dimensions: dimensions as u64,
        metric: SimilarityMetric::Cosine,
        create_collection: true,
    })
    .await
    .map_err(map_agent_error)?;

    let indexed = index_documents(&mut store, documents, embedder, ingestion)
        .await
        .map_err(map_global_error)?;
    Ok(indexed.len())
}

#[cfg(not(feature = "qdrant"))]
#[allow(clippy::too_many_arguments)]
async fn index_qdrant(
    _documents: &[Document],
    _embedder: &CliEmbeddingProvider,
    _ingestion: &RagIngestionConfig,
    _dimensions: usize,
    _qdrant_url: Option<&str>,
    _qdrant_api_key: Option<&str>,
    _qdrant_collection: &str,
) -> Result<usize, CliError> {
    Err(CliError::Other(
        "qdrant backend requires mofa-cli built with --features qdrant".to_string(),
    ))
}

#[cfg(feature = "qdrant")]
#[allow(clippy::too_many_arguments)]
async fn query_qdrant(
    query: &str,
    embedder: &CliEmbeddingProvider,
    top_k: usize,
    threshold: Option<f32>,
    dimensions: usize,
    qdrant_url: Option<&str>,
    qdrant_api_key: Option<&str>,
    qdrant_collection: &str,
) -> Result<Vec<SearchResult>, CliError> {
    let url = qdrant_url
        .ok_or_else(|| CliError::Other("qdrant backend requires --qdrant-url".to_string()))?;
    let store = QdrantVectorStore::new(QdrantConfig {
        url: url.to_string(),
        api_key: qdrant_api_key.map(str::to_string),
        collection_name: qdrant_collection.to_string(),
        vector_dimensions: dimensions as u64,
        metric: SimilarityMetric::Cosine,
        create_collection: true,
    })
    .await
    .map_err(map_agent_error)?;

    query_store(&store, embedder, query, top_k, threshold)
        .await
        .map_err(map_global_error)
}

#[cfg(not(feature = "qdrant"))]
#[allow(clippy::too_many_arguments)]
async fn query_qdrant(
    _query: &str,
    _embedder: &CliEmbeddingProvider,
    _top_k: usize,
    _threshold: Option<f32>,
    _dimensions: usize,
    _qdrant_url: Option<&str>,
    _qdrant_api_key: Option<&str>,
    _qdrant_collection: &str,
) -> Result<Vec<SearchResult>, CliError> {
    Err(CliError::Other(
        "qdrant backend requires mofa-cli built with --features qdrant".to_string(),
    ))
}

fn load_documents(paths: &[PathBuf]) -> Result<Vec<Document>, CliError> {
    let mut documents = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let text = fs::read_to_string(path)?;
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("document");
        let mut metadata = HashMap::new();
        metadata.insert("source_path".to_string(), path.display().to_string());
        documents.push(Document {
            id: format!("{stem}-{index}"),
            text,
            metadata,
        });
    }
    Ok(documents)
}

fn load_local_index(path: &Path) -> Result<LocalRagIndex, CliError> {
    if !path.exists() {
        return Ok(LocalRagIndex::default());
    }
    let data = fs::read(path)?;
    let index: LocalRagIndex = serde_json::from_slice(&data)?;
    Ok(index)
}

fn save_local_index(path: &Path, index: &LocalRagIndex) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let encoded = serde_json::to_vec_pretty(index)?;
    fs::write(path, encoded)?;
    Ok(())
}

fn print_results(query: &str, results: &[SearchResult]) {
    println!("RAG query: {}", query);
    if results.is_empty() {
        println!("No results found.");
        return;
    }

    for (index, result) in results.iter().enumerate() {
        println!(
            "{}. score={:.4} id={} text={}",
            index + 1,
            result.score,
            result.id,
            truncate_text(&result.text, 140)
        );
    }
}

fn truncate_text(input: &str, max_chars: usize) -> String {
    // Single pass: take up to max_chars characters. If we consumed the
    // entire input the string is short enough — return it directly.
    let truncated: String = input.chars().take(max_chars).collect();
    if truncated.len() == input.len() {
        return truncated;
    }
    // Input was longer; append ellipsis.
    let mut output = truncated;
    output.push_str("...");
    output
}

fn map_agent_error(err: mofa_kernel::agent::error::AgentError) -> CliError {
    CliError::Other(err.to_string())
}

fn map_global_error(err: mofa_kernel::agent::types::error::GlobalError) -> CliError {
    CliError::Other(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn in_memory_index_then_query_roundtrip() {
        let dir = tempdir().unwrap();
        let doc_path = dir.path().join("doc.txt");
        let index_path = dir.path().join("rag-index.json");
        fs::write(
            &doc_path,
            "MoFA uses a microkernel architecture with RAG support.",
        )
        .unwrap();

        run_index(
            vec![doc_path.clone()],
            "in-memory",
            &index_path,
            32,
            128,
            16,
            false,
            None,
            None,
            "unused",
            "deterministic",
            None,
            None,
            None,
        )
        .await
        .unwrap();

        assert!(index_path.exists());
        run_query(
            "microkernel architecture",
            "in-memory",
            &index_path,
            32,
            3,
            None,
            None,
            None,
            "unused",
            "deterministic",
            None,
            None,
            None,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn in_memory_query_without_index_fails() {
        let dir = tempdir().unwrap();
        let missing_index = dir.path().join("missing.json");

        let err = run_query(
            "anything",
            "in-memory",
            &missing_index,
            32,
            3,
            None,
            None,
            None,
            "unused",
            "deterministic",
            None,
            None,
            None,
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("run 'mofa rag index' first"));
    }
}
