//! RBAC Policy
//!
//! Policy definition and evaluation for role-based access control.

use crate::security::rbac::roles::{Role, RoleRegistry};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// RBAC Policy configuration
#[derive(Debug, Clone)]
pub struct RbacPolicy {
    /// Role registry
    roles: RoleRegistry,
    /// Subject-to-role mappings (e.g., agent_id -> role_name)
    subject_roles: HashMap<String, Vec<String>>,
    /// Default role for subjects without explicit role assignment
    default_role: Option<String>,
    /// Whether to deny by default (if no role matches)
    deny_by_default: bool,
}

impl RbacPolicy {
    /// Create a new RBAC policy
    pub fn new() -> Self {
        Self {
            roles: RoleRegistry::new(),
            subject_roles: HashMap::new(),
            default_role: None,
            deny_by_default: true,
        }
    }

    /// Add a role to the policy
    pub fn add_role(&mut self, role: Role) {
        self.roles.register_role(role);
    }

    /// Assign a role to a subject
    pub fn assign_role(&mut self, subject: impl Into<String>, role: impl Into<String>) {
        self.subject_roles
            .entry(subject.into())
            .or_default()
            .push(role.into());
    }

    /// Set the default role for subjects without explicit assignment
    pub fn with_default_role(mut self, role: impl Into<String>) -> Self {
        self.default_role = Some(role.into());
        self
    }

    /// Set whether to deny by default
    pub fn with_deny_by_default(mut self, deny: bool) -> Self {
        self.deny_by_default = deny;
        self
    }

    /// Get roles for a subject
    pub fn get_subject_roles(&self, subject: &str) -> Vec<String> {
        self.subject_roles.get(subject).cloned().unwrap_or_else(|| {
            // Use default role if available
            self.default_role
                .clone()
                .map(|r| vec![r])
                .unwrap_or_default()
        })
    }

    /// Check if a subject has a specific permission
    pub fn check_permission(&self, subject: &str, permission: &str) -> bool {
        let roles = self.get_subject_roles(subject);

        if roles.is_empty() {
            return !self.deny_by_default;
        }

        // Check if any of the subject's roles have the permission
        for role_name in roles {
            if self.roles.has_permission(&role_name, permission) {
                return true;
            }
        }

        false
    }

    /// Get all permissions for a subject
    pub fn get_subject_permissions(&self, subject: &str) -> std::collections::HashSet<String> {
        let roles = self.get_subject_roles(subject);
        let mut permissions = std::collections::HashSet::new();

        for role_name in roles {
            permissions.extend(self.roles.get_all_permissions(&role_name));
        }

        permissions
    }
}

impl Default for RbacPolicy {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rbac_policy() {
        let mut policy = RbacPolicy::new();

        // Define roles
        let admin = Role::new("admin")
            .with_permission("tool:delete")
            .with_permission("tool:create");

        let user = Role::new("user").with_permission("tool:read");

        policy.add_role(admin);
        policy.add_role(user);

        // Assign roles
        policy.assign_role("agent-1", "admin");
        policy.assign_role("agent-2", "user");

        // Check permissions
        assert!(policy.check_permission("agent-1", "tool:delete"));
        assert!(policy.check_permission("agent-2", "tool:read"));
        assert!(!policy.check_permission("agent-2", "tool:delete"));
    }

    #[test]
    fn test_default_role() {
        let mut policy = RbacPolicy::new().with_default_role("guest");

        let guest = Role::new("guest").with_permission("tool:read");

        policy.add_role(guest);

        // Subject without explicit role should get default role
        assert!(policy.check_permission("unknown-agent", "tool:read"));
        assert!(!policy.check_permission("unknown-agent", "tool:delete"));
    }
}
