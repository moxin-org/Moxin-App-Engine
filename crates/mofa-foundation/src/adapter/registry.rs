//! Adapter Registry for runtime model adapter discovery and management
//!
//! This module provides the [`AdapterRegistry`] which manages adapter registration,
//! discovery, and resolution based on model configuration and hardware profiles.

use std::collections::HashMap;

use super::config::{HardwareProfile, ModelConfig};
use super::descriptor::{AdapterDescriptor, Modality, ModelFormat};
use super::error::{AdapterError, RejectionReason, ResolutionError};

/// Result of adapter resolution containing the selected adapter and any rejection reasons
#[derive(Debug)]
pub struct ResolutionResult {
    /// The selected adapter
    pub adapter: AdapterDescriptor,
    /// Reasons why other candidates were rejected (for debugging/logging)
    pub rejection_reasons: HashMap<String, Vec<RejectionReason>>,
}

/// Registry for managing model adapters
///
/// The registry provides:
/// - Register/discover adapters at runtime
/// - Resolve candidates for a given ModelConfig + HardwareProfile
/// - Deterministic selection with structured rejection reasons
#[derive(Debug, Default)]
pub struct AdapterRegistry {
    adapters: HashMap<String, AdapterDescriptor>,
}

impl AdapterRegistry {
    /// Create a new empty adapter registry
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    /// Register an adapter with the registry
    ///
    /// # Errors
    /// Returns [`AdapterError::AlreadyRegistered`] if an adapter with the same ID already exists
    pub fn register(&mut self, adapter: AdapterDescriptor) -> Result<(), AdapterError> {
        let id = adapter.id.clone();
        if self.adapters.contains_key(&id) {
            return Err(AdapterError::AlreadyRegistered(id));
        }
        self.adapters.insert(id, adapter);
        Ok(())
    }

    /// Register an adapter, replacing any existing adapter with the same ID
    pub fn register_or_replace(&mut self, adapter: AdapterDescriptor) {
        self.adapters.insert(adapter.id.clone(), adapter);
    }

    /// Unregister an adapter from the registry
    ///
    /// # Errors
    /// Returns [`AdapterError::NotFound`] if no adapter with the given ID exists
    pub fn unregister(&mut self, id: &str) -> Result<AdapterDescriptor, AdapterError> {
        self.adapters
            .remove(id)
            .ok_or_else(|| AdapterError::NotFound(id.to_string()))
    }

    /// Get an adapter by ID
    pub fn get(&self, id: &str) -> Option<&AdapterDescriptor> {
        self.adapters.get(id)
    }

    /// Get all registered adapters
    pub fn adapters(&self) -> impl Iterator<Item = &AdapterDescriptor> {
        self.adapters.values()
    }

    /// Get the number of registered adapters
    pub fn len(&self) -> usize {
        self.adapters.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.adapters.is_empty()
    }

    /// Resolve the best adapter for the given model configuration and hardware profile
    ///
    /// This method performs deterministic resolution with the following steps:
    /// 1. Filter by modality (hard constraint)
    /// 2. Filter by format (hard constraint)
    /// 3. Filter by quantization (hard constraint, if specified)
    /// 4. Filter by hardware compatibility (hard constraint)
    /// 5. Filter by priority threshold (soft constraint)
    /// 6. Filter out experimental adapters unless explicitly allowed
    /// 7. Select highest priority, then by name for deterministic tie-break
    ///
    /// # Errors
    /// Returns [`ResolutionError::NoCompatibleAdapter`] if no compatible adapter is found
    pub fn resolve(
        &self,
        config: &ModelConfig,
        hardware: &HardwareProfile,
    ) -> Result<ResolutionResult, ResolutionError> {
        self.resolve_with_rejections(config, hardware)
            .map(|(adapter, reasons)| ResolutionResult {
                adapter,
                rejection_reasons: reasons,
            })
    }

    /// Resolve adapter with detailed rejection reasons for each candidate
    fn resolve_with_rejections(
        &self,
        config: &ModelConfig,
        hardware: &HardwareProfile,
    ) -> Result<(AdapterDescriptor, HashMap<String, Vec<RejectionReason>>), ResolutionError> {
        let mut candidates: Vec<(AdapterDescriptor, Vec<RejectionReason>)> = Vec::new();

        for adapter in self.adapters.values() {
            let mut rejections = Vec::new();

            // Step 1: Check modality support (hard constraint)
            if !adapter.supports_modality(&config.required_modality) {
                rejections.push(RejectionReason::ModalityMismatch {
                    required: config.required_modality.to_string(),
                    supported: adapter
                        .supported_modalities
                        .iter()
                        .map(|m| m.to_string())
                        .collect(),
                });
            }

            // Step 2: Check format support (hard constraint)
            if let Some(ref required_format) = config.required_format {
                let format = parse_format(required_format);
                if !adapter.supports_format(&format) {
                    rejections.push(RejectionReason::FormatMismatch {
                        required: required_format.clone(),
                        supported: adapter
                            .supported_formats
                            .iter()
                            .map(|f| f.to_string())
                            .collect(),
                    });
                }
            }

            // Step 3: Check quantization support (hard constraint, if specified)
            if let Some(ref required_quant) = config.required_quantization
                && !adapter.supports_quantization(required_quant)
            {
                rejections.push(RejectionReason::QuantizationMismatch {
                    required: required_quant.clone(),
                    supported: adapter
                        .supported_quantizations
                        .iter()
                        .map(|q| q.name.clone())
                        .collect(),
                });
            }

            // Step 4: Check hardware compatibility (hard constraint)
            if !adapter.supports_hardware(hardware) {
                rejections.push(RejectionReason::HardwareConstraint {
                    constraint: "hardware_compatibility".to_string(),
                    reason: "Adapter does not meet hardware requirements".to_string(),
                });
            }

            // Step 5: Check priority threshold (soft constraint)
            if let Some(min_priority) = config.min_priority
                && adapter.priority < min_priority
            {
                rejections.push(RejectionReason::PriorityTooLow {
                    required_min_priority: min_priority,
                    adapter_priority: adapter.priority,
                });
            }

            // Step 6: Check experimental flag
            if adapter.experimental && !config.allow_experimental {
                rejections.push(RejectionReason::HardwareConstraint {
                    constraint: "experimental".to_string(),
                    reason: "Experimental adapter not allowed".to_string(),
                });
            }

            // If no hard constraints failed, add as candidate
            let has_hard_rejection = rejections
                .iter()
                .any(|r| r.severity() == super::error::RejectionSeverity::Hard);

            if !has_hard_rejection {
                candidates.push((adapter.clone(), rejections));
            } else {
                // Store rejection reasons for debugging
                candidates.push((adapter.clone(), rejections));
            }
        }

        // Filter to only candidates without hard rejections
        let valid_candidates: Vec<_> = candidates
            .iter()
            .filter(|(_, reasons)| {
                !reasons
                    .iter()
                    .any(|r| r.severity() == super::error::RejectionSeverity::Hard)
            })
            .collect();

        if valid_candidates.is_empty() {
            return Err(ResolutionError::NoCompatibleAdapter);
        }

        // Sort by priority (descending), then by name (ascending) for deterministic tie-break
        let mut sorted_candidates: Vec<_> = valid_candidates
            .iter()
            .map(|(adapter, reasons)| (adapter, reasons))
            .collect();

        sorted_candidates.sort_by(|a, b| {
            let priority_cmp = b.0.priority.cmp(&a.0.priority);
            if priority_cmp == std::cmp::Ordering::Equal {
                a.0.name.cmp(&b.0.name)
            } else {
                priority_cmp
            }
        });

        // Clone the best adapter to avoid borrow issues
        let best_adapter = sorted_candidates[0].0.clone();
        let best_adapter_id = best_adapter.id.clone();

        // Collect rejection reasons for all other candidates
        let rejection_reasons: HashMap<String, Vec<RejectionReason>> = candidates
            .into_iter()
            .filter(|(adapter, _)| adapter.id != best_adapter_id)
            .map(|(adapter, reasons)| (adapter.id, reasons))
            .collect();

        Ok((best_adapter, rejection_reasons))
    }

    /// Find all adapters that support a given modality
    pub fn find_by_modality(
        &self,
        modality: &Modality,
    ) -> impl Iterator<Item = &AdapterDescriptor> {
        self.adapters
            .values()
            .filter(|a| a.supports_modality(modality))
    }

    /// Find all adapters that support a given format
    pub fn find_by_format(&self, format: &ModelFormat) -> impl Iterator<Item = &AdapterDescriptor> {
        self.adapters.values().filter(|a| a.supports_format(format))
    }

    /// Find all adapters compatible with the given hardware profile
    pub fn find_compatible(
        &self,
        hardware: &HardwareProfile,
    ) -> impl Iterator<Item = &AdapterDescriptor> {
        self.adapters
            .values()
            .filter(|a| a.supports_hardware(hardware))
    }
}

/// Parse a format string into a ModelFormat enum
fn parse_format(format_str: &str) -> ModelFormat {
    match format_str.to_lowercase().as_str() {
        "safetensors" => ModelFormat::Safetensors,
        "gguf" => ModelFormat::GGUF,
        "pytorch" | "pt" | "ckpt" => ModelFormat::Pytorch,
        "onnx" => ModelFormat::Onnx,
        "tensorflow" | "tf" => ModelFormat::Tensorflow,
        "mlx" => ModelFormat::MLX,
        "ggml" => ModelFormat::GGML,
        other => ModelFormat::Custom(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_descriptors() -> Vec<AdapterDescriptor> {
        vec![
            AdapterDescriptor::builder()
                .id("llama-cpp")
                .name("Llama.cpp Backend")
                .supported_modality(Modality::LLM)
                .supported_format(ModelFormat::GGUF)
                .supported_quantizations(vec!["q4_k".to_string(), "q8_0".to_string()])
                .priority(100)
                .build()
                .unwrap(),
            AdapterDescriptor::builder()
                .id("huggingface")
                .name("HuggingFace Backend")
                .supported_modality(Modality::LLM)
                .supported_format(ModelFormat::Safetensors)
                .supported_quantizations(vec!["bf16".to_string(), "fp16".to_string()])
                .priority(90)
                .build()
                .unwrap(),
            AdapterDescriptor::builder()
                .id("mlx-backend")
                .name("Apple MLX Backend")
                .supported_modality(Modality::LLM)
                .supported_modality(Modality::VLM)
                .supported_format(ModelFormat::MLX)
                .priority(80)
                .build()
                .unwrap(),
        ]
    }

    #[test]
    fn test_register_adapter() {
        let mut registry = AdapterRegistry::new();
        let adapter = AdapterDescriptor::builder()
            .id("test")
            .name("Test")
            .supported_modality(Modality::LLM)
            .supported_format(ModelFormat::Safetensors)
            .build()
            .unwrap();

        assert!(registry.register(adapter.clone()).is_ok());
        assert!(registry.register(adapter).is_err()); // Duplicate
    }

    #[test]
    fn test_resolve_basic() {
        let mut registry = AdapterRegistry::new();
        for adapter in create_test_descriptors() {
            registry.register(adapter).unwrap();
        }

        let config = ModelConfig::builder()
            .model_id("llama-3-8b")
            .required_modality(Modality::LLM)
            .required_format("gguf")
            .build()
            .unwrap();

        let hardware = HardwareProfile::builder()
            .os("linux")
            .cpu_family("x86_64")
            .gpu_available(true)
            .gpu_type("cuda")
            .build();

        let result = registry.resolve(&config, &hardware);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().adapter.id, "llama-cpp");
    }

    #[test]
    fn test_resolve_no_compatible() {
        let mut registry = AdapterRegistry::new();
        for adapter in create_test_descriptors() {
            registry.register(adapter).unwrap();
        }

        let config = ModelConfig::builder()
            .model_id("unknown-model")
            .required_modality(Modality::ASR) // Not supported by any adapter
            .build()
            .unwrap();

        let hardware = HardwareProfile::default();

        let result = registry.resolve(&config, &hardware);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ResolutionError::NoCompatibleAdapter
        ));
    }

    #[test]
    fn test_resolve_deterministic_tie_break() {
        let mut registry = AdapterRegistry::new();

        // Two adapters with same priority
        registry
            .register(
                AdapterDescriptor::builder()
                    .id("adapter-a")
                    .name("Adapter A")
                    .supported_modality(Modality::LLM)
                    .supported_format(ModelFormat::Safetensors)
                    .priority(100)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        registry
            .register(
                AdapterDescriptor::builder()
                    .id("adapter-b")
                    .name("Adapter B")
                    .supported_modality(Modality::LLM)
                    .supported_format(ModelFormat::Safetensors)
                    .priority(100)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        let config = ModelConfig::builder()
            .model_id("test")
            .required_modality(Modality::LLM)
            .required_format("safetensors")
            .build()
            .unwrap();

        let hardware = HardwareProfile::default();

        let result = registry.resolve(&config, &hardware).unwrap();
        // Should select "Adapter A" (alphabetically first due to tie-break)
        assert_eq!(result.adapter.name, "Adapter A");
    }

    #[test]
    fn test_find_by_modality() {
        let mut registry = AdapterRegistry::new();
        for adapter in create_test_descriptors() {
            registry.register(adapter).unwrap();
        }

        let llm_adapters: Vec<_> = registry.find_by_modality(&Modality::LLM).collect();
        assert_eq!(llm_adapters.len(), 3);

        let vlm_adapters: Vec<_> = registry.find_by_modality(&Modality::VLM).collect();
        assert_eq!(vlm_adapters.len(), 1);
    }
}
