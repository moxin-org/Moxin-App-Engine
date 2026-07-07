//! CLI context providing access to backend services

use crate::CliError;
use crate::plugin_catalog::{DEFAULT_PLUGIN_REPO_ID, PluginRepoEntry, default_repos};
use crate::state::PersistentAgentRegistry;
use crate::store::PersistedStore;
use crate::utils::AgentProcessManager;
use crate::utils::paths;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mofa_foundation::agent::base::BaseAgent;
use mofa_foundation::agent::components::tool::EchoTool;
use mofa_foundation::agent::session::SessionManager;
use mofa_foundation::agent::tools::registry::{ToolRegistry, ToolSource};
use mofa_kernel::agent::AgentCapabilities;
use mofa_kernel::agent::components::tool::ToolExt;
use mofa_kernel::agent::config::AgentConfig;
use mofa_kernel::agent::core::MoFAAgent;
use mofa_kernel::agent::error::{AgentError, AgentResult};
use mofa_kernel::agent::plugins::PluginRegistry;
use mofa_runtime::agent::AgentFactory;
use mofa_runtime::agent::plugins::{HttpPlugin, SimplePluginRegistry};
use mofa_runtime::agent::registry::AgentRegistry;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

const BUILTIN_HTTP_PLUGIN_KIND: &str = "builtin:http";
const BUILTIN_ECHO_TOOL_KIND: &str = "builtin:echo";
const CLI_BASE_FACTORY_KIND: &str = "cli-base";

/// Persistent entry for agent configuration and state metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfigEntry {
    /// Unique identifier for the agent.
    pub id: String,
    /// Human-readable name of the agent.
    pub name: String,
    /// Last known execution state (e.g., "Running", "Ready").
    pub state: String,
    /// Timestamp when the agent was last started.
    pub started_at: DateTime<Utc>,
    /// Optional AI provider used by the agent (e.g., "openai", "anthropic").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Optional AI model name (e.g., "gpt-4", "claude-3").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional description of the agent's purpose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSpecEntry {
    pub id: String,
    pub kind: String,
    pub enabled: bool,
    pub config: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    /// semver string recorded at install time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// base64-encoded Ed25519 public key stored for future revocation checks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpecEntry {
    pub id: String,
    pub kind: String,
    pub enabled: bool,
    pub config: Value,
}

/// Shared context for CLI commands, holding references to backend services
pub struct CliContext {
    /// Session manager with file-based persistence
    pub session_manager: SessionManager,
    /// In-memory agent registry
    pub agent_registry: AgentRegistry,
    /// Persistent agent metadata store
    pub agent_store: PersistedStore<AgentConfigEntry>,
    /// Persistent plugin source specifications
    pub plugin_store: PersistedStore<PluginSpecEntry>,
    /// Persistent tool source specifications
    pub tool_store: PersistedStore<ToolSpecEntry>,
    /// Persistent plugin repository definitions
    pub plugin_repo_store: PersistedStore<PluginRepoEntry>,
    /// Persistent agent state storage
    pub persistent_agents: Arc<PersistentAgentRegistry>,
    /// Agent process manager for spawning/managing processes
    pub process_manager: AgentProcessManager,
    /// In-memory plugin registry
    pub plugin_registry: Arc<SimplePluginRegistry>,
    /// In-memory tool registry
    pub tool_registry: ToolRegistry,
    /// Platform-specific data directory (~/.local/share/mofa or equivalent)
    pub data_dir: PathBuf,
    /// Platform-specific config directory (~/.config/mofa or equivalent)
    pub config_dir: PathBuf,
}

impl CliContext {
    /// Initialize the CLI context with default backend services
    pub async fn new() -> Result<Self, CliError> {
        let data_dir = paths::ensure_mofa_data_dir()?;
        let config_dir = paths::ensure_mofa_config_dir()?;
        migrate_legacy_nested_sessions(&data_dir)?;

        let session_manager = SessionManager::with_jsonl(&data_dir).await.map_err(|e| {
            CliError::InitError(format!("Failed to initialize session manager: {}", e))
        })?;
        let agent_store = PersistedStore::new(data_dir.join("agents"))?;
        let agent_registry = AgentRegistry::new();
        register_default_agent_factories(&agent_registry).await?;
        let plugin_store = PersistedStore::new(data_dir.join("plugins"))?;
        let tool_store = PersistedStore::new(data_dir.join("tools"))?;
        let plugin_repo_store = PersistedStore::new(data_dir.join("plugin_repos"))?;
        seed_default_specs(&plugin_store, &tool_store)?;
        seed_default_repos(&plugin_repo_store)?;

        let plugin_registry = Arc::new(SimplePluginRegistry::new());
        replay_persisted_plugins(&plugin_registry, &plugin_store)?;
        let mut tool_registry = ToolRegistry::new();
        replay_persisted_tools(&mut tool_registry, &tool_store)?;

        let states_dir = data_dir.join("agent_states");
        let persistent_agents = Arc::new(PersistentAgentRegistry::new(states_dir).await.map_err(
            |e| {
                CliError::InitError(format!(
                    "Failed to initialize persistent agent registry: {}",
                    e
                ))
            },
        )?);

        let process_manager = AgentProcessManager::new(config_dir.clone());

        Ok(Self {
            session_manager,
            agent_registry,
            agent_store,
            plugin_store,
            tool_store,
            plugin_repo_store,
            persistent_agents,
            process_manager,
            plugin_registry,
            tool_registry,
            data_dir,
            config_dir,
        })
    }
}

#[cfg(test)]
impl CliContext {
    pub async fn with_temp_dir(temp_dir: &std::path::Path) -> Result<Self, CliError> {
        let data_dir = temp_dir.join("data");
        let config_dir = temp_dir.join("config");
        std::fs::create_dir_all(&data_dir)?;
        std::fs::create_dir_all(&config_dir)?;
        migrate_legacy_nested_sessions(&data_dir)?;

        let session_manager = SessionManager::with_jsonl(&data_dir)
            .await
            .map_err(|e| CliError::InitError(format!("{}", e)))?;
        let agent_store = PersistedStore::new(data_dir.join("agents"))?;
        let agent_registry = AgentRegistry::new();
        register_default_agent_factories(&agent_registry).await?;
        let plugin_store = PersistedStore::new(data_dir.join("plugins"))?;
        let tool_store = PersistedStore::new(data_dir.join("tools"))?;
        let plugin_repo_store = PersistedStore::new(data_dir.join("plugin_repos"))?;
        seed_default_specs(&plugin_store, &tool_store)?;
        seed_default_repos(&plugin_repo_store)?;

        let plugin_registry = Arc::new(SimplePluginRegistry::new());
        replay_persisted_plugins(&plugin_registry, &plugin_store)?;
        let mut tool_registry = ToolRegistry::new();
        replay_persisted_tools(&mut tool_registry, &tool_store)?;

        let states_dir = data_dir.join("agent_states");
        let persistent_agents = Arc::new(PersistentAgentRegistry::new(states_dir).await.map_err(
            |e| {
                CliError::InitError(format!(
                    "Failed to initialize persistent agent registry: {}",
                    e
                ))
            },
        )?);

        let process_manager = AgentProcessManager::new(config_dir.clone());

        Ok(Self {
            session_manager,
            agent_registry,
            agent_store,
            plugin_store,
            tool_store,
            plugin_repo_store,
            persistent_agents,
            process_manager,
            plugin_registry,
            tool_registry,
            data_dir,
            config_dir,
        })
    }
}

struct CliBaseAgentFactory;

#[async_trait]
impl AgentFactory for CliBaseAgentFactory {
    async fn create(&self, config: AgentConfig) -> AgentResult<Arc<RwLock<dyn MoFAAgent>>> {
        let mut agent =
            BaseAgent::new(config.id, config.name).with_capabilities(self.default_capabilities());

        if let Some(description) = config.description {
            agent = agent.with_description(description);
        }

        if let Some(version) = config.version {
            agent = agent.with_version(version);
        }

        Ok(Arc::new(RwLock::new(agent)))
    }

    fn type_id(&self) -> &str {
        CLI_BASE_FACTORY_KIND
    }

    fn default_capabilities(&self) -> AgentCapabilities {
        AgentCapabilities::builder().tag("cli").tag("base").build()
    }

    fn validate_config(&self, config: &AgentConfig) -> AgentResult<()> {
        if config.id.trim().is_empty() {
            return Err(AgentError::ConfigError(
                "Agent id cannot be empty".to_string(),
            ));
        }
        if config.name.trim().is_empty() {
            return Err(AgentError::ConfigError(
                "Agent name cannot be empty".to_string(),
            ));
        }
        if !config.enabled {
            return Err(AgentError::ConfigError(
                "Cannot start disabled agent config".to_string(),
            ));
        }
        Ok(())
    }

    fn description(&self) -> Option<&str> {
        Some("Default CLI base-agent factory")
    }
}

async fn register_default_agent_factories(agent_registry: &AgentRegistry) -> Result<(), CliError> {
    if agent_registry
        .list_factory_types()
        .await
        .iter()
        .any(|kind| kind == CLI_BASE_FACTORY_KIND)
    {
        return Ok(());
    }

    agent_registry
        .register_factory(Arc::new(CliBaseAgentFactory))
        .await
        .map_err(|e| {
            CliError::InitError(format!("Failed to register default agent factory: {}", e))
        })?;

    Ok(())
}

fn seed_default_specs(
    plugin_store: &PersistedStore<PluginSpecEntry>,
    tool_store: &PersistedStore<ToolSpecEntry>,
) -> Result<(), CliError> {
    let default_plugin = PluginSpecEntry {
        id: "http-plugin".to_string(),
        kind: BUILTIN_HTTP_PLUGIN_KIND.to_string(),
        enabled: true,
        config: serde_json::json!({
            "url": "https://example.com",
        }),
        description: Some("Built-in HTTP helper plugin".to_string()),
        repo_id: Some(DEFAULT_PLUGIN_REPO_ID.to_string()),
        version: Some("1.0.0".to_string()),
        publisher_key: None,
    };
    if plugin_store.get(&default_plugin.id)?.is_none() {
        plugin_store.save(&default_plugin.id, &default_plugin)?;
    }

    let default_tool = ToolSpecEntry {
        id: "echo".to_string(),
        kind: BUILTIN_ECHO_TOOL_KIND.to_string(),
        enabled: true,
        config: Value::Null,
    };
    if tool_store.get(&default_tool.id)?.is_none() {
        tool_store.save(&default_tool.id, &default_tool)?;
    }

    Ok(())
}

/// Instantiate a plugin from a persisted spec entry.
///
/// Returns `Some(plugin)` for recognised builtin kinds, or `None` for
/// unknown kinds (forward-compatible).
pub fn instantiate_plugin_from_spec(
    spec: &PluginSpecEntry,
) -> Option<Arc<dyn mofa_kernel::agent::plugins::Plugin>> {
    match spec.kind.as_str() {
        BUILTIN_HTTP_PLUGIN_KIND => {
            let url = spec
                .config
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("https://example.com");
            Some(Arc::new(HttpPlugin::new(url)))
        }
        _ => None,
    }
}

fn seed_default_repos(store: &PersistedStore<PluginRepoEntry>) -> Result<(), CliError> {
    if !store.list()?.is_empty() {
        return Ok(());
    }
    for repo in default_repos() {
        store.save(&repo.id, &repo)?;
    }
    Ok(())
}

fn replay_persisted_plugins(
    plugin_registry: &Arc<SimplePluginRegistry>,
    plugin_store: &PersistedStore<PluginSpecEntry>,
) -> Result<(), CliError> {
    for (_, spec) in plugin_store.list()? {
        if !spec.enabled {
            continue;
        }

        if let Some(plugin) = instantiate_plugin_from_spec(&spec) {
            plugin_registry.register(plugin).map_err(|e| {
                CliError::PluginError(format!("Failed to register plugin '{}': {}", spec.id, e))
            })?;
        }
    }

    Ok(())
}

fn replay_persisted_tools(
    tool_registry: &mut ToolRegistry,
    tool_store: &PersistedStore<ToolSpecEntry>,
) -> Result<(), CliError> {
    for (_, spec) in tool_store.list()? {
        if !spec.enabled {
            continue;
        }

        match spec.kind.as_str() {
            BUILTIN_ECHO_TOOL_KIND => {
                tool_registry
                    .register_with_source(EchoTool.into_dynamic(), ToolSource::Builtin)
                    .map_err(|e| {
                        CliError::ToolError(format!("Failed to register tool '{}': {}", spec.id, e))
                    })?;
            }
            _ => {
                // Ignore unknown kinds for forward compatibility.
            }
        }
    }

    Ok(())
}

fn migrate_legacy_nested_sessions(data_dir: &Path) -> Result<(), CliError> {
    let sessions_dir = data_dir.join("sessions");
    let legacy_dir = sessions_dir.join("sessions");
    if !legacy_dir.exists() {
        return Ok(());
    }

    std::fs::create_dir_all(&sessions_dir)?;
    for entry in std::fs::read_dir(&legacy_dir)? {
        let entry = entry?;
        let src = entry.path();
        let dst = sessions_dir.join(entry.file_name());

        if dst.exists() {
            continue;
        }

        std::fs::rename(&src, &dst)?;
    }

    let _ = std::fs::remove_dir(&legacy_dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofa_foundation::agent::session::Session;
    use mofa_kernel::agent::components::tool::ToolRegistry as ToolRegistryTrait;
    use mofa_kernel::agent::plugins::PluginRegistry as PluginRegistryTrait;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_agent_store_persists_across_context_instances() {
        let temp = TempDir::new().unwrap();

        let ctx1 = CliContext::with_temp_dir(temp.path()).await.unwrap();
        let entry = AgentConfigEntry {
            id: "persisted-agent".to_string(),
            name: "Persisted Agent".to_string(),
            state: "Running".to_string(),
            started_at: Utc::now(),
            provider: None,
            model: None,
            description: None,
        };
        ctx1.agent_store.save("persisted-agent", &entry).unwrap();
        drop(ctx1);

        let ctx2 = CliContext::with_temp_dir(temp.path()).await.unwrap();
        let loaded = ctx2.agent_store.get("persisted-agent").unwrap();

        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().id, "persisted-agent");
    }

    #[tokio::test]
    async fn test_legacy_nested_sessions_are_migrated() {
        let temp = TempDir::new().unwrap();
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        let legacy_manager = SessionManager::with_jsonl(data_dir.join("sessions"))
            .await
            .unwrap();
        let mut session = Session::new("legacy-session");
        session.add_message("user", "hello");
        legacy_manager.save(&session).await.unwrap();

        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();
        let loaded = ctx.session_manager.get("legacy-session").await.unwrap();

        assert!(loaded.is_some());
        assert!(
            data_dir
                .join("sessions")
                .join("legacy-session.jsonl")
                .exists()
        );
    }

    #[tokio::test]
    async fn test_plugin_and_tool_specs_replayed_on_startup() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        assert!(PluginRegistryTrait::contains(
            ctx.plugin_registry.as_ref(),
            "http-plugin"
        ));
        assert!(ToolRegistryTrait::contains(&ctx.tool_registry, "echo"));
    }

    #[tokio::test]
    async fn test_disabled_plugin_spec_is_not_replayed() {
        let temp = TempDir::new().unwrap();

        let ctx1 = CliContext::with_temp_dir(temp.path()).await.unwrap();
        let mut spec = ctx1.plugin_store.get("http-plugin").unwrap().unwrap();
        spec.enabled = false;
        ctx1.plugin_store.save("http-plugin", &spec).unwrap();
        drop(ctx1);

        let ctx2 = CliContext::with_temp_dir(temp.path()).await.unwrap();
        assert!(!PluginRegistryTrait::contains(
            ctx2.plugin_registry.as_ref(),
            "http-plugin"
        ));
    }

    #[tokio::test]
    async fn test_default_agent_factory_registered_on_startup() {
        let temp = TempDir::new().unwrap();
        let ctx = CliContext::with_temp_dir(temp.path()).await.unwrap();

        let factory_types = ctx.agent_registry.list_factory_types().await;
        assert!(factory_types.iter().any(|k| k == CLI_BASE_FACTORY_KIND));
    }
}
