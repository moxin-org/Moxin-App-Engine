//! RAG Query Tool for MoFA Agents.
//!
//! Provides a tool that allows agents to query a vector store for relevant documents.
//! This integrates the existing RAG pipeline with the agent tool system.
//!
//! # Example
//!
//! ```rust,ignore
//! use mofa_foundation::agent::tools::rag::RagTool;
//! use mofa_foundation::rag::{InMemoryVectorStore, LlmEmbeddingAdapter, RagQueryConfig};
//! use std::sync::Arc;
//!
//! let store = Arc::new(tokio::sync::RwLock::new(InMemoryVectorStore::cosine()));
//! let embedder = Arc::new(LlmEmbeddingAdapter::new(client, config));
//! let rag_tool = RagTool::new(store, embedder);
//! ```

use async_trait::async_trait;
use mofa_kernel::agent::components::tool::{ToolInput, ToolMetadata, ToolResult};
use mofa_kernel::rag::VectorStore;
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;

use crate::agent::components::tool::SimpleTool;
use crate::rag::embedding_adapter::LlmEmbeddingAdapter;
use crate::rag::query_documents;
use crate::rag::vector_store::InMemoryVectorStore;
use crate::rag::RagQueryConfig;

/// Tool for querying documents via RAG (Retrieval-Augmented Generation).
///
/// This tool allows agents to search a vector store for relevant documents
/// based on a query string. It uses the embedded query to find the most
/// similar documents in the store.
///
/// # Input Schema
///
/// ```json
/// {
///     "query": "string",       // Required: The search query
///     "top_k": number          // Optional: Number of results to return (default: 5)
/// }
/// ```
///
/// # Output Schema
///
/// ```json
/// {
///     "results": [
///         {
///             "content": "string",  // Document content
///             "score": number        // Similarity score
///         }
///     ],
///     "combined_context": "string"  // All results joined together
/// }
/// ```
#[derive(Debug)]
pub struct RagTool<S: VectorStore> {
    store: Arc<tokio::sync::RwLock<S>>,
    embedder: Arc<LlmEmbeddingAdapter>,
}

impl<S: VectorStore> RagTool<S> {
    /// Create a new RagTool with the given vector store and embedder.
    pub fn new(store: Arc<tokio::sync::RwLock<S>>, embedder: Arc<LlmEmbeddingAdapter>) -> Self {
        Self { store, embedder }
    }
}

/// Output result for each retrieved chunk.
#[derive(Debug, Serialize)]
struct ChunkResult {
    content: String,
    score: f32,
}

/// Output from the RAG query tool.
#[derive(Debug, Serialize)]
struct RagToolOutput {
    results: Vec<ChunkResult>,
    combined_context: String,
}

impl<S: VectorStore> Default for RagTool<S> {
    fn default() -> Self {
        // This won't actually be used since we require explicit construction
        panic!("RagTool requires explicit store and embedder")
    }
}

#[async_trait]
impl<S: VectorStore + Send + Sync> SimpleTool for RagTool<S> {
    fn name(&self) -> &str {
        "rag_query"
    }

    fn description(&self) -> &str {
        "Query a document vector store for relevant context. \
         Use this tool to retrieve relevant documents or context from a \
         knowledge base. Input requires a 'query' string and optionally \
         'top_k' for number of results (default: 5)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query string to find relevant documents"
                },
                "top_k": {
                    "type": "number",
                    "description": "Optional: Number of results to return (default: 5)"
                }
            },
            "required": ["query"]
        })
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::new().with_category("retrieval")
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        // Get query parameter
        let query = match input.get_str("query") {
            Some(q) => q.to_string(),
            None => return ToolResult::failure("missing required parameter: query"),
        };

        let query = query.trim();
        if query.is_empty() {
            return ToolResult::failure("query must not be empty".to_string());
        }

        // Get optional top_k parameter
        let top_k = input
            .get_number("top_k")
            .map(|n| n as usize)
            .unwrap_or(5)
            .max(1);

        // Configure the query
        let config = RagQueryConfig::default().with_top_k(top_k);

        // Get the store (RwLockReadGuard doesn't return Result in Tokio)
        let store_read = self.store.read().await;

        // Execute the query
        let result = query_documents(&*store_read, &*self.embedder, query, &config).await;

        match result {
            Ok(retrieval_result) => {
                let results: Vec<ChunkResult> = retrieval_result
                    .chunks
                    .into_iter()
                    .map(|chunk| ChunkResult {
                        content: chunk.text,
                        score: chunk.score,
                    })
                    .collect();

                let output = RagToolOutput {
                    combined_context: retrieval_result.context,
                    results,
                };

                ToolResult::success(json!(output))
            }
            Err(e) => ToolResult::failure(format!("RAG query failed: {}", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rag::SimilarityMetric;

    // Helper to create a simple in-memory store for testing
    fn create_test_store() -> InMemoryVectorStore {
        InMemoryVectorStore::new(SimilarityMetric::Cosine)
    }

    // Note: Full tests would require a mock embedder, which is complex to set up.
    // The integration tests in the demo will validate the full functionality.
}
