//! Runtime Model Adapter Registry with Capability/Format Negotiation
//!
//! This module provides:
//! - [`AdapterDescriptor`]: Describes adapter capabilities (modalities, formats, quantization, hardware)
//! - [`AdapterRegistry`]: Runtime adapter registration and discovery
//! - [`ModelConfig`]: Configuration for model selection
//! - [`HardwareProfile`]: Hardware requirements for model execution
//! - Deterministic resolver with hard constraints and stable tie-break rules
//!
//! # Example
//!
//! ```rust
//! use mofa_foundation::adapter::{AdapterDescriptor, AdapterRegistry, ModelConfig, Modality, ModelFormat, HardwareProfile};
//! use mofa_foundation::hardware::{HardwareCapability, GpuType, OsClassification, CpuFamily};
//!
//! // Create a registry
//! let mut registry = AdapterRegistry::new();
//!
//! // Register an adapter for LLM with safetensors format
//! let descriptor = AdapterDescriptor::builder()
//!     .id("llama-cpp-backend")
//!     .name("Llama.cpp Backend")
//!     .supported_modality(Modality::LLM)
//!     .supported_format(ModelFormat::Safetensors)
//!     .supported_quantization("q4_k".to_string())
//!     .priority(100)
//!     .build()?;
//! registry.register(descriptor);
//!
//! // Resolve adapter for a given model config and hardware
//! let config = ModelConfig::builder()
//!     .model_id("llama-3-8b")
//!     .required_modality(Modality::LLM)
//!     .required_format_model(ModelFormat::Safetensors)
//!     .build()?;
//!
//! let hw_capability = HardwareCapability {
//!     os: OsClassification::Linux,
//!     cpu_family: CpuFamily::X86_64,
//!     gpu_available: true,
//!     gpu_type: Some(GpuType::Cuda),
//!     total_memory_bytes: 32_000_000_000,
//!     available_memory_bytes: 16_000_000_000,
//! };
//!
//! let hardware = HardwareProfile::from(hw_capability);
//!
//! let result = registry.resolve(&config, &hardware);
//! assert!(result.is_ok());
//! ```

pub mod config;
pub mod descriptor;
pub mod error;
pub mod registry;
pub mod resolver;
pub mod scheduler;

pub use config::{HardwareProfile, ModelConfig};
pub use descriptor::{AdapterDescriptor, Modality, ModelFormat, QuantizationProfile};
pub use error::{AdapterError, RejectionReason, ResolutionError};
pub use registry::AdapterRegistry;
pub use scheduler::{
    AdmissionDecision, AdmissionReason, DeferredQueue, DeferredRequest, MemoryBudget,
    MemoryThresholds, Scheduler, SchedulerPolicy, StabilityControl,
};
