//! File system watcher for plugin changes
//!
//! Monitors plugin directories for file changes and triggers reload events

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{CreateKind, ModifyKind, RemoveKind, RenameMode},
};
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, error, info, warn};

/// Watch event kinds
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEventKind {
    /// Plugin file created
    Created,
    /// Plugin file modified
    Modified,
    /// Plugin file removed
    Removed,
    /// Plugin file renamed
    Renamed { from: PathBuf, to: PathBuf },
}

/// Watch event
#[derive(Debug, Clone)]
pub struct WatchEvent {
    /// Event kind
    pub kind: WatchEventKind,
    /// Affected path
    pub path: PathBuf,
    /// Timestamp
    pub timestamp: std::time::Instant,
}

impl WatchEvent {
    /// Create a new watch event
    pub fn new(kind: WatchEventKind, path: PathBuf) -> Self {
        Self {
            kind,
            path,
            timestamp: std::time::Instant::now(),
        }
    }

    /// Check if this is a plugin file
    pub fn is_plugin_file(&self) -> bool {
        let ext = self.path.extension().and_then(|e| e.to_str());
        matches!(ext, Some("so") | Some("dylib") | Some("dll"))
    }
}

/// Handle a `RenameMode::From` event, checking for an orphaned previous path.
///
/// If `rename_from` already holds a path (meaning a prior From was never
/// paired with a To), that path is treated as a removal and returned as a
/// `WatchEvent`. The new path then replaces it.
fn handle_rename_from(rename_from: &mut Option<PathBuf>, new_path: PathBuf) -> Option<WatchEvent> {
    let orphan_event = rename_from.take().map(|orphaned| {
        warn!(
            "Orphaned rename-from path, treating as removal: {:?}",
            orphaned
        );
        WatchEvent::new(WatchEventKind::Removed, orphaned)
    });
    *rename_from = Some(new_path);
    orphan_event
}

/// Watch configuration
#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// Debounce duration for rapid file changes
    pub debounce_duration: Duration,
    /// File extensions to watch
    pub extensions: Vec<String>,
    /// Whether to watch subdirectories
    pub recursive: bool,
    /// Ignore patterns (glob patterns)
    pub ignore_patterns: Vec<String>,
    /// Maximum events per second (rate limiting)
    pub max_events_per_sec: u32,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            debounce_duration: Duration::from_millis(500),
            extensions: vec!["so".to_string(), "dylib".to_string(), "dll".to_string()],
            recursive: false,
            ignore_patterns: vec!["*.tmp".to_string(), "*.swp".to_string(), "*~".to_string()],
            max_events_per_sec: 100,
        }
    }
}

impl WatchConfig {
    /// Create a new watch config
    pub fn new() -> Self {
        Self::default()
    }

    /// Set debounce duration
    pub fn with_debounce(mut self, duration: Duration) -> Self {
        self.debounce_duration = duration;
        self
    }

    /// Add file extension to watch
    pub fn with_extension(mut self, ext: &str) -> Self {
        self.extensions.push(ext.to_string());
        self
    }

    /// Set recursive mode
    pub fn with_recursive(mut self, recursive: bool) -> Self {
        self.recursive = recursive;
        self
    }

    /// Add ignore pattern
    pub fn with_ignore(mut self, pattern: &str) -> Self {
        self.ignore_patterns.push(pattern.to_string());
        self
    }

    /// Check if a path should be watched
    pub fn should_watch(&self, path: &Path) -> bool {
        // Check extension
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !self.extensions.is_empty() && !self.extensions.iter().any(|e| e == ext) {
            return false;
        }

        // Check ignore patterns (simple implementation)
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        for pattern in &self.ignore_patterns {
            if pattern.starts_with('*') && file_name.ends_with(&pattern[1..]) {
                return false;
            }
            if pattern.ends_with('*') && file_name.starts_with(&pattern[..pattern.len() - 1]) {
                return false;
            }
            if file_name == pattern {
                return false;
            }
        }

        true
    }
}

/// Plugin file watcher
pub struct PluginWatcher {
    /// Watched directories
    watch_paths: Arc<RwLock<Vec<PathBuf>>>,
    /// Configuration
    config: WatchConfig,
    /// Event sender
    event_tx: mpsc::Sender<WatchEvent>,
    /// Event receiver (taken by consumer)
    event_rx: Option<mpsc::Receiver<WatchEvent>>,
    /// Internal watcher handle
    watcher: Option<RecommendedWatcher>,
    /// Last event times for debouncing
    last_events: Arc<RwLock<HashMap<PathBuf, std::time::Instant>>>,
    /// Shutdown signal
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl PluginWatcher {
    /// Create a new plugin watcher
    pub fn new(config: WatchConfig) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);

        Self {
            watch_paths: Arc::new(RwLock::new(Vec::new())),
            config,
            event_tx,
            event_rx: Some(event_rx),
            watcher: None,
            last_events: Arc::new(RwLock::new(HashMap::new())),
            shutdown_tx: None,
        }
    }

    /// Take the event receiver (can only be called once)
    pub fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<WatchEvent>> {
        self.event_rx.take()
    }

    /// Add a directory to watch
    pub async fn watch<P: AsRef<Path>>(&mut self, path: P) -> Result<(), notify::Error> {
        let path = path.as_ref().to_path_buf();

        if !path.exists() {
            warn!("Watch path does not exist: {:?}", path);
            return Ok(());
        }

        info!("Adding watch path: {:?}", path);

        // Add to tracked paths
        {
            let mut paths = self.watch_paths.write().await;
            if !paths.contains(&path) {
                paths.push(path.clone());
            }
        }

        // Add to watcher if running
        if let Some(ref mut watcher) = self.watcher {
            let mode = if self.config.recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            watcher.watch(&path, mode)?;
        }

        Ok(())
    }

    /// Remove a directory from watching
    pub async fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Result<(), notify::Error> {
        let path = path.as_ref().to_path_buf();

        info!("Removing watch path: {:?}", path);

        // Remove from tracked paths
        {
            let mut paths = self.watch_paths.write().await;
            paths.retain(|p| p != &path);
        }

        // Remove from watcher if running
        if let Some(ref mut watcher) = self.watcher {
            watcher.unwatch(&path)?;
        }

        Ok(())
    }

    /// Start watching for changes
    pub async fn start(&mut self) -> Result<(), notify::Error> {
        info!("Starting plugin watcher");

        let event_tx = self.event_tx.clone();
        let config = self.config.clone();
        let last_events = self.last_events.clone();
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

        self.shutdown_tx = Some(shutdown_tx);

        // Create the file system watcher
        let (tx, mut rx) = mpsc::channel(1024);

        let watcher_config = Config::default().with_poll_interval(Duration::from_millis(100));

        let mut watcher = RecommendedWatcher::new(
            move |result: Result<Event, notify::Error>| {
                if let Ok(event) = result {
                    let _ = tx.blocking_send(event);
                }
            },
            watcher_config,
        )?;

        // Add all tracked paths
        let mode = if self.config.recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };

        let paths = self.watch_paths.read().await;
        for path in paths.iter() {
            watcher.watch(path, mode)?;
        }

        self.watcher = Some(watcher);

        // Spawn event processing task
        tokio::spawn(async move {
            let mut rename_from: Option<PathBuf> = None;

            loop {
                tokio::select! {
                    Some(event) = rx.recv() => {
                        // Process file system event
                        for path in event.paths {
                            // Skip if should not watch
                            if !config.should_watch(&path) {
                                continue;
                            }

                            // Debounce check
                            let should_process = {
                                let mut last = last_events.write().await;
                                let now = std::time::Instant::now();

                                if let Some(last_time) = last.get(&path) {
                                    if now.duration_since(*last_time) < config.debounce_duration {
                                        false
                                    } else {
                                        last.insert(path.clone(), now);
                                        true
                                    }
                                } else {
                                    last.insert(path.clone(), now);
                                    true
                                }
                            };

                            if !should_process {
                                debug!("Debounced event for {:?}", path);
                                continue;
                            }

                            // Convert to watch event
                            let watch_event = match event.kind {
                                EventKind::Create(CreateKind::File) => {
                                    Some(WatchEvent::new(WatchEventKind::Created, path.clone()))
                                }
                                EventKind::Modify(ModifyKind::Data(_)) |
                                EventKind::Modify(ModifyKind::Any) => {
                                    Some(WatchEvent::new(WatchEventKind::Modified, path.clone()))
                                }
                                EventKind::Remove(RemoveKind::File) => {
                                    Some(WatchEvent::new(WatchEventKind::Removed, path.clone()))
                                }
                                EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                                    if let Some(evt) = handle_rename_from(&mut rename_from, path.clone()) {
                                        if event_tx.send(evt).await.is_err() {
                                            error!("Failed to send watch event");
                                            return;
                                        }
                                    }
                                    None
                                }
                                EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                                    if let Some(from) = rename_from.take() {
                                        Some(WatchEvent::new(
                                            WatchEventKind::Renamed {
                                                from: from.clone(),
                                                to: path.clone(),
                                            },
                                            path.clone(),
                                        ))
                                    } else {
                                        Some(WatchEvent::new(WatchEventKind::Created, path.clone()))
                                    }
                                }
                                _ => None,
                            };

                            if let Some(evt) = watch_event {
                                debug!("Watch event: {:?}", evt);
                                if event_tx.send(evt).await.is_err() {
                                    error!("Failed to send watch event");
                                    return;
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Plugin watcher shutting down");
                        return;
                    }
                }
            }
        });

        Ok(())
    }

    /// Stop watching
    pub async fn stop(&mut self) {
        info!("Stopping plugin watcher");

        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        // Drop the watcher
        self.watcher = None;
    }

    /// Get watched paths
    pub async fn watched_paths(&self) -> Vec<PathBuf> {
        self.watch_paths.read().await.clone()
    }

    /// Check if a path is being watched
    pub async fn is_watching<P: AsRef<Path>>(&self, path: P) -> bool {
        let paths = self.watch_paths.read().await;
        paths.contains(&path.as_ref().to_path_buf())
    }

    /// Get configuration
    pub fn config(&self) -> &WatchConfig {
        &self.config
    }

    /// Scan for existing plugin files
    pub async fn scan_existing(&self) -> Vec<PathBuf> {
        let mut plugins = Vec::new();
        let paths = self.watch_paths.read().await;

        for watch_path in paths.iter() {
            if let Ok(entries) = std::fs::read_dir(watch_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && self.config.should_watch(&path) {
                        plugins.push(path);
                    }
                }
            }
        }

        plugins
    }
}

impl Drop for PluginWatcher {
    fn drop(&mut self) {
        // Watcher will be dropped automatically
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watch_config_default() {
        let config = WatchConfig::default();
        assert_eq!(config.extensions.len(), 3);
        assert!(!config.recursive);
    }

    #[test]
    fn test_should_watch() {
        let config = WatchConfig::default();

        // Should watch plugin files
        assert!(config.should_watch(Path::new("/path/to/plugin.so")));
        assert!(config.should_watch(Path::new("/path/to/plugin.dylib")));
        assert!(config.should_watch(Path::new("/path/to/plugin.dll")));

        // Should not watch non-plugin files
        assert!(!config.should_watch(Path::new("/path/to/file.txt")));
        assert!(!config.should_watch(Path::new("/path/to/file.rs")));

        // Should not watch ignored patterns
        assert!(!config.should_watch(Path::new("/path/to/plugin.so.tmp")));
        assert!(!config.should_watch(Path::new("/path/to/plugin.swp")));
    }

    #[test]
    fn test_watch_event() {
        let event = WatchEvent::new(
            WatchEventKind::Modified,
            PathBuf::from("/path/to/plugin.so"),
        );

        assert!(event.is_plugin_file());
        assert!(matches!(event.kind, WatchEventKind::Modified));
    }

    #[tokio::test]
    async fn test_plugin_watcher_new() {
        let config = WatchConfig::default();
        let mut watcher = PluginWatcher::new(config);

        assert!(watcher.take_event_receiver().is_some());
        assert!(watcher.take_event_receiver().is_none()); // Can only take once
    }

    #[test]
    fn test_handle_rename_from_no_previous() {
        let mut state: Option<PathBuf> = None;
        let result = handle_rename_from(&mut state, PathBuf::from("/tmp/a.so"));
        assert!(result.is_none(), "no orphan event when rename_from is empty");
        assert_eq!(state, Some(PathBuf::from("/tmp/a.so")));
    }

    #[test]
    fn test_handle_rename_from_orphaned_previous() {
        let mut state = Some(PathBuf::from("/tmp/old.so"));
        let result = handle_rename_from(&mut state, PathBuf::from("/tmp/new.so"));

        // The orphaned path must produce a Removed event
        let evt = result.expect("orphaned rename_from should emit Removed event");
        assert!(matches!(evt.kind, WatchEventKind::Removed));
        assert_eq!(evt.path, PathBuf::from("/tmp/old.so"));

        // State must now track the new path
        assert_eq!(state, Some(PathBuf::from("/tmp/new.so")));
    }

    #[test]
    fn test_handle_rename_from_chained_orphans() {
        let mut state: Option<PathBuf> = None;

        // First From — no orphan
        let r1 = handle_rename_from(&mut state, PathBuf::from("/a"));
        assert!(r1.is_none());

        // Second From without To — first path is orphaned
        let r2 = handle_rename_from(&mut state, PathBuf::from("/b"));
        let evt = r2.expect("should emit Removed for /a");
        assert_eq!(evt.path, PathBuf::from("/a"));

        // Third From without To — second path is orphaned
        let r3 = handle_rename_from(&mut state, PathBuf::from("/c"));
        let evt = r3.expect("should emit Removed for /b");
        assert_eq!(evt.path, PathBuf::from("/b"));

        assert_eq!(state, Some(PathBuf::from("/c")));
    }
}
