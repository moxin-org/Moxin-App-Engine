//! A2A Agent Card ingestion for CapabilityRegistry.
//!
//! The Agent2Agent (A2A) protocol (Google, 2025) defines a standard JSON
//! format -- the Agent Card -- that any compliant agent publishes at
//! `/.well-known/agent.json`. This module parses Agent Cards and registers
//! the agent's skills as capabilities in the local CapabilityRegistry,
//! so remote A2A agents are discoverable through the same keyword-based
//! routing pipeline used for local agents.
//!
//! # Example
//!
//! ```rust,no_run
//! use mofa_foundation::swarm::a2a::{A2AAgentCard, A2ACardIngester};
//! use mofa_foundation::capability_registry::CapabilityRegistry;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let card_json = r#"{
//!     "name": "document-parser",
//!     "description": "Extracts structured data from PDF and Word documents",
//!     "url": "https://agents.example.com/doc-parser",
//!     "version": "1.2.0",
//!     "skills": [
//!         {
//!             "id": "extract-tables",
//!             "name": "Extract Tables",
//!             "description": "Extracts tables from documents into JSON",
//!             "tags": ["extraction", "tables", "pdf"]
//!         }
//!     ]
//! }"#;
//!
//! let card: A2AAgentCard = serde_json::from_str(card_json)?;
//! let mut registry = CapabilityRegistry::new();
//! A2ACardIngester::register(&card, &mut registry);
//! # Ok(())
//! # }
//! ```

use serde::{Deserialize, Serialize};

use crate::capability_registry::CapabilityRegistry;
use mofa_kernel::agent::capabilities::AgentCapabilities;
use mofa_kernel::agent::manifest::AgentManifest;

/// An A2A Agent Card as defined by the Agent2Agent protocol specification.
///
/// Agent Cards are published at `/.well-known/agent.json` by any A2A-compliant
/// agent. They describe the agent's identity, capabilities, and the skills it
/// can perform. MoFA ingests these cards so remote agents become discoverable
/// through the local CapabilityRegistry without manual registration.
///
/// Reference: <https://google.github.io/A2A/>
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct A2AAgentCard {
    /// Human-readable name of the agent.
    pub name: String,

    /// Short description of what the agent does. Used as the primary text for
    /// keyword matching in the capability registry.
    #[serde(default)]
    pub description: String,

    /// URL at which the agent's A2A endpoint is reachable.
    pub url: String,

    /// Semantic version of the agent implementation.
    #[serde(default = "default_version")]
    pub version: String,

    /// Skills this agent can perform. Each skill's tags are merged into the
    /// AgentCapabilities registered in the CapabilityRegistry.
    #[serde(default)]
    pub skills: Vec<A2ASkill>,

    /// Protocol-level capabilities such as streaming and push notifications.
    #[serde(default)]
    pub capabilities: A2ACapabilities,

    /// Accepted input modalities. Defaults to ["text"] if not specified.
    #[serde(default = "default_text_mode")]
    pub default_input_modes: Vec<String>,

    /// Produced output modalities. Defaults to ["text"] if not specified.
    #[serde(default = "default_text_mode")]
    pub default_output_modes: Vec<String>,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

fn default_text_mode() -> Vec<String> {
    vec!["text".to_string()]
}

/// A single skill declared in an A2A Agent Card.
///
/// Skills are the unit of capability in the A2A protocol. Each skill has a
/// unique identifier within the agent, a human-readable name, a description,
/// and optional tags. MoFA merges all skill tags into a single
/// AgentCapabilities set registered for the agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct A2ASkill {
    /// Unique identifier for this skill within the agent.
    pub id: String,

    /// Human-readable skill name.
    pub name: String,

    /// Description of what this skill does and when to use it.
    #[serde(default)]
    pub description: String,

    /// Tags that describe this skill's domain. Merged into the AgentCapabilities
    /// tags registered in the CapabilityRegistry.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Example inputs that trigger this skill.
    #[serde(default)]
    pub examples: Vec<String>,
}

/// Protocol-level capability flags from an A2A Agent Card.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct A2ACapabilities {
    /// Whether the agent supports streaming responses.
    #[serde(default)]
    pub streaming: bool,

    /// Whether the agent supports push notifications.
    #[serde(default)]
    pub push_notifications: bool,

    /// Whether the agent supports state transition history.
    #[serde(default)]
    pub state_transition_history: bool,
}

/// Parses A2A Agent Cards and registers remote agents into a local
/// CapabilityRegistry.
///
/// Each skill's tags are merged into a single AgentCapabilities set for the
/// agent. The agent id is derived from its URL so re-ingesting the same card
/// is idempotent (the registry overwrites the existing entry).
pub struct A2ACardIngester;

impl A2ACardIngester {
    /// Register all skills from an A2A Agent Card into the capability registry.
    ///
    /// The agent id is derived from the card URL using character substitution so
    /// that ingesting the same card twice overwrites rather than duplicates. All
    /// skill tags are merged into a single AgentCapabilities set. The card
    /// description is used as the agent manifest description for keyword routing.
    pub fn register(card: &A2AAgentCard, registry: &mut CapabilityRegistry) {
        let agent_id = Self::stable_id(&card.url);

        // Collect all tags from all skills into one capability set.
        let mut caps_builder = AgentCapabilities::builder();
        for skill in &card.skills {
            for tag in &skill.tags {
                caps_builder = caps_builder.with_tag(tag.clone());
            }
        }
        // Mirror A2A streaming flag into the AgentCapabilities.
        if card.capabilities.streaming {
            caps_builder = caps_builder.supports_streaming(true);
        }
        let capabilities = caps_builder.build();

        let manifest = AgentManifest::builder(agent_id.clone(), card.name.clone())
            .description(card.description.clone())
            .capabilities(capabilities)
            .build();

        registry.register(manifest);
    }

    /// Derive a stable agent id from a URL by replacing non-alphanumeric
    /// characters with underscores. This ensures re-ingesting the same card
    /// is idempotent.
    pub fn stable_id(url: &str) -> String {
        url.chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
            .trim_matches('_')
            .to_string()
    }

    /// Parse an A2A Agent Card from a JSON string.
    ///
    /// Returns an error if the JSON is malformed or missing required fields.
    pub fn from_json(json: &str) -> Result<A2AAgentCard, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Parse and immediately register an A2A Agent Card from a JSON string.
    ///
    /// Convenience method combining `from_json` and `register`.
    pub fn ingest_json(
        json: &str,
        registry: &mut CapabilityRegistry,
    ) -> Result<(), serde_json::Error> {
        let card = Self::from_json(json)?;
        Self::register(&card, registry);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_registry::CapabilityRegistry;

    fn sample_card_json() -> &'static str {
        r#"{
            "name": "document-parser",
            "description": "Extracts structured data from documents",
            "url": "https://agents.example.com/doc-parser",
            "version": "1.2.0",
            "skills": [
                {
                    "id": "extract-tables",
                    "name": "Extract Tables",
                    "description": "Extracts tables from PDF and Word documents into JSON",
                    "tags": ["extraction", "tables", "pdf", "word"]
                },
                {
                    "id": "extract-text",
                    "name": "Extract Text",
                    "description": "Returns plain text content from any supported document format",
                    "tags": ["extraction", "text", "ocr"]
                }
            ],
            "capabilities": {
                "streaming": true,
                "pushNotifications": false
            }
        }"#
    }

    #[test]
    fn parse_card_from_json() {
        let card = A2ACardIngester::from_json(sample_card_json()).unwrap();
        assert_eq!(card.name, "document-parser");
        assert_eq!(card.url, "https://agents.example.com/doc-parser");
        assert_eq!(card.version, "1.2.0");
        assert_eq!(card.skills.len(), 2);
        assert!(card.capabilities.streaming);
        assert!(!card.capabilities.push_notifications);
    }

    #[test]
    fn register_creates_agent_profile_in_registry() {
        let card = A2ACardIngester::from_json(sample_card_json()).unwrap();
        let mut registry = CapabilityRegistry::new();
        A2ACardIngester::register(&card, &mut registry);

        let agent_id = A2ACardIngester::stable_id(&card.url);
        let manifest = registry.find_by_id(&agent_id).expect("agent should be registered");
        assert_eq!(manifest.name, "document-parser");
    }

    #[test]
    fn skills_become_capabilities_with_correct_tags() {
        let card = A2ACardIngester::from_json(sample_card_json()).unwrap();
        let mut registry = CapabilityRegistry::new();
        A2ACardIngester::register(&card, &mut registry);

        let agent_id = A2ACardIngester::stable_id(&card.url);
        let manifest = registry.find_by_id(&agent_id).unwrap();
        assert!(manifest.capabilities.has_tag("pdf"));
        assert!(manifest.capabilities.has_tag("ocr"));
        assert!(manifest.capabilities.has_tag("extraction"));
    }

    #[test]
    fn ingest_json_is_idempotent() {
        let mut registry = CapabilityRegistry::new();
        A2ACardIngester::ingest_json(sample_card_json(), &mut registry).unwrap();
        A2ACardIngester::ingest_json(sample_card_json(), &mut registry).unwrap();
        // Registry should have exactly one agent, not two.
        assert_eq!(registry.agent_count(), 1);
    }

    #[test]
    fn stable_id_is_deterministic() {
        let id1 = A2ACardIngester::stable_id("https://agents.example.com/doc-parser");
        let id2 = A2ACardIngester::stable_id("https://agents.example.com/doc-parser");
        assert_eq!(id1, id2);
    }

    #[test]
    fn stable_id_differs_for_different_urls() {
        let id1 = A2ACardIngester::stable_id("https://agents.example.com/parser");
        let id2 = A2ACardIngester::stable_id("https://agents.example.com/summarizer");
        assert_ne!(id1, id2);
    }

    #[test]
    fn card_with_no_skills_registers_empty_capabilities() {
        let json = r#"{
            "name": "no-skill-agent",
            "description": "An agent with no declared skills",
            "url": "https://example.com/agent"
        }"#;
        let mut registry = CapabilityRegistry::new();
        A2ACardIngester::ingest_json(json, &mut registry).unwrap();
        let id = A2ACardIngester::stable_id("https://example.com/agent");
        let manifest = registry.find_by_id(&id).unwrap();
        assert_eq!(manifest.capabilities.tags.len(), 0);
    }

    #[test]
    fn streaming_flag_is_reflected_in_capabilities() {
        let card = A2ACardIngester::from_json(sample_card_json()).unwrap();
        let mut registry = CapabilityRegistry::new();
        A2ACardIngester::register(&card, &mut registry);

        let agent_id = A2ACardIngester::stable_id(&card.url);
        let manifest = registry.find_by_id(&agent_id).unwrap();
        assert!(manifest.capabilities.supports_streaming);
    }

    #[test]
    fn malformed_json_returns_error() {
        let mut registry = CapabilityRegistry::new();
        let result = A2ACardIngester::ingest_json("not valid json {{{", &mut registry);
        assert!(result.is_err());
    }
}
