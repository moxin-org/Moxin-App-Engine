//! Governance layer: SLA enforcement, RBAC checks, and audit export.

use std::collections::HashMap;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::error::{OrchestratorError, OrchestratorResult};

/// Roles supported by the built-in RBAC table.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Role {
    Admin,
    Operator,
    Viewer,
}

/// Actions that can be authorized by the governance layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Action {
    RunSwarm,
    ApproveHitl,
    RejectHitl,
    ExportAudit,
    InstallPlugin,
}

/// A single SLA violation record written when a task exceeds its budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlaViolation {
    pub task_id: String,
    pub elapsed_ms: u64,
    pub budget_ms: u64,
    pub recorded_at: i64,
}

/// Central governance layer for a swarm execution.
///
/// Responsibilities:
/// - RBAC: check that a principal has permission for an action before execution
/// - SLA: compare elapsed time against budget; emit violations
/// - Audit export: serialize all recorded violations to JSONL
#[derive(Debug)]
pub struct GovernanceLayer {
    /// principal -> role mapping
    roles: HashMap<String, Role>,
    /// SLA budget in milliseconds per task ID
    sla_budgets: HashMap<String, u64>,
    /// Recorded SLA violations during this execution
    violations: Vec<SlaViolation>,
}

impl GovernanceLayer {
    /// Create an empty governance layer. Use the builder methods to populate
    /// roles and SLA budgets before starting execution.
    pub fn new() -> Self {
        Self {
            roles: HashMap::new(),
            sla_budgets: HashMap::new(),
            violations: Vec::new(),
        }
    }

    /// Register a principal with a role.
    pub fn with_role(mut self, principal: impl Into<String>, role: Role) -> Self {
        self.roles.insert(principal.into(), role);
        self
    }

    /// Set an SLA budget (in milliseconds) for a named task.
    pub fn with_sla(mut self, task_id: impl Into<String>, budget_ms: u64) -> Self {
        self.sla_budgets.insert(task_id.into(), budget_ms);
        self
    }

    /// Check whether a principal is authorized to perform an action.
    ///
    /// Current policy:
    /// - `Admin`    — all actions
    /// - `Operator` — RunSwarm, ApproveHitl, RejectHitl, InstallPlugin
    /// - `Viewer`   — ExportAudit only
    pub fn rbac_check(&self, principal: &str, action: &Action) -> OrchestratorResult<()> {
        let role = self.roles.get(principal).ok_or_else(|| {
            OrchestratorError::Governance(format!("unknown principal '{principal}'"))
        })?;

        let allowed = match role {
            Role::Admin => true,
            Role::Operator => matches!(
                action,
                Action::RunSwarm | Action::ApproveHitl | Action::RejectHitl | Action::InstallPlugin
            ),
            Role::Viewer => matches!(action, Action::ExportAudit),
        };

        if !allowed {
            return Err(OrchestratorError::Governance(format!(
                "principal '{principal}' with role {role:?} is not permitted to perform {action:?}"
            )));
        }

        Ok(())
    }

    /// Compare elapsed time against the SLA budget for a task.
    /// Records a [`SlaViolation`] if over budget and returns an error.
    pub fn check_sla(&mut self, task_id: &str, elapsed_ms: u64) -> OrchestratorResult<()> {
        let Some(&budget_ms) = self.sla_budgets.get(task_id) else {
            return Ok(());
        };

        if elapsed_ms > budget_ms {
            let violation = SlaViolation {
                task_id: task_id.to_string(),
                elapsed_ms,
                budget_ms,
                recorded_at: Utc::now().timestamp_millis(),
            };
            warn!(
                task_id = %task_id,
                elapsed_ms = elapsed_ms,
                budget_ms = budget_ms,
                "SLA violation recorded"
            );
            self.violations.push(violation);
            return Err(OrchestratorError::Governance(format!(
                "task '{task_id}' exceeded SLA budget: {elapsed_ms}ms > {budget_ms}ms"
            )));
        }

        Ok(())
    }

    /// Return a slice of all recorded SLA violations.
    pub fn violations(&self) -> &[SlaViolation] {
        &self.violations
    }

    /// Serialize all SLA violations to newline-delimited JSON and write to
    /// the given path. Creates parent directories if they do not exist.
    pub fn export_audit_jsonl(&self, path: &std::path::Path) -> OrchestratorResult<()> {
        use std::io::Write;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                OrchestratorError::Internal(format!("failed to create audit dir: {e}"))
            })?;
        }

        let mut file = std::fs::File::create(path).map_err(|e| {
            OrchestratorError::Internal(format!("failed to create audit file: {e}"))
        })?;

        for violation in &self.violations {
            let line = serde_json::to_string(violation).map_err(|e| {
                OrchestratorError::Internal(format!("serialization error: {e}"))
            })?;
            writeln!(file, "{line}").map_err(|e| {
                OrchestratorError::Internal(format!("write error: {e}"))
            })?;
        }

        Ok(())
    }
}

impl Default for GovernanceLayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn admin_can_do_everything() {
        let gov = GovernanceLayer::new().with_role("alice", Role::Admin);
        assert!(gov.rbac_check("alice", &Action::RunSwarm).is_ok());
        assert!(gov.rbac_check("alice", &Action::ApproveHitl).is_ok());
        assert!(gov.rbac_check("alice", &Action::ExportAudit).is_ok());
        assert!(gov.rbac_check("alice", &Action::InstallPlugin).is_ok());
    }

    #[test]
    fn viewer_cannot_run_swarm() {
        let gov = GovernanceLayer::new().with_role("bob", Role::Viewer);
        assert!(gov.rbac_check("bob", &Action::RunSwarm).is_err());
        assert!(gov.rbac_check("bob", &Action::ExportAudit).is_ok());
    }

    #[test]
    fn operator_cannot_export_audit() {
        let gov = GovernanceLayer::new().with_role("carol", Role::Operator);
        assert!(gov.rbac_check("carol", &Action::RunSwarm).is_ok());
        assert!(gov.rbac_check("carol", &Action::ExportAudit).is_err());
    }

    #[test]
    fn unknown_principal_is_rejected() {
        let gov = GovernanceLayer::new();
        assert!(gov.rbac_check("unknown", &Action::RunSwarm).is_err());
    }

    #[test]
    fn sla_within_budget_passes() {
        let mut gov = GovernanceLayer::new().with_sla("task-1", 1000);
        assert!(gov.check_sla("task-1", 500).is_ok());
        assert!(gov.violations().is_empty());
    }

    #[test]
    fn sla_over_budget_records_violation() {
        let mut gov = GovernanceLayer::new().with_sla("task-1", 1000);
        assert!(gov.check_sla("task-1", 2000).is_err());
        assert_eq!(gov.violations().len(), 1);
        assert_eq!(gov.violations()[0].task_id, "task-1");
    }

    #[test]
    fn sla_no_budget_always_passes() {
        let mut gov = GovernanceLayer::new();
        assert!(gov.check_sla("task-unknown", 99999).is_ok());
    }

    #[test]
    fn export_audit_jsonl_roundtrip() {
        let mut gov = GovernanceLayer::new().with_sla("task-x", 100);
        let _ = gov.check_sla("task-x", 500);

        let dir = tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        gov.export_audit_jsonl(&path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("task-x"));
        assert!(content.contains("500"));
    }
}
