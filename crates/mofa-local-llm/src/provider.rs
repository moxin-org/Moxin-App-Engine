//! Linux local inference provider
//!
//! Implements `ModelProvider` from `mofa-foundation` so that the hardware-detected
//! backend slots into the existing `ModelPool` orchestrator without any changes to
//! downstream code.

use crate::config::LinuxInferenceConfig;
use crate::hardware::{ComputeBackend, HardwareInfo};
use async_trait::async_trait;
use mofa_foundation::orchestrator::traits::{
    ModelProvider, ModelProviderConfig, ModelType, OrchestratorError, OrchestratorResult,
};
use mofa_kernel::llm::BoxTokenStream;
use serde_json::Value;
use std::collections::HashMap;
use sysinfo::{MemoryRefreshKind, RefreshKind, System};

/// A local inference provider that runs on Linux using the best available
/// compute backend (CUDA → ROCm → Vulkan → CPU).
///
/// Integrates with `mofa-foundation`'s `ModelOrchestrator` / `ModelPool` via
/// the `ModelProvider` trait so it can be swapped in without touching callers.
pub struct LinuxLocalProvider {
    config: LinuxInferenceConfig,
    hardware: HardwareInfo,
    active_backend: ComputeBackend,
    loaded: bool,
    memory_usage: u64,
}

impl LinuxLocalProvider {
    /// Create a new provider from config.
    ///
    /// Hardware detection runs at construction time. The active backend is
    /// either the one specified in `config.backend_override` or the best
    /// automatically detected backend.
    pub fn new(config: LinuxInferenceConfig) -> OrchestratorResult<Self> {
        let hardware = HardwareInfo::detect();

        let active_backend = if let Some(ref forced) = config.backend_override {
            if !hardware.available_backends.contains(forced) {
                return Err(OrchestratorError::DeviceError(format!(
                    "requested backend {} is not available on this system",
                    forced
                )));
            }
            forced.clone()
        } else {
            hardware.backend.clone()
        };

        tracing::info!(
            backend = %active_backend,
            vram_bytes = hardware.vram_bytes,
            available_ram = hardware.available_ram_bytes,
            "LinuxLocalProvider initialized"
        );

        Ok(Self {
            config,
            hardware,
            active_backend,
            loaded: false,
            memory_usage: 0,
        })
    }

    /// Effective memory limit: explicit config value or 80% of usable RAM.
    ///
    /// Falls back to total RAM when available RAM is unreported (e.g. macOS),
    /// with a floor of 512 MB so the value is always non-zero.
    fn effective_memory_limit(&self) -> u64 {
        self.config.memory_limit_bytes.unwrap_or_else(|| {
            const FLOOR: u64 = 512 * 1024 * 1024;
            let base = if self.hardware.available_ram_bytes > 0 {
                self.hardware.available_ram_bytes
            } else {
                self.hardware.total_ram_bytes
            };
            ((base as f64 * 0.8) as u64).max(FLOOR)
        })
    }

    /// Check current available RAM and fail early if insufficient.
    ///
    /// Skips the check when sysinfo cannot report available memory (e.g. macOS),
    /// to avoid false positives on non-Linux platforms.
    fn check_memory(&self) -> OrchestratorResult<()> {
        let mut sys = System::new_with_specifics(
            RefreshKind::new().with_memory(MemoryRefreshKind::everything()),
        );
        sys.refresh_memory();
        let available = sys.available_memory();

        // sysinfo returns 0 on platforms where available memory cannot be
        // determined (e.g. macOS). Skip the check rather than rejecting incorrectly.
        if available == 0 {
            return Ok(());
        }

        let limit = self.effective_memory_limit();
        if available < limit / 4 {
            return Err(OrchestratorError::MemoryConstrained(format!(
                "only {} MB available, need at least {} MB",
                available / (1024 * 1024),
                (limit / 4) / (1024 * 1024)
            )));
        }
        Ok(())
    }

    /// Convert this provider's config into the standard `ModelProviderConfig`
    pub fn to_provider_config(&self) -> ModelProviderConfig {
        ModelProviderConfig {
            model_name: self.config.model_name.clone(),
            model_path: self.config.model_path.clone(),
            device: self.active_backend.to_string().to_lowercase(),
            model_type: ModelType::Llm,
            max_context_length: Some(4096),
            quantization: None,
            extra_config: {
                let mut m = HashMap::new();
                if let Some(threads) = self.config.num_threads {
                    m.insert("num_threads".into(), Value::Number(threads.into()));
                }
                if let Some(ref cores) = self.config.thread_affinity {
                    m.insert(
                        "thread_affinity".into(),
                        Value::Array(cores.iter().map(|&c| Value::Number(c.into())).collect()),
                    );
                }
                m
            },
        }
    }
}

#[async_trait]
impl ModelProvider for LinuxLocalProvider {
    fn name(&self) -> &str {
        "LinuxLocalProvider"
    }

    fn model_id(&self) -> &str {
        &self.config.model_name
    }

    fn model_type(&self) -> &ModelType {
        &ModelType::Llm
    }

    async fn load(&mut self) -> OrchestratorResult<()> {
        if self.loaded {
            return Ok(());
        }

        self.check_memory()?;

        // Validate the model path exists
        if !std::path::Path::new(&self.config.model_path).exists() {
            return Err(OrchestratorError::ModelLoadFailed(format!(
                "model file not found: {}",
                self.config.model_path
            )));
        }

        tracing::info!(
            model = %self.config.model_name,
            path = %self.config.model_path,
            backend = %self.active_backend,
            "loading model"
        );

        // Estimate memory usage based on model file size
        self.memory_usage = std::fs::metadata(&self.config.model_path)
            .map(|m| m.len())
            .unwrap_or(0);

        self.loaded = true;
        Ok(())
    }

    async fn unload(&mut self) -> OrchestratorResult<()> {
        self.loaded = false;
        self.memory_usage = 0;
        tracing::info!(model = %self.config.model_name, "model unloaded");
        Ok(())
    }

    fn is_loaded(&self) -> bool {
        self.loaded
    }

    async fn infer(&self, input: &str) -> OrchestratorResult<String> {
        if !self.loaded {
            return Err(OrchestratorError::InferenceFailed(
                "model is not loaded".into(),
            ));
        }

        tracing::debug!(
            model = %self.config.model_name,
            backend = %self.active_backend,
            input_len = input.len(),
            "running inference"
        );

        // Delegate to the backend-specific runner.
        // When the `linux-candle` feature is enabled on mofa-foundation, callers
        // can use the existing LinuxCandleProvider there. This provider's role
        // is the hardware-detection and config layer that wraps any backend.
        match self.active_backend {
            ComputeBackend::Cuda => self.run_inference_cuda(input),
            ComputeBackend::Rocm => self.run_inference_rocm(input),
            ComputeBackend::Vulkan => self.run_inference_vulkan(input),
            ComputeBackend::Cpu => self.run_inference_cpu(input),
        }
    }

    async fn infer_stream(&self, input: &str) -> OrchestratorResult<BoxTokenStream> {
        use futures::StreamExt;
        use mofa_kernel::llm::{StreamChunk, FinishReason, StreamError};

        if !self.loaded {
            return Err(OrchestratorError::InferenceFailed(
                "model is not loaded".into(),
            ));
        }

        // Get the full inference result first
        let output = self.infer(input).await?;
        
        // Simulate token-by-token streaming
        let tokens: Vec<String> = output.split_whitespace().map(String::from).collect();
        
        let stream = futures::stream::iter(tokens)
            .then(|token| async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(25)).await;
                Ok::<_, StreamError>(StreamChunk::text(token))
            })
            .chain(futures::stream::once(async move {
                Ok(StreamChunk::done(FinishReason::Stop))
            }));
        
        Ok(Box::pin(stream))
    }

    fn memory_usage_bytes(&self) -> u64 {
        self.memory_usage
    }

    fn get_metadata(&self) -> HashMap<String, Value> {
        let mut m = HashMap::new();
        m.insert(
            "model_id".into(),
            Value::String(self.config.model_name.clone()),
        );
        m.insert(
            "backend".into(),
            Value::String(self.active_backend.to_string()),
        );
        m.insert(
            "model_path".into(),
            Value::String(self.config.model_path.clone()),
        );
        m.insert(
            "vram_bytes".into(),
            Value::Number(self.hardware.vram_bytes.into()),
        );
        m.insert(
            "available_backends".into(),
            Value::Array(
                self.hardware
                    .available_backends
                    .iter()
                    .map(|b| Value::String(b.to_string()))
                    .collect(),
            ),
        );
        if let Some(threads) = self.config.num_threads {
            m.insert("num_threads".into(), Value::Number(threads.into()));
        }
        m
    }

    async fn health_check(&self) -> OrchestratorResult<bool> {
        if !self.loaded {
            return Ok(false);
        }
        // Verify model file is still accessible
        Ok(std::path::Path::new(&self.config.model_path).exists())
    }
}

// ============================================================================
// Backend dispatch stubs
// ============================================================================
// These call the appropriate backend. When the linux-candle feature is active
// on mofa-foundation, real inference happens there. These stubs make the
// provider compile-time correct on any platform while keeping the dispatch
// logic centralized here.

impl LinuxLocalProvider {
    fn run_inference_cuda(&self, input: &str) -> OrchestratorResult<String> {
        tracing::debug!("dispatching to CUDA backend");
        self.run_inference_stub("cuda", input)
    }

    fn run_inference_rocm(&self, input: &str) -> OrchestratorResult<String> {
        tracing::debug!("dispatching to ROCm backend");
        self.run_inference_stub("rocm", input)
    }

    fn run_inference_vulkan(&self, input: &str) -> OrchestratorResult<String> {
        tracing::debug!("dispatching to Vulkan backend");
        self.run_inference_stub("vulkan", input)
    }

    fn run_inference_cpu(&self, input: &str) -> OrchestratorResult<String> {
        tracing::debug!("dispatching to CPU backend");
        self.run_inference_stub("cpu", input)
    }

    /// Stub that returns a realistic inference response.
    /// Replace this with real backend calls when integrating with candle / llama.cpp.
    fn run_inference_stub(&self, backend: &str, input: &str) -> OrchestratorResult<String> {
        // Generate a response that includes both the backend (for test compatibility)
        // and the inference result format (for demo streaming)
        let input_tokens = input.split_whitespace().count();
        let response = format!(
            "[{} backend] [local:{}] Inference result for: {} input_tokens={}",
            backend,
            self.config.model_name,
            input,
            input_tokens
        );
        Ok(response)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_provider() -> LinuxLocalProvider {
        LinuxLocalProvider::new(
            LinuxInferenceConfig::new("test-model", "/tmp/test.gguf")
                .with_backend(ComputeBackend::Cpu),
        )
        .expect("provider creation should succeed for CPU")
    }

    #[tokio::test]
    async fn test_load_missing_file_fails() {
        let mut p = make_provider();
        let result = p.load().await;
        assert!(result.is_err());
        assert!(matches!(result, Err(OrchestratorError::ModelLoadFailed(_))));
    }

    #[tokio::test]
    async fn test_infer_before_load_fails() {
        let p = make_provider();
        let result = p.infer("hello").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(OrchestratorError::InferenceFailed(_))));
    }

    #[test]
    fn test_provider_name() {
        let p = make_provider();
        assert_eq!(p.name(), "LinuxLocalProvider");
    }

    #[test]
    fn test_model_id() {
        let p = make_provider();
        assert_eq!(p.model_id(), "test-model");
    }

    #[test]
    fn test_not_loaded_initially() {
        let p = make_provider();
        assert!(!p.is_loaded());
    }

    #[test]
    fn test_memory_usage_zero_when_unloaded() {
        let p = make_provider();
        assert_eq!(p.memory_usage_bytes(), 0);
    }

    #[test]
    fn test_metadata_contains_backend() {
        let p = make_provider();
        let meta = p.get_metadata();
        assert!(meta.contains_key("backend"));
        assert_eq!(meta["backend"], Value::String("CPU".into()));
    }

    #[test]
    fn test_invalid_backend_override_fails() {
        // On a machine without CUDA, forcing cuda should fail
        // (This test passes trivially when CUDA is actually present)
        let info = HardwareInfo::detect();
        if !info.available_backends.contains(&ComputeBackend::Cuda) {
            let result = LinuxLocalProvider::new(
                LinuxInferenceConfig::new("model", "/tmp/x").with_backend(ComputeBackend::Cuda),
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_effective_memory_limit_default() {
        let p = make_provider();
        let limit = p.effective_memory_limit();
        assert!(limit > 0);
    }

    #[test]
    fn test_to_provider_config() {
        let p = make_provider();
        let cfg = p.to_provider_config();
        assert_eq!(cfg.model_name, "test-model");
        assert_eq!(cfg.device, "cpu");
        assert_eq!(cfg.model_type, ModelType::Llm);
    }

    #[tokio::test]
    async fn test_health_check_not_loaded() {
        let p = make_provider();
        let healthy = p.health_check().await.unwrap();
        assert!(!healthy);
    }

    // ========================================================================
    // Trait-dispatch tests: ensure ModelProvider methods are callable through
    // the trait interface (mirrors the bench pattern that was broken).
    // ========================================================================

    #[tokio::test]
    async fn test_infer_via_trait_object_before_load() {
        let p = make_provider();
        // Calling infer through a trait reference must produce InferenceFailed
        let provider: &dyn ModelProvider = &p;
        let result = provider.infer("hello").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(OrchestratorError::InferenceFailed(_))));
    }

    #[tokio::test]
    async fn test_load_then_infer_with_real_file() {
        // Create a temporary file so load() succeeds
        let dir = std::env::temp_dir().join("mofa_test_provider");
        std::fs::create_dir_all(&dir).unwrap();
        let model_file = dir.join("test.gguf");
        std::fs::write(&model_file, b"fake-model-bytes").unwrap();

        let config = LinuxInferenceConfig::new("test-model", model_file.to_str().unwrap())
            .with_backend(ComputeBackend::Cpu);
        let mut provider = LinuxLocalProvider::new(config).unwrap();

        // load succeeds with a real file
        provider.load().await.unwrap();
        assert!(provider.is_loaded());
        assert!(provider.memory_usage_bytes() > 0);

        // infer succeeds after load
        let response = provider.infer("test prompt").await.unwrap();
        assert!(!response.is_empty());
        assert!(response.contains("cpu"));

        // health_check reports healthy
        assert!(provider.health_check().await.unwrap());

        // unload resets state
        provider.unload().await.unwrap();
        assert!(!provider.is_loaded());
        assert_eq!(provider.memory_usage_bytes(), 0);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_double_load_is_idempotent() {
        let dir = std::env::temp_dir().join("mofa_test_double_load");
        std::fs::create_dir_all(&dir).unwrap();
        let model_file = dir.join("test.gguf");
        std::fs::write(&model_file, b"model-data").unwrap();

        let config = LinuxInferenceConfig::new("model", model_file.to_str().unwrap())
            .with_backend(ComputeBackend::Cpu);
        let mut p = LinuxLocalProvider::new(config).unwrap();

        p.load().await.unwrap();
        assert!(p.is_loaded());

        // second load should succeed without error
        p.load().await.unwrap();
        assert!(p.is_loaded());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_infer_after_unload_fails() {
        let dir = std::env::temp_dir().join("mofa_test_unload_infer");
        std::fs::create_dir_all(&dir).unwrap();
        let model_file = dir.join("test.gguf");
        std::fs::write(&model_file, b"data").unwrap();

        let config = LinuxInferenceConfig::new("m", model_file.to_str().unwrap())
            .with_backend(ComputeBackend::Cpu);
        let mut p = LinuxLocalProvider::new(config).unwrap();

        p.load().await.unwrap();
        p.unload().await.unwrap();

        // infer must fail after unload
        let result = p.infer("hello").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(OrchestratorError::InferenceFailed(_))));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_stub_response_format() {
        let dir = std::env::temp_dir().join("mofa_test_stub_fmt");
        std::fs::create_dir_all(&dir).unwrap();
        let model_file = dir.join("test.gguf");
        std::fs::write(&model_file, b"data").unwrap();

        let config = LinuxInferenceConfig::new("my-llama", model_file.to_str().unwrap())
            .with_backend(ComputeBackend::Cpu);
        let p = LinuxLocalProvider::new(config).unwrap();

        let out = p.run_inference_stub("cpu", "hello world");
        assert!(out.is_ok());
        let text = out.unwrap();
        assert!(text.contains("cpu backend"));
        assert!(text.contains("my-llama"));
        assert!(text.contains("input_tokens=2"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_metadata_keys_complete() {
        let p = make_provider();
        let meta = p.get_metadata();
        assert!(meta.contains_key("model_id"));
        assert!(meta.contains_key("backend"));
        assert!(meta.contains_key("model_path"));
        assert!(meta.contains_key("vram_bytes"));
        assert!(meta.contains_key("available_backends"));
    }

    #[test]
    fn test_model_type_is_llm() {
        let p = make_provider();
        assert_eq!(*p.model_type(), ModelType::Llm);
    }
}
