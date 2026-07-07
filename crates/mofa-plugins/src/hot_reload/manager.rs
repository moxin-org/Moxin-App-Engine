//! Hot-reload manager
//!
//! Coordinates plugin loading, watching, and reloading

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, RwLock, broadcast, mpsc};
use tracing::{debug, error, info, warn};

use super::loader::{DynamicPlugin, PluginLoadError, PluginLoader};
use super::registry::{PluginInfo, PluginRegistry, PluginVersion};
use super::state::{StateManager, StateSnapshot};
use super::watcher::{PluginWatcher, WatchConfig, WatchEventKind};
use crate::{PluginContext, PluginResult};

/// Hot-reload configuration extension with watcher-specific settings
#[derive(Debug, Clone)]
pub struct HotReloadConfig {
    /// Base kernel hot reload configuration
    pub base: mofa_kernel::plugin::HotReloadConfig,
    /// Watch configuration
    pub watch_config: WatchConfig,
    /// Graceful shutdown timeout
    pub shutdown_timeout: Duration,
    /// Enable parallel reloading
    pub parallel_reload: bool,
}

impl Default for HotReloadConfig {
    fn default() -> Self {
        Self {
            base: mofa_kernel::plugin::HotReloadConfig::default(),
            watch_config: WatchConfig::default(),
            shutdown_timeout: Duration::from_secs(30),
            parallel_reload: false,
        }
    }
}

impl HotReloadConfig {
    /// Create a new configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set base kernel hot reload configuration
    pub fn with_base(mut self, base: mofa_kernel::plugin::HotReloadConfig) -> Self {
        self.base = base;
        self
    }

    /// Set reload strategy
    pub fn with_strategy(mut self, strategy: mofa_kernel::plugin::ReloadStrategy) -> Self {
        self.base.strategy = strategy;
        self
    }

    /// Enable/disable state preservation
    pub fn with_preserve_state(mut self, enabled: bool) -> Self {
        self.base.preserve_state = enabled;
        self
    }

    /// Enable/disable auto rollback
    pub fn with_auto_rollback(mut self, enabled: bool) -> Self {
        self.base.auto_rollback = enabled;
        self
    }

    /// Set max reload attempts
    pub fn with_max_attempts(mut self, max: u32) -> Self {
        self.base.max_reload_attempts = max;
        self
    }

    /// Set reload cooldown
    pub fn with_reload_cooldown(mut self, cooldown: Duration) -> Self {
        self.base.reload_cooldown = cooldown;
        self
    }

    /// Set watch configuration
    pub fn with_watch_config(mut self, watch_config: WatchConfig) -> Self {
        self.watch_config = watch_config;
        self
    }

    /// Set shutdown timeout
    pub fn with_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }

    /// Enable/disable parallel reload
    pub fn with_parallel_reload(mut self, enabled: bool) -> Self {
        self.parallel_reload = enabled;
        self
    }
}

// Use kernel-defined hot reload types
pub use mofa_kernel::plugin::{ReloadEvent, ReloadStrategy};

/// Reload result
#[derive(Debug)]
pub struct ReloadResult {
    /// Plugin ID
    pub plugin_id: String,
    /// Success status
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Reload duration
    pub duration: Duration,
    /// Whether state was preserved
    pub state_preserved: bool,
    /// Number of attempts
    pub attempts: u32,
}

/// Reload error types.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReloadError {
    #[error("Plugin not found: {0}")]
    PluginNotFound(String),

    #[error("Load error: {0}")]
    LoadError(#[from] PluginLoadError),

    #[error("State preservation failed: {0}")]
    StateError(String),

    #[error("Initialization failed: {0}")]
    InitError(String),

    #[error("Max reload attempts exceeded")]
    MaxAttemptsExceeded,

    #[error("Plugin still in use")]
    PluginInUse,

    #[error("Watch error: {0}")]
    WatchError(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Error-stack–backed result alias for reload operations.
///
/// Obtain one by calling [`IntoReloadReport::into_report`] on a
/// `Result<T, ReloadError>`.
pub type ReloadReport<T> = ::std::result::Result<T, error_stack::Report<ReloadError>>;

/// Extension trait to upgrade a `Result<T, ReloadError>` into a [`ReloadReport<T>`].
pub trait IntoReloadReport<T> {
    /// Wrap the error in an `error_stack::Report`.
    fn into_report(self) -> ReloadReport<T>;
}

impl<T> IntoReloadReport<T> for ::std::result::Result<T, ReloadError> {
    #[inline]
    fn into_report(self) -> ReloadReport<T> {
        self.map_err(error_stack::Report::new)
    }
}

/// Loaded plugin entry
struct LoadedPlugin {
    /// The dynamic plugin, wrapped in a per-plugin Mutex so that execute()
    /// can release the global `loaded_plugins` RwLock before awaiting.
    plugin: Arc<Mutex<DynamicPlugin>>,
    /// Plugin info
    info: PluginInfo,
    /// Reload attempt counter
    reload_attempts: u32,
    /// Last reload time
    last_reload: Option<std::time::Instant>,
}

/// Hot-reload manager
pub struct HotReloadManager {
    /// Configuration
    config: HotReloadConfig,
    /// Plugin loader
    loader: Arc<PluginLoader>,
    /// File watcher
    watcher: Arc<RwLock<PluginWatcher>>,
    /// Plugin registry
    registry: Arc<PluginRegistry>,
    /// State manager
    state_manager: Arc<StateManager>,
    /// Loaded plugins
    loaded_plugins: Arc<RwLock<HashMap<String, LoadedPlugin>>>,
    /// Event broadcaster
    event_tx: broadcast::Sender<ReloadEvent>,
    /// Shutdown signal
    shutdown_tx: Option<mpsc::Sender<()>>,
    /// Running flag
    running: Arc<RwLock<bool>>,
    /// Plugin context for initialization
    plugin_context: Option<PluginContext>,
}

impl HotReloadManager {
    /// Create a new hot-reload manager
    pub fn new(config: HotReloadConfig) -> Self {
        let (event_tx, _) = broadcast::channel(1024);

        Self {
            loader: Arc::new(PluginLoader::new()),
            watcher: Arc::new(RwLock::new(PluginWatcher::new(config.watch_config.clone()))),
            registry: Arc::new(PluginRegistry::new()),
            state_manager: Arc::new(StateManager::new()),
            loaded_plugins: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            shutdown_tx: None,
            running: Arc::new(RwLock::new(false)),
            plugin_context: None,
            config,
        }
    }

    /// Set plugin context for initialization
    pub fn with_context(mut self, context: PluginContext) -> Self {
        self.plugin_context = Some(context);
        self
    }

    /// Subscribe to reload events
    pub fn subscribe(&self) -> broadcast::Receiver<ReloadEvent> {
        self.event_tx.subscribe()
    }

    /// Get the plugin registry
    pub fn registry(&self) -> Arc<PluginRegistry> {
        self.registry.clone()
    }

    /// Get the state manager
    pub fn state_manager(&self) -> Arc<StateManager> {
        self.state_manager.clone()
    }

    /// Add a watch directory
    pub async fn add_watch_path<P: AsRef<Path>>(&self, path: P) -> Result<(), ReloadError> {
        let path = path.as_ref();
        info!("Adding watch path: {:?}", path);

        let mut watcher = self.watcher.write().await;
        watcher
            .watch(path)
            .await
            .map_err(|e| ReloadError::WatchError(e.to_string()))?;

        Ok(())
    }

    /// Start the hot-reload manager
    pub async fn start(&mut self) -> Result<(), ReloadError> {
        info!("Starting hot-reload manager");

        {
            let mut running = self.running.write().await;
            if *running {
                return Err(ReloadError::Internal("Already running".to_string()));
            }
            *running = true;
        }

        // Start the watcher
        {
            let mut watcher = self.watcher.write().await;
            watcher
                .start()
                .await
                .map_err(|e| ReloadError::WatchError(e.to_string()))?;
        }

        // Scan for existing plugins
        let existing = {
            let watcher = self.watcher.read().await;
            watcher.scan_existing().await
        };

        for path in existing {
            if let Err(e) = self.load_plugin(&path).await {
                warn!("Failed to load existing plugin {:?}: {}", path, e);
            }
        }

        // Start event processing
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);

        self.spawn_event_processor(shutdown_rx).await;

        info!("Hot-reload manager started");
        Ok(())
    }

    /// Stop the hot-reload manager
    pub async fn stop(&mut self) -> Result<(), ReloadError> {
        info!("Stopping hot-reload manager");

        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        // Stop watcher
        {
            let mut watcher = self.watcher.write().await;
            watcher.stop().await;
        }

        // Unload all plugins
        let plugin_ids: Vec<String> = {
            let plugins = self.loaded_plugins.read().await;
            plugins.keys().cloned().collect()
        };

        for plugin_id in plugin_ids {
            if let Err(e) = self.unload_plugin(&plugin_id).await {
                warn!("Failed to unload plugin {}: {}", plugin_id, e);
            }
        }

        {
            let mut running = self.running.write().await;
            *running = false;
        }

        info!("Hot-reload manager stopped");
        Ok(())
    }

    /// Spawn the event processor task
    async fn spawn_event_processor(&self, mut shutdown_rx: mpsc::Receiver<()>) {
        let watcher = self.watcher.clone();
        let event_tx = self.event_tx.clone();
        let config = self.config.clone();
        let loader = self.loader.clone();
        let registry = self.registry.clone();
        let state_manager = self.state_manager.clone();
        let loaded_plugins = self.loaded_plugins.clone();
        let running = self.running.clone();
        let plugin_context = self.plugin_context.clone();

        tokio::spawn(async move {
            // Get event receiver
            let mut event_rx = {
                let mut w = watcher.write().await;
                match w.take_event_receiver() {
                    Some(rx) => rx,
                    None => {
                        error!("Failed to get event receiver");
                        return;
                    }
                }
            };

            let mut pending_reloads: HashMap<PathBuf, std::time::Instant> = HashMap::new();

            loop {
                tokio::select! {
                    Some(watch_event) = event_rx.recv() => {
                        debug!("Received watch event: {:?}", watch_event);

                        match watch_event.kind {
                            WatchEventKind::Created => {
                                // New plugin file
                                let _ = event_tx.send(ReloadEvent::PluginDiscovered {
                                    path: watch_event.path.clone(),
                                });

                                // Load the new plugin
                                if let Err(e) = Self::handle_load(
                                    &watch_event.path,
                                    &loader,
                                    &registry,
                                    &loaded_plugins,
                                    &event_tx,
                                    plugin_context.as_ref(),
                                ).await {
                                    warn!("Failed to load new plugin: {}", e);
                                }
                            }

                            WatchEventKind::Modified => {
                                // Queue reload based on strategy
                                match config.base.strategy {
                                    ReloadStrategy::Immediate => {
                                        if let Err(e) = Self::handle_reload(
                                            &watch_event.path,
                                            &loader,
                                            &registry,
                                            &state_manager,
                                            &loaded_plugins,
                                            &event_tx,
                                            &config,
                                            plugin_context.as_ref(),
                                        ).await {
                                            warn!("Failed to reload plugin {:?}: {}", watch_event.path, e);
                                        }
                                    }
                                    ReloadStrategy::Debounced(duration) => {
                                        pending_reloads.insert(
                                            watch_event.path.clone(),
                                            std::time::Instant::now() + duration,
                                        );
                                    }
                                    ReloadStrategy::Manual => {
                                        debug!("Manual reload mode - ignoring change");
                                    }
                                    ReloadStrategy::OnIdle => {
                                        pending_reloads.insert(
                                            watch_event.path.clone(),
                                            std::time::Instant::now() + Duration::from_secs(5),
                                        );
                                    }
                                    _ => {
                                        debug!("Unhandled reload strategy");
                                    }
                                }
                            }

                            WatchEventKind::Removed => {
                                // Plugin removed
                                if let Some(info) = registry.get_by_path(&watch_event.path).await {
                                    let _ = event_tx.send(ReloadEvent::PluginRemoved {
                                        plugin_id: info.id.clone(),
                                        path: watch_event.path.clone(),
                                    });

                                    // Unload the plugin — release the write lock before
                                    // awaiting unload() to avoid holding it across .await.
                                    let removed = {
                                        let mut plugins = loaded_plugins.write().await;
                                        plugins.remove(&info.id)
                                    };
                                    if let Some(entry) = removed {
                                        let _ = entry.plugin.lock().await.plugin_mut().unload().await;
                                    }
                                    let _ = registry.unregister(&info.id).await;
                                }
                            }

                            WatchEventKind::Renamed { from, to } => {
                                // Handle rename as remove + create
                                if let Some(info) = registry.get_by_path(&from).await {
                                    let _ = registry.unregister(&info.id).await;
                                }

                                if let Err(e) = Self::handle_load(
                                    &to,
                                    &loader,
                                    &registry,
                                    &loaded_plugins,
                                    &event_tx,
                                    plugin_context.as_ref(),
                                ).await {
                                    warn!("Failed to load renamed plugin: {}", e);
                                }
                            }
                        }
                    }

                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        // Process pending debounced reloads
                        let now = std::time::Instant::now();
                        let ready: Vec<PathBuf> = pending_reloads
                            .iter()
                            .filter(|(_, time)| &now >= time)
                            .map(|(path, _)| path.clone())
                            .collect();

                        for path in ready {
                            pending_reloads.remove(&path);
                            if let Err(e) = Self::handle_reload(
                                &path,
                                &loader,
                                &registry,
                                &state_manager,
                                &loaded_plugins,
                                &event_tx,
                                &config,
                                plugin_context.as_ref(),
                            ).await {
                                warn!("Failed to reload plugin {:?}: {}", path, e);
                            }
                        }
                    }

                    _ = shutdown_rx.recv() => {
                        info!("Event processor shutting down");
                        return;
                    }
                }

                // Check if still running
                if !*running.read().await {
                    return;
                }
            }
        });
    }

    /// Handle loading a new plugin
    async fn handle_load(
        path: &Path,
        loader: &PluginLoader,
        registry: &PluginRegistry,
        loaded_plugins: &RwLock<HashMap<String, LoadedPlugin>>,
        event_tx: &broadcast::Sender<ReloadEvent>,
        context: Option<&PluginContext>,
    ) -> Result<(), ReloadError> {
        info!("Loading plugin from {:?}", path);

        // Load the library
        let library = loader.load_library(path).await?;

        // Create plugin instance
        let mut dynamic_plugin = loader.create_plugin(path).await?;

        // Initialize if context available
        if let Some(ctx) = context {
            dynamic_plugin
                .plugin_mut()
                .load(ctx)
                .await
                .map_err(|e| ReloadError::InitError(e.to_string()))?;
            dynamic_plugin
                .plugin_mut()
                .init_plugin()
                .await
                .map_err(|e| ReloadError::InitError(e.to_string()))?;
            dynamic_plugin
                .plugin_mut()
                .start()
                .await
                .map_err(|e| ReloadError::InitError(e.to_string()))?;
        }

        let metadata = library.metadata();
        let plugin_id = metadata.id.clone();

        // Create plugin info
        let mut info = PluginInfo::new(
            &plugin_id,
            &metadata.name,
            PluginVersion::parse(&metadata.version).unwrap_or_default(),
        )
        .with_library_path(path)
        .with_description(&metadata.description);

        for cap in &metadata.capabilities {
            info = info.with_capability(cap);
        }
        for dep in &metadata.dependencies {
            info = info.with_dependency(dep);
        }

        info.mark_loaded();

        // Register
        registry
            .register(info.clone())
            .await
            .map_err(ReloadError::Internal)?;

        // Store
        let entry = LoadedPlugin {
            plugin: Arc::new(Mutex::new(dynamic_plugin)),
            info,
            reload_attempts: 0,
            last_reload: None,
        };

        let mut plugins = loaded_plugins.write().await;
        plugins.insert(plugin_id.clone(), entry);

        let _ = event_tx.send(ReloadEvent::ReloadCompleted {
            plugin_id,
            path: path.to_path_buf(),
            success: true,
            duration: Duration::from_millis(0),
        });

        Ok(())
    }

    /// Handle reloading a plugin
    #[allow(clippy::too_many_arguments)]
    async fn handle_reload(
        path: &Path,
        loader: &PluginLoader,
        registry: &PluginRegistry,
        state_manager: &StateManager,
        loaded_plugins: &RwLock<HashMap<String, LoadedPlugin>>,
        event_tx: &broadcast::Sender<ReloadEvent>,
        config: &HotReloadConfig,
        context: Option<&PluginContext>,
    ) -> Result<(), ReloadError> {
        let start = std::time::Instant::now();

        // Find the plugin
        let plugin_id = match registry.get_by_path(path).await {
            Some(info) => info.id,
            None => {
                // New plugin, treat as load
                if let Err(e) =
                    Self::handle_load(path, loader, registry, loaded_plugins, event_tx, context)
                        .await
                {
                    warn!("Failed to load plugin: {}", e);
                    return Err(e);
                }
                return Ok(());
            }
        };

        info!("Reloading plugin: {}", plugin_id);

        let _ = event_tx.send(ReloadEvent::ReloadStarted {
            plugin_id: plugin_id.clone(),
            path: path.to_path_buf(),
        });

        // Preserve state if enabled
        let saved_state = if config.base.preserve_state {
            let plugins = loaded_plugins.read().await;
            if let Some(entry) = plugins.get(&plugin_id) {
                // Create a basic snapshot from stats
                let stats = entry.plugin.lock().await.plugin().stats();
                let mut snapshot = StateSnapshot::new(&plugin_id, &entry.info.version.to_string());
                for (key, value) in stats {
                    snapshot.data.insert(key, value);
                }
                let _ = state_manager.save_snapshot(snapshot.clone()).await;

                let _ = event_tx.send(ReloadEvent::StatePreserved {
                    plugin_id: plugin_id.clone(),
                });

                Some(snapshot)
            } else {
                None
            }
        } else {
            None
        };

        // Unload current plugin — remove from map first, then stop/unload
        // outside the write-lock so we don't block concurrent operations.
        {
            let removed = {
                let mut plugins = loaded_plugins.write().await;
                plugins.remove(&plugin_id)
            };
            if let Some(entry) = removed {
                let mut p = entry.plugin.lock().await;
                let _ = p.plugin_mut().stop().await;
                let _ = p.plugin_mut().unload().await;
            }
        }

        // Reload the library
        let library = match loader.reload_library(path).await {
            Ok(lib) => lib,
            Err(e) => {
                error!("Failed to reload library: {}", e);

                let _ = event_tx.send(ReloadEvent::ReloadFailed {
                    plugin_id: plugin_id.clone(),
                    path: path.to_path_buf(),
                    error: e.to_string(),
                    attempt: 1,
                });

                // Rollback if enabled
                if config.base.auto_rollback
                    && let Some(_snapshot) = saved_state
                {
                    let _ = event_tx.send(ReloadEvent::RollbackTriggered {
                        plugin_id: plugin_id.clone(),
                        reason: "Library reload failed".to_string(),
                    });
                }

                return Err(ReloadError::LoadError(e));
            }
        };

        // Create new plugin instance
        let mut dynamic_plugin = match loader.create_plugin(path).await {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to create plugin instance: {}", e);

                let _ = event_tx.send(ReloadEvent::ReloadFailed {
                    plugin_id: plugin_id.clone(),
                    path: path.to_path_buf(),
                    error: e.to_string(),
                    attempt: 1,
                });

                return Err(ReloadError::LoadError(e));
            }
        };

        // Initialize
        if let Some(ctx) = context {
            if let Err(e) = dynamic_plugin.plugin_mut().load(ctx).await {
                error!("Failed to load plugin: {}", e);
                return Err(ReloadError::InitError(e.to_string()));
            }
            if let Err(e) = dynamic_plugin.plugin_mut().init_plugin().await {
                error!("Failed to initialize plugin: {}", e);
                return Err(ReloadError::InitError(e.to_string()));
            }
            if let Err(e) = dynamic_plugin.plugin_mut().start().await {
                error!("Failed to start plugin: {}", e);
                return Err(ReloadError::InitError(e.to_string()));
            }
        }

        // Update registry
        let metadata = library.metadata();
        let mut info = PluginInfo::new(
            &plugin_id,
            &metadata.name,
            PluginVersion::parse(&metadata.version).unwrap_or_default(),
        )
        .with_library_path(path);

        info.mark_reloaded();

        let _ = registry.update(info.clone()).await;

        // Store new plugin
        let entry = LoadedPlugin {
            plugin: Arc::new(Mutex::new(dynamic_plugin)),
            info,
            reload_attempts: 0,
            last_reload: Some(std::time::Instant::now()),
        };

        let mut plugins = loaded_plugins.write().await;
        plugins.insert(plugin_id.clone(), entry);

        // Restore state if available
        if let Some(_snapshot) = saved_state {
            let _ = event_tx.send(ReloadEvent::StateRestored {
                plugin_id: plugin_id.clone(),
            });
        }

        let duration = start.elapsed();
        info!("Plugin {} reloaded in {:?}", plugin_id, duration);

        let _ = event_tx.send(ReloadEvent::ReloadCompleted {
            plugin_id,
            path: path.to_path_buf(),
            success: true,
            duration,
        });

        Ok(())
    }

    /// Load a plugin from path
    pub async fn load_plugin<P: AsRef<Path>>(&self, path: P) -> Result<String, ReloadError> {
        let path = path.as_ref();

        Self::handle_load(
            path,
            &self.loader,
            &self.registry,
            &self.loaded_plugins,
            &self.event_tx,
            self.plugin_context.as_ref(),
        )
        .await?;

        // Get the plugin ID
        let info =
            self.registry.get_by_path(path).await.ok_or_else(|| {
                ReloadError::Internal("Plugin not registered after load".to_string())
            })?;

        Ok(info.id)
    }

    /// Unload a plugin
    pub async fn unload_plugin(&self, plugin_id: &str) -> Result<(), ReloadError> {
        info!("Unloading plugin: {}", plugin_id);

        // Preserve state
        if self.config.base.preserve_state {
            let plugins = self.loaded_plugins.read().await;
            if let Some(entry) = plugins.get(plugin_id) {
                let stats = entry.plugin.lock().await.plugin().stats();
                let mut snapshot = StateSnapshot::new(plugin_id, &entry.info.version.to_string());
                for (key, value) in stats {
                    snapshot.data.insert(key, value);
                }
                let _ = self.state_manager.save_snapshot(snapshot).await;
            }
        }

        // Remove from the map first, then stop/unload outside the write-lock
        // so we don't hold it across async plugin lifecycle awaits.
        let removed = {
            let mut plugins = self.loaded_plugins.write().await;
            plugins.remove(plugin_id)
        };
        if let Some(entry) = removed {
            let mut p = entry.plugin.lock().await;
            p.plugin_mut()
                .stop()
                .await
                .map_err(|e| ReloadError::Internal(e.to_string()))?;
            p.plugin_mut()
                .unload()
                .await
                .map_err(|e| ReloadError::Internal(e.to_string()))?;
        } else {
            return Err(ReloadError::PluginNotFound(plugin_id.to_string()));
        }

        // Unregister
        self.registry
            .unregister(plugin_id)
            .await
            .map_err(ReloadError::Internal)?;

        Ok(())
    }

    /// Manually trigger a reload
    pub async fn reload_plugin(&self, plugin_id: &str) -> Result<ReloadResult, ReloadError> {
        let info = self
            .registry
            .get(plugin_id)
            .await
            .ok_or_else(|| ReloadError::PluginNotFound(plugin_id.to_string()))?;

        let path = info
            .library_path
            .ok_or_else(|| ReloadError::Internal("No library path".to_string()))?;

        let start = std::time::Instant::now();

        Self::handle_reload(
            &path,
            &self.loader,
            &self.registry,
            &self.state_manager,
            &self.loaded_plugins,
            &self.event_tx,
            &self.config,
            self.plugin_context.as_ref(),
        )
        .await?;

        Ok(ReloadResult {
            plugin_id: plugin_id.to_string(),
            success: true,
            error: None,
            duration: start.elapsed(),
            state_preserved: self.config.base.preserve_state,
            attempts: 1,
        })
    }

    /// Get a plugin reference
    pub async fn get_plugin_info(&self, plugin_id: &str) -> Option<PluginInfo> {
        self.registry.get(plugin_id).await
    }

    /// Execute a plugin.
    ///
    /// The global `loaded_plugins` read-lock is released before awaiting the
    /// plugin's `execute()` call. This allows concurrent executions of
    /// different plugins to proceed in parallel and prevents hot-reload
    /// operations from being blocked by long-running plugin calls.
    pub async fn execute(&self, plugin_id: &str, input: String) -> PluginResult<String> {
        // Acquire read lock only long enough to clone the per-plugin Arc handle.
        let plugin_mutex = {
            let plugins = self.loaded_plugins.read().await;
            plugins
                .get(plugin_id)
                .map(|e| Arc::clone(&e.plugin))
                .ok_or_else(|| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                        "Plugin {} not found",
                        plugin_id
                    ))
                })?
        }; // read lock released here — before any .await

        // Per-plugin mutex serialises concurrent calls to the same instance
        // without blocking access to any other plugin in the map.
        plugin_mutex.lock().await.plugin_mut().execute(input).await
    }

    /// List all loaded plugins
    pub async fn list_plugins(&self) -> Vec<String> {
        let plugins = self.loaded_plugins.read().await;
        plugins.keys().cloned().collect()
    }

    /// Check if manager is running
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }
}

impl Default for HotReloadManager {
    fn default() -> Self {
        Self::new(HotReloadConfig::default())
    }
}
