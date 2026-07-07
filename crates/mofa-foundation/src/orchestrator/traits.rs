//! Model Orchestrator Traits
//!
//! Core traits for managing and orchestrating edge ML models:
//! - `ModelProvider`: Loads, queries, and manages a single model instance
//! - `ModelOrchestrator`: Orchestrates multiple models with lifecycle, scheduling, and pipeline routing

use async_trait::async_trait;
use futures::StreamExt;
use mofa_kernel::llm::BoxTokenStream;
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use thiserror::Error;
use tokio::time::{sleep, Duration};

// ============================================================================
// Model Type — Task-based routing
// ============================================================================

/// The functional type of an ML model — used to route requests to the right model
///
/// When multiple models are registered, the orchestrator uses `ModelType` to pick
/// the correct one for each pipeline stage automatically.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ModelType {
    /// Automatic Speech Recognition (e.g., FunASR, Whisper)
    Asr,
    /// Large Language Model (e.g., Qwen, Llama)
    Llm,
    /// Text-to-Speech synthesis (e.g., GPT-SoVITS, Kokoro)
    Tts,
    /// Text/audio embedding model
    Embedding,
    /// General-purpose or undefined model type
    Other(String),
}

impl fmt::Display for ModelType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelType::Asr => write!(f, "ASR"),
            ModelType::Llm => write!(f, "LLM"),
            ModelType::Tts => write!(f, "TTS"),
            ModelType::Embedding => write!(f, "Embedding"),
            ModelType::Other(name) => write!(f, "{}", name),
        }
    }
}

// ============================================================================
// Degradation — Dynamic precision switching under memory pressure
// ============================================================================

/// Quantization precision levels for dynamic degradation
///
/// When memory is constrained and LRU eviction cannot free enough space,
/// the orchestrator can reduce model precision to lower memory footprint.
///
/// Order of degradation: `Full` → `Half` → `Int8` → `Int4`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum DegradationLevel {
    /// FP32 — highest quality, highest memory (~4 bytes/param)
    Full,
    /// FP16 — half precision (~2 bytes/param)
    Half,
    /// INT8 quantized (~1 byte/param)
    Int8,
    /// INT4 quantized (~0.5 bytes/param) — maximum compression
    Int4,
}

impl fmt::Display for DegradationLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DegradationLevel::Full => write!(f, "FP32"),
            DegradationLevel::Half => write!(f, "FP16"),
            DegradationLevel::Int8 => write!(f, "INT8"),
            DegradationLevel::Int4 => write!(f, "INT4"),
        }
    }
}

impl DegradationLevel {
    /// Returns the quantization string identifier used in model configs
    pub fn as_quantization_str(&self) -> &'static str {
        match self {
            DegradationLevel::Full => "f32",
            DegradationLevel::Half => "f16",
            DegradationLevel::Int8 => "q8_0",
            DegradationLevel::Int4 => "q4_0",
        }
    }

    /// Returns the next (more compressed) degradation level, or `None` if already at maximum compression
    pub fn next_level(&self) -> Option<DegradationLevel> {
        match self {
            DegradationLevel::Full => Some(DegradationLevel::Half),
            DegradationLevel::Half => Some(DegradationLevel::Int8),
            DegradationLevel::Int8 => Some(DegradationLevel::Int4),
            DegradationLevel::Int4 => None, // Already at maximum compression
        }
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Model orchestrator error types
#[derive(Debug, Clone, Error)]
#[non_exhaustive]
pub enum OrchestratorError {
    /// Model loading failed
    #[error("Model load failed: {0}")]
    ModelLoadFailed(String),

    /// Model inference failed
    #[error("Model inference failed: {0}")]
    InferenceFailed(String),

    /// Model not found
    #[error("Model not found: {0}")]
    ModelNotFound(String),

    /// No model registered for the requested task type
    #[error("No model available for task type: {0}")]
    NoModelForType(String),

    /// Memory constrained — insufficient available system memory
    #[error("Memory constrained: {0}")]
    MemoryConstrained(String),

    /// Device error (GPU unavailable, etc.)
    #[error("Device error: {0}")]
    DeviceError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Pool is at capacity and cannot accept more models
    #[error("Pool at capacity: {0}")]
    PoolCapacityExceeded(String),

    /// LRU eviction failed
    #[error("LRU eviction failed: {0}")]
    EvictionFailed(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Pipeline execution failed
    #[error("Pipeline error: {0}")]
    PipelineError(String),

    /// Other errors
    #[error("Orchestrator error: {0}")]
    Other(String),
}

/// Plain result alias for orchestrator operations (backward-compatible).
pub type OrchestratorResult<T> = Result<T, OrchestratorError>;

/// Error-stack–backed result alias for orchestrator operations.
///
/// Note: `OrchestratorError` derives [`Clone`]; `error_stack::Report` does not
/// — clone the inner error before wrapping when needed.
pub type OrchestratorReport<T> = ::std::result::Result<T, error_stack::Report<OrchestratorError>>;

/// Extension trait to convert [`OrchestratorResult<T>`] into [`OrchestratorReport<T>`].
pub trait IntoOrchestratorReport<T> {
    /// Wrap the error in an `error_stack::Report`.
    fn into_report(self) -> OrchestratorReport<T>;
}

impl<T> IntoOrchestratorReport<T> for OrchestratorResult<T> {
    #[inline]
    fn into_report(self) -> OrchestratorReport<T> {
        self.map_err(error_stack::Report::new)
    }
}

// ============================================================================
// Model Provider Configuration
// ============================================================================

/// Configuration for initializing a model provider
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelProviderConfig {
    /// Human-readable name of the model (used as ID)
    pub model_name: String,
    /// Path to the model weights on disk (or HuggingFace repo ID)
    pub model_path: String,
    /// Device type: `"cuda"` or `"cpu"`
    pub device: String,
    /// The functional type of this model — used for task-based routing
    pub model_type: ModelType,
    /// Maximum context window (tokens)
    pub max_context_length: Option<usize>,
    /// Quantization level (e.g., `"q4_0"`, `"q8_0"`, `"f16"`, `"f32"`)
    pub quantization: Option<String>,
    /// Custom configuration options (arbitrary key-value pairs)
    pub extra_config: HashMap<String, Value>,
}

// ============================================================================
// Model Provider Trait — single model lifecycle
// ============================================================================

/// Model provider trait — handles loading and inference for a single model
///
/// Implementers manage:
/// - Loading models from disk or HuggingFace Hub
/// - Running inference (synchronously or via streaming)
/// - Reporting current memory usage
/// - Graceful cleanup on unload
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Provider name (e.g., `"LinuxCandleProvider"`)
    fn name(&self) -> &str;

    /// The model's unique identifier
    fn model_id(&self) -> &str;

    /// The functional type of this model
    fn model_type(&self) -> &ModelType;

    /// Load the model into memory
    async fn load(&mut self) -> OrchestratorResult<()>;

    /// Unload the model from memory, freeing resources
    async fn unload(&mut self) -> OrchestratorResult<()>;

    /// Whether the model is currently loaded and ready for inference
    fn is_loaded(&self) -> bool;

    /// Run inference with the given input string, return response string
    async fn infer(&self, input: &str) -> OrchestratorResult<String>;

    /// Run streaming inference with the given input string, return token stream
    ///
    /// Default implementation calls `infer()` and splits the result into tokens.
    /// Providers should override this for true streaming support.
    async fn infer_stream(&self, input: &str) -> OrchestratorResult<BoxTokenStream> {
        // Default: call infer() and split into word-by-word stream
        let output = self.infer(input).await?;
        // Collect owned strings to avoid lifetime issues
        let tokens: Vec<String> = output.split_whitespace().map(String::from).collect();
        use mofa_kernel::llm::{StreamChunk, FinishReason, StreamError};
        let stream = futures::stream::iter(tokens)
            .then(|token| async move {
                sleep(Duration::from_millis(30)).await;
                Ok::<_, StreamError>(StreamChunk::text(token))
            })
            .chain(futures::stream::once(async move {
                Ok(StreamChunk::done(FinishReason::Stop))
            }));
        Ok(Box::pin(stream))
    }

    /// Current memory usage in bytes
    fn memory_usage_bytes(&self) -> u64;

    /// Key-value metadata about the model (model_id, device, quantization, etc.)
    fn get_metadata(&self) -> HashMap<String, Value>;

    /// Verify the model is functioning correctly
    async fn health_check(&self) -> OrchestratorResult<bool>;
}

// ============================================================================
// Pool Statistics
// ============================================================================

/// Real-time statistics about the model pool state
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolStatistics {
    /// Number of currently loaded models
    pub loaded_models_count: usize,
    /// Total memory used by all loaded models (bytes)
    pub total_memory_usage: u64,
    /// Available system memory (bytes)
    pub available_memory: u64,
    /// Number of models queued for loading
    pub queued_models_count: usize,
    /// Timestamp of collection
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

// ============================================================================
// Model Orchestrator Trait — multi-model pool management
// ============================================================================

/// Model orchestrator trait — manages multiple models with lifecycle and scheduling
///
/// Implementers handle:
/// - A concurrent pool of models with LRU eviction
/// - Memory usage monitoring and admission control
/// - Task-type based routing (route ASR requests to ASR models, etc.)
/// - Dynamic precision degradation under memory pressure
#[async_trait]
pub trait ModelOrchestrator: Send + Sync {
    /// Orchestrator name (e.g., `"ModelPool"`)
    fn name(&self) -> &str;

    // -------------------------------------------------------------------------
    // Model registration
    // -------------------------------------------------------------------------

    /// Register a model in the pool (does not load it into memory)
    async fn register_model(&self, config: ModelProviderConfig) -> OrchestratorResult<()>;

    /// Unregister and unload a model from the pool
    async fn unregister_model(&self, model_id: &str) -> OrchestratorResult<()>;

    // -------------------------------------------------------------------------
    // Lifecycle
    // -------------------------------------------------------------------------

    /// Load a model into memory, triggering LRU eviction if memory is constrained
    async fn load_model(&self, model_id: &str) -> OrchestratorResult<()>;

    /// Unload a model from memory
    async fn unload_model(&self, model_id: &str) -> OrchestratorResult<()>;

    /// Whether the given model is currently loaded
    fn is_model_loaded(&self, model_id: &str) -> bool;

    // -------------------------------------------------------------------------
    // Inference
    // -------------------------------------------------------------------------

    /// Run inference on a model, automatically loading it if needed
    async fn infer(&self, model_id: &str, input: &str) -> OrchestratorResult<String>;

    /// Route a request to the best available model for the given task type.
    ///
    /// Returns the ID of the selected model. The caller can then call `infer()`.
    /// Selection priority: loaded model of matching type → registered model of matching type.
    async fn route_by_type(&self, task: &ModelType) -> OrchestratorResult<String>;

    // -------------------------------------------------------------------------
    // Introspection
    // -------------------------------------------------------------------------

    /// Current pool statistics (memory, loaded count, etc.)
    fn get_statistics(&self) -> OrchestratorResult<PoolStatistics>;

    /// IDs of all registered models
    fn list_models(&self) -> Vec<String>;

    /// IDs of currently loaded models
    fn list_loaded_models(&self) -> Vec<String>;

    // -------------------------------------------------------------------------
    // Memory management
    // -------------------------------------------------------------------------

    /// Manually trigger LRU eviction to free at least `target_bytes`
    ///
    /// Returns the number of models evicted.
    async fn trigger_eviction(&self, target_bytes: u64) -> OrchestratorResult<usize>;

    /// Set the memory threshold (bytes). When total model memory exceeds this, auto-eviction triggers.
    async fn set_memory_threshold(&self, bytes: u64) -> OrchestratorResult<()>;

    /// Get the current memory threshold (bytes)
    fn get_memory_threshold(&self) -> u64;

    /// Set the idle timeout. Models unused for longer than this are candidates for LRU eviction.
    async fn set_idle_timeout_secs(&self, secs: u64) -> OrchestratorResult<()>;

    /// Get the current idle timeout (seconds)
    fn get_idle_timeout_secs(&self) -> u64;
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = OrchestratorError::ModelNotFound("gpt2".to_string());
        assert_eq!(err.to_string(), "Model not found: gpt2");

        let err = OrchestratorError::MemoryConstrained("Need 8GB, have 2GB".to_string());
        assert!(err.to_string().contains("Memory constrained"));

        let err = OrchestratorError::NoModelForType("Asr".to_string());
        assert!(err.to_string().contains("No model available"));
    }

    #[test]
    fn test_config_creation() {
        let config = ModelProviderConfig {
            model_name: "Llama-2-7B".to_string(),
            model_path: "/models/llama-2-7b.gguf".to_string(),
            device: "cuda".to_string(),
            model_type: ModelType::Llm,
            max_context_length: Some(4096),
            quantization: Some("q4_0".to_string()),
            extra_config: HashMap::new(),
        };

        assert_eq!(config.model_name, "Llama-2-7B");
        assert_eq!(config.device, "cuda");
        assert_eq!(config.model_type, ModelType::Llm);
    }

    #[test]
    fn test_degradation_level_ordering() {
        assert!(DegradationLevel::Full < DegradationLevel::Half);
        assert!(DegradationLevel::Half < DegradationLevel::Int8);
        assert!(DegradationLevel::Int8 < DegradationLevel::Int4);
    }

    #[test]
    fn test_degradation_level_next() {
        assert_eq!(
            DegradationLevel::Full.next_level(),
            Some(DegradationLevel::Half)
        );
        assert_eq!(
            DegradationLevel::Half.next_level(),
            Some(DegradationLevel::Int8)
        );
        assert_eq!(
            DegradationLevel::Int8.next_level(),
            Some(DegradationLevel::Int4)
        );
        assert_eq!(DegradationLevel::Int4.next_level(), None);
    }

    #[test]
    fn test_degradation_quantization_strings() {
        assert_eq!(DegradationLevel::Full.as_quantization_str(), "f32");
        assert_eq!(DegradationLevel::Half.as_quantization_str(), "f16");
        assert_eq!(DegradationLevel::Int8.as_quantization_str(), "q8_0");
        assert_eq!(DegradationLevel::Int4.as_quantization_str(), "q4_0");
    }

    #[test]
    fn test_model_type_equality() {
        assert_eq!(ModelType::Llm, ModelType::Llm);
        assert_ne!(ModelType::Asr, ModelType::Tts);
        assert_eq!(
            ModelType::Other("custom".into()),
            ModelType::Other("custom".into())
        );
    }

    #[test]
    fn test_model_type_display() {
        assert_eq!(ModelType::Asr.to_string(), "ASR");
        assert_eq!(ModelType::Llm.to_string(), "LLM");
        assert_eq!(ModelType::Tts.to_string(), "TTS");
        assert_eq!(ModelType::Embedding.to_string(), "Embedding");
        assert_eq!(ModelType::Other("custom".into()).to_string(), "custom");
    }

    #[test]
    fn test_degradation_level_display() {
        assert_eq!(DegradationLevel::Full.to_string(), "FP32");
        assert_eq!(DegradationLevel::Half.to_string(), "FP16");
        assert_eq!(DegradationLevel::Int8.to_string(), "INT8");
        assert_eq!(DegradationLevel::Int4.to_string(), "INT4");
    }

    #[test]
    fn test_model_type_serde_roundtrip() {
        let types = vec![
            ModelType::Asr,
            ModelType::Llm,
            ModelType::Tts,
            ModelType::Embedding,
            ModelType::Other("custom".into()),
        ];
        for model_type in types {
            let json = serde_json::to_string(&model_type).expect("serialize");
            let deserialized: ModelType = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(model_type, deserialized);
        }
    }

    #[test]
    fn test_degradation_level_serde_roundtrip() {
        let levels = vec![
            DegradationLevel::Full,
            DegradationLevel::Half,
            DegradationLevel::Int8,
            DegradationLevel::Int4,
        ];
        for level in levels {
            let json = serde_json::to_string(&level).expect("serialize");
            let deserialized: DegradationLevel = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(level, deserialized);
        }
    }

    #[test]
    fn test_model_provider_config_serde_roundtrip() {
        let config = ModelProviderConfig {
            model_name: "qwen-7b".to_string(),
            model_path: "/models/qwen-7b".to_string(),
            device: "cuda".to_string(),
            model_type: ModelType::Llm,
            max_context_length: Some(4096),
            quantization: Some("q4_0".to_string()),
            extra_config: HashMap::new(),
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let deserialized: ModelProviderConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.model_name, "qwen-7b");
        assert_eq!(deserialized.model_type, ModelType::Llm);
        assert_eq!(deserialized.quantization, Some("q4_0".to_string()));
    }

    #[test]
    fn test_pool_statistics_serde() {
        let stats = PoolStatistics {
            loaded_models_count: 2,
            total_memory_usage: 8_000_000_000,
            available_memory: 16_000_000_000,
            queued_models_count: 1,
            timestamp: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&stats).expect("serialize");
        let deserialized: PoolStatistics = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.loaded_models_count, 2);
        assert_eq!(deserialized.total_memory_usage, 8_000_000_000);
    }
}
