mod plugin_loader;
mod watcher;

pub use plugin_loader::{PluginHandle, PluginInfo, PluginLoadError, PluginRegistry};
pub use watcher::{watch_plugin_dir, HotReloadEvent, WatcherGuard};

use std::{
    path::{Path, PathBuf},
    sync::mpsc::TryRecvError,
    thread,
    time::Duration,
};

pub fn hot_reload_enabled() -> bool {
    std::env::var("MOFA_HOT_RELOAD")
        .map(|v| v.trim() == "1")
        .unwrap_or(false)
}

pub fn load_all_plugins(
    plugin_dir: &Path,
    registry: &PluginRegistry,
) -> usize {
    let mut loaded = 0usize;

    let entries = match std::fs::read_dir(plugin_dir) {
        Ok(e) => e,
        Err(err) => {
            tracing::error!(
                path = %plugin_dir.display(),
                error = %err,
                "failed to read plugin directory"
            );
            return 0;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join("plugin.toml").exists() {
            match PluginHandle::load(&path) {
                Ok(handle) => {
                    tracing::info!(
                        id = %handle.info.id,
                        version = %handle.info.version,
                        path = %path.display(),
                        "plugin loaded"
                    );
                    registry.register(handle);
                    loaded += 1;
                }
                Err(err) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %err,
                        "failed to load plugin"
                    );
                }
            }
        }
    }

    loaded
}

pub fn spawn_hot_reload_thread<F>(
    plugin_dir: PathBuf,
    registry: PluginRegistry,
    on_reload: F,
) -> Result<WatcherGuard, notify::Error>
where
    F: Fn(&PluginInfo) + Send + 'static,
{
    let (guard, rx) = watch_plugin_dir(&plugin_dir)?;

    thread::spawn(move || {
        loop {
            match rx.try_recv() {
                Ok(HotReloadEvent::Updated(lib_path)) => {
                    let dir = lib_path
                        .parent()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| plugin_dir.clone());

                    thread::sleep(Duration::from_millis(200));

                    match PluginHandle::load(&dir) {
                        Ok(handle) => {
                            tracing::info!(
                                id = %handle.info.id,
                                version = %handle.info.version,
                                "hot-reloaded plugin"
                            );
                            on_reload(&handle.info);
                            registry.register(handle);
                        }
                        Err(err) => {
                            tracing::warn!(
                                path = %dir.display(),
                                error = %err,
                                "hot-reload failed"
                            );
                        }
                    }
                }
                Ok(HotReloadEvent::Removed(lib_path)) => {
                    let stem = lib_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");
                    tracing::info!(stem, "plugin library removed; unregistering");
                    for id in registry.get_ids() {
                        if stem.contains(&id) {
                            registry.unregister(&id);
                        }
                    }
                }
                Err(TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(TryRecvError::Disconnected) => {
                    tracing::info!("hot-reload watcher channel closed; exiting thread");
                    break;
                }
            }
        }
    });

    Ok(guard)
}
