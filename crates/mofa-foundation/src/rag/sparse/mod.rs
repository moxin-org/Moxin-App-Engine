//! Sparse retrieval module
//!
//! Provides BM25-based sparse retrieval implementations.

pub mod bm25;
pub mod index;

pub use bm25::Bm25Retriever;
pub use index::Bm25Index;
