//! Adapter descriptor for model inference backends
//!
//! This module defines the [`AdapterDescriptor`] which describes an adapter's
//! capabilities including supported modalities, model formats, quantization profiles,
//! and hardware constraints.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::error::AdapterError;

/// Supported model modalities
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    /// Large Language Model (text generation, chat)
    LLM,
    /// Vision-Language Model (image understanding, multimodal)
    VLM,
    /// Automatic Speech Recognition (speech to text)
    ASR,
    /// Text-to-Speech (text to speech)
    TTS,
    /// Embedding model (text to vector)
    Embedding,
    /// Image generation model
    Diffusion,
    /// Mixture of Experts model
    MoE,
}

impl std::fmt::Display for Modality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Modality::LLM => write!(f, "llm"),
            Modality::VLM => write!(f, "vlm"),
            Modality::ASR => write!(f, "asr"),
            Modality::TTS => write!(f, "tts"),
            Modality::Embedding => write!(f, "embedding"),
            Modality::Diffusion => write!(f, "diffusion"),
            Modality::MoE => write!(f, "moe"),
        }
    }
}

/// Supported model formats
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelFormat {
    /// Safetensors format (HuggingFace)
    Safetensors,
    /// GGUF format (llama.cpp)
    GGUF,
    /// PyTorch checkpoint
    Pytorch,
    /// ONNX format
    Onnx,
    /// TensorFlow SavedModel
    Tensorflow,
    /// MLX format (Apple Silicon)
    MLX,
    /// GGML format (legacy llama.cpp)
    GGML,
    /// Custom/Proprietary format
    Custom(String),
}

impl std::fmt::Display for ModelFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelFormat::Safetensors => write!(f, "safetensors"),
            ModelFormat::GGUF => write!(f, "gguf"),
            ModelFormat::Pytorch => write!(f, "pytorch"),
            ModelFormat::Onnx => write!(f, "onnx"),
            ModelFormat::Tensorflow => write!(f, "tensorflow"),
            ModelFormat::MLX => write!(f, "mlx"),
            ModelFormat::GGML => write!(f, "ggml"),
            ModelFormat::Custom(name) => write!(f, "custom:{}", name),
        }
    }
}

impl ModelFormat {
    /// Check if this format is compatible with another format
    /// Some formats can be converted to others
    pub fn is_compatible_with(&self, other: &ModelFormat) -> bool {
        match (self, other) {
            // GGML and GGUF are compatible (GGUF is the successor)
            (ModelFormat::GGML, ModelFormat::GGUF) | (ModelFormat::GGUF, ModelFormat::GGML) => true,
            // Same format is always compatible
            _ => self == other,
        }
    }
}

/// Quantization profile for model inference
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuantizationProfile {
    /// Quantization name (e.g., "q4_k", "q8_0", "f16")
    pub name: String,
    /// Bits per parameter
    pub bits: u8,
    /// Description of the quantization
    pub description: Option<String>,
}

impl QuantizationProfile {
    /// Create a new quantization profile
    pub fn new(name: impl Into<String>, bits: u8) -> Self {
        Self {
            name: name.into(),
            bits,
            description: None,
        }
    }

    /// Create a new quantization profile with description
    pub fn with_description(
        name: impl Into<String>,
        bits: u8,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            bits,
            description: Some(description.into()),
        }
    }
}

impl std::fmt::Display for QuantizationProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}bit)", self.name, self.bits)
    }
}

/// Hardware constraints for adapter execution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HardwareConstraint {
    /// Minimum RAM required in MB
    pub min_ram_mb: Option<u64>,
    /// Minimum VRAM required in MB
    pub min_vram_mb: Option<u64>,
    /// Required OS (if any)
    pub required_os: Option<Vec<String>>,
    /// Required CPU features
    pub required_cpu_features: Option<Vec<String>>,
    /// Required GPU type (if any)
    pub required_gpu_type: Option<String>,
    /// Whether GPU is required
    pub gpu_required: bool,
}

impl HardwareConstraint {
    /// Create a new hardware constraint builder
    pub fn builder() -> HardwareConstraintBuilder {
        HardwareConstraintBuilder::new()
    }

    /// Check if this hardware constraint is satisfied by the given requirements
    pub fn is_satisfied_by(&self, profile: &super::HardwareProfile) -> bool {
        // Check RAM requirement
        if let Some(min_ram) = self.min_ram_mb
            && profile.available_ram_mb < min_ram
        {
            return false;
        }

        // Check VRAM requirement
        if let Some(min_vram) = self.min_vram_mb
            && profile.available_vram_mb.unwrap_or(0) < min_vram
        {
            return false;
        }

        // Check GPU requirement
        if self.gpu_required && !profile.gpu_available {
            return false;
        }

        // Check GPU type requirement
        if let Some(ref required_gpu) = self.required_gpu_type {
            if let Some(ref available_gpu) = profile.gpu_type {
                if available_gpu != required_gpu {
                    return false;
                }
            } else {
                return false;
            }
        }

        // Check OS requirement
        if let Some(ref required_os) = self.required_os
            && !required_os.is_empty()
            && !required_os.contains(&profile.os)
        {
            return false;
        }

        true
    }
}

/// Builder for hardware constraints
#[derive(Debug)]
pub struct HardwareConstraintBuilder {
    constraint: HardwareConstraint,
}

impl Default for HardwareConstraintBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl HardwareConstraintBuilder {
    pub fn new() -> Self {
        Self {
            constraint: HardwareConstraint::default(),
        }
    }

    pub fn min_ram_mb(mut self, mb: u64) -> Self {
        self.constraint.min_ram_mb = Some(mb);
        self
    }

    pub fn min_vram_mb(mut self, mb: u64) -> Self {
        self.constraint.min_vram_mb = Some(mb);
        self
    }

    pub fn required_os(mut self, os: Vec<String>) -> Self {
        self.constraint.required_os = Some(os);
        self
    }

    pub fn required_gpu_type(mut self, gpu_type: impl Into<String>) -> Self {
        self.constraint.required_gpu_type = Some(gpu_type.into());
        self
    }

    pub fn gpu_required(mut self, required: bool) -> Self {
        self.constraint.gpu_required = required;
        self
    }

    pub fn build(self) -> HardwareConstraint {
        self.constraint
    }
}

/// Adapter descriptor describing an inference backend's capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterDescriptor {
    /// Unique identifier for this adapter
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Optional description
    pub description: Option<String>,
    /// Supported modalities
    pub supported_modalities: HashSet<Modality>,
    /// Supported model formats
    pub supported_formats: HashSet<ModelFormat>,
    /// Supported quantization profiles
    pub supported_quantizations: Vec<QuantizationProfile>,
    /// Hardware constraints
    pub hardware_constraint: HardwareConstraint,
    /// Priority (higher = preferred when all else equal)
    pub priority: i32,
    /// Estimated latency in ms (for scoring)
    pub estimated_latency_ms: Option<u32>,
    /// Whether this adapter is experimental
    pub experimental: bool,
}

impl AdapterDescriptor {
    /// Create a new adapter descriptor builder
    pub fn builder() -> AdapterDescriptorBuilder {
        AdapterDescriptorBuilder::new()
    }

    /// Check if this adapter supports the given modality
    pub fn supports_modality(&self, modality: &Modality) -> bool {
        self.supported_modalities.contains(modality)
    }

    /// Check if this adapter supports the given format
    pub fn supports_format(&self, format: &ModelFormat) -> bool {
        self.supported_formats
            .iter()
            .any(|f| f.is_compatible_with(format))
    }

    /// Check if this adapter supports the given quantization
    pub fn supports_quantization(&self, quantization: &str) -> bool {
        self.supported_quantizations
            .iter()
            .any(|q| q.name == quantization)
    }

    /// Check if this adapter can handle the given hardware profile
    pub fn supports_hardware(&self, profile: &super::HardwareProfile) -> bool {
        self.hardware_constraint.is_satisfied_by(profile)
    }
}

/// Builder for adapter descriptors
#[derive(Debug)]
pub struct AdapterDescriptorBuilder {
    descriptor: AdapterDescriptor,
}

impl Default for AdapterDescriptorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl AdapterDescriptorBuilder {
    pub fn new() -> Self {
        Self {
            descriptor: AdapterDescriptor {
                id: String::new(),
                name: String::new(),
                description: None,
                supported_modalities: HashSet::new(),
                supported_formats: HashSet::new(),
                supported_quantizations: Vec::new(),
                hardware_constraint: HardwareConstraint::default(),
                priority: 0,
                estimated_latency_ms: None,
                experimental: false,
            },
        }
    }

    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.descriptor.id = id.into();
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.descriptor.name = name.into();
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.descriptor.description = Some(description.into());
        self
    }

    pub fn supported_modality(mut self, modality: Modality) -> Self {
        self.descriptor.supported_modalities.insert(modality);
        self
    }

    pub fn supported_modalities(mut self, modalities: impl IntoIterator<Item = Modality>) -> Self {
        self.descriptor.supported_modalities.extend(modalities);
        self
    }

    pub fn supported_format(mut self, format: ModelFormat) -> Self {
        self.descriptor.supported_formats.insert(format);
        self
    }

    pub fn supported_formats(mut self, formats: impl IntoIterator<Item = ModelFormat>) -> Self {
        self.descriptor.supported_formats.extend(formats);
        self
    }

    pub fn supported_quantization(mut self, quantization: impl Into<String>) -> Self {
        self.descriptor
            .supported_quantizations
            .push(QuantizationProfile::new(quantization, 0));
        self
    }

    pub fn supported_quantizations(mut self, quantizations: Vec<String>) -> Self {
        self.descriptor.supported_quantizations = quantizations
            .into_iter()
            .map(|q| QuantizationProfile::new(q, 0))
            .collect();
        self
    }

    pub fn hardware_constraint(mut self, constraint: HardwareConstraint) -> Self {
        self.descriptor.hardware_constraint = constraint;
        self
    }

    pub fn priority(mut self, priority: i32) -> Self {
        self.descriptor.priority = priority;
        self
    }

    pub fn estimated_latency_ms(mut self, latency_ms: u32) -> Self {
        self.descriptor.estimated_latency_ms = Some(latency_ms);
        self
    }

    pub fn experimental(mut self, experimental: bool) -> Self {
        self.descriptor.experimental = experimental;
        self
    }

    pub fn build(self) -> Result<AdapterDescriptor, super::error::AdapterError> {
        if self.descriptor.id.is_empty() {
            return Err(super::error::AdapterError::InvalidConfig(
                "AdapterDescriptor must have an id".into(),
            ));
        }
        if self.descriptor.name.is_empty() {
            return Err(super::error::AdapterError::InvalidConfig(
                "AdapterDescriptor must have a name".into(),
            ));
        }
        if self.descriptor.supported_modalities.is_empty() {
            return Err(super::error::AdapterError::InvalidConfig(
                "AdapterDescriptor must have at least one supported modality".into(),
            ));
        }
        if self.descriptor.supported_formats.is_empty() {
            return Err(super::error::AdapterError::InvalidConfig(
                "AdapterDescriptor must have at least one supported format".into(),
            ));
        }
        Ok(self.descriptor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapter_descriptor_builder() {
        let descriptor = AdapterDescriptor::builder()
            .id("test-adapter")
            .name("Test Adapter")
            .description("A test adapter")
            .supported_modality(Modality::LLM)
            .supported_format(ModelFormat::Safetensors)
            .supported_quantization("q4_k")
            .priority(100)
            .build()
            .unwrap();

        assert_eq!(descriptor.id, "test-adapter");
        assert_eq!(descriptor.name, "Test Adapter");
        assert!(descriptor.supports_modality(&Modality::LLM));
        assert!(descriptor.supports_format(&ModelFormat::Safetensors));
        assert!(descriptor.supports_quantization("q4_k"));
        assert_eq!(descriptor.priority, 100);
    }

    #[test]
    fn test_format_compatibility() {
        assert!(ModelFormat::GGUF.is_compatible_with(&ModelFormat::GGML));
        assert!(ModelFormat::GGML.is_compatible_with(&ModelFormat::GGUF));
        assert!(ModelFormat::Safetensors.is_compatible_with(&ModelFormat::Safetensors));
        assert!(!ModelFormat::Safetensors.is_compatible_with(&ModelFormat::GGUF));
    }

    #[test]
    fn test_modality_display() {
        assert_eq!(Modality::LLM.to_string(), "llm");
        assert_eq!(Modality::VLM.to_string(), "vlm");
    }
}
