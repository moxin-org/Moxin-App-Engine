//! Unified Inference Orchestration Layer.
//!
//! This module provides the runtime integration layer that composes
//! existing Idea 3 components into a single, policy-driven inference
//! control plane:
//!
//! - **Routing** (`routing.rs`): Policy engine for local vs cloud decisions
//! - **Model Pool** (`model_pool.rs`): LRU model cache with idle-timeout eviction
//! - **Orchestrator** (`orchestrator.rs`): Central control plane tying everything together
//! - **Types** (`types.rs`): Shared request/response types
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use mofa_foundation::inference::{
//!     InferenceOrchestrator, OrchestratorConfig, InferenceRequest,
//! };
//!
//! let config = OrchestratorConfig::default();
//! let mut orchestrator = InferenceOrchestrator::new(config);
//!
//! let request = InferenceRequest::new("llama-3-7b", "Hello!", 7168);
//! let result = orchestrator.infer(&request);
//!
//! println!("Routed to: {}", result.routed_to);
//! println!("Output: {}", result.output);
//! ```

pub mod model_pool;
pub mod orchestrator;
pub mod routing;
pub mod types;

// Re-export primary public API
pub use crate::scheduler::{AdmissionOutcome, DeferredRequest};
pub use orchestrator::{InferenceOrchestrator, OrchestratorConfig};
pub use routing::{RoutingDecision, RoutingPolicy};
pub use types::{InferenceRequest, InferenceResult, Precision, RequestPriority, RoutedBackend};
