

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use libloading::{Library, Symbol};

#[derive(Clone, Debug)]
pub struct PluginInfo {
    pub id: String,
    pub version: String,
}


pub struct PluginHandle {
    _lib: Library,
    pub info: PluginInfo,
    
    pub create_raw: unsafe extern "C" fn() -> *mut (),
}

impl std::fmt::Debug for PluginHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginHandle")
            .field("info", &self.info)
            .finish()
    }
}


#[derive(Debug, thiserror::Error)]
pub enum PluginLoadError {
    #[error("libloading error: {0}")]
    Lib(#[from] libloading::Error),

    #[error("manifest read error for {path}: {source}")]
    Manifest {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("manifest parse error for {path}: {source}")]
    ManifestParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("missing symbol `{symbol}` in {path}")]
    MissingSymbol { symbol: String, path: PathBuf },
}

#[derive(serde::Deserialize, Debug)]
struct PluginManifest {
    plugin: PluginSection,
}

#[derive(serde::Deserialize, Debug)]
struct PluginSection {
    id: String,
    version: String,
    entry: String, 
}

impl PluginHandle {

    pub fn load(plugin_dir: &Path) -> Result<Self, PluginLoadError> {
        let manifest_path = plugin_dir.join("plugin.toml");
        let raw = std::fs::read_to_string(&manifest_path).map_err(|e| {
            PluginLoadError::Manifest {
                path: manifest_path.clone(),
                source: e,
            }
        })?;
        let manifest: PluginManifest =
            toml::from_str(&raw).map_err(|e| PluginLoadError::ManifestParse {
                path: manifest_path,
                source: e,
            })?;

        let lib_path = plugin_dir.join(&manifest.plugin.entry);

        
        let lib = unsafe { Library::new(&lib_path)? };

        let create_raw: Symbol<unsafe extern "C" fn() -> *mut ()> = unsafe {
            lib.get(b"mofa_app_create\0").map_err(|e| {
                let _ = e;
                PluginLoadError::MissingSymbol {
                    symbol: "mofa_app_create".into(),
                    path: lib_path.clone(),
                }
            })?
        };

        
        let create_raw: unsafe extern "C" fn() -> *mut () =
            unsafe { std::mem::transmute(create_raw.into_raw()) };

        Ok(PluginHandle {
            _lib: lib,
            info: PluginInfo {
                id: manifest.plugin.id,
                version: manifest.plugin.version,
            },
            create_raw,
        })
    }
}

#[derive(Debug, Default)]
pub struct PluginRegistry {
    inner: Arc<Mutex<HashMap<String, PluginHandle>>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, handle: PluginHandle) {
        let id = handle.info.id.clone();
        let mut guard = self.inner.lock().expect("registry lock poisoned");
        if let Some(old) = guard.insert(id.clone(), handle) {
            tracing::info!(
                plugin_id = %id,
                old_version = %old.info.version,
                "unregistered old plugin version"
            );
        }
    }

    pub fn unregister(&self, id: &str) -> Option<PluginHandle> {
        self.inner
            .lock()
            .expect("registry lock poisoned")
            .remove(id)
    }

    pub fn get_ids(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("registry lock poisoned")
            .keys()
            .cloned()
            .collect()
    }
}
