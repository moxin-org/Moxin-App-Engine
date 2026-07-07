//! Swarm-specific agent scoring types and dispatch abstraction.
//!
//! This module defines the pure domain types and scoring policy
//! for swarm-aware agent selection.
//!
//! ## quick start
//!
//! ```rust,ignore
//! use mofa_foundation::swarm::{AgentCapabilitySpec, AgentRuntimeScore, compute_agent_score};
//!
//! let spec = AgentCapabilitySpec::new("summarizer", ["Summarise"], 4);
//! let score_val = compute_agent_score(spec.max_concurrency, 1, 0.9);
//! // score_val ≈ 0.675  (25 % load × 0.9 EWMA)
//! ```

use serde::{Deserialize, Serialize};

/// Core types
/// Static description of an agent's capabilities and scheduling limits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentCapabilitySpec {
    pub id: String,
    pub capabilities: Vec<String>,
    pub max_concurrency: u32,
    pub cost_per_token: Option<f64>,
}

impl AgentCapabilitySpec {
    /// Create a new spec with default `cost_per_token = None`.
    pub fn new(
        id: impl Into<String>,
        capabilities: impl IntoIterator<Item = impl Into<String>>,
        max_concurrency: u32,
    ) -> Self {
        Self {
            id: id.into(),
            capabilities: capabilities.into_iter().map(Into::into).collect(),
            max_concurrency,
            cost_per_token: None,
        }
    }

    /// Returns `true` if this agent advertises the given capability tag.
    pub fn has_capability(&self, cap: &str) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }
}

/// Runtime load and reliability snapshot for an agent.
///
/// Stored and updated by the runtime registry; passed into
/// [`compute_agent_score`] to produce a comparable score.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct AgentRuntimeScore {
    pub current_load: u32,
    pub success_rate_ewma: f64,
}

impl Default for AgentRuntimeScore {
    fn default() -> Self {
        Self { current_load: 0, success_rate_ewma: 1.0 }
    }
}

/// An agent together with its computed dispatch score.
/// Returned by [`SwarmAgentLookup::find_best`]. Higher `score` is better.
#[derive(Debug, Clone)]
pub struct RankedAgent {
    pub spec: AgentCapabilitySpec,
    pub runtime: AgentRuntimeScore,
    pub score: f64,
}

/// Scoring function
/// Compute a dispatch score for an agent given its current load and EWMA.
///
/// `score = (1 − load_ratio) × success_rate_ewma`
///
/// - Returns `0.0` when `max_concurrency == 0` or `current_load >= max_concurrency`.
/// - Both inputs are clamped to their valid ranges before use.
///
/// ## O-notation
///
/// This function is O(1). Callers are responsible for iterating over
/// candidate agents; a typical `find_best` call is **O(k)** where *k*
/// is the number of agents advertising the requested capability.
/// ```
pub fn compute_agent_score(max_concurrency: u32, current_load: u32, success_rate_ewma: f64) -> f64 {
    if max_concurrency == 0 {
        return 0.0;
    }
    let load_ratio = (current_load as f64 / max_concurrency as f64).clamp(0.0, 1.0);
    (1.0 - load_ratio) * success_rate_ewma.clamp(0.0, 1.0)
}

// Lookup trait
pub trait SwarmAgentLookup: Send + Sync {
    fn register(&self, spec: AgentCapabilitySpec);
    fn deregister(&self, agent_id: &str);
    fn find_best(&self, capability: &str) -> Option<RankedAgent>;
    fn record_outcome(&self, agent_id: &str, success: bool);
}

#[cfg(test)]
mod tests {
    use super::*;
    // AgentCapabilitySpec
    #[test]
    fn test_has_capability() {
        let spec = AgentCapabilitySpec::new("a1", ["Summarise", "WebFetch"], 4);
        assert!(spec.has_capability("Summarise"));
        assert!(spec.has_capability("WebFetch"));
        assert!(!spec.has_capability("CodeGen"));
    }

    // compute_agent_score
    #[test]
    fn test_score_zero_load_perfect_ewma() {
        // 0 load, EWMA = 1.0 → score = 1.0
        let s = compute_agent_score(4, 0, 1.0);
        assert!((s - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_score_at_capacity_is_zero() {
        assert_eq!(compute_agent_score(2, 2, 1.0), 0.0);
        assert_eq!(compute_agent_score(0, 0, 1.0), 0.0);
    }

    #[test]
    fn test_score_partial_load() {
        // 1/4 load, EWMA 0.9 → (1 - 0.25) * 0.9 = 0.675
        let s = compute_agent_score(4, 1, 0.9);
        assert!((s - 0.675).abs() < 1e-9, "got {s}");
    }

    #[test]
    fn test_score_clamps_ewma_above_one() {
        // EWMA clamped to 1.0
        let s = compute_agent_score(4, 0, 1.5);
        assert!((s - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_score_low_ewma_penalises() {
        let high = compute_agent_score(4, 0, 1.0);
        let low  = compute_agent_score(4, 0, 0.3);
        assert!(high > low);
    }

    #[test]
    fn test_score_high_load_penalises() {
        let light = compute_agent_score(4, 1, 1.0);
        let heavy  = compute_agent_score(4, 3, 1.0);
        assert!(light > heavy);
    }

    #[test]
    fn test_runtime_score_default_is_optimistic() {
        let rs = AgentRuntimeScore::default();
        assert_eq!(rs.current_load, 0);
        assert!((rs.success_rate_ewma - 1.0).abs() < 1e-9);
    }
}
