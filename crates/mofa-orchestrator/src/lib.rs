//! # mofa-orchestrator
//!
//! High-level entry point for multi-agent goal execution in the MoFA framework.
//!
//! This crate implements the **Cognitive Swarm Orchestrator** proposed in
//! GSoC 2026 Idea 5. It connects a natural-language goal to a coordinated
//! team of specialized agents via a seven-stage pipeline:
//!
//! ```text
//! Goal (string)
//!   -> TaskAnalyzer      (LLM-driven SubtaskDAG with risk levels)
//!   -> SwarmComposer     (cost-aware agent assignment, 7 patterns)
//!   -> HITLGovernor      (suspend at trust boundaries, multi-channel notify)
//!   -> GovernanceLayer   (RBAC checks, SLA enforcement, audit trail)
//!   -> Scheduler         (SequentialScheduler | ParallelScheduler)
//!   -> SemanticDiscovery (BM25 + dense-vector RRF agent lookup)
//!   -> SmithObservatory  (pluggable TraceBackend, OTel spans)
//! ```
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use mofa_orchestrator::orchestrator::{SwarmOrchestrator, OrchestratorConfig};
//!
//! #[tokio::main]
//! async fn main() {
//!     let orch = SwarmOrchestrator::new(OrchestratorConfig::default());
//!     let result = orch
//!         .run_goal("review these contracts for compliance issues")
//!         .await
//!         .expect("orchestration failed");
//!     println!("done: {} tasks succeeded", result.tasks_succeeded);
//! }
//! ```
//!
//! ## GSoC Implementation Status
//!
//! This crate establishes the public API surface and the module structure.
//! Each pipeline stage is stubbed with a `// TODO: GSoC Phase N` comment
//! marking the exact deliverable. All stubs compile and the tests pass.
//! Full wiring is the GSoC 2026 deliverable.

pub mod error;
pub mod gateway_client;
pub mod governance;
pub mod notifiers;
pub mod orchestrator;

pub use error::{OrchestratorError, OrchestratorResult};
pub use gateway_client::{
    AuditEntry, CachedCapability, CacheStats, CapabilityKind, CapabilityRequest,
    CapabilityResponse, GatewayCapabilityClient, GatewayClientConfig, GatewayClientError,
    GatewayTransport, MockGatewayTransport, SwarmCapabilityRegistry,
};
pub use governance::{Action, GovernanceLayer, Role, SlaViolation};
pub use notifiers::{FeishuNotifier, GateEvent, GateEventKind, LogNotifier, Notifier, SlackNotifier, TelegramNotifier};
pub use orchestrator::{OrchestratorConfig, SwarmOrchestrator, SwarmResult};
