//! MoFA Gateway - Framework-Level Control Plane and Gateway
//!
//! This crate provides a production-grade distributed control plane and gateway
//! for the MoFA framework, enabling multi-node coordination, consensus-based
//! state management, and intelligent request routing.
//!
//! # Architecture
//!
//! The gateway consists of two main components:
//!
//! 1. **Control Plane**: Distributed coordination using Raft consensus
//! 2. **Gateway Layer**: Request routing, load balancing, rate limiting
//!
//! # Quick Start
//!
//! ## Simple Gateway Mode (Default)
//!
//! ```rust,no_run
//! use mofa_gateway::{Gateway, GatewayConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Start gateway (simple mode - no distributed features)
//!     let mut gateway = Gateway::new(GatewayConfig::default()).await?;
//!     gateway.start().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Distributed Mode (with Raft Consensus)
//!
//! ```rust,no_run
//! use mofa_gateway::{ControlPlane, Gateway, ControlPlaneConfig, GatewayConfig};
//! use mofa_gateway::consensus::storage::RaftStorage;
//! use mofa_gateway::consensus::transport_impl::InMemoryTransport;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create storage and transport
//!     let storage = Arc::new(RaftStorage::new());
//!     let transport = Arc::new(InMemoryTransport::new());
//!
//!     // Start control plane
//!     let mut config = ControlPlaneConfig::default();
//!     config.cluster_nodes.push(config.node_id.clone());
//!     let control_plane = ControlPlane::new(config, storage, transport as _).await?;
//!     control_plane.start().await?;
//!
//!     // Start gateway with control plane
//!     let mut gateway = Gateway::with_control_plane(
//!         GatewayConfig::default(),
//!         Some(Arc::new(control_plane))
//!     ).await?;
//!     gateway.start().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Features
//!
//! - **Distributed Consensus**: Raft-based consensus algorithm
//! - **State Replication**: Replicated state machine for consistency
//! - **Load Balancing**: Multiple algorithms (round-robin, least-connections, weighted)
//! - **Rate Limiting**: Token bucket and sliding window algorithms
//! - **Health Checking**: Automatic node health monitoring
//! - **Circuit Breakers**: Prevent cascading failures
//! - **Observability**: Prometheus metrics, OpenTelemetry tracing

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod agent_executor;
pub mod consensus;
pub mod control_plane;
pub mod error;
pub mod gateway;
pub mod handlers;
pub mod inference_bridge;
pub mod middleware;
pub mod observability;
pub mod proxy;
pub mod server;
pub mod state;
pub mod state_machine;
pub mod types;

#[cfg(feature = "openai-compat")]
pub mod openai_compat;

/// Unified streaming abstractions (SSE + WebSocket) for gateway handlers.
///
/// # Modules
///
/// - [`streaming::SseBuilder`] — converts any [`mofa_kernel::llm::streaming::BoxTokenStream`]
///   into an OpenAI-compatible SSE response (`role` → content chunks → stop → `[DONE]`).
/// - [`streaming::proxy`] — SSE-aware HTTP proxy passthrough; streams `text/event-stream`
///   responses without buffering and strips hop-by-hop headers.
/// - [`streaming::ws`] — `GET /ws/v1/chat/completions` WebSocket endpoint with
///   mid-stream cancellation support.
#[cfg(feature = "openai-compat")]
pub mod streaming;

// Re-export main types
pub use control_plane::{ControlPlane, ControlPlaneConfig};
pub use error::{ControlPlaneError, GatewayError, GatewayResult};
pub use gateway::{Gateway, GatewayConfig};
pub use inference_bridge::{ChatCompletionRequest, ChatCompletionResponse, InferenceBridge};
pub use server::{GatewayServer, ServerConfig};
pub use types::*;
