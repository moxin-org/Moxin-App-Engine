//! WASM Plugin Manager
//!
//! Manages multiple WASM plugins with lifecycle management

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{RwLock, broadcast};
use tracing::{error, info};

use super::plugin::{PluginMetrics, WasmPlugin, WasmPluginConfig, WasmPluginState};
use super::runtime::{RuntimeConfig, WasmRuntime};
use super::types::{PluginCapability, PluginManifest, WasmError, WasmResult};

/// Plugin handle for external reference
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginHandle(pub String);

impl PluginHandle {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }

    pub fn id(&self) -> &str {
        &self.0
    }
}

impl From<&str> for PluginHandle {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for PluginHandle {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Loaded plugin information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedPlugin {
    /// Plugin ID
    pub id: String,
    /// Plugin manifest
    pub manifest: PluginManifest,
    /// Current state
    pub state: WasmPluginState,
    /// Load timestamp
    pub loaded_at: u64,
    /// Last activity timestamp
    pub last_activity: u64,
    /// Execution metrics
    pub metrics: PluginMetrics,
}

/// Plugin event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginEvent {
    /// Plugin loaded
    Loaded {
        plugin_id: String,
        manifest: Box<PluginManifest>,
    },
    /// Plugin initialized
    Initialized { plugin_id: String },
    /// Plugin state changed
    StateChanged {
        plugin_id: String,
        old_state: WasmPluginState,
        new_state: WasmPluginState,
    },
    /// Plugin executed function
    Executed {
        plugin_id: String,
        function: String,
        duration_ms: u64,
        success: bool,
    },
    /// Plugin unloaded
    Unloaded { plugin_id: String },
    /// Plugin error
    Error { plugin_id: String, error: String },
}

/// Plugin registry for tracking loaded plugins
pub struct PluginRegistry {
    /// Registered plugins by ID
    plugins: RwLock<HashMap<String, PluginInfo>>,
    /// Plugins by capability
    by_capability: RwLock<HashMap<PluginCapability, Vec<String>>>,
}

/// Plugin info stored in registry
struct PluginInfo {
    manifest: PluginManifest,
    source_hash: String,
    registered_at: u64,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: RwLock::new(HashMap::new()),
            by_capability: RwLock::new(HashMap::new()),
        }
    }

    /// Register a plugin
    pub async fn register(&self, plugin_id: &str, manifest: PluginManifest, source_hash: &str) {
        let info = PluginInfo {
            manifest: manifest.clone(),
            source_hash: source_hash.to_string(),
            registered_at: now_secs(),
        };

        // Register in main map
        self.plugins
            .write()
            .await
            .insert(plugin_id.to_string(), info);

        // Index by capability
        let mut by_cap = self.by_capability.write().await;
        for cap in &manifest.capabilities {
            by_cap
                .entry(cap.clone())
                .or_insert_with(Vec::new)
                .push(plugin_id.to_string());
        }
    }

    /// Unregister a plugin
    pub async fn unregister(&self, plugin_id: &str) {
        if let Some(info) = self.plugins.write().await.remove(plugin_id) {
            // Remove from capability index
            let mut by_cap = self.by_capability.write().await;
            for cap in &info.manifest.capabilities {
                if let Some(ids) = by_cap.get_mut(cap) {
                    ids.retain(|id| id != plugin_id);
                }
            }
        }
    }

    /// Get plugins with a specific capability
    pub async fn with_capability(&self, cap: &PluginCapability) -> Vec<String> {
        self.by_capability
            .read()
            .await
            .get(cap)
            .cloned()
            .unwrap_or_default()
    }

    /// Check if plugin is registered
    pub async fn is_registered(&self, plugin_id: &str) -> bool {
        self.plugins.read().await.contains_key(plugin_id)
    }

    /// Get plugin manifest
    pub async fn get_manifest(&self, plugin_id: &str) -> Option<PluginManifest> {
        self.plugins
            .read()
            .await
            .get(plugin_id)
            .map(|info| info.manifest.clone())
    }

    /// List all registered plugins
    pub async fn list(&self) -> Vec<String> {
        self.plugins.read().await.keys().cloned().collect()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// WASM Plugin Manager
pub struct WasmPluginManager {
    /// WASM runtime
    runtime: Arc<WasmRuntime>,
    /// Loaded plugins
    plugins: RwLock<HashMap<String, Arc<WasmPlugin>>>,
    /// Plugin registry
    registry: PluginRegistry,
    /// Event broadcaster
    event_tx: broadcast::Sender<PluginEvent>,
    /// Default plugin config
    default_config: WasmPluginConfig,
    /// Manager statistics
    stats: RwLock<ManagerStats>,
}

/// Manager statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManagerStats {
    /// Total plugins loaded
    pub total_loaded: u64,
    /// Total plugins unloaded
    pub total_unloaded: u64,
    /// Currently active plugins
    pub active_plugins: usize,
    /// Total function calls
    pub total_calls: u64,
    /// Failed function calls
    pub failed_calls: u64,
    /// Total execution time in milliseconds
    pub total_execution_time_ms: u64,
}

impl WasmPluginManager {
    /// Create a new plugin manager
    pub fn new(runtime: Arc<WasmRuntime>) -> Self {
        let (event_tx, _) = broadcast::channel(1024);

        Self {
            runtime,
            plugins: RwLock::new(HashMap::new()),
            registry: PluginRegistry::new(),
            event_tx,
            default_config: WasmPluginConfig::default(),
            stats: RwLock::new(ManagerStats::default()),
        }
    }

    /// Create with custom runtime config
    pub fn with_runtime_config(config: RuntimeConfig) -> WasmResult<Self> {
        let runtime = Arc::new(WasmRuntime::new(config)?);
        Ok(Self::new(runtime))
    }

    /// Get the runtime
    pub fn runtime(&self) -> &Arc<WasmRuntime> {
        &self.runtime
    }

    /// Subscribe to plugin events
    pub fn subscribe(&self) -> broadcast::Receiver<PluginEvent> {
        self.event_tx.subscribe()
    }

    /// Set default plugin configuration
    pub fn set_default_config(&mut self, config: WasmPluginConfig) {
        self.default_config = config;
    }

    /// Load a plugin from bytes
    pub async fn load_bytes(
        &self,
        bytes: &[u8],
        config: Option<WasmPluginConfig>,
    ) -> WasmResult<PluginHandle> {
        let config = config.unwrap_or_else(|| self.default_config.clone());
        let plugin_id = config.id.clone();

        // Check if already loaded
        if self.plugins.read().await.contains_key(&plugin_id) {
            return Err(WasmError::PluginAlreadyLoaded(plugin_id));
        }

        // Create plugin via runtime
        let plugin = self.runtime.create_plugin_from_bytes(bytes, config).await?;
        let manifest = plugin.manifest().clone();

        // Store plugin
        let plugin = Arc::new(plugin);
        self.plugins
            .write()
            .await
            .insert(plugin_id.clone(), plugin.clone());

        // Register in registry
        self.registry
            .register(&plugin_id, manifest.clone(), "")
            .await;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_loaded += 1;
            stats.active_plugins = self.plugins.read().await.len();
        }

        // Emit event
        let _ = self.event_tx.send(PluginEvent::Loaded {
            plugin_id: plugin_id.clone(),
            manifest: Box::new(manifest),
        });

        info!("Loaded plugin: {}", plugin_id);
        Ok(PluginHandle::new(&plugin_id))
    }

    /// Load a plugin from WAT
    pub async fn load_wat(
        &self,
        wat: &str,
        config: Option<WasmPluginConfig>,
    ) -> WasmResult<PluginHandle> {
        let bytes = wat.to_string().into_bytes();
        self.load_bytes(&bytes, config).await
    }

    /// Load a plugin from file
    pub async fn load_file(
        &self,
        path: &Path,
        config: Option<WasmPluginConfig>,
    ) -> WasmResult<PluginHandle> {
        let bytes = tokio::fs::read(path).await?;
        self.load_bytes(&bytes, config).await
    }

    /// Initialize a plugin
    pub async fn initialize(&self, handle: &PluginHandle) -> WasmResult<()> {
        let plugin = self.get_plugin(handle).await?;

        let old_state = plugin.state().await;
        plugin.initialize().await?;
        let new_state = plugin.state().await;

        let _ = self.event_tx.send(PluginEvent::StateChanged {
            plugin_id: handle.id().to_string(),
            old_state,
            new_state,
        });

        let _ = self.event_tx.send(PluginEvent::Initialized {
            plugin_id: handle.id().to_string(),
        });

        Ok(())
    }

    /// Unload a plugin
    pub async fn unload(&self, handle: &PluginHandle) -> WasmResult<()> {
        let plugin_id = handle.id();

        // Get and stop plugin
        if let Some(plugin) = self.plugins.write().await.remove(plugin_id) {
            plugin.stop().await?;
        }

        // Unregister
        self.registry.unregister(plugin_id).await;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_unloaded += 1;
            stats.active_plugins = self.plugins.read().await.len();
        }

        let _ = self.event_tx.send(PluginEvent::Unloaded {
            plugin_id: plugin_id.to_string(),
        });

        info!("Unloaded plugin: {}", plugin_id);
        Ok(())
    }

    /// Get a plugin by handle
    pub async fn get_plugin(&self, handle: &PluginHandle) -> WasmResult<Arc<WasmPlugin>> {
        self.plugins
            .read()
            .await
            .get(handle.id())
            .cloned()
            .ok_or_else(|| WasmError::PluginNotFound(handle.id().to_string()))
    }

    /// Call a function on a plugin
    pub async fn call_i32(
        &self,
        handle: &PluginHandle,
        function: &str,
        args: &[wasmtime::Val],
    ) -> WasmResult<i32> {
        let plugin = self.get_plugin(handle).await?;
        let start = Instant::now();

        let result = plugin.call_i32(function, args).await;
        let duration = start.elapsed();

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_calls += 1;
            stats.total_execution_time_ms += duration.as_millis() as u64;
            if result.is_err() {
                stats.failed_calls += 1;
            }
        }

        // Emit event
        let _ = self.event_tx.send(PluginEvent::Executed {
            plugin_id: handle.id().to_string(),
            function: function.to_string(),
            duration_ms: duration.as_millis() as u64,
            success: result.is_ok(),
        });

        result
    }

    /// Call a void function on a plugin
    pub async fn call_void(
        &self,
        handle: &PluginHandle,
        function: &str,
        args: &[wasmtime::Val],
    ) -> WasmResult<()> {
        let plugin = self.get_plugin(handle).await?;
        let start = Instant::now();

        let result = plugin.call_void(function, args).await;
        let duration = start.elapsed();

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_calls += 1;
            stats.total_execution_time_ms += duration.as_millis() as u64;
            if result.is_err() {
                stats.failed_calls += 1;
            }
        }

        // Emit event
        let _ = self.event_tx.send(PluginEvent::Executed {
            plugin_id: handle.id().to_string(),
            function: function.to_string(),
            duration_ms: duration.as_millis() as u64,
            success: result.is_ok(),
        });

        result
    }

    /// Get plugin state
    pub async fn get_state(&self, handle: &PluginHandle) -> WasmResult<WasmPluginState> {
        let plugin = self.get_plugin(handle).await?;
        Ok(plugin.state().await)
    }

    /// Get plugin metrics
    pub async fn get_metrics(&self, handle: &PluginHandle) -> WasmResult<PluginMetrics> {
        let plugin = self.get_plugin(handle).await?;
        Ok(plugin.metrics().await)
    }

    /// Get plugin info
    pub async fn get_info(&self, handle: &PluginHandle) -> WasmResult<LoadedPlugin> {
        let plugin = self.get_plugin(handle).await?;

        Ok(LoadedPlugin {
            id: plugin.id().to_string(),
            manifest: plugin.manifest().clone(),
            state: plugin.state().await,
            loaded_at: now_secs(),
            last_activity: now_secs(),
            metrics: plugin.metrics().await,
        })
    }

    /// List all loaded plugins
    pub async fn list_plugins(&self) -> Vec<PluginHandle> {
        self.plugins
            .read()
            .await
            .keys()
            .map(|id| PluginHandle::new(id))
            .collect()
    }

    /// Get plugins with specific capability
    pub async fn plugins_with_capability(&self, cap: &PluginCapability) -> Vec<PluginHandle> {
        self.registry
            .with_capability(cap)
            .await
            .into_iter()
            .map(PluginHandle)
            .collect()
    }

    /// Get manager statistics
    pub async fn stats(&self) -> ManagerStats {
        let mut stats = self.stats.read().await.clone();
        stats.active_plugins = self.plugins.read().await.len();
        stats
    }

    /// Unload all plugins
    pub async fn unload_all(&self) -> WasmResult<()> {
        let handles: Vec<_> = self.list_plugins().await;
        for handle in handles {
            if let Err(e) = self.unload(&handle).await {
                error!("Failed to unload plugin {}: {}", handle.id(), e);
            }
        }
        Ok(())
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::super::types::ExecutionConfig;
    use super::*;

    /// Create a test runtime without async support for synchronous tests
    fn create_test_runtime() -> WasmResult<WasmRuntime> {
        let config = RuntimeConfig {
            execution_config: ExecutionConfig {
                async_support: false,
                fuel_metering: false,
                epoch_interruption: false,
                ..ExecutionConfig::default()
            },
            ..RuntimeConfig::default()
        };
        WasmRuntime::new(config)
    }

    #[test]
    fn test_plugin_handle() {
        let handle = PluginHandle::new("test-plugin");
        assert_eq!(handle.id(), "test-plugin");

        let handle2: PluginHandle = "another".into();
        assert_eq!(handle2.id(), "another");
    }

    #[tokio::test]
    async fn test_plugin_registry() {
        let registry = PluginRegistry::new();

        let manifest = PluginManifest::new_from_str("test", "1.0.0").unwrap()
            .with_capability(PluginCapability::ReadConfig)
            .with_capability(PluginCapability::SendMessage);

        registry.register("test", manifest, "hash123").await;

        assert!(registry.is_registered("test").await);
        assert!(!registry.is_registered("other").await);

        let with_read = registry
            .with_capability(&PluginCapability::ReadConfig)
            .await;
        assert!(with_read.contains(&"test".to_string()));

        registry.unregister("test").await;
        assert!(!registry.is_registered("test").await);
    }

    #[tokio::test]
    async fn test_plugin_manager_creation() {
        let runtime = Arc::new(create_test_runtime().unwrap());
        let manager = WasmPluginManager::new(runtime);

        let stats = manager.stats().await;
        assert_eq!(stats.active_plugins, 0);
        assert_eq!(stats.total_loaded, 0);
    }

    #[tokio::test]
    async fn test_plugin_manager_load_wat() {
        let runtime = Arc::new(create_test_runtime().unwrap());
        let manager = WasmPluginManager::new(runtime);

        let wat = r#"
            (module
                (func (export "greet") (result i32)
                    i32.const 42
                )
            )
        "#;

        let mut config = WasmPluginConfig::new("greet-plugin");
        config.resource_limits.max_fuel = None;
        let handle = manager.load_wat(wat, Some(config)).await.unwrap();

        assert_eq!(handle.id(), "greet-plugin");

        let plugins = manager.list_plugins().await;
        assert_eq!(plugins.len(), 1);

        // Initialize
        manager.initialize(&handle).await.unwrap();

        // Call function
        let result = manager.call_i32(&handle, "greet", &[]).await.unwrap();
        assert_eq!(result, 42);

        // Check stats
        let stats = manager.stats().await;
        assert_eq!(stats.total_calls, 1);
        assert_eq!(stats.active_plugins, 1);

        // Unload
        manager.unload(&handle).await.unwrap();
        assert_eq!(manager.list_plugins().await.len(), 0);
    }

    #[tokio::test]
    async fn test_plugin_manager_events() {
        let runtime = Arc::new(create_test_runtime().unwrap());
        let manager = WasmPluginManager::new(runtime);

        let mut rx = manager.subscribe();

        let wat = r#"(module (func (export "test")))"#;
        let mut config = WasmPluginConfig::new("event-test");
        config.resource_limits.max_fuel = None;

        let handle = manager.load_wat(wat, Some(config)).await.unwrap();

        // Should receive Loaded event
        if let Ok(event) = rx.try_recv() {
            match event {
                PluginEvent::Loaded { plugin_id, .. } => {
                    assert_eq!(plugin_id, "event-test");
                }
                _ => panic!("Expected Loaded event"),
            }
        }

        manager.unload(&handle).await.unwrap();
    }
}
