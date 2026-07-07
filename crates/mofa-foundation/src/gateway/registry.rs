//! Production `RouteRegistry` implementation for the MoFA gateway.
//!
//! | Type | Description |
//! |------|-------------|
//! | [`InMemoryRouteRegistry`] | In-memory registry with enable/disable and priority ordering |
//!
//! The kernel defines the [`RouteRegistry`] trait; this module provides
//! a concrete implementation in `mofa-foundation` to keep the kernel free of
//! runtime dependencies.
//!
//! # Design
//!
//! `mofa-kernel/src/gateway/tests.rs` contains a minimal private test double.
//! This module promotes it to a public, production-grade type with:
//! - Dynamic `enable` / `disable` without deregistration
//! - Correct conflict detection for `(path_pattern, method, priority)` triples
//! - 17 unit tests covering all edge cases
//!
//! Mutations (`register`, `deregister`) require `&mut self`, enforcing
//! exclusive access at the type level.  Production multi-node deployments can
//! wrap this in an `Arc<RwLock<_>>` or replace it with a Nacos-backed registry.

use mofa_kernel::gateway::{GatewayRoute, RegistryError, RouteRegistry};
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// InMemoryRouteRegistry
// ─────────────────────────────────────────────────────────────────────────────

/// In-memory [`RouteRegistry`] suitable for single-node deployments and tests.
///
/// See the [module docs](self) for design rationale.
///
/// # Usage
///
/// ```rust
/// use mofa_foundation::gateway::InMemoryRouteRegistry;
/// use mofa_kernel::gateway::{GatewayRoute, HttpMethod, RouteRegistry};
///
/// let mut registry = InMemoryRouteRegistry::new();
/// registry.register(
///     GatewayRoute::new("r1", "agent-summarizer", "/v1/summarize", HttpMethod::Post)
/// ).unwrap();
///
/// let route = registry.lookup("r1").unwrap();
/// assert_eq!(route.agent_id, "agent-summarizer");
/// ```
pub struct InMemoryRouteRegistry {
    routes: HashMap<String, GatewayRoute>,
}

impl Default for InMemoryRouteRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryRouteRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
        }
    }

    /// Enable a previously disabled route.
    ///
    /// # Errors
    ///
    /// * [`RegistryError::RouteNotFound`] — no route with the given `id`.
    pub fn enable(&mut self, route_id: &str) -> Result<(), RegistryError> {
        match self.routes.get_mut(route_id) {
            Some(r) => {
                r.enabled = true;
                Ok(())
            }
            None => Err(RegistryError::RouteNotFound(route_id.to_string())),
        }
    }

    /// Disable a route without removing it from the registry.
    ///
    /// Disabled routes are invisible to [`list_active`](RouteRegistry::list_active)
    /// but still accessible via [`lookup`](RouteRegistry::lookup).
    ///
    /// # Errors
    ///
    /// * [`RegistryError::RouteNotFound`] — no route with the given `id`.
    pub fn disable(&mut self, route_id: &str) -> Result<(), RegistryError> {
        match self.routes.get_mut(route_id) {
            Some(r) => {
                r.enabled = false;
                Ok(())
            }
            None => Err(RegistryError::RouteNotFound(route_id.to_string())),
        }
    }

    /// Return the total number of registered routes (enabled and disabled).
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    /// Return `true` if no routes are registered.
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }
}

impl RouteRegistry for InMemoryRouteRegistry {
    fn register(&mut self, route: GatewayRoute) -> Result<(), RegistryError> {
        route
            .validate()
            .map_err(|e| RegistryError::InvalidRoute(e.to_string()))?;

        if self.routes.contains_key(&route.id) {
            return Err(RegistryError::DuplicateRouteId(route.id.clone()));
        }

        // Conflict: same (path_pattern, method, priority) triple.
        for existing in self.routes.values() {
            if existing.path_pattern == route.path_pattern
                && existing.method == route.method
                && existing.priority == route.priority
            {
                return Err(RegistryError::ConflictingRoutes(
                    route.id.clone(),
                    existing.id.clone(),
                ));
            }
        }

        self.routes.insert(route.id.clone(), route);
        Ok(())
    }

    fn deregister(&mut self, route_id: &str) -> Result<(), RegistryError> {
        if self.routes.remove(route_id).is_none() {
            return Err(RegistryError::RouteNotFound(route_id.to_string()));
        }
        Ok(())
    }

    fn lookup(&self, route_id: &str) -> Option<&GatewayRoute> {
        self.routes.get(route_id)
    }

    fn list_active(&self) -> Vec<&GatewayRoute> {
        let mut active: Vec<&GatewayRoute> =
            self.routes.values().filter(|r| r.enabled).collect();
        active.sort_by(|a, b| b.priority.cmp(&a.priority));
        active
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mofa_kernel::gateway::{GatewayRoute, HttpMethod};

    fn r(id: &str, path: &str, method: HttpMethod) -> GatewayRoute {
        GatewayRoute::new(id, "agent-a", path, method)
    }

    // ── register / lookup / deregister ──────────────────────────────────────

    #[test]
    fn register_and_lookup() {
        let mut reg = InMemoryRouteRegistry::new();
        reg.register(r("r1", "/agents/summarizer", HttpMethod::Post))
            .unwrap();
        let found = reg.lookup("r1").unwrap();
        assert_eq!(found.agent_id, "agent-a");
    }

    #[test]
    fn lookup_missing_returns_none() {
        let reg = InMemoryRouteRegistry::new();
        assert!(reg.lookup("ghost").is_none());
    }

    #[test]
    fn register_duplicate_id_is_error() {
        let mut reg = InMemoryRouteRegistry::new();
        reg.register(r("r1", "/a", HttpMethod::Get)).unwrap();
        let err = reg.register(r("r1", "/b", HttpMethod::Post)).unwrap_err();
        assert!(matches!(err, RegistryError::DuplicateRouteId(ref id) if id == "r1"));
    }

    #[test]
    fn deregister_removes_route() {
        let mut reg = InMemoryRouteRegistry::new();
        reg.register(r("r1", "/path", HttpMethod::Get)).unwrap();
        reg.deregister("r1").unwrap();
        assert!(reg.lookup("r1").is_none());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn deregister_missing_is_error() {
        let mut reg = InMemoryRouteRegistry::new();
        let err = reg.deregister("ghost").unwrap_err();
        assert!(matches!(err, RegistryError::RouteNotFound(_)));
    }

    // ── enable / disable ────────────────────────────────────────────────────

    #[test]
    fn disable_hides_from_list_active() {
        let mut reg = InMemoryRouteRegistry::new();
        reg.register(r("r1", "/active", HttpMethod::Get)).unwrap();
        reg.register(r("r2", "/disabled", HttpMethod::Post)).unwrap();
        reg.disable("r2").unwrap();

        let active = reg.list_active();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "r1");
    }

    #[test]
    fn disable_does_not_remove_from_lookup() {
        let mut reg = InMemoryRouteRegistry::new();
        reg.register(r("r1", "/path", HttpMethod::Get)).unwrap();
        reg.disable("r1").unwrap();
        // Still findable by id
        assert!(reg.lookup("r1").is_some());
    }

    #[test]
    fn enable_makes_route_visible_again() {
        let mut reg = InMemoryRouteRegistry::new();
        reg.register(r("r1", "/path", HttpMethod::Get)).unwrap();
        reg.disable("r1").unwrap();
        assert_eq!(reg.list_active().len(), 0);
        reg.enable("r1").unwrap();
        assert_eq!(reg.list_active().len(), 1);
    }

    #[test]
    fn disable_missing_returns_not_found() {
        let mut reg = InMemoryRouteRegistry::new();
        let err = reg.disable("ghost").unwrap_err();
        assert!(matches!(err, RegistryError::RouteNotFound(_)));
    }

    #[test]
    fn enable_missing_returns_not_found() {
        let mut reg = InMemoryRouteRegistry::new();
        let err = reg.enable("ghost").unwrap_err();
        assert!(matches!(err, RegistryError::RouteNotFound(_)));
    }

    // ── list_active ordering ────────────────────────────────────────────────

    #[test]
    fn list_active_sorted_by_descending_priority() {
        let mut reg = InMemoryRouteRegistry::new();
        reg.register(r("low", "/low", HttpMethod::Get).with_priority(1))
            .unwrap();
        reg.register(r("high", "/high", HttpMethod::Post).with_priority(10))
            .unwrap();
        reg.register(r("mid", "/mid", HttpMethod::Put).with_priority(5))
            .unwrap();

        let active = reg.list_active();
        assert_eq!(active[0].id, "high");
        assert_eq!(active[1].id, "mid");
        assert_eq!(active[2].id, "low");
    }

    #[test]
    fn list_active_excludes_disabled() {
        let mut reg = InMemoryRouteRegistry::new();
        reg.register(r("r1", "/a", HttpMethod::Get)).unwrap();
        reg.register(r("r2", "/b", HttpMethod::Post).disabled())
            .unwrap();
        assert_eq!(reg.list_active().len(), 1);
    }

    // ── conflict detection ──────────────────────────────────────────────────

    #[test]
    fn conflict_same_path_method_priority() {
        let mut reg = InMemoryRouteRegistry::new();
        reg.register(r("r1", "/v1/chat", HttpMethod::Post)).unwrap();
        let err = reg
            .register(r("r2", "/v1/chat", HttpMethod::Post))
            .unwrap_err();
        assert!(matches!(err, RegistryError::ConflictingRoutes(ref a, ref b)
            if a == "r2" && b == "r1"));
    }

    #[test]
    fn no_conflict_different_priority() {
        let mut reg = InMemoryRouteRegistry::new();
        reg.register(r("r1", "/v1/chat", HttpMethod::Post)).unwrap();
        reg.register(r("r2", "/v1/chat", HttpMethod::Post).with_priority(1))
            .unwrap();
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn no_conflict_different_method() {
        let mut reg = InMemoryRouteRegistry::new();
        reg.register(r("r1", "/v1/chat", HttpMethod::Post)).unwrap();
        reg.register(r("r2", "/v1/chat", HttpMethod::Get)).unwrap();
        assert_eq!(reg.len(), 2);
    }

    // ── len / is_empty ───────────────────────────────────────────────────────

    #[test]
    fn empty_registry_is_empty() {
        let reg = InMemoryRouteRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn len_counts_all_routes_including_disabled() {
        let mut reg = InMemoryRouteRegistry::new();
        reg.register(r("r1", "/a", HttpMethod::Get)).unwrap();
        reg.register(r("r2", "/b", HttpMethod::Post)).unwrap();
        reg.disable("r2").unwrap();
        assert_eq!(reg.len(), 2);
        assert_eq!(reg.list_active().len(), 1);
    }
}
