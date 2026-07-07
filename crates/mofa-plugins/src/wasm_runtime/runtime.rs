//! WASM Runtime Core
//!
//! Core runtime management for WASM plugin execution

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, info};
use wasmtime::*;

use super::plugin::{WasmPlugin, WasmPluginConfig};
use super::types::{ExecutionConfig, MemoryConfig, ResourceLimits, WasmError, WasmResult};

/// WASM runtime configuration
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Resource limits
    pub resource_limits: ResourceLimits,
    /// Execution configuration
    pub execution_config: ExecutionConfig,
    /// Memory configuration
    pub memory_config: MemoryConfig,
    /// Enable module caching
    pub enable_cache: bool,
    /// Cache directory
    pub cache_dir: Option<String>,
    /// Maximum cached modules
    pub max_cached_modules: usize,
    /// Enable parallel compilation
    pub parallel_compilation: bool,
    /// Cranelift optimization level
    pub optimization_level: OptimizationLevel,
}

/// Optimization level for compilation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizationLevel {
    None,
    Speed,
    SpeedAndSize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            resource_limits: ResourceLimits::default(),
            execution_config: ExecutionConfig::default(),
            memory_config: MemoryConfig::default(),
            enable_cache: true,
            cache_dir: None,
            max_cached_modules: 100,
            parallel_compilation: true,
            optimization_level: OptimizationLevel::Speed,
        }
    }
}

impl RuntimeConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_resource_limits(mut self, limits: ResourceLimits) -> Self {
        self.resource_limits = limits;
        self
    }

    pub fn with_cache_dir(mut self, dir: &str) -> Self {
        self.cache_dir = Some(dir.to_string());
        self
    }

    pub fn with_optimization(mut self, level: OptimizationLevel) -> Self {
        self.optimization_level = level;
        self
    }

    /// Convert to wasmtime Config
    fn to_wasmtime_config(&self) -> Config {
        let mut config = Config::new();

        // Async support
        config.async_support(self.execution_config.async_support);

        // Fuel metering
        config.consume_fuel(self.execution_config.fuel_metering);

        // Epoch interruption
        config.epoch_interruption(self.execution_config.epoch_interruption);

        // Debug info
        config.debug_info(self.execution_config.debug_info);

        // WASM features
        config.wasm_reference_types(self.execution_config.reference_types);
        config.wasm_simd(self.execution_config.simd);
        config.wasm_bulk_memory(self.execution_config.bulk_memory);
        config.wasm_multi_value(self.execution_config.multi_value);
        config.wasm_threads(self.execution_config.threads);

        // Optimization level
        match self.optimization_level {
            OptimizationLevel::None => {
                config.cranelift_opt_level(wasmtime::OptLevel::None);
            }
            OptimizationLevel::Speed => {
                config.cranelift_opt_level(wasmtime::OptLevel::Speed);
            }
            OptimizationLevel::SpeedAndSize => {
                config.cranelift_opt_level(wasmtime::OptLevel::SpeedAndSize);
            }
        }

        // Parallel compilation
        config.parallel_compilation(self.parallel_compilation);

        config
    }
}

/// Compiled module with metadata
pub struct CompiledModule {
    /// Module name/ID
    pub name: String,
    /// Compiled wasmtime module
    pub module: Module,
    /// Compilation time
    pub compile_time_ms: u64,
    /// Module size in bytes
    pub size_bytes: usize,
    /// Hash of source bytes
    pub source_hash: String,
    /// Compilation timestamp
    pub compiled_at: u64,
}

impl CompiledModule {
    pub fn new(name: &str, module: Module, source_bytes: &[u8], compile_time_ms: u64) -> Self {
        // SECURITY: Use SHA-256 for collision-resistant cache keys.
        let source_hash = sha256_hash(source_bytes);

        Self {
            name: name.to_string(),
            module,
            compile_time_ms,
            size_bytes: source_bytes.len(),
            source_hash,
            compiled_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

/// Module cache for compiled WASM modules
pub struct ModuleCache {
    /// Cached modules by name (insertion-ordered for FIFO eviction)
    modules: RwLock<IndexMap<String, Arc<CompiledModule>>>,
    /// Cache by source hash
    by_hash: RwLock<HashMap<String, String>>,
    /// Maximum entries
    max_entries: usize,
    /// Cache hits
    hits: RwLock<u64>,
    /// Cache misses
    misses: RwLock<u64>,
}

impl ModuleCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            modules: RwLock::new(IndexMap::new()),
            by_hash: RwLock::new(HashMap::new()),
            max_entries,
            hits: RwLock::new(0),
            misses: RwLock::new(0),
        }
    }

    /// Get module by name
    pub async fn get(&self, name: &str) -> Option<Arc<CompiledModule>> {
        let modules = self.modules.read().await;
        if let Some(module) = modules.get(name).cloned() {
            *self.hits.write().await += 1;
            Some(module)
        } else {
            *self.misses.write().await += 1;
            None
        }
    }

    /// Get module by source hash
    pub async fn get_by_hash(&self, hash: &str) -> Option<Arc<CompiledModule>> {
        let by_hash = self.by_hash.read().await;
        if let Some(name) = by_hash.get(hash) {
            self.get(name).await
        } else {
            *self.misses.write().await += 1;
            None
        }
    }

    /// Insert a compiled module
    pub async fn insert(&self, module: CompiledModule) {
        let name = module.name.clone();
        let hash = module.source_hash.clone();
        let arc = Arc::new(module);

        let mut modules = self.modules.write().await;

        // Evict if at capacity (FIFO: remove oldest inserted entry)
        if modules.len() >= self.max_entries {
            if let Some((evicted_name, evicted_module)) = modules.shift_remove_index(0) {
                modules.insert(name.clone(), arc);
                drop(modules);
                let mut by_hash = self.by_hash.write().await;
                by_hash.remove(&evicted_module.source_hash);
                by_hash.insert(hash, name);
                tracing::debug!(
                    evicted = %evicted_name,
                    "ModuleCache: evicted oldest module to make room"
                );
                return;
            }
        }

        modules.insert(name.clone(), arc);
        drop(modules);

        self.by_hash.write().await.insert(hash, name);
    }

    /// Remove a module from cache
    pub async fn remove(&self, name: &str) {
        let mut modules = self.modules.write().await;
        if let Some(module) = modules.remove(name) {
            self.by_hash.write().await.remove(&module.source_hash);
        }
    }

    /// Clear the cache
    pub async fn clear(&self) {
        self.modules.write().await.clear();
        self.by_hash.write().await.clear();
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        let modules = self.modules.read().await;
        CacheStats {
            entries: modules.len(),
            total_size_bytes: modules.values().map(|m| m.size_bytes).sum(),
            hits: *self.hits.read().await,
            misses: *self.misses.read().await,
        }
    }
}

impl Default for ModuleCache {
    fn default() -> Self {
        Self::new(100)
    }
}

/// Cache statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub entries: usize,
    pub total_size_bytes: usize,
    pub hits: u64,
    pub misses: u64,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// Runtime statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeStats {
    /// Total modules compiled
    pub modules_compiled: u64,
    /// Total compilation time in milliseconds
    pub total_compile_time_ms: u64,
    /// Total plugins created
    pub plugins_created: u64,
    /// Currently active plugins
    pub active_plugins: u64,
    /// Total executions
    pub total_executions: u64,
    /// Failed executions
    pub failed_executions: u64,
    /// Cache statistics
    pub cache_stats: Option<CacheStats>,
}

/// WASM Runtime
pub struct WasmRuntime {
    /// Configuration
    config: RuntimeConfig,
    /// Wasmtime engine
    engine: Engine,
    /// Module cache
    cache: ModuleCache,
    /// Runtime statistics
    stats: RwLock<RuntimeStats>,
    /// Created time
    created_at: Instant,
}

impl WasmRuntime {
    /// Create a new WASM runtime
    pub fn new(config: RuntimeConfig) -> WasmResult<Self> {
        let wasmtime_config = config.to_wasmtime_config();
        let engine = Engine::new(&wasmtime_config)
            .map_err(|e| WasmError::Internal(format!("Failed to create engine: {}", e)))?;

        let cache = ModuleCache::new(config.max_cached_modules);

        info!(
            "WASM runtime created with config: {:?}",
            config.optimization_level
        );

        Ok(Self {
            config,
            engine,
            cache,
            stats: RwLock::new(RuntimeStats::default()),
            created_at: Instant::now(),
        })
    }

    /// Create with default configuration
    pub fn default_runtime() -> WasmResult<Self> {
        Self::new(RuntimeConfig::default())
    }

    /// Get the wasmtime engine
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Get runtime configuration
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Compile a WASM module from bytes
    pub async fn compile(&self, name: &str, bytes: &[u8]) -> WasmResult<Arc<CompiledModule>> {
        // Check cache first using a cryptographic hash to prevent cache poisoning.
        let hash = sha256_hash(bytes);
        if let Some(cached) = self.cache.get_by_hash(&hash).await {
            debug!("Using cached module for {}", name);
            return Ok(cached);
        }

        // Compile
        let start = Instant::now();
        let module = Module::new(&self.engine, bytes)
            .map_err(|e| WasmError::CompilationError(e.to_string()))?;
        let compile_time = start.elapsed().as_millis() as u64;

        let compiled = CompiledModule::new(name, module, bytes, compile_time);

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.modules_compiled += 1;
            stats.total_compile_time_ms += compile_time;
        }

        // Cache it
        self.cache.insert(compiled).await;

        info!(
            "Compiled module {} in {}ms ({} bytes)",
            name,
            compile_time,
            bytes.len()
        );

        self.cache
            .get(name)
            .await
            .ok_or_else(|| WasmError::Internal("Failed to retrieve compiled module".to_string()))
    }

    /// Compile from WAT format
    pub async fn compile_wat(&self, name: &str, wat: &str) -> WasmResult<Arc<CompiledModule>> {
        let bytes = wat.as_bytes().to_vec();
        self.compile(name, &bytes).await
    }

    /// Compile from file
    pub async fn compile_file(&self, name: &str, path: &Path) -> WasmResult<Arc<CompiledModule>> {
        let bytes = tokio::fs::read(path).await?;
        self.compile(name, &bytes).await
    }

    /// Create a plugin from compiled module
    pub async fn create_plugin(
        &self,
        compiled: &CompiledModule,
        config: WasmPluginConfig,
    ) -> WasmResult<WasmPlugin> {
        // Create module clone since WasmPlugin needs ownership
        let module_bytes = compiled
            .module
            .serialize()
            .map_err(|e| WasmError::Internal(e.to_string()))?;

        let module = unsafe {
            Module::deserialize(&self.engine, &module_bytes)
                .map_err(|e| WasmError::LoadError(e.to_string()))?
        };

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.plugins_created += 1;
            stats.active_plugins += 1;
        }

        // Create plugin directly using internal constructor with async support flag
        let plugin = create_plugin_from_module(
            &self.engine,
            module,
            config,
            self.config.execution_config.async_support,
        )?;

        info!(
            "Created plugin {} from module {}",
            plugin.id(),
            compiled.name
        );
        Ok(plugin)
    }

    /// Create a plugin directly from bytes
    pub async fn create_plugin_from_bytes(
        &self,
        bytes: &[u8],
        config: WasmPluginConfig,
    ) -> WasmResult<WasmPlugin> {
        let compiled = self.compile(&config.id, bytes).await?;
        self.create_plugin(&compiled, config).await
    }

    /// Create a plugin directly from WAT
    pub async fn create_plugin_from_wat(
        &self,
        wat: &str,
        config: WasmPluginConfig,
    ) -> WasmResult<WasmPlugin> {
        let bytes = wat.as_bytes().to_vec();
        self.create_plugin_from_bytes(&bytes, config).await
    }

    /// Get runtime statistics
    pub async fn stats(&self) -> RuntimeStats {
        let mut stats = self.stats.read().await.clone();
        stats.cache_stats = Some(self.cache.stats().await);
        stats
    }

    /// Get cache statistics
    pub async fn cache_stats(&self) -> CacheStats {
        self.cache.stats().await
    }

    /// Clear the module cache
    pub async fn clear_cache(&self) {
        self.cache.clear().await;
        info!("Module cache cleared");
    }

    /// Get uptime in seconds
    pub fn uptime_secs(&self) -> u64 {
        self.created_at.elapsed().as_secs()
    }

    /// Increment epoch for epoch-based interruption
    pub fn increment_epoch(&self) {
        self.engine.increment_epoch();
    }

    /// Start epoch ticker for timeout support
    pub fn start_epoch_ticker(&self) -> tokio::task::JoinHandle<()> {
        let engine = self.engine.clone();
        let tick_ms = self.config.execution_config.epoch_tick_ms;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(tick_ms));
            loop {
                interval.tick().await;
                engine.increment_epoch();
            }
        })
    }
}

/// Helper function to create plugin from module
fn create_plugin_from_module(
    engine: &Engine,
    module: Module,
    config: WasmPluginConfig,
    async_support: bool,
) -> WasmResult<WasmPlugin> {
    use super::host::HostContext;
    use super::plugin::*;

    let manifest = {
        let mut manifest = super::types::PluginManifest::new(&config.id, "1.0.0");
        for export in module.exports() {
            if let ExternType::Func(_) = export.ty() {
                manifest.exports.push(super::types::PluginExport::function(
                    export.name(),
                    vec![],
                    vec![],
                ));
            }
        }
        manifest
    };

    let host_context = Arc::new(HostContext::new(
        &config.id,
        config.allowed_capabilities.clone(),
    ));

    Ok(WasmPlugin::from_parts_with_async(
        config.id.clone(),
        config,
        manifest,
        module,
        engine.clone(),
        host_context,
        async_support,
    ))
}

/// Compute a cryptographic SHA-256 digest of `data` and return it as a
/// lowercase hex string.  This replaces the old `DefaultHasher`-based
/// function whose 64-bit output was trivially collisible, enabling
/// cache-poisoning attacks against compiled WASM modules.
fn sha256_hash(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize(); // [u8; 32]

    // Format the 256-bit digest as a 64-char lowercase hex string.
    digest
        .iter()
        .fold(String::with_capacity(64), |mut acc, byte| {
            use std::fmt::Write;
            let _ = write!(acc, "{:02x}", byte);
            acc
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_config() {
        let config = RuntimeConfig::new().with_optimization(OptimizationLevel::Speed);

        assert_eq!(config.optimization_level, OptimizationLevel::Speed);
        assert!(config.enable_cache);
    }

    #[tokio::test]
    async fn test_module_cache() {
        let cache = ModuleCache::new(10);

        let stats = cache.stats().await;
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);

        // Miss
        let _ = cache.get("nonexistent").await;
        assert_eq!(*cache.misses.read().await, 1);
    }

    #[tokio::test]
    async fn test_wasm_runtime_creation() {
        let config = RuntimeConfig::default();
        let runtime = WasmRuntime::new(config).unwrap();

        let stats = runtime.stats().await;
        assert_eq!(stats.modules_compiled, 0);
        assert_eq!(stats.plugins_created, 0);
    }

    #[tokio::test]
    async fn test_compile_wat() {
        let runtime = WasmRuntime::default_runtime().unwrap();

        let wat = r#"
            (module
                (func (export "answer") (result i32)
                    i32.const 42
                )
            )
        "#;

        let compiled = runtime.compile_wat("test", wat).await.unwrap();
        assert_eq!(compiled.name, "test");
        assert!(compiled.compile_time_ms > 0);

        let stats = runtime.stats().await;
        assert_eq!(stats.modules_compiled, 1);
    }

    #[test]
    fn test_cache_stats_hit_rate() {
        let stats = CacheStats {
            entries: 10,
            total_size_bytes: 1000,
            hits: 80,
            misses: 20,
        };

        assert_eq!(stats.hit_rate(), 0.8);
    }
}
