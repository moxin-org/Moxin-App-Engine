//! Agent capability registry for runtime discovery and task routing.
//!
//! The `CapabilityRegistry` is distinct from the runtime `AgentRegistry`
//! which tracks running instances. This registry is about what agents *can*
//! do, not whether they are currently alive.

use mofa_kernel::agent::manifest::AgentManifest;
use std::collections::HashMap;

/// Stores agent manifests and answers routing queries.
///
/// Orchestrators query the registry to find the right agent for a task
/// without holding hardcoded references to specific agent instances.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_foundation::CapabilityRegistry;
/// use mofa_kernel::AgentManifest;
///
/// let mut registry = CapabilityRegistry::new();
/// registry.register(
///     AgentManifest::builder("agent-001", "Researcher")
///         .description("searches the web and summarizes documents")
///         .build(),
/// );
///
/// let matches = registry.query("summarize web content");
/// assert!(!matches.is_empty());
/// ```
#[derive(Debug, Default)]
pub struct CapabilityRegistry {
    manifests: HashMap<String, AgentManifest>,
}

impl CapabilityRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an agent manifest, replacing any previous entry for the same ID.
    pub fn register(&mut self, manifest: AgentManifest) {
        self.manifests.insert(manifest.agent_id.clone(), manifest);
    }

    /// Removes an agent manifest by ID. Returns the manifest if it existed.
    pub fn unregister(&mut self, agent_id: &str) -> Option<AgentManifest> {
        self.manifests.remove(agent_id)
    }

    /// Looks up a manifest by agent ID.
    pub fn find_by_id(&self, agent_id: &str) -> Option<&AgentManifest> {
        self.manifests.get(agent_id)
    }

    /// Returns all agents whose capability tags include `tag`.
    pub fn find_by_tag(&self, tag: &str) -> Vec<&AgentManifest> {
        self.manifests
            .values()
            .filter(|m| m.capabilities.has_tag(tag))
            .collect()
    }

    /// Queries agents by natural-language description using keyword matching.
    ///
    /// Scores each manifest by counting how many words from `query` appear in
    /// the manifest's description and capability tags. Returns results sorted
    /// by descending relevance score, excluding zero-score entries.
    pub fn query(&self, query: &str) -> Vec<&AgentManifest> {
        let keywords: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();

        let mut scored: Vec<(usize, &AgentManifest)> = self
            .manifests
            .values()
            .filter_map(|m| {
                let tags_str = m
                    .capabilities
                    .tags
                    .iter()
                    .map(|t| t.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let haystack = format!("{} {}", m.description.to_lowercase(), tags_str);
                let score = keywords
                    .iter()
                    .filter(|kw| haystack.contains(kw.as_str()))
                    .count();
                if score > 0 { Some((score, m)) } else { None }
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, m)| m).collect()
    }

    /// Returns all registered manifests.
    pub fn all(&self) -> Vec<&AgentManifest> {
        self.manifests.values().collect()
    }

    /// Returns the number of registered agents.
    pub fn len(&self) -> usize {
        self.manifests.len()
    }

    /// Returns true if no agents are registered.
    pub fn is_empty(&self) -> bool {
        self.manifests.is_empty()
    }

    /// Returns the number of registered agents.
    ///
    /// Alias for `len` used in A2A ingestion tests to verify idempotency.
    pub fn agent_count(&self) -> usize {
        self.manifests.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofa_kernel::agent::capabilities::AgentCapabilities;

    fn make_registry() -> CapabilityRegistry {
        let mut registry = CapabilityRegistry::new();

        registry.register(
            AgentManifest::builder("agent-research", "ResearchAgent")
                .description("searches the web and summarizes documents and articles")
                .capabilities(
                    AgentCapabilities::builder()
                        .with_tag("research")
                        .with_tag("summarization")
                        .with_tag("web")
                        .build(),
                )
                .build(),
        );

        registry.register(
            AgentManifest::builder("agent-code", "CodeAgent")
                .description("writes reviews and debugs Rust and Python code")
                .capabilities(
                    AgentCapabilities::builder()
                        .with_tag("coding")
                        .with_tag("rust")
                        .with_tag("python")
                        .build(),
                )
                .build(),
        );

        registry
    }

    #[test]
    fn test_register_and_find_by_id() {
        let registry = make_registry();
        assert!(registry.find_by_id("agent-research").is_some());
        assert!(registry.find_by_id("agent-code").is_some());
        assert!(registry.find_by_id("nonexistent").is_none());
    }

    #[test]
    fn test_find_by_tag() {
        let registry = make_registry();
        let coding = registry.find_by_tag("coding");
        assert_eq!(coding.len(), 1);
        assert_eq!(coding[0].agent_id, "agent-code");
    }

    #[test]
    fn test_query_returns_best_match_first() {
        let registry = make_registry();

        let results = registry.query("summarize web documents");
        assert!(!results.is_empty());
        assert_eq!(results[0].agent_id, "agent-research");

        let results = registry.query("write rust code");
        assert!(!results.is_empty());
        assert_eq!(results[0].agent_id, "agent-code");
    }

    #[test]
    fn test_query_no_match_returns_empty() {
        let registry = make_registry();
        let results = registry.query("quantum physics simulation");
        assert!(results.is_empty());
    }

    #[test]
    fn test_unregister() {
        let mut registry = make_registry();
        assert_eq!(registry.len(), 2);
        registry.unregister("agent-code");
        assert_eq!(registry.len(), 1);
        assert!(registry.find_by_id("agent-code").is_none());
    }
}
