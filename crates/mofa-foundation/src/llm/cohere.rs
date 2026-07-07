//! Cohere Provider Implementation
//!
//! Provides Cohere API integration as an [`LLMProvider`], currently focused on
//! the [Embed API v2](https://docs.cohere.com/reference/embed).
//!
//! Supported embedding models: `embed-english-v3.0`, `embed-multilingual-v3.0`,
//! `embed-v4.0`.
//!
//! **Note:** Cohere also offers chat/generation models (Command R, Command R+)
//! and a Rerank API.  This provider currently implements **embedding only**.
//! Chat support can be added in a future iteration.
//!
//! # Examples
//!
//! ```rust,ignore
//! use mofa_foundation::llm::cohere::{CohereProvider, CohereConfig};
//!
//! // Basic usage for embeddings
//! let provider = CohereProvider::new("co-xxxx");
//!
//! // With custom model and input type
//! let provider = CohereProvider::with_config(
//!     CohereConfig::new("co-xxxx")
//!         .with_model("embed-v4.0")
//!         .with_input_type(CohereInputType::SearchQuery)
//! );
//! ```

use super::provider::{LLMProvider, ModelCapabilities, ModelInfo};
use super::types::*;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Cohere `input_type` parameter — tells the API how to optimise the
/// embedding for the intended use-case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub enum CohereInputType {
    /// Use when embedding documents for storage / indexing.
    #[default]
    SearchDocument,
    /// Use when embedding a user query for retrieval.
    SearchQuery,
    /// General-purpose classification input.
    Classification,
    /// General-purpose clustering input.
    Clustering,
}

impl CohereInputType {
    /// The string recognized by the Cohere Embed API.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            Self::SearchDocument => "search_document",
            Self::SearchQuery => "search_query",
            Self::Classification => "classification",
            Self::Clustering => "clustering",
        }
    }
}

impl std::fmt::Display for CohereInputType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_api_str())
    }
}

/// Truncation strategy for inputs exceeding the model's token limit
/// (512 tokens for embed-v3.0 models).
///
/// When `None`, the Cohere API returns an error if any input exceeds the
/// limit.  Set to `"END"` or `"START"` to auto-truncate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CohereTruncate {
    /// Drop tokens from the end of the input.
    #[serde(rename = "END")]
    End,
    /// Drop tokens from the start of the input.
    #[serde(rename = "START")]
    Start,
}

impl CohereTruncate {
    /// The string recognized by the Cohere API.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            Self::End => "END",
            Self::Start => "START",
        }
    }
}

/// Configuration for the Cohere provider.
#[derive(Debug, Clone)]
pub struct CohereConfig {
    /// Cohere API key (**required**).
    pub api_key: String,
    /// Default embedding model.
    pub default_model: String,
    /// How the input texts will be used (controls optimisation on the
    /// Cohere side).
    pub input_type: CohereInputType,
    /// Base URL of the Cohere API.
    pub base_url: String,
    /// Maximum texts per single API call.  Cohere caps this at 96.
    pub batch_size: usize,
    /// Truncation strategy for inputs exceeding 512 tokens.
    /// When `None`, the API returns an error on over-length input.
    /// Recommended: `Some(CohereTruncate::End)` for most RAG use-cases.
    pub truncate: Option<CohereTruncate>,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for CohereConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            default_model: "embed-english-v3.0".to_string(),
            input_type: CohereInputType::default(),
            base_url: "https://api.cohere.com".to_string(),
            batch_size: 96,
            truncate: Some(CohereTruncate::End),
            timeout_secs: 30,
        }
    }
}

impl CohereConfig {
    /// Create a new config with the given API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            ..Default::default()
        }
    }

    /// Create a config from environment variables.
    ///
    /// Reads `COHERE_API_KEY`, `COHERE_MODEL`, `COHERE_BASE_URL`.
    pub fn from_env() -> Self {
        Self {
            api_key: std::env::var("COHERE_API_KEY").unwrap_or_default(),
            default_model: std::env::var("COHERE_MODEL")
                .unwrap_or_else(|_| "embed-english-v3.0".to_string()),
            base_url: std::env::var("COHERE_BASE_URL")
                .unwrap_or_else(|_| "https://api.cohere.com".to_string()),
            ..Default::default()
        }
    }

    /// Override the model name.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = model.into();
        self
    }

    /// Override the input type.
    pub fn with_input_type(mut self, input_type: CohereInputType) -> Self {
        self.input_type = input_type;
        self
    }

    /// Override the base URL (useful for proxies / on-prem deployments).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Override the batch size (clamped to 1..=96).
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size.clamp(1, 96);
        self
    }

    /// Override the per-request timeout (seconds).
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Override the truncation strategy.
    ///
    /// Pass `None` to disable truncation (the API will error on
    /// inputs exceeding 512 tokens).
    pub fn with_truncate(mut self, truncate: Option<CohereTruncate>) -> Self {
        self.truncate = truncate;
        self
    }
}

// ---------------------------------------------------------------------------
// Cohere API request / response shapes (private)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct CohereEmbedRequest<'a> {
    model: &'a str,
    texts: &'a [String],
    input_type: &'a str,
    embedding_types: &'a [&'a str],
    #[serde(skip_serializing_if = "Option::is_none")]
    truncate: Option<&'a str>,
}

#[derive(Deserialize)]
struct CohereEmbedResponse {
    embeddings: CohereEmbeddings,
}

#[derive(Deserialize)]
struct CohereEmbeddings {
    float: Vec<Vec<f32>>,
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

/// Cohere [`LLMProvider`] implementation — currently provides **embedding**
/// support.  Chat/generation (Command R) can be added in a future iteration.
///
/// Calls the Cohere Embed API v2 (`POST {base_url}/v2/embed`) via
/// `reqwest` and translates to/from MoFA's standard
/// [`EmbeddingRequest`]/[`EmbeddingResponse`] types.
pub struct CohereProvider {
    config: CohereConfig,
    http: reqwest::Client,
}

impl CohereProvider {
    /// Create a provider with the given API key and default settings.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_config(CohereConfig::new(api_key))
    }

    /// Create a provider reading `COHERE_API_KEY` / `COHERE_MODEL` /
    /// `COHERE_BASE_URL` from the environment.
    pub fn from_env() -> Self {
        Self::with_config(CohereConfig::from_env())
    }

    /// Create a provider from an explicit [`CohereConfig`].
    pub fn with_config(config: CohereConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("failed to build reqwest client");
        Self { config, http }
    }

    /// Access the underlying config.
    pub fn config(&self) -> &CohereConfig {
        &self.config
    }

    /// Calls the Cohere Embed API for a single batch of texts and
    /// returns the raw `Vec<Vec<f32>>`.
    async fn call_embed_api(&self, texts: &[String]) -> LLMResult<Vec<Vec<f32>>> {
        let url = format!("{}/v2/embed", self.config.base_url.trim_end_matches('/'));
        let input_type_str = self.config.input_type.as_api_str();

        let body = CohereEmbedRequest {
            model: &self.config.default_model,
            texts,
            input_type: input_type_str,
            embedding_types: &["float"],
            truncate: self.config.truncate.map(|t| t.as_api_str()),
        };

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    LLMError::Timeout(format!(
                        "Cohere embedding request timed out after {}s",
                        self.config.timeout_secs
                    ))
                } else {
                    LLMError::NetworkError(e.to_string())
                }
            })?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(LLMError::RateLimited(
                "Cohere API rate limited — retry after backoff".to_string(),
            ));
        }
        if !status.is_success() {
            let body_text = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            return Err(LLMError::ApiError {
                code: Some(status.as_u16().to_string()),
                message: body_text,
            });
        }

        let embed_response: CohereEmbedResponse = response
            .json()
            .await
            .map_err(|e| LLMError::Other(format!("failed to parse Cohere embed response: {e}")))?;

        Ok(embed_response.embeddings.float)
    }
}

#[async_trait]
impl LLMProvider for CohereProvider {
    fn name(&self) -> &str {
        "cohere"
    }

    fn default_model(&self) -> &str {
        &self.config.default_model
    }

    fn supported_models(&self) -> Vec<&str> {
        vec![
            "embed-english-v3.0",
            "embed-multilingual-v3.0",
            "embed-english-light-v3.0",
            "embed-multilingual-light-v3.0",
            "embed-v4.0",
        ]
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn supports_tools(&self) -> bool {
        false
    }

    fn supports_vision(&self) -> bool {
        false
    }

    fn supports_embedding(&self) -> bool {
        true
    }

    // -- Chat is not yet implemented (Cohere does support chat via Command R) --

    async fn chat(&self, _request: ChatCompletionRequest) -> LLMResult<ChatCompletionResponse> {
        Err(LLMError::ProviderNotSupported(
            "Cohere chat is not yet implemented; this provider currently supports embedding only"
                .to_string(),
        ))
    }

    // -- Embedding --

    async fn embedding(&self, request: EmbeddingRequest) -> LLMResult<EmbeddingResponse> {
        let texts: Vec<String> = match request.input {
            EmbeddingInput::Single(s) => vec![s],
            EmbeddingInput::Multiple(v) => v,
        };

        if texts.is_empty() {
            return Ok(EmbeddingResponse {
                object: "list".to_string(),
                model: request.model.clone(),
                data: vec![],
                usage: EmbeddingUsage {
                    prompt_tokens: 0,
                    total_tokens: 0,
                },
            });
        }

        // Sub-batch at the Cohere max of 96 texts per call
        let batch_size = self.config.batch_size.clamp(1, 96);
        let mut all_vectors: Vec<Vec<f32>> = Vec::with_capacity(texts.len());

        for chunk in texts.chunks(batch_size) {
            let chunk_vec: Vec<String> = chunk.to_vec();
            let vectors = self.call_embed_api(&chunk_vec).await?;
            if vectors.len() != chunk.len() {
                return Err(LLMError::Other(format!(
                    "Cohere embedding count mismatch: expected {}, got {}",
                    chunk.len(),
                    vectors.len()
                )));
            }
            all_vectors.extend(vectors);
        }

        // Convert to EmbeddingData
        let data: Vec<EmbeddingData> = all_vectors
            .into_iter()
            .enumerate()
            .map(|(i, embedding)| EmbeddingData {
                object: "embedding".to_string(),
                index: u32::try_from(i).unwrap_or(u32::MAX),
                embedding,
            })
            .collect();

        // Cohere does not expose token counts in the v2 embed response,
        // so we report zeros.  The EmbeddingUsage fields are non-optional
        // in MoFA's type system but this is consistent with how some
        // providers behave when counts are unavailable.
        Ok(EmbeddingResponse {
            object: "list".to_string(),
            model: request.model,
            data,
            usage: EmbeddingUsage {
                prompt_tokens: 0,
                total_tokens: 0,
            },
        })
    }

    async fn health_check(&self) -> LLMResult<bool> {
        // A lightweight check: embed a single token.  If the API key is
        // valid and the service is up this will succeed.
        let texts = vec!["health".to_string()];
        match self.call_embed_api(&texts).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn get_model_info(&self, model: &str) -> LLMResult<ModelInfo> {
        let info = match model {
            "embed-english-v3.0" => ModelInfo {
                id: model.to_string(),
                name: "Cohere Embed English v3.0".to_string(),
                description: Some("English-only embedding model, 1024 dimensions".to_string()),
                context_window: Some(512),
                max_output_tokens: None,
                training_cutoff: None,
                capabilities: ModelCapabilities {
                    streaming: false,
                    tools: false,
                    vision: false,
                    json_mode: false,
                    json_schema: false,
                },
            },
            "embed-multilingual-v3.0" => ModelInfo {
                id: model.to_string(),
                name: "Cohere Embed Multilingual v3.0".to_string(),
                description: Some(
                    "Multilingual embedding model, 1024 dimensions, 100+ languages".to_string(),
                ),
                context_window: Some(512),
                max_output_tokens: None,
                training_cutoff: None,
                capabilities: ModelCapabilities {
                    streaming: false,
                    tools: false,
                    vision: false,
                    json_mode: false,
                    json_schema: false,
                },
            },
            "embed-v4.0" => ModelInfo {
                id: model.to_string(),
                name: "Cohere Embed v4.0".to_string(),
                description: Some(
                    "Latest Cohere embedding model with improved retrieval quality".to_string(),
                ),
                context_window: Some(512),
                max_output_tokens: None,
                training_cutoff: None,
                capabilities: ModelCapabilities {
                    streaming: false,
                    tools: false,
                    vision: false,
                    json_mode: false,
                    json_schema: false,
                },
            },
            _ => {
                return Err(LLMError::ProviderNotSupported(format!(
                    "unknown Cohere model '{model}'"
                )));
            }
        };
        Ok(info)
    }
}

impl std::fmt::Debug for CohereProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CohereProvider")
            .field("config", &self.config)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ----- Config -----

    #[test]
    fn default_config() {
        let config = CohereConfig::default();
        assert_eq!(config.default_model, "embed-english-v3.0");
        assert_eq!(config.input_type, CohereInputType::SearchDocument);
        assert_eq!(config.base_url, "https://api.cohere.com");
        assert_eq!(config.batch_size, 96);
        assert!(config.api_key.is_empty());
    }

    #[test]
    fn config_builder_chain() {
        let config = CohereConfig::new("test-key")
            .with_model("embed-v4.0")
            .with_input_type(CohereInputType::SearchQuery)
            .with_base_url("https://custom.endpoint.com")
            .with_batch_size(50)
            .with_timeout(90);

        assert_eq!(config.api_key, "test-key");
        assert_eq!(config.default_model, "embed-v4.0");
        assert_eq!(config.input_type, CohereInputType::SearchQuery);
        assert_eq!(config.base_url, "https://custom.endpoint.com");
        assert_eq!(config.batch_size, 50);
        assert_eq!(config.timeout_secs, 90);
    }

    #[test]
    fn batch_size_clamped() {
        assert_eq!(CohereConfig::default().with_batch_size(0).batch_size, 1);
        assert_eq!(CohereConfig::default().with_batch_size(200).batch_size, 96);
    }

    #[test]
    fn input_type_display() {
        assert_eq!(
            CohereInputType::SearchDocument.to_string(),
            "search_document"
        );
        assert_eq!(CohereInputType::SearchQuery.to_string(), "search_query");
        assert_eq!(
            CohereInputType::Classification.to_string(),
            "classification"
        );
        assert_eq!(CohereInputType::Clustering.to_string(), "clustering");
    }

    // ----- Provider trait methods -----

    #[test]
    fn provider_metadata() {
        let provider = CohereProvider::new("key");
        assert_eq!(provider.name(), "cohere");
        assert_eq!(provider.default_model(), "embed-english-v3.0");
        assert!(provider.supports_embedding());
        assert!(!provider.supports_streaming());
        assert!(!provider.supports_tools());
        assert!(!provider.supports_vision());
    }

    #[tokio::test]
    async fn chat_returns_not_supported() {
        let provider = CohereProvider::new("key");
        let request = ChatCompletionRequest::new("embed-english-v3.0");
        let result = provider.chat(request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn embedding_empty_input() {
        let provider = CohereProvider::new("key");
        let request = EmbeddingRequest {
            model: "embed-english-v3.0".to_string(),
            input: EmbeddingInput::Multiple(vec![]),
            encoding_format: None,
            dimensions: None,
            user: None,
        };
        let result = provider.embedding(request).await.unwrap();
        assert!(result.data.is_empty());
    }

    // ----- Truncate -----

    #[test]
    fn truncate_default_is_end() {
        let config = CohereConfig::default();
        assert_eq!(config.truncate, Some(CohereTruncate::End));
    }

    #[test]
    fn truncate_api_str() {
        assert_eq!(CohereTruncate::End.as_api_str(), "END");
        assert_eq!(CohereTruncate::Start.as_api_str(), "START");
    }

    #[test]
    fn truncate_can_be_disabled() {
        let config = CohereConfig::new("key").with_truncate(None);
        assert!(config.truncate.is_none());
    }

    // ----- Model info -----

    #[tokio::test]
    async fn get_model_info_known_model() {
        let provider = CohereProvider::new("key");
        let info = provider.get_model_info("embed-english-v3.0").await.unwrap();
        assert_eq!(info.id, "embed-english-v3.0");
        assert!(!info.capabilities.streaming);
    }

    #[tokio::test]
    async fn get_model_info_unknown_model_errors() {
        let provider = CohereProvider::new("key");
        let result = provider.get_model_info("nonexistent-model").await;
        assert!(result.is_err());
    }

    // ----- from_env -----

    #[test]
    fn from_env_uses_defaults_when_vars_unset() {
        // Don't set any env vars — should fall back to defaults
        let config = CohereConfig::from_env();
        assert_eq!(config.default_model, "embed-english-v3.0");
        assert_eq!(config.base_url, "https://api.cohere.com");
    }
}
