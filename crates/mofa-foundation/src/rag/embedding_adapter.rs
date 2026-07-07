//! RAG Embedding Adapter Layer
//!
//! Bridges MoFA's foundation `LLMClient` (OpenAI / Ollama via `LLMProvider`)
//! to the runtime `EmbeddingProvider` trait, providing:
//!
//! - Pluggable provider selection via [`RagEmbeddingProvider`]
//! - Batch configuration via [`RagEmbeddingConfig`]
//! - Deterministic chunk IDs for idempotent indexing
//! - Timeout handling and error translation

use crate::llm::client::LLMClient;
use crate::llm::types::{EmbeddingInput, EmbeddingRequest, LLMError};
use serde::{Deserialize, Serialize};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Configuration types
// ---------------------------------------------------------------------------

/// Supported embedding providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub enum RagEmbeddingProvider {
    /// OpenAI embedding API (default: `text-embedding-3-small`).
    #[default]
    OpenAi,
    /// Local Ollama embedding API (default: `nomic-embed-text`).
    Ollama,
    /// Cohere Embed API (default: `embed-english-v3.0`).
    /// Requires the `cohere` feature flag.
    #[cfg(feature = "cohere")]
    Cohere,
}

impl std::fmt::Display for RagEmbeddingProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RagEmbeddingProvider::OpenAi => write!(f, "openai"),
            RagEmbeddingProvider::Ollama => write!(f, "ollama"),
            #[cfg(feature = "cohere")]
            RagEmbeddingProvider::Cohere => write!(f, "cohere"),
        }
    }
}

impl std::str::FromStr for RagEmbeddingProvider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "openai" | "open_ai" => Ok(Self::OpenAi),
            "ollama" => Ok(Self::Ollama),
            #[cfg(feature = "cohere")]
            "cohere" => Ok(Self::Cohere),
            other => Err(format!(
                "unknown embedding provider '{}'; expected 'openai', 'ollama'\
                 {}",
                other,
                if cfg!(feature = "cohere") {
                    ", or 'cohere'"
                } else {
                    ""
                },
            )),
        }
    }
}

/// Configuration for the RAG embedding adapter.
///
/// Note: `provider` controls default model selection and timeout hints.
/// The actual LLM backend is determined by the `LLMClient` passed to
/// `LlmEmbeddingAdapter::new()`. Callers are responsible for ensuring
/// the client matches the configured provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagEmbeddingConfig {
    /// Which provider to use (controls default model name and timeout hints).
    pub provider: RagEmbeddingProvider,
    /// Model name override.  When `None`, provider defaults are used:
    /// - OpenAI: `text-embedding-3-small`
    /// - Ollama: `nomic-embed-text`
    pub model: Option<String>,
    /// Maximum number of texts per embedding API call.
    /// Capped internally at 2048 (OpenAI hard limit).
    pub batch_size: usize,
    /// Expected embedding dimensionality.
    /// When `None`, the first embedding response determines the value.
    pub dimensions: Option<usize>,
    /// Per-request timeout.
    pub timeout: Duration,
}

impl Default for RagEmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: RagEmbeddingProvider::default(),
            batch_size: 256,
            dimensions: None,
            model: None,
            timeout: Duration::from_secs(30),
        }
    }
}

impl RagEmbeddingConfig {
    /// Create a config for OpenAI with defaults.
    pub fn openai() -> Self {
        Self {
            provider: RagEmbeddingProvider::OpenAi,
            ..Default::default()
        }
    }

    /// Create a config for Ollama with defaults.
    pub fn ollama() -> Self {
        Self {
            provider: RagEmbeddingProvider::Ollama,
            model: Some("nomic-embed-text".to_string()),
            timeout: Duration::from_secs(60), // local inference is slower
            ..Default::default()
        }
    }

    /// Create a config for Cohere with defaults.
    ///
    /// Requires the `cohere` feature flag.
    #[cfg(feature = "cohere")]
    pub fn cohere() -> Self {
        Self {
            provider: RagEmbeddingProvider::Cohere,
            model: Some("embed-english-v3.0".to_string()),
            batch_size: 96, // Cohere API limit
            ..Default::default()
        }
    }

    /// Override the model name.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Override batch size.
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size.clamp(1, 2048);
        self
    }

    /// Override expected dimensions.
    pub fn with_dimensions(mut self, dimensions: usize) -> Self {
        self.dimensions = Some(dimensions);
        self
    }

    /// Override timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Resolve the model name, falling back to provider defaults.
    pub fn resolved_model(&self) -> &str {
        self.model.as_deref().unwrap_or(match self.provider {
            RagEmbeddingProvider::OpenAi => "text-embedding-3-small",
            RagEmbeddingProvider::Ollama => "nomic-embed-text",
            #[cfg(feature = "cohere")]
            RagEmbeddingProvider::Cohere => "embed-english-v3.0",
        })
    }
}

// ---------------------------------------------------------------------------
// LLM-backed embedding adapter
// ---------------------------------------------------------------------------

/// Embedding adapter that delegates to an `LLMClient` (OpenAI or Ollama).
///
/// Provides `embed_one` and `embed_batch` methods that wrap the underlying
/// `LLMProvider::embedding()` with batching, timeout, and dimension detection.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_foundation::rag::embedding_adapter::{LlmEmbeddingAdapter, RagEmbeddingConfig};
/// use mofa_foundation::llm::{LLMClient, OpenAIProvider, OpenAIConfig};
/// use std::sync::Arc;
///
/// let provider = Arc::new(OpenAIProvider::from_env());
/// let client = LLMClient::new(provider);
///
/// let adapter = LlmEmbeddingAdapter::new(client, RagEmbeddingConfig::openai());
/// ```
pub struct LlmEmbeddingAdapter {
    client: LLMClient,
    config: RagEmbeddingConfig,
    /// Lazily detected dimensions from the first API response.
    detected_dimensions: std::sync::OnceLock<usize>,
}

impl LlmEmbeddingAdapter {
    /// Create a new adapter wrapping the given `LLMClient`.
    pub fn new(client: LLMClient, config: RagEmbeddingConfig) -> Self {
        let detected_dimensions = std::sync::OnceLock::new();
        if let Some(dim) = config.dimensions {
            let _ = detected_dimensions.set(dim);
        }
        Self {
            client,
            config,
            detected_dimensions,
        }
    }

    /// Access the underlying config.
    pub fn config(&self) -> &RagEmbeddingConfig {
        &self.config
    }

    /// Returns the embedding dimensions (detected or configured).
    pub fn dimensions(&self) -> Option<usize> {
        self.detected_dimensions.get().copied()
    }

    /// Embed a single text, returning the vector.
    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbeddingAdapterError> {
        let batch = self.embed_batch(&[text.to_string()]).await?;
        batch
            .into_iter()
            .next()
            .ok_or(EmbeddingAdapterError::EmptyResponse)
    }

    /// Embed a batch of texts, respecting `batch_size` from config.
    ///
    /// Internally splits large inputs into sub-batches and concatenates
    /// the results in order.
    pub async fn embed_batch(
        &self,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, EmbeddingAdapterError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let model = self.config.resolved_model().to_string();
        // Clamp batch_size to valid range in case it was deserialized unchecked
        let batch_size = self.config.batch_size.clamp(1, 2048);
        let mut all_embeddings = Vec::with_capacity(texts.len());

        // Safe u32 conversion for dimensions
        let dimensions = self
            .config
            .dimensions
            .map(|d| u32::try_from(d).map_err(|_| EmbeddingAdapterError::DimensionOverflow(d)))
            .transpose()?;

        for chunk in texts.chunks(batch_size) {
            let request = EmbeddingRequest {
                model: model.clone(),
                input: EmbeddingInput::Multiple(chunk.to_vec()),
                encoding_format: None,
                dimensions,
                user: None,
            };

            let response = tokio::time::timeout(
                self.config.timeout,
                self.client.provider().embedding(request),
            )
            .await
            .map_err(|_| EmbeddingAdapterError::Timeout(self.config.timeout))?
            .map_err(EmbeddingAdapterError::Llm)?;

            if response.data.is_empty() {
                return Err(EmbeddingAdapterError::EmptyResponse);
            }

            // Detect and validate dimensions
            let expected_dim = if let Some(det) = self.detected_dimensions.get() {
                *det
            } else if let Some(first) = response.data.first() {
                let len = first.embedding.len();
                let _ = self.detected_dimensions.set(len);
                len
            } else {
                return Err(EmbeddingAdapterError::EmptyResponse);
            };

            for datum in response.data {
                if datum.embedding.len() != expected_dim {
                    return Err(EmbeddingAdapterError::DimensionMismatch {
                        expected: expected_dim,
                        got: datum.embedding.len(),
                    });
                }
                all_embeddings.push(datum.embedding);
            }
        }

        if all_embeddings.len() != texts.len() {
            return Err(EmbeddingAdapterError::CountMismatch {
                expected: texts.len(),
                got: all_embeddings.len(),
            });
        }

        Ok(all_embeddings)
    }
}

// Make Debug available even though LLMClient isn't Debug
impl std::fmt::Debug for LlmEmbeddingAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmEmbeddingAdapter")
            .field("config", &self.config)
            .field("detected_dimensions", &self.detected_dimensions)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Deterministic chunk IDs
// ---------------------------------------------------------------------------

/// Generate a deterministic chunk ID from document ID, chunk index, and
/// a content hash prefix.
///
/// Format: `{doc_id}:{chunk_idx}:{hash_prefix}`
///
/// This enables idempotent indexing — re-indexing the same document with
/// unchanged content produces identical chunk IDs, skipping unnecessary
/// vector store writes.
pub fn deterministic_chunk_id(doc_id: &str, chunk_idx: usize, chunk_text: &str) -> String {
    let hash = simple_hash(chunk_text);
    format!("{}:{}:{:016x}", doc_id, chunk_idx, hash)
}

/// Fast, non-cryptographic hash for content dedup.  Not for security — just
/// for detecting content changes between indexing runs.
fn simple_hash(text: &str) -> u64 {
    // FNV-1a 64-bit
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors specific to the embedding adapter layer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EmbeddingAdapterError {
    /// The underlying LLM provider returned an error.
    #[error("LLM embedding error: {0}")]
    Llm(#[from] LLMError),

    /// The embedding API returned no data.
    #[error("embedding provider returned empty response")]
    EmptyResponse,

    /// The embedding count doesn't match the input count.
    #[error("embedding count mismatch: expected {expected}, got {got}")]
    CountMismatch { expected: usize, got: usize },

    /// An embedding vector had unexpected dimensions.
    #[error("dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch { expected: usize, got: usize },

    /// Configured dimensions exceed u32 range.
    #[error("dimensions value {0} overflows u32")]
    DimensionOverflow(usize),

    /// The embedding request timed out.
    #[error("embedding request timed out after {0:?}")]
    Timeout(Duration),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ----- RagEmbeddingProvider parsing -----

    #[test]
    fn parse_provider_from_str() {
        assert_eq!(
            "openai".parse::<RagEmbeddingProvider>().unwrap(),
            RagEmbeddingProvider::OpenAi
        );
        assert_eq!(
            "ollama".parse::<RagEmbeddingProvider>().unwrap(),
            RagEmbeddingProvider::Ollama
        );
        assert_eq!(
            "OPENAI".parse::<RagEmbeddingProvider>().unwrap(),
            RagEmbeddingProvider::OpenAi
        );
        assert!("unknown_provider".parse::<RagEmbeddingProvider>().is_err());
    }

    #[test]
    fn provider_display() {
        assert_eq!(RagEmbeddingProvider::OpenAi.to_string(), "openai");
        assert_eq!(RagEmbeddingProvider::Ollama.to_string(), "ollama");
    }

    #[cfg(feature = "cohere")]
    #[test]
    fn parse_cohere_provider_from_str() {
        assert_eq!(
            "cohere".parse::<RagEmbeddingProvider>().unwrap(),
            RagEmbeddingProvider::Cohere
        );
    }

    #[cfg(feature = "cohere")]
    #[test]
    fn cohere_provider_display() {
        assert_eq!(RagEmbeddingProvider::Cohere.to_string(), "cohere");
    }

    #[cfg(feature = "cohere")]
    #[test]
    fn cohere_config_defaults() {
        let config = RagEmbeddingConfig::cohere();
        assert_eq!(config.provider, RagEmbeddingProvider::Cohere);
        assert_eq!(config.resolved_model(), "embed-english-v3.0");
        assert_eq!(config.batch_size, 96);
    }

    // ----- Config -----

    #[test]
    fn default_config_uses_openai() {
        let config = RagEmbeddingConfig::default();
        assert_eq!(config.provider, RagEmbeddingProvider::OpenAi);
        assert_eq!(config.resolved_model(), "text-embedding-3-small");
        assert_eq!(config.batch_size, 256);
        assert!(config.dimensions.is_none());
    }

    #[test]
    fn ollama_config_defaults() {
        let config = RagEmbeddingConfig::ollama();
        assert_eq!(config.provider, RagEmbeddingProvider::Ollama);
        assert_eq!(config.resolved_model(), "nomic-embed-text");
        assert_eq!(config.timeout, Duration::from_secs(60));
    }

    #[test]
    fn config_builder_chain() {
        let config = RagEmbeddingConfig::openai()
            .with_model("text-embedding-3-large")
            .with_batch_size(128)
            .with_dimensions(1536)
            .with_timeout(Duration::from_secs(45));

        assert_eq!(config.resolved_model(), "text-embedding-3-large");
        assert_eq!(config.batch_size, 128);
        assert_eq!(config.dimensions, Some(1536));
        assert_eq!(config.timeout, Duration::from_secs(45));
    }

    #[test]
    fn batch_size_clamped() {
        let config = RagEmbeddingConfig::default().with_batch_size(0);
        assert_eq!(config.batch_size, 1);

        let config = RagEmbeddingConfig::default().with_batch_size(100_000);
        assert_eq!(config.batch_size, 2048);
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = RagEmbeddingConfig::openai()
            .with_model("custom-model")
            .with_dimensions(768);

        let json = serde_json::to_string(&config).unwrap();
        let parsed: RagEmbeddingConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.provider, RagEmbeddingProvider::OpenAi);
        assert_eq!(parsed.resolved_model(), "custom-model");
        assert_eq!(parsed.dimensions, Some(768));
    }

    // ----- Deterministic chunk IDs -----

    #[test]
    fn deterministic_id_is_stable() {
        let id1 = deterministic_chunk_id("doc-abc", 0, "Hello world");
        let id2 = deterministic_chunk_id("doc-abc", 0, "Hello world");
        assert_eq!(id1, id2);
    }

    #[test]
    fn deterministic_id_changes_with_content() {
        let id1 = deterministic_chunk_id("doc-abc", 0, "Hello world");
        let id2 = deterministic_chunk_id("doc-abc", 0, "Hello world!");
        assert_ne!(id1, id2);
    }

    #[test]
    fn deterministic_id_changes_with_chunk_index() {
        let id1 = deterministic_chunk_id("doc-abc", 0, "Hello world");
        let id2 = deterministic_chunk_id("doc-abc", 1, "Hello world");
        assert_ne!(id1, id2);
    }

    #[test]
    fn deterministic_id_changes_with_doc_id() {
        let id1 = deterministic_chunk_id("doc-1", 0, "Hello world");
        let id2 = deterministic_chunk_id("doc-2", 0, "Hello world");
        assert_ne!(id1, id2);
    }

    #[test]
    fn deterministic_id_format() {
        let id = deterministic_chunk_id("my-doc", 3, "test");
        assert!(id.starts_with("my-doc:3:"));
        // Should have exactly 2 colons (doc_id:chunk_idx:hash)
        assert_eq!(id.matches(':').count(), 2);
        // Hash portion should be 16 hex characters
        let hash_part = id.rsplit(':').next().unwrap();
        assert_eq!(hash_part.len(), 16);
        assert!(hash_part.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ----- simple_hash -----

    #[test]
    fn hash_is_deterministic() {
        assert_eq!(simple_hash("hello"), simple_hash("hello"));
    }

    #[test]
    fn hash_differs_for_different_inputs() {
        assert_ne!(simple_hash("hello"), simple_hash("world"));
    }

    #[test]
    fn hash_handles_empty_string() {
        // Should not panic, and produce a stable value
        let h = simple_hash("");
        assert_eq!(h, simple_hash(""));
    }
}
