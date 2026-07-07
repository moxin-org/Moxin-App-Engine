use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DEFAULT_PLUGIN_REPO_ID: &str = "official";
pub const DEFAULT_PLUGIN_REPO_URL: &str = "https://plugins.mofa.dev";
pub const DEFAULT_PLUGIN_REPO_DESCRIPTION: &str = "Official MoFA plugin catalog.";

/// Plugin repository metadata stored on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRepoEntry {
    pub id: String,
    pub url: String,
    pub description: Option<String>,
    pub last_synced: Option<DateTime<Utc>>,
}

/// Describes a plugin that can be installed from a repository.
#[derive(Debug, Clone)]
pub struct PluginCatalogEntry {
    pub id: String,
    pub repo_id: String,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub config: Value,
    /// semver string, e.g. "1.2.3"
    pub version: Option<String>,
    /// base64-encoded Ed25519 public key of the publisher
    pub publisher_key: Option<String>,
    /// base64-encoded Ed25519 signature over the canonical registry payload
    pub signature: Option<String>,
}

fn base_catalog() -> Vec<PluginCatalogEntry> {
    vec![PluginCatalogEntry {
        id: "http-plugin".to_string(),
        repo_id: DEFAULT_PLUGIN_REPO_ID.to_string(),
        name: "HTTP Helper".to_string(),
        description:
            "Implements the builtin HTTP helper plugin and exposes an HTTP client to agents."
                .to_string(),
        kind: "builtin:http".to_string(),
        config: serde_json::json!({ "url": "https://example.com" }),
        version: Some("1.0.0".to_string()),
        // populated by the official registry; None in the embedded catalog
        publisher_key: None,
        signature: None,
    }]
}

/// Returns the catalog repositories that should be seeded by default.
pub fn default_repos() -> Vec<PluginRepoEntry> {
    vec![PluginRepoEntry {
        id: DEFAULT_PLUGIN_REPO_ID.to_string(),
        url: DEFAULT_PLUGIN_REPO_URL.to_string(),
        description: Some(DEFAULT_PLUGIN_REPO_DESCRIPTION.to_string()),
        last_synced: None,
    }]
}

/// Returns all catalog entries available to the CLI.
pub fn catalog_entries() -> Vec<PluginCatalogEntry> {
    base_catalog()
}

/// Returns catalog entries filtered by repository identifier.
pub fn catalog_for_repo(repo_id: &str) -> Vec<PluginCatalogEntry> {
    base_catalog()
        .into_iter()
        .filter(|entry| entry.repo_id == repo_id)
        .collect()
}

/// Find a single catalog entry by repository and plugin id.
pub fn find_catalog_entry(repo_id: &str, plugin_id: &str) -> Option<PluginCatalogEntry> {
    base_catalog()
        .into_iter()
        .find(|entry| entry.repo_id == repo_id && entry.id == plugin_id)
}
