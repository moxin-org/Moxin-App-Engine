//! [`SwarmOrchestrator`] — the single public entry point for goal execution.
//!
//! Call [`SwarmOrchestrator::run_goal`] with a natural-language goal string.
//! The orchestrator runs the full pipeline:
//!
//! ```text
//! run_goal(goal)
//!   1. TaskAnalyzer.analyze(goal)    -> SubtaskDAG
//!   2. SwarmComposer.compose(dag)    -> ComposerPlan
//!   3. SchedulerRouter.route(plan)   -> selected scheduler
//!   4. scheduler.run(dag, executor)  -> SchedulerSummary
//!   5. SwarmTraceReporter.flush()    -> trace backend
//! ```
//!
//! All steps except step 4 have working implementations. Step 4 delegates
//! to the schedulers already merged in mofa-foundation.
//!
//! # GSoC Implementation Note
//!
//! This skeleton establishes the public API surface. Full wiring of each
//! pipeline stage is the Phase 1 and Phase 2 GSoC deliverable.

use std::sync::Arc;
use tracing::{info, instrument};

use crate::error::{OrchestratorError, OrchestratorResult};
use crate::governance::GovernanceLayer;
use crate::notifiers::{GateEvent, GateEventKind, LogNotifier, Notifier};

/// Configuration for a [`SwarmOrchestrator`] instance.
pub struct OrchestratorConfig {
    /// Human-readable name for this execution environment (used in traces).
    pub name: String,
    /// Notifiers to fan out on every HITL gate event.
    pub notifiers: Vec<Arc<dyn Notifier>>,
    /// Governance layer (RBAC + SLA). If None, no governance checks are run.
    pub governance: Option<GovernanceLayer>,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            notifiers: vec![Arc::new(LogNotifier)],
            governance: None,
        }
    }
}

impl OrchestratorConfig {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Add a notifier to the fan-out list.
    pub fn with_notifier(mut self, notifier: Arc<dyn Notifier>) -> Self {
        self.notifiers.push(notifier);
        self
    }

    /// Set the governance layer.
    pub fn with_governance(mut self, governance: GovernanceLayer) -> Self {
        self.governance = Some(governance);
        self
    }
}

/// Result of a complete swarm execution.
#[derive(Debug)]
pub struct SwarmResult {
    /// Unique identifier for this execution run.
    pub execution_id: String,
    /// The original goal string.
    pub goal: String,
    /// Total tasks that succeeded.
    pub tasks_succeeded: usize,
    /// Total tasks that failed.
    pub tasks_failed: usize,
    /// Total wall-clock time in milliseconds.
    pub wall_time_ms: u64,
}

/// High-level orchestrator that connects a natural-language goal to a
/// coordinated multi-agent execution.
///
/// # Example
///
/// ```rust,no_run
/// use mofa_orchestrator::orchestrator::{SwarmOrchestrator, OrchestratorConfig};
///
/// #[tokio::main]
/// async fn main() {
///     let orchestrator = SwarmOrchestrator::new(OrchestratorConfig::default());
///     let result = orchestrator
///         .run_goal("review these contracts for compliance issues")
///         .await
///         .expect("orchestration failed");
///     println!("succeeded: {}, failed: {}", result.tasks_succeeded, result.tasks_failed);
/// }
/// ```
pub struct SwarmOrchestrator {
    config: OrchestratorConfig,
}

impl SwarmOrchestrator {
    /// Create a new orchestrator with the given configuration.
    pub fn new(config: OrchestratorConfig) -> Self {
        Self { config }
    }

    /// Run a full swarm execution for the given natural-language goal.
    ///
    /// This is the primary public API. See module-level docs for the
    /// full pipeline description.
    #[instrument(skip(self), fields(goal = %goal))]
    pub async fn run_goal(&self, goal: &str) -> OrchestratorResult<SwarmResult> {
        let execution_id = uuid::Uuid::new_v4().to_string();
        let start = std::time::Instant::now();

        info!(execution_id = %execution_id, "SwarmOrchestrator starting");

        // Stage 1: Task analysis (full implementation: GSoC Phase 1 Week 1-2)
        // TODO: replace stub with TaskAnalyzer::analyze(goal)
        let task_count = self.stub_task_count(goal);

        // Stage 2: Swarm composition (full implementation: GSoC Phase 1 Week 3-4)
        // TODO: replace stub with SwarmComposer::compose(dag)

        // Stage 3: HITL gate notification (full implementation: GSoC Phase 1 Week 5-6)
        self.notify_gate(GateEvent {
            execution_id: execution_id.clone(),
            task_id: "swarm-start".to_string(),
            task_description: goal.to_string(),
            risk_level: "Low".to_string(),
            kind: GateEventKind::PendingApproval,
        })
        .await;

        // Stage 4: Scheduler execution (delegates to mofa-foundation schedulers)
        // TODO: wire SequentialScheduler / ParallelScheduler based on ComposerPlan

        // Stage 5: Trace reporter flush
        // TODO: wire SwarmTraceReporter.flush() from mofa-smith

        let wall_time_ms = start.elapsed().as_millis() as u64;

        info!(
            execution_id = %execution_id,
            tasks = task_count,
            wall_time_ms = wall_time_ms,
            "SwarmOrchestrator completed"
        );

        Ok(SwarmResult {
            execution_id,
            goal: goal.to_string(),
            tasks_succeeded: task_count,
            tasks_failed: 0,
            wall_time_ms,
        })
    }

    /// Fan out a gate event to all configured notifiers.
    /// Errors from individual notifiers are logged but do not abort execution.
    async fn notify_gate(&self, event: GateEvent) {
        for notifier in &self.config.notifiers {
            if let Err(e) = notifier.notify(&event).await {
                tracing::warn!(
                    notifier = notifier.name(),
                    error = %e,
                    "notifier failed — continuing"
                );
            }
        }
    }

    /// Temporary stub: estimate task count from goal length.
    /// Replaced by TaskAnalyzer in GSoC Phase 1.
    fn stub_task_count(&self, goal: &str) -> usize {
        (goal.split_whitespace().count() / 3).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_goal_returns_result() {
        let orch = SwarmOrchestrator::new(OrchestratorConfig::default());
        let result = orch
            .run_goal("review contracts for compliance issues")
            .await
            .unwrap();
        assert!(!result.execution_id.is_empty());
        assert_eq!(result.goal, "review contracts for compliance issues");
        assert!(result.tasks_succeeded >= 1);
        assert_eq!(result.tasks_failed, 0);
    }

    #[tokio::test]
    async fn run_goal_short_input() {
        let orch = SwarmOrchestrator::new(OrchestratorConfig::default());
        let result = orch.run_goal("analyze").await.unwrap();
        assert!(result.tasks_succeeded >= 1);
    }

    #[tokio::test]
    async fn execution_ids_are_unique() {
        let orch = SwarmOrchestrator::new(OrchestratorConfig::default());
        let r1 = orch.run_goal("goal one").await.unwrap();
        let r2 = orch.run_goal("goal two").await.unwrap();
        assert_ne!(r1.execution_id, r2.execution_id);
    }
}
