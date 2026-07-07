//! Model configuration and hardware profile types
//!
//! This module defines the configuration types used for adapter resolution.

use serde::{Deserialize, Serialize};

use super::descriptor::{Modality, ModelFormat};
use crate::hardware::{CpuFamily, GpuType, HardwareCapability, OsClassification};

/// Configuration for a model request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Model identifier (e.g., "llama-3-8b", "mistral-7b")
    pub model_id: String,
    /// Required modality (LLM, VLM, ASR, TTS, Embedding)
    pub required_modality: Modality,
    /// Required model format
    pub required_format: Option<String>,
    /// Required quantization (if any)
    pub required_quantization: Option<String>,
    /// Minimum priority threshold (adapters below this priority are ignored)
    pub min_priority: Option<i32>,
    /// Preferred hardware constraints
    pub preferred_hardware: Option<HardwarePreferences>,
    /// Whether to allow experimental adapters
    pub allow_experimental: bool,
}

impl ModelConfig {
    /// Create a new model config builder
    pub fn builder() -> ModelConfigBuilder {
        ModelConfigBuilder::new()
    }
}

/// Builder for model configuration
#[derive(Debug)]
pub struct ModelConfigBuilder {
    config: ModelConfig,
}

impl Default for ModelConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: ModelConfig {
                model_id: String::new(),
                required_modality: Modality::LLM,
                required_format: None,
                required_quantization: None,
                min_priority: None,
                preferred_hardware: None,
                allow_experimental: false,
            },
        }
    }

    pub fn model_id(mut self, id: impl Into<String>) -> Self {
        self.config.model_id = id.into();
        self
    }

    pub fn required_modality(mut self, modality: Modality) -> Self {
        self.config.required_modality = modality;
        self
    }

    pub fn required_format(mut self, format: impl Into<String>) -> Self {
        self.config.required_format = Some(format.into());
        self
    }

    /// Set required format using ModelFormat enum
    pub fn required_format_model(mut self, format: ModelFormat) -> Self {
        self.config.required_format = Some(format.to_string());
        self
    }

    pub fn required_quantization(mut self, quantization: impl Into<String>) -> Self {
        self.config.required_quantization = Some(quantization.into());
        self
    }

    pub fn min_priority(mut self, priority: i32) -> Self {
        self.config.min_priority = Some(priority);
        self
    }

    pub fn preferred_hardware(mut self, hardware: HardwarePreferences) -> Self {
        self.config.preferred_hardware = Some(hardware);
        self
    }

    pub fn allow_experimental(mut self, allow: bool) -> Self {
        self.config.allow_experimental = allow;
        self
    }

    /// Build the ModelConfig, validating all required fields.
    ///
    /// # Errors
    ///
    /// Returns [`AdapterError::InvalidConfig`] if:
    /// - `model_id` is empty
    pub fn build(self) -> Result<ModelConfig, super::error::AdapterError> {
        if self.config.model_id.is_empty() {
            return Err(super::error::AdapterError::InvalidConfig(
                "ModelConfig must have a model_id".to_string(),
            ));
        }
        Ok(self.config)
    }
}

/// Hardware profile describing available hardware resources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareProfile {
    /// Operating system
    pub os: String,
    /// CPU family
    pub cpu_family: String,
    /// Available RAM in MB
    pub available_ram_mb: u64,
    /// Available VRAM in MB (if GPU available)
    pub available_vram_mb: Option<u64>,
    /// Whether GPU is available
    pub gpu_available: bool,
    /// GPU type (if available)
    pub gpu_type: Option<String>,
}

impl Default for HardwareProfile {
    fn default() -> Self {
        Self {
            os: "unknown".to_string(),
            cpu_family: "unknown".to_string(),
            available_ram_mb: 8192, // 8GB default
            available_vram_mb: None,
            gpu_available: false,
            gpu_type: None,
        }
    }
}

impl From<HardwareCapability> for HardwareProfile {
    fn from(capability: HardwareCapability) -> Self {
        let os = match capability.os {
            OsClassification::MacOS => "macos".to_string(),
            OsClassification::Windows => "windows".to_string(),
            OsClassification::Linux => "linux".to_string(),
            OsClassification::Other(s) => s,
        };

        let cpu_family = match capability.cpu_family {
            CpuFamily::AppleSilicon => "apple_silicon".to_string(),
            CpuFamily::X86_64 => "x86_64".to_string(),
            CpuFamily::Arm => "arm".to_string(),
            CpuFamily::Other(s) => s,
        };

        let gpu_type = capability.gpu_type.map(|g| match g {
            GpuType::Metal => "metal".to_string(),
            GpuType::Cuda => "cuda".to_string(),
            GpuType::Rocm => "rocm".to_string(),
            GpuType::IntelGpu => "intel_gpu".to_string(),
        });

        Self {
            os,
            cpu_family,
            available_ram_mb: 8192, // Default, would need system query
            available_vram_mb: None,
            gpu_available: capability.gpu_available,
            gpu_type,
        }
    }
}

impl HardwareProfile {
    /// Create a new hardware profile builder
    pub fn builder() -> HardwareProfileBuilder {
        HardwareProfileBuilder::new()
    }

    /// Check if this profile has a specific GPU type
    pub fn has_gpu_type(&self, gpu_type: &str) -> bool {
        self.gpu_available && self.gpu_type.as_deref() == Some(gpu_type)
    }

    /// Check if this profile meets minimum RAM requirement
    pub fn has_min_ram(&self, min_mb: u64) -> bool {
        self.available_ram_mb >= min_mb
    }

    /// Check if this profile meets minimum VRAM requirement
    pub fn has_min_vram(&self, min_mb: u64) -> bool {
        self.available_vram_mb.unwrap_or(0) >= min_mb
    }
}

/// Builder for hardware profile
#[derive(Debug)]
pub struct HardwareProfileBuilder {
    profile: HardwareProfile,
}

impl Default for HardwareProfileBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl HardwareProfileBuilder {
    pub fn new() -> Self {
        Self {
            profile: HardwareProfile::default(),
        }
    }

    pub fn os(mut self, os: impl Into<String>) -> Self {
        self.profile.os = os.into();
        self
    }

    pub fn cpu_family(mut self, cpu_family: impl Into<String>) -> Self {
        self.profile.cpu_family = cpu_family.into();
        self
    }

    pub fn available_ram_mb(mut self, mb: u64) -> Self {
        self.profile.available_ram_mb = mb;
        self
    }

    pub fn available_vram_mb(mut self, mb: u64) -> Self {
        self.profile.available_vram_mb = Some(mb);
        self
    }

    pub fn gpu_available(mut self, available: bool) -> Self {
        self.profile.gpu_available = available;
        self
    }

    pub fn gpu_type(mut self, gpu_type: impl Into<String>) -> Self {
        self.profile.gpu_type = Some(gpu_type.into());
        self.profile.gpu_available = true;
        self
    }

    pub fn build(self) -> HardwareProfile {
        self.profile
    }
}

/// Preferred hardware characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwarePreferences {
    /// Preferred GPU type
    pub preferred_gpu_type: Option<String>,
    /// Whether GPU is preferred
    pub prefer_gpu: bool,
    /// Maximum acceptable latency in ms
    pub max_latency_ms: Option<u32>,
    /// Maximum memory budget in MB
    pub max_memory_mb: Option<u64>,
}

impl Default for HardwarePreferences {
    fn default() -> Self {
        Self {
            preferred_gpu_type: None,
            prefer_gpu: true,
            max_latency_ms: None,
            max_memory_mb: None,
        }
    }
}

impl HardwarePreferences {
    /// Create a new hardware preferences builder
    pub fn builder() -> HardwarePreferencesBuilder {
        HardwarePreferencesBuilder::new()
    }
}

/// Builder for hardware preferences
#[derive(Debug)]
pub struct HardwarePreferencesBuilder {
    preferences: HardwarePreferences,
}

impl Default for HardwarePreferencesBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl HardwarePreferencesBuilder {
    pub fn new() -> Self {
        Self {
            preferences: HardwarePreferences::default(),
        }
    }

    pub fn preferred_gpu_type(mut self, gpu_type: impl Into<String>) -> Self {
        self.preferences.preferred_gpu_type = Some(gpu_type.into());
        self
    }

    pub fn prefer_gpu(mut self, prefer: bool) -> Self {
        self.preferences.prefer_gpu = prefer;
        self
    }

    pub fn max_latency_ms(mut self, latency_ms: u32) -> Self {
        self.preferences.max_latency_ms = Some(latency_ms);
        self
    }

    pub fn max_memory_mb(mut self, memory_mb: u64) -> Self {
        self.preferences.max_memory_mb = Some(memory_mb);
        self
    }

    pub fn build(self) -> HardwarePreferences {
        self.preferences
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_config_builder() {
        let config = ModelConfig::builder()
            .model_id("llama-3-8b")
            .required_modality(Modality::LLM)
            .required_format("safetensors")
            .required_quantization("q4_k")
            .min_priority(50)
            .build()
            .expect("valid model config");

        assert_eq!(config.model_id, "llama-3-8b");
        assert_eq!(config.required_modality, Modality::LLM);
        assert_eq!(config.required_format, Some("safetensors".to_string()));
        assert_eq!(config.required_quantization, Some("q4_k".to_string()));
        assert_eq!(config.min_priority, Some(50));
    }

    #[test]
    fn test_model_config_builder_missing_model_id() {
        let result = ModelConfig::builder()
            .required_modality(Modality::LLM)
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_hardware_profile_builder() {
        let profile = HardwareProfile::builder()
            .os("linux")
            .cpu_family("x86_64")
            .available_ram_mb(16384)
            .gpu_available(true)
            .gpu_type("cuda")
            .build();

        assert_eq!(profile.os, "linux");
        assert_eq!(profile.cpu_family, "x86_64");
        assert_eq!(profile.available_ram_mb, 16384);
        assert!(profile.gpu_available);
        assert_eq!(profile.gpu_type, Some("cuda".to_string()));
    }

    #[test]
    fn test_hardware_profile_has_gpu_type() {
        let profile = HardwareProfile::builder()
            .gpu_available(true)
            .gpu_type("cuda")
            .build();

        assert!(profile.has_gpu_type("cuda"));
        assert!(!profile.has_gpu_type("metal"));
    }
}
