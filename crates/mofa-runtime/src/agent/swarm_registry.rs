//! DashMap-backed swarm agent registry with EWMA load scoring.
//!
//! Implements [`SwarmAgentLookup`] for use by the swarm executor and composer.
//! Stores agent specs and runtime state concurrently via [`DashMap`].

use dashmap::DashMap;
use mofa_foundation::swarm::{
    AgentCapabilitySpec, AgentRuntimeScore, RankedAgent, SwarmAgentLookup, compute_agent_score,
};

// Internal state
struct AgentState {
    spec: AgentCapabilitySpec,
    score: AgentRuntimeScore,
}

/// SwarmAgentRegistry
/// Concurrent, load-aware agent registry for swarm scheduling.
///
/// # Design
/// - `agents`: maps agent ID to `AgentState` (spec + EWMA score + current load).
/// - `capability_index`: maps capability tag → list of agent IDs for O(k) lookup.
/// - EWMA update: `ewma = alpha × outcome + (1 - alpha) × ewma`
///
/// ```
pub struct SwarmAgentRegistry {
    agents: DashMap<String, AgentState>,
    capability_index: DashMap<String, Vec<String>>,
    /// EWMA smoothing factor (0 < alpha ≤ 1). Default: 0.2.
    ewma_alpha: f64,
}

impl SwarmAgentRegistry {
    pub fn new(ewma_alpha: f64) -> Self {
        assert!(ewma_alpha > 0.0 && ewma_alpha <= 1.0, "ewma_alpha must be in (0, 1]");
        Self {
            agents: DashMap::new(),
            capability_index: DashMap::new(),
            ewma_alpha,
        }
    }

    /// Increment the active-task load counter for an agent.
    pub fn increment_load(&self, agent_id: &str) {
        if let Some(mut entry) = self.agents.get_mut(agent_id) {
            entry.score.current_load += 1;
        }
    }

    /// Read the current runtime score for an agent (load + EWMA).
    pub fn get_score(&self, agent_id: &str) -> Option<AgentRuntimeScore> {
        self.agents.get(agent_id).map(|e| e.score)
    }

    // Rebuild capability index entry for a single agent.
    fn index_agent(&self, spec: &AgentCapabilitySpec) {
        for cap in &spec.capabilities {
            self.capability_index
                .entry(cap.clone())
                .or_default()
                .push(spec.id.clone());
        }
    }

    // Remove agent ID from all capability index entries.
    fn unindex_agent(&self, spec: &AgentCapabilitySpec) {
        for cap in &spec.capabilities {
            if let Some(mut ids) = self.capability_index.get_mut(cap) {
                ids.retain(|id| id != &spec.id);
            }
        }
    }
}

// SwarmAgentLookup impl
impl SwarmAgentLookup for SwarmAgentRegistry {
    /// Register or update an agent. EWMA is **preserved** on re-registration
    /// so that historical reliability survives agent restarts.
    fn register(&self, spec: AgentCapabilitySpec) {
        let preserved_ewma = self
            .agents
            .get(&spec.id)
            .map(|e| e.score.success_rate_ewma)
            .unwrap_or(1.0); // optimistic prior for new agents

        // Remove old capability index entries before updating spec.
        if let Some(existing) = self.agents.get(&spec.id) {
            self.unindex_agent(&existing.spec);
        }

        self.index_agent(&spec);
        self.agents.insert(
            spec.id.clone(),
            AgentState {
                spec,
                score: AgentRuntimeScore {
                    current_load: 0,
                    success_rate_ewma: preserved_ewma,
                },
            },
        );
    }

    fn deregister(&self, agent_id: &str) {
        if let Some((_, state)) = self.agents.remove(agent_id) {
            self.unindex_agent(&state.spec);
        }
    }

    /// Return the highest-scoring available agent for `capability`
    fn find_best(&self, capability: &str) -> Option<RankedAgent> {
        let ids = self.capability_index.get(capability)?;

        ids.iter()
            .filter_map(|id| {
                let entry = self.agents.get(id)?;
                let AgentState { spec, score } = &*entry;

                if score.current_load >= spec.max_concurrency {
                    return None;
                }

                let s = compute_agent_score(
                    spec.max_concurrency,
                    score.current_load,
                    score.success_rate_ewma,
                );
                Some(RankedAgent {
                    spec: spec.clone(),
                    runtime: *score,
                    score: s,
                })
            })
            .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap())
    }

    /// Update EWMA for `agent_id` and decrement its active load by one.
    fn record_outcome(&self, agent_id: &str, success: bool) {
        if let Some(mut entry) = self.agents.get_mut(agent_id) {
            let outcome = if success { 1.0_f64 } else { 0.0_f64 };
            let old = entry.score.success_rate_ewma;
            entry.score.success_rate_ewma = self.ewma_alpha * outcome + (1.0 - self.ewma_alpha) * old;
            entry.score.current_load = entry.score.current_load.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofa_foundation::swarm::AgentCapabilitySpec;

    fn make_registry() -> SwarmAgentRegistry {
        SwarmAgentRegistry::new(0.2)
    }

    fn agent(id: &str, caps: &[&str], max: u32) -> AgentCapabilitySpec {
        AgentCapabilitySpec::new(id, caps.iter().copied(), max)
    }

    #[test]
    fn test_find_best_returns_none_before_registration() {
        let reg = make_registry();
        assert!(reg.find_best("Summarise").is_none());
    }

    #[test]
    fn test_find_best_prefers_low_load() {
        let reg = make_registry();
        reg.register(agent("a1", &["Summarise"], 4));
        reg.register(agent("a2", &["Summarise"], 4));

        // Artificially load a1 with 3 tasks.
        reg.increment_load("a1");
        reg.increment_load("a1");
        reg.increment_load("a1");

        let best = reg.find_best("Summarise").expect("should find one");
        assert_eq!(best.spec.id, "a2", "a2 has lower load → higher score");
    }

    #[test]
    fn test_find_best_returns_none_when_all_full() {
        let reg = make_registry();
        reg.register(agent("a1", &["Summarise"], 2));
        reg.increment_load("a1");
        reg.increment_load("a1");
        assert!(reg.find_best("Summarise").is_none());
    }

    #[test]
    fn test_ewma_decreases_on_failure() {
        let reg = make_registry();
        reg.register(agent("a1", &["Summarise"], 4));
        reg.increment_load("a1");

        let before = reg.agents.get("a1").unwrap().score.success_rate_ewma;
        reg.record_outcome("a1", false);
        let after = reg.agents.get("a1").unwrap().score.success_rate_ewma;

        assert!(after < before, "EWMA should decrease after failure");
    }

    #[test]
    fn test_ewma_preserved_on_reregister() {
        let reg = make_registry();
        reg.register(agent("a1", &["Summarise"], 4));
        reg.increment_load("a1");
        reg.record_outcome("a1", false); // lower EWMA from default 1.0

        let ewma_before = reg.agents.get("a1").unwrap().score.success_rate_ewma;
        assert!(ewma_before < 1.0);

        // Re-register same agent (e.g. after restart).
        reg.register(agent("a1", &["Summarise"], 4));
        let ewma_after = reg.agents.get("a1").unwrap().score.success_rate_ewma;

        assert_eq!(ewma_before, ewma_after, "EWMA must survive re-registration");
    }

    #[test]
    fn test_capability_index_updated_on_deregister() {
        let reg = make_registry();
        reg.register(agent("a1", &["Summarise"], 4));

        assert!(reg.find_best("Summarise").is_some());
        reg.deregister("a1");
        assert!(reg.find_best("Summarise").is_none());
    }

    #[test]
    fn test_load_decremented_after_outcome() {
        let reg = make_registry();
        reg.register(agent("a1", &["Summarise"], 4));
        reg.increment_load("a1");

        let load_before = reg.agents.get("a1").unwrap().score.current_load;
        assert_eq!(load_before, 1);

        reg.record_outcome("a1", true);
        let load_after = reg.agents.get("a1").unwrap().score.current_load;
        assert_eq!(load_after, 0, "load should decrement after recording outcome");
    }
}
