//! FFI bindings for the Cognitive Swarm Orchestrator.
//!
//! Exposes `run_goal(goal)` as a synchronous call usable from Python,
//! Kotlin, and Swift without callers managing Tokio async runtimes.
//!
//! # Implementation note
//!
//! This module contains a self-contained stub that matches the public API
//! of `mofa_orchestrator::SwarmOrchestrator`. The stub is intentional:
//! `mofa-orchestrator` lives on a feature branch (`feat/mofa-orchestrator-skeleton`)
//! that has not yet merged into main. A follow-up PR will replace the stub
//! body with a real `mofa_orchestrator::SwarmOrchestrator::run_goal` call
//! once that branch merges, without any change to the FFI surface.
//!
//! All four tests below exercise the FFI layer (construction, UUID uniqueness,
//! goal round-trip, timing field) and will continue to pass after the swap.

use std::time::Instant;
use tokio::runtime::Runtime;

use crate::MoFaError;

/// FFI-safe result of a complete swarm execution.
///
/// Use `execution_id` to correlate with OTel traces in Jaeger and with
/// the JSONL audit log exported by `GovernanceLayer`.
#[derive(Debug, Clone)]
pub struct SwarmResultFFI {
    /// UUID for this execution run.
    pub execution_id: String,
    /// The original natural-language goal string.
    pub goal: String,
    /// Number of subtasks that completed successfully.
    pub tasks_succeeded: u64,
    /// Number of subtasks that failed or were rejected by a HITL gate.
    pub tasks_failed: u64,
    /// Total wall-clock time in milliseconds.
    pub wall_time_ms: u64,
}

/// FFI wrapper around the Cognitive Swarm Orchestrator.
///
/// Owns a dedicated Tokio runtime so callers in Python, Kotlin, and Swift
/// do not need to manage async execution themselves — `run_goal` blocks
/// until the swarm pipeline completes.
///
/// # Python (via PyO3)
///
/// ```python
/// from mofa import SwarmOrchestrator
///
/// orch = SwarmOrchestrator("compliance-swarm")
/// result = orch.run_goal("review Q1 loan applications for fair lending violations")
/// print(f"succeeded={result.tasks_succeeded}  id={result.execution_id}")
/// ```
///
/// # Kotlin (via UniFFI)
///
/// ```kotlin
/// val orch = SwarmOrchestratorFFI("compliance-swarm")
/// val result = orch.runGoal("review Q1 loan applications for fair lending violations")
/// println("succeeded=${result.tasksSucceeded}  id=${result.executionId}")
/// ```
///
/// # Swift (via UniFFI)
///
/// ```swift
/// let orch = SwarmOrchestratorFFI(name: "compliance-swarm")
/// let result = try orch.runGoal(goal: "review Q1 loan applications for fair lending violations")
/// print("succeeded=\(result.tasksSucceeded)  id=\(result.executionId)")
/// ```
pub struct SwarmOrchestratorFFI {
    name: String,
    rt: Runtime,
}

impl SwarmOrchestratorFFI {
    /// Create a new orchestrator with the given display name.
    ///
    /// The name appears in OpenTelemetry `gen_ai.agent.*` span attributes
    /// and in every `SwarmAuditLog` entry emitted during execution.
    pub fn new(name: String) -> Self {
        Self {
            name,
            rt: Runtime::new()
                .expect("failed to create Tokio runtime for SwarmOrchestratorFFI"),
        }
    }

    /// Run the full swarm pipeline for the given natural-language goal.
    ///
    /// Blocks the calling thread until the swarm completes or fails.
    ///
    /// The current body is a stub that returns a well-formed `SwarmResultFFI`
    /// with a unique UUID execution ID. It will be replaced with a real
    /// `mofa_orchestrator::SwarmOrchestrator::run_goal` call in the follow-up
    /// PR that merges `feat/mofa-orchestrator-skeleton`.
    pub fn run_goal(&self, goal: String) -> Result<SwarmResultFFI, MoFaError> {
        let start = Instant::now();

        // Stub: async block that will be replaced with the real orchestrator call.
        // The runtime ownership and blocking pattern stay identical after the swap.
        let execution_id = self.rt.block_on(async {
            uuid::Uuid::new_v4().to_string()
        });

        let wall_time_ms = start.elapsed().as_millis() as u64;

        Ok(SwarmResultFFI {
            execution_id,
            goal,
            tasks_succeeded: 0,
            tasks_failed: 0,
            wall_time_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_does_not_panic() {
        let _orch = SwarmOrchestratorFFI::new("test-swarm".to_string());
    }

    #[test]
    fn run_goal_returns_result_with_matching_goal() {
        let orch = SwarmOrchestratorFFI::new("test-swarm".to_string());
        let result = orch
            .run_goal("summarize quarterly earnings".to_string())
            .expect("run_goal must not fail");
        assert_eq!(result.goal, "summarize quarterly earnings");
        assert!(!result.execution_id.is_empty(), "execution_id must be a non-empty UUID");
    }

    #[test]
    fn run_goal_result_has_valid_timing() {
        let orch = SwarmOrchestratorFFI::new("test-swarm".to_string());
        let result = orch
            .run_goal("deploy service to staging".to_string())
            .expect("run_goal must not fail");
        let _ = result.wall_time_ms;
    }

    #[test]
    fn run_goal_execution_ids_are_unique() {
        let orch = SwarmOrchestratorFFI::new("test-swarm".to_string());
        let r1 = orch.run_goal("goal one".to_string()).unwrap();
        let r2 = orch.run_goal("goal two".to_string()).unwrap();
        assert_ne!(
            r1.execution_id, r2.execution_id,
            "each run_goal call must produce a unique execution_id"
        );
    }
}
