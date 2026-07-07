//! WASM Plugin Wrapper
//!
//! Wraps WASM modules as plugins with lifecycle management

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use wasmtime::*;

use super::host::{DefaultHostFunctions, HostContext};
use super::types::{
    ExecutionConfig, PluginCapability, PluginManifest, ResourceLimits, WasmError, WasmResult,
    WasmValue,
};

/// WASM plugin state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WasmPluginState {
    /// Plugin is created but not initialized
    #[default]
    Created,
    /// Plugin is initializing
    Initializing,
    /// Plugin is ready to execute
    Ready,
    /// Plugin is currently executing
    Running,
    /// Plugin is paused
    Paused,
    /// Plugin encountered an error
    Error,
    /// Plugin is stopped
    Stopped,
}

/// WASM plugin configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmPluginConfig {
    /// Plugin ID
    pub id: String,
    /// Resource limits
    pub resource_limits: ResourceLimits,
    /// Execution configuration
    pub execution_config: ExecutionConfig,
    /// Allowed capabilities
    pub allowed_capabilities: Vec<PluginCapability>,
    /// Initial configuration values
    pub initial_config: HashMap<String, WasmValue>,
    /// Enable caching
    pub enable_caching: bool,
}

impl Default for WasmPluginConfig {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            resource_limits: ResourceLimits::default(),
            execution_config: ExecutionConfig::default(),
            allowed_capabilities: vec![PluginCapability::ReadConfig, PluginCapability::SendMessage],
            initial_config: HashMap::new(),
            enable_caching: true,
        }
    }
}

impl WasmPluginConfig {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            ..Default::default()
        }
    }

    pub fn with_capability(mut self, cap: PluginCapability) -> Self {
        if !self.allowed_capabilities.contains(&cap) {
            self.allowed_capabilities.push(cap);
        }
        self
    }

    pub fn with_resource_limits(mut self, limits: ResourceLimits) -> Self {
        self.resource_limits = limits;
        self
    }

    pub fn with_config(mut self, key: &str, value: WasmValue) -> Self {
        self.initial_config.insert(key.to_string(), value);
        self
    }
}

/// Plugin execution metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginMetrics {
    /// Total number of calls
    pub call_count: u64,
    /// Successful calls
    pub success_count: u64,
    /// Failed calls
    pub error_count: u64,
    /// Total execution time in nanoseconds
    pub total_execution_time_ns: u64,
    /// Average execution time in nanoseconds
    pub avg_execution_time_ns: u64,
    /// Peak memory usage in bytes
    pub peak_memory_bytes: u64,
    /// Current memory usage in bytes
    pub current_memory_bytes: u64,
    /// Fuel consumed (if metering enabled)
    pub fuel_consumed: u64,
    /// Last execution timestamp
    pub last_execution: u64,
}

impl PluginMetrics {
    pub fn record_execution(&mut self, duration_ns: u64, success: bool) {
        self.call_count += 1;
        if success {
            self.success_count += 1;
        } else {
            self.error_count += 1;
        }
        self.total_execution_time_ns += duration_ns;
        self.avg_execution_time_ns = self.total_execution_time_ns / self.call_count;
        self.last_execution = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
}

/// Plugin instance wrapping wasmtime Instance
pub struct PluginInstance {
    /// Wasmtime store
    store: Store<PluginState>,
    /// Wasmtime instance
    instance: Instance,
    /// Plugin manifest
    manifest: PluginManifest,
}

/// Plugin state stored in wasmtime Store
pub struct PluginState {
    /// Host context
    pub host_context: Arc<HostContext>,
    /// Host functions implementation
    pub host_functions: Arc<DefaultHostFunctions>,
    /// Limits configuration
    pub limits: StoreLimits,
    /// Execution start time
    pub execution_start: Option<Instant>,
    /// Fuel limit
    pub fuel_limit: Option<u64>,
}

/// Store limits for resource control
pub struct StoreLimits {
    pub max_memory_bytes: u64,
    pub max_table_elements: u32,
    pub max_instances: u32,
}

impl Default for StoreLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 16 * 1024 * 1024, // 16MB
            max_table_elements: 10000,
            max_instances: 10,
        }
    }
}

impl ResourceLimiter for StoreLimits {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool> {
        let max = maximum.unwrap_or(self.max_memory_bytes as usize);
        Ok(desired <= max && desired <= self.max_memory_bytes as usize)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool> {
        let max = maximum.unwrap_or(self.max_table_elements as usize);
        Ok(desired <= max && desired <= self.max_table_elements as usize)
    }
}

/// WASM Plugin
pub struct WasmPlugin {
    /// Plugin ID
    id: String,
    /// Plugin configuration
    config: WasmPluginConfig,
    /// Plugin manifest
    manifest: PluginManifest,
    /// Current state
    state: RwLock<WasmPluginState>,
    /// Compiled module
    module: Module,
    /// Wasmtime engine
    engine: Engine,
    /// Host context
    host_context: Arc<HostContext>,
    /// Execution metrics
    metrics: RwLock<PluginMetrics>,
    /// Instance (created on demand)
    instance: RwLock<Option<PluginInstance>>,
    /// Whether async support is enabled in the engine
    async_support: bool,
}

impl WasmPlugin {
    /// Create a new WASM plugin from module bytes
    pub fn from_bytes(engine: &Engine, bytes: &[u8], config: WasmPluginConfig) -> WasmResult<Self> {
        Self::from_bytes_with_async(engine, bytes, config, true)
    }

    /// Create a new WASM plugin from module bytes with async support flag
    pub fn from_bytes_with_async(
        engine: &Engine,
        bytes: &[u8],
        config: WasmPluginConfig,
        async_support: bool,
    ) -> WasmResult<Self> {
        let module =
            Module::new(engine, bytes).map_err(|e| WasmError::CompilationError(e.to_string()))?;

        // Extract manifest from module (could be custom section or export)
        let manifest = Self::extract_manifest(&module, &config);

        let host_context = Arc::new(HostContext::new(
            &config.id,
            config.allowed_capabilities.clone(),
        ));

        // Set initial config values
        for (key, value) in &config.initial_config {
            // Spawn a task to set config since we can't use async here
            let ctx = host_context.clone();
            let k = key.clone();
            let v = value.clone();
            tokio::spawn(async move {
                ctx.set_config(&k, v).await;
            });
        }

        Ok(Self {
            id: config.id.clone(),
            config,
            manifest,
            state: RwLock::new(WasmPluginState::Created),
            module,
            engine: engine.clone(),
            host_context,
            metrics: RwLock::new(PluginMetrics::default()),
            instance: RwLock::new(None),
            async_support,
        })
    }

    /// Create from WAT (WebAssembly Text format)
    pub fn from_wat(engine: &Engine, wat: &str, config: WasmPluginConfig) -> WasmResult<Self> {
        Self::from_wat_with_async(engine, wat, config, true)
    }

    /// Create from WAT with async support flag
    pub fn from_wat_with_async(
        engine: &Engine,
        wat: &str,
        config: WasmPluginConfig,
        async_support: bool,
    ) -> WasmResult<Self> {
        let bytes = wat.to_string().into_bytes();
        Self::from_bytes_with_async(engine, &bytes, config, async_support)
    }

    /// Create from file path
    pub fn from_file(
        engine: &Engine,
        path: &std::path::Path,
        config: WasmPluginConfig,
    ) -> WasmResult<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_bytes(engine, &bytes, config)
    }

    fn extract_manifest(module: &Module, config: &WasmPluginConfig) -> PluginManifest {
        // Try to extract from custom section or use defaults
        let mut manifest = PluginManifest::new_from_str(&config.id, "1.0.0").unwrap();

        // List exports from module
        for export in module.exports() {
            match export.ty() {
                ExternType::Func(_) => {
                    manifest.exports.push(super::types::PluginExport::function(
                        export.name(),
                        vec![],
                        vec![],
                    ));
                }
                ExternType::Memory(_) => {
                    manifest
                        .exports
                        .push(super::types::PluginExport::memory(export.name()));
                }
                _ => {}
            }
        }

        manifest
    }

    /// Get plugin ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get plugin manifest
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    /// Get current state
    pub async fn state(&self) -> WasmPluginState {
        *self.state.read().await
    }

    /// Get metrics
    pub async fn metrics(&self) -> PluginMetrics {
        self.metrics.read().await.clone()
    }

    /// Initialize the plugin
    pub async fn initialize(&self) -> WasmResult<()> {
        let mut state = self.state.write().await;
        if *state != WasmPluginState::Created && *state != WasmPluginState::Stopped {
            return Err(WasmError::ExecutionError(format!(
                "Cannot initialize plugin in state {:?}",
                *state
            )));
        }

        *state = WasmPluginState::Initializing;
        drop(state);

        // Create instance
        self.create_instance().await?;

        // Call _initialize if exported
        if self.has_export("_initialize").await {
            self.call_void("_initialize", &[]).await?;
        }

        *self.state.write().await = WasmPluginState::Ready;
        info!("Plugin {} initialized", self.id);
        Ok(())
    }

    /// Check if export exists
    pub async fn has_export(&self, name: &str) -> bool {
        // Check if the module has this export
        for export in self.module.exports() {
            if export.name() == name {
                return true;
            }
        }
        false
    }

    /// Create a new instance
    async fn create_instance(&self) -> WasmResult<()> {
        let host_functions = Arc::new(DefaultHostFunctions::new(self.host_context.clone()));

        let limits = StoreLimits {
            max_memory_bytes: self.config.resource_limits.max_memory_pages as u64 * 65536,
            max_table_elements: self.config.resource_limits.max_table_elements,
            max_instances: self.config.resource_limits.max_instances,
        };

        let plugin_state = PluginState {
            host_context: self.host_context.clone(),
            host_functions: host_functions.clone(),
            limits,
            execution_start: None,
            fuel_limit: self.config.resource_limits.max_fuel,
        };

        let mut store = Store::new(&self.engine, plugin_state);

        // Set fuel if metering enabled
        if let Some(fuel) = self.config.resource_limits.max_fuel {
            store
                .set_fuel(fuel)
                .map_err(|e| WasmError::Internal(e.to_string()))?;
        }

        // Create linker with host functions
        let mut linker = Linker::new(&self.engine);
        Self::add_host_functions(&mut linker, host_functions)?;

        // Use async or sync instantiation based on engine configuration
        let instance = if self.async_support {
            linker
                .instantiate_async(&mut store, &self.module)
                .await
                .map_err(|e| WasmError::InstantiationError(e.to_string()))?
        } else {
            linker
                .instantiate(&mut store, &self.module)
                .map_err(|e| WasmError::InstantiationError(e.to_string()))?
        };

        let plugin_instance = PluginInstance {
            store,
            instance,
            manifest: self.manifest.clone(),
        };

        *self.instance.write().await = Some(plugin_instance);
        Ok(())
    }

    fn add_host_functions(
        linker: &mut Linker<PluginState>,
        _host_functions: Arc<DefaultHostFunctions>,
    ) -> WasmResult<()> {
        // Add host_log function
        linker
            .func_wrap(
                "env",
                "host_log",
                |_caller: Caller<'_, PluginState>, level: i32, ptr: i32, len: i32| {
                    // In real implementation, read string from memory and call host_functions.log()
                    debug!("host_log called: level={}, ptr={}, len={}", level, ptr, len);
                    0i32 // Success
                },
            )
            .map_err(|e| WasmError::Internal(e.to_string()))?;

        // Add host_now_ms function
        linker
            .func_wrap(
                "env",
                "host_now_ms",
                |_caller: Caller<'_, PluginState>| -> i64 {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as i64
                },
            )
            .map_err(|e| WasmError::Internal(e.to_string()))?;

        // Add host_alloc function
        linker
            .func_wrap(
                "env",
                "host_alloc",
                |_caller: Caller<'_, PluginState>, size: i32| -> i32 {
                    // Simple bump allocator simulation
                    debug!("host_alloc called: size={}", size);
                    0 // Return null for now
                },
            )
            .map_err(|e| WasmError::Internal(e.to_string()))?;

        // Add host_free function
        linker
            .func_wrap(
                "env",
                "host_free",
                |_caller: Caller<'_, PluginState>, ptr: i32| {
                    debug!("host_free called: ptr={}", ptr);
                },
            )
            .map_err(|e| WasmError::Internal(e.to_string()))?;

        // Add abort function (used by AssemblyScript and others)
        linker
            .func_wrap(
                "env",
                "abort",
                |_caller: Caller<'_, PluginState>, msg: i32, file: i32, line: i32, col: i32| {
                    error!(
                        "WASM abort: msg={}, file={}, line={}, col={}",
                        msg, file, line, col
                    );
                },
            )
            .map_err(|e| WasmError::Internal(e.to_string()))?;

        Ok(())
    }

    /// Call a function with i32 return value
    pub async fn call_i32(&self, name: &str, args: &[Val]) -> WasmResult<i32> {
        let start = Instant::now();
        let result = self.call_internal(name, args).await;
        let duration = start.elapsed();

        let success = result.is_ok();
        self.metrics
            .write()
            .await
            .record_execution(duration.as_nanos() as u64, success);

        match result {
            Ok(vals) => {
                if let Some(Val::I32(v)) = vals.first() {
                    Ok(*v)
                } else {
                    Err(WasmError::TypeMismatch {
                        expected: "i32".to_string(),
                        actual: format!("{:?}", vals),
                    })
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Call a function with i64 return value
    pub async fn call_i64(&self, name: &str, args: &[Val]) -> WasmResult<i64> {
        let start = Instant::now();
        let result = self.call_internal(name, args).await;
        let duration = start.elapsed();

        let success = result.is_ok();
        self.metrics
            .write()
            .await
            .record_execution(duration.as_nanos() as u64, success);

        match result {
            Ok(vals) => {
                if let Some(Val::I64(v)) = vals.first() {
                    Ok(*v)
                } else {
                    Err(WasmError::TypeMismatch {
                        expected: "i64".to_string(),
                        actual: format!("{:?}", vals),
                    })
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Call a void function
    pub async fn call_void(&self, name: &str, args: &[Val]) -> WasmResult<()> {
        let start = Instant::now();
        let result = self.call_internal(name, args).await;
        let duration = start.elapsed();

        let success = result.is_ok();
        self.metrics
            .write()
            .await
            .record_execution(duration.as_nanos() as u64, success);

        result.map(|_| ())
    }

    async fn call_internal(&self, name: &str, args: &[Val]) -> WasmResult<Vec<Val>> {
        let state = self.state().await;
        if state != WasmPluginState::Ready && state != WasmPluginState::Running {
            return Err(WasmError::ExecutionError(format!(
                "Plugin not ready, current state: {:?}",
                state
            )));
        }

        let mut instance_guard = self.instance.write().await;
        let instance = instance_guard
            .as_mut()
            .ok_or_else(|| WasmError::ExecutionError("Instance not created".to_string()))?;

        let func = instance
            .instance
            .get_func(&mut instance.store, name)
            .ok_or_else(|| WasmError::ExportNotFound(name.to_string()))?;

        let ty = func.ty(&instance.store);
        let mut results = vec![Val::I32(0); ty.results().len()];

        // Use async or sync call based on engine configuration
        if self.async_support {
            func.call_async(&mut instance.store, args, &mut results)
                .await
                .map_err(|e| WasmError::ExecutionError(e.to_string()))?;
        } else {
            func.call(&mut instance.store, args, &mut results)
                .map_err(|e| WasmError::ExecutionError(e.to_string()))?;
        }

        Ok(results)
    }

    /// Stop the plugin
    pub async fn stop(&self) -> WasmResult<()> {
        let mut state = self.state.write().await;

        // Call _cleanup if exported
        if *state == WasmPluginState::Ready || *state == WasmPluginState::Running {
            drop(state);
            if self.has_export("_cleanup").await {
                let _ = self.call_void("_cleanup", &[]).await;
            }
            state = self.state.write().await;
        }

        *state = WasmPluginState::Stopped;
        *self.instance.write().await = None;

        info!("Plugin {} stopped", self.id);
        Ok(())
    }

    /// Create plugin from pre-existing parts (used by runtime)
    pub fn from_parts(
        id: String,
        config: WasmPluginConfig,
        manifest: PluginManifest,
        module: Module,
        engine: Engine,
        host_context: Arc<HostContext>,
    ) -> Self {
        Self::from_parts_with_async(id, config, manifest, module, engine, host_context, true)
    }

    /// Create plugin from pre-existing parts with async support flag
    pub fn from_parts_with_async(
        id: String,
        config: WasmPluginConfig,
        manifest: PluginManifest,
        module: Module,
        engine: Engine,
        host_context: Arc<HostContext>,
        async_support: bool,
    ) -> Self {
        Self {
            id,
            config,
            manifest,
            state: RwLock::new(WasmPluginState::Created),
            module,
            engine,
            host_context,
            metrics: RwLock::new(PluginMetrics::default()),
            instance: RwLock::new(None),
            async_support,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_engine() -> Engine {
        let mut config = Config::new();
        config.async_support(false);
        Engine::new(&config).unwrap()
    }

    #[test]
    fn test_plugin_config() {
        let config = WasmPluginConfig::new("test-plugin")
            .with_capability(PluginCapability::Storage)
            .with_config("key", WasmValue::String("value".into()));

        assert_eq!(config.id, "test-plugin");
        assert!(
            config
                .allowed_capabilities
                .contains(&PluginCapability::Storage)
        );
        assert!(config.initial_config.contains_key("key"));
    }

    #[test]
    fn test_plugin_metrics() {
        let mut metrics = PluginMetrics::default();

        metrics.record_execution(1000, true);
        assert_eq!(metrics.call_count, 1);
        assert_eq!(metrics.success_count, 1);

        metrics.record_execution(2000, false);
        assert_eq!(metrics.call_count, 2);
        assert_eq!(metrics.error_count, 1);
        assert_eq!(metrics.avg_execution_time_ns, 1500);
    }

    #[test]
    fn test_plugin_state_default() {
        let state = WasmPluginState::default();
        assert_eq!(state, WasmPluginState::Created);
    }

    #[tokio::test]
    async fn test_wasm_plugin_from_wat() {
        let engine = create_test_engine();

        let wat = r#"
            (module
                (func (export "add") (param i32 i32) (result i32)
                    local.get 0
                    local.get 1
                    i32.add
                )
                (func (export "double") (param i32) (result i32)
                    local.get 0
                    i32.const 2
                    i32.mul
                )
            )
        "#;

        // Use config without fuel metering since test engine doesn't support it
        let mut config = WasmPluginConfig::new("test-math");
        config.resource_limits.max_fuel = None;

        // Use non-async mode since test engine has async_support disabled
        let plugin = WasmPlugin::from_wat_with_async(&engine, wat, config, false).unwrap();

        assert_eq!(plugin.id(), "test-math");
        assert_eq!(plugin.state().await, WasmPluginState::Created);

        // Initialize
        plugin.initialize().await.unwrap();
        assert_eq!(plugin.state().await, WasmPluginState::Ready);

        // Call functions
        let result = plugin
            .call_i32("add", &[Val::I32(3), Val::I32(4)])
            .await
            .unwrap();
        assert_eq!(result, 7);

        let result = plugin.call_i32("double", &[Val::I32(21)]).await.unwrap();
        assert_eq!(result, 42);

        // Check metrics
        let metrics = plugin.metrics().await;
        assert_eq!(metrics.call_count, 2);
        assert_eq!(metrics.success_count, 2);

        // Stop
        plugin.stop().await.unwrap();
        assert_eq!(plugin.state().await, WasmPluginState::Stopped);
    }
}
