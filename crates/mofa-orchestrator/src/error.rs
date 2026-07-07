//! Unified error type for the orchestrator crate.

use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OrchestratorError {
    #[error("task analysis failed: {0}")]
    Analysis(String),

    #[error("swarm composition failed: {0}")]
    Composition(String),

    #[error("HITL gate rejected task '{task_id}': {reason}")]
    HitlRejected { task_id: String, reason: String },

    #[error("HITL gate timed out waiting for approval of task '{task_id}'")]
    HitlTimeout { task_id: String },

    #[error("governance check failed: {0}")]
    Governance(String),

    #[error("scheduler error: {0}")]
    Scheduler(String),

    #[error("notification delivery failed: {0}")]
    Notification(String),

    #[error("internal error: {0}")]
    Internal(String),
}

pub type OrchestratorResult<T> = Result<T, OrchestratorError>;
