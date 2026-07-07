//! Default notifier that writes gate events to the tracing log.
//! Zero external dependencies — always available as a fallback.

use async_trait::async_trait;
use tracing::info;

use super::{GateEvent, GateEventKind, Notifier};
use crate::error::OrchestratorResult;

/// Writes every gate event to `tracing::info`. Used as the default notifier
/// when no external webhooks are configured.
#[derive(Debug, Default)]
pub struct LogNotifier;

#[async_trait]
impl Notifier for LogNotifier {
    fn name(&self) -> &str {
        "log"
    }

    async fn notify(&self, event: &GateEvent) -> OrchestratorResult<()> {
        match &event.kind {
            GateEventKind::PendingApproval => {
                info!(
                    execution_id = %event.execution_id,
                    task_id = %event.task_id,
                    risk = %event.risk_level,
                    "[HITL] awaiting approval: {}",
                    event.task_description
                );
            }
            GateEventKind::Approved => {
                info!(
                    execution_id = %event.execution_id,
                    task_id = %event.task_id,
                    "[HITL] approved: {}",
                    event.task_description
                );
            }
            GateEventKind::Rejected { reason } => {
                info!(
                    execution_id = %event.execution_id,
                    task_id = %event.task_id,
                    reason = %reason,
                    "[HITL] rejected: {}",
                    event.task_description
                );
            }
            GateEventKind::TimedOut => {
                info!(
                    execution_id = %event.execution_id,
                    task_id = %event.task_id,
                    "[HITL] timed out waiting for approval: {}",
                    event.task_description
                );
            }
        }
        Ok(())
    }
}
