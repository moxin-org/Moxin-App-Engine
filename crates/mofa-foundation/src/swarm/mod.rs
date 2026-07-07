pub mod analyzer;
pub mod config;
pub mod dag;
pub mod hitl_gate;
pub mod patterns;
pub mod telemetry;

pub use analyzer::{RiskAwareAnalysis, RiskSummary, TaskAnalyzer};
pub use config::{
    AgentSpec, AuditEvent, AuditEventKind, HITLMode, SLAConfig, SwarmConfig, SwarmMetrics,
    SwarmResult, SwarmStatus,
};
pub use dag::{DependencyEdge, DependencyKind, RiskLevel, SubtaskDAG, SubtaskStatus, SwarmSubtask};
pub use hitl_gate::{HITLDecision, HITLGateMetrics, HITLNotifier, SwarmHITLGate};
pub use patterns::CoordinationPattern;
pub mod scheduler;
pub use scheduler::{
    FailurePolicy, ParallelScheduler, DebateScheduler, SchedulerSummary, SequentialScheduler, SubtaskExecutorFn,
    SwarmScheduler, SwarmSchedulerConfig, TaskExecutionResult, TaskOutcome,
};
pub use telemetry::{audit_batch_to_debug, audit_to_debug};
