//! RAG (Retrieval-Augmented Generation) implementations
//!
//! Provides concrete implementations of the vector store trait defined
//! in mofa-kernel, along with utilities for document chunking.

pub mod chunker;
pub mod default_reranker;
pub mod embedding_adapter;
pub mod indexing;
pub mod loaders;
pub mod pipeline_adapters;
pub mod recursive_chunker;
pub mod retrieval;
pub mod score_reranker;
pub mod similarity;
pub mod streaming_generator;
pub mod vector_store;

#[cfg(feature = "qdrant")]
pub mod qdrant_store;

#[cfg(feature = "pgvector")]
pub mod storage;

pub use chunker::{ChunkConfig, TextChunker};
pub use default_reranker::IdentityReranker;
pub use embedding_adapter::{
    EmbeddingAdapterError, LlmEmbeddingAdapter, RagEmbeddingConfig, RagEmbeddingProvider,
    deterministic_chunk_id,
};
pub use indexing::{
    IndexDocument, IndexMode, IndexResult, RagIndexConfig, RagOrchestrationError, index_documents,
};
pub use loaders::{DocumentLoader, LoaderError, LoaderResult, MarkdownLoader, TextLoader};
pub use pipeline_adapters::{InMemoryRetriever, SimpleGenerator};
pub use recursive_chunker::{RecursiveChunkConfig, RecursiveChunker};
pub use retrieval::{RagQueryConfig, RetrievalResult, RetrievedChunk, query_documents};
pub use score_reranker::ScoreReranker;
pub use similarity::compute_similarity;
pub use streaming_generator::PassthroughStreamingGenerator;
pub use vector_store::InMemoryVectorStore;

#[cfg(feature = "qdrant")]
pub use qdrant_store::{QdrantConfig, QdrantVectorStore};

#[cfg(feature = "pgvector")]
pub use storage::pgvector::{PgVectorConfig, PgVectorStore};

// Re-export kernel types for convenience
pub use mofa_kernel::rag::{
    Document, DocumentChunk, GenerateInput, Generator, GeneratorChunk, RagPipeline,
    RagPipelineOutput, Reranker, Retriever, ScoredDocument, SearchResult, SimilarityMetric,
    VectorStore,
};
