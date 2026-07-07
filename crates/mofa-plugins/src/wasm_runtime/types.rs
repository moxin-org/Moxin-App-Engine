//! WASM type definitions
//!
//! Core types for the WASM plugin runtime

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use thiserror::Error;

/// WASM value types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WasmValue {
    /// 32-bit integer
    I32(i32),
    /// 64-bit integer
    I64(i64),
    /// 32-bit float
    F32(f32),
    /// 64-bit float
    F64(f64),
    /// Boolean
    Bool(bool),
    /// String
    String(String),
    /// Byte array
    Bytes(Vec<u8>),
    /// Null value
    Null,
    /// Array of values
    Array(Vec<WasmValue>),
    /// Map of values
    Map(HashMap<String, WasmValue>),
}

impl WasmValue {
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            WasmValue::I32(v) => Some(*v),
            WasmValue::I64(v) => Some(*v as i32),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            WasmValue::I32(v) => Some(*v as i64),
            WasmValue::I64(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match self {
            WasmValue::F32(v) => Some(*v),
            WasmValue::F64(v) => Some(*v as f32),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            WasmValue::F32(v) => Some(*v as f64),
            WasmValue::F64(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            WasmValue::Bool(v) => Some(*v),
            WasmValue::I32(v) => Some(*v != 0),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            WasmValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            WasmValue::Bytes(b) => Some(b),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, WasmValue::Null)
    }
}

impl From<i32> for WasmValue {
    fn from(v: i32) -> Self {
        WasmValue::I32(v)
    }
}

impl From<i64> for WasmValue {
    fn from(v: i64) -> Self {
        WasmValue::I64(v)
    }
}

impl From<f32> for WasmValue {
    fn from(v: f32) -> Self {
        WasmValue::F32(v)
    }
}

impl From<f64> for WasmValue {
    fn from(v: f64) -> Self {
        WasmValue::F64(v)
    }
}

impl From<bool> for WasmValue {
    fn from(v: bool) -> Self {
        WasmValue::Bool(v)
    }
}

impl From<String> for WasmValue {
    fn from(v: String) -> Self {
        WasmValue::String(v)
    }
}

impl From<&str> for WasmValue {
    fn from(v: &str) -> Self {
        WasmValue::String(v.to_string())
    }
}

impl From<Vec<u8>> for WasmValue {
    fn from(v: Vec<u8>) -> Self {
        WasmValue::Bytes(v)
    }
}

/// WASM type definitions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WasmType {
    I32,
    I64,
    F32,
    F64,
    V128,
    FuncRef,
    ExternRef,
}

impl fmt::Display for WasmType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WasmType::I32 => write!(f, "i32"),
            WasmType::I64 => write!(f, "i64"),
            WasmType::F32 => write!(f, "f32"),
            WasmType::F64 => write!(f, "f64"),
            WasmType::V128 => write!(f, "v128"),
            WasmType::FuncRef => write!(f, "funcref"),
            WasmType::ExternRef => write!(f, "externref"),
        }
    }
}

/// WASM runtime errors.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WasmError {
    #[error("Failed to compile WASM module: {0}")]
    CompilationError(String),

    #[error("Failed to instantiate WASM module: {0}")]
    InstantiationError(String),

    #[error("Failed to load WASM module: {0}")]
    LoadError(String),

    #[error("Export not found: {0}")]
    ExportNotFound(String),

    #[error("Import not found: {module}.{name}")]
    ImportNotFound { module: String, name: String },

    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: String, actual: String },

    #[error("Memory access out of bounds: offset={offset}, size={size}")]
    MemoryOutOfBounds { offset: u32, size: u32 },

    #[error("Memory allocation failed: requested {size} bytes")]
    AllocationFailed { size: u32 },

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Timeout: execution exceeded {0}ms")]
    Timeout(u64),

    #[error("Resource limit exceeded: {0}")]
    ResourceLimitExceeded(String),

    #[error("Invalid plugin manifest: {0}")]
    InvalidManifest(String),

    #[error("Plugin not found: {0}")]
    PluginNotFound(String),

    #[error("Plugin already loaded: {0}")]
    PluginAlreadyLoaded(String),

    #[error("Host function error: {0}")]
    HostFunctionError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Plain result alias for WASM operations (backward-compatible).
pub type WasmResult<T> = Result<T, WasmError>;

/// Error-stack–backed result alias for WASM operations.
pub type WasmReport<T> = ::std::result::Result<T, error_stack::Report<WasmError>>;

/// Extension trait to convert [`WasmResult<T>`] into [`WasmReport<T>`].
pub trait IntoWasmReport<T> {
    /// Wrap the error in an `error_stack::Report`.
    fn into_report(self) -> WasmReport<T>;
}

impl<T> IntoWasmReport<T> for WasmResult<T> {
    #[inline]
    fn into_report(self) -> WasmReport<T> {
        self.map_err(error_stack::Report::new)
    }
}

/// Plugin capabilities
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PluginCapability {
    /// Can read configuration
    ReadConfig,
    /// Can write configuration
    WriteConfig,
    /// Can send messages
    SendMessage,
    /// Can receive messages
    ReceiveMessage,
    /// Can call tools
    CallTool,
    /// Can access storage
    Storage,
    /// Can make HTTP requests
    HttpClient,
    /// Can access filesystem (sandboxed)
    FileSystem,
    /// Can use timers
    Timer,
    /// Can use random number generation
    Random,
    /// Custom capability
    Custom(String),
}

impl fmt::Display for PluginCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginCapability::ReadConfig => write!(f, "read_config"),
            PluginCapability::WriteConfig => write!(f, "write_config"),
            PluginCapability::SendMessage => write!(f, "send_message"),
            PluginCapability::ReceiveMessage => write!(f, "receive_message"),
            PluginCapability::CallTool => write!(f, "call_tool"),
            PluginCapability::Storage => write!(f, "storage"),
            PluginCapability::HttpClient => write!(f, "http_client"),
            PluginCapability::FileSystem => write!(f, "filesystem"),
            PluginCapability::Timer => write!(f, "timer"),
            PluginCapability::Random => write!(f, "random"),
            PluginCapability::Custom(s) => write!(f, "custom:{}", s),
        }
    }
}

/// Declared dependency requirement for a plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDep {
    pub name: String,
    pub req: VersionReq,
}

impl PluginDep {
    /// Create a new dependency with the provided semver requirement
    pub fn new(name: &str, req: VersionReq) -> Self {
        Self {
            name: name.to_string(),
            req,
        }
    }

    /// Helper that parses a textual requirement
    pub fn parse(name: &str, requirement: &str) -> Result<Self, semver::Error> {
        let req = VersionReq::parse(requirement)?;
        Ok(Self {
            name: name.to_string(),
            req,
        })
    }
}

/// Audit status captured by the marketplace trust pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditStatus {
    Unknown,
    Passed,
    Failed,
    Pending,
}

impl Default for AuditStatus {
    fn default() -> Self {
        Self::Unknown
    }
}

impl AuditStatus {
    pub fn trust_bonus(&self) -> f32 {
        match self {
            Self::Passed => 1.0,
            Self::Pending => 0.5,
            Self::Unknown => 0.25,
            Self::Failed => 0.0,
        }
    }
}

/// Plugin manifest describing the plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Plugin name
    pub name: String,
    /// Plugin version
    pub version: Version,
    /// Yanked versions should not be selected by default
    #[serde(default)]
    pub yanked: bool,
    /// Deprecated marker kept for registry policy and install-time warnings
    #[serde(default)]
    pub deprecated: bool,
    /// Trust score (0.0 - 1.0)
    #[serde(default)]
    pub trust_score: f32,
    /// Community rating in the range 0.0 - 1.0
    #[serde(default)]
    pub community_rating: f32,
    /// Aggregate download count used by the trust model
    #[serde(default)]
    pub download_count: u64,
    /// Ed25519 signature stored with the published plugin artifact
    #[serde(default)]
    pub signature: String,
    /// Audit state used by marketplace trust policies
    #[serde(default)]
    pub audit_status: AuditStatus,
    /// Internal flag to preserve explicit trust overrides across builder calls.
    #[serde(skip)]
    trust_score_locked: bool,
    /// Plugin description
    pub description: Option<String>,
    /// Plugin author
    pub author: Option<String>,
    /// Plugin license
    pub license: Option<String>,
    /// Required capabilities
    pub capabilities: Vec<PluginCapability>,
    /// Exported functions
    pub exports: Vec<PluginExport>,
    /// Minimum runtime version
    pub min_runtime_version: Option<String>,
    /// Plugin-specific configuration schema
    pub config_schema: Option<serde_json::Value>,
    /// Plugin metadata
    pub metadata: HashMap<String, String>,
    /// Declared dependencies
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub dependencies: HashMap<String, VersionReq>,
}

impl Default for PluginManifest {
    fn default() -> Self {
        Self {
            name: "unknown".to_string(),
            version: Version::new(0, 0, 0),
            yanked: false,
            deprecated: false,
            trust_score: 1.0,
            community_rating: 0.0,
            download_count: 0,
            signature: String::new(),
            audit_status: AuditStatus::Unknown,
            trust_score_locked: false,
            description: None,
            author: None,
            license: None,
            capabilities: Vec::new(),
            exports: Vec::new(),
            min_runtime_version: None,
            config_schema: None,
            metadata: HashMap::new(),
            dependencies: HashMap::new(),
        }
    }
}

impl PluginManifest {
    pub fn new(name: &str, version: Version) -> Self {
        Self {
            name: name.to_string(),
            version,
            ..Default::default()
        }
    }

    pub fn new_from_str(name: &str, version: &str) -> Result<Self, semver::Error> {
        Ok(Self::new(name, Version::parse(version)?))
    }

    pub fn with_description(mut self, description: &str) -> Self {
        self.description = Some(description.to_string());
        self
    }

    pub fn with_capability(mut self, capability: PluginCapability) -> Self {
        if !self.capabilities.contains(&capability) {
            self.capabilities.push(capability);
        }
        self
    }

    pub fn with_export(mut self, export: PluginExport) -> Self {
        self.exports.push(export);
        self
    }

    pub fn with_dependency(mut self, dependency: PluginDep) -> Self {
        self.dependencies.insert(dependency.name, dependency.req);
        self
    }

    pub fn with_trust_score(mut self, trust_score: f32) -> Self {
        self.trust_score = trust_score.clamp(0.0, 1.0);
        self.trust_score_locked = true;
        self
    }

    pub fn with_signature(mut self, signature: impl Into<String>) -> Self {
        self.signature = signature.into();
        self
    }

    pub fn with_community_rating(mut self, community_rating: f32) -> Self {
        self.community_rating = community_rating.clamp(0.0, 1.0);
        self.recompute_trust_score();
        self
    }

    pub fn with_download_count(mut self, download_count: u64) -> Self {
        self.download_count = download_count;
        self.recompute_trust_score();
        self
    }

    pub fn with_audit_status(mut self, audit_status: AuditStatus) -> Self {
        self.audit_status = audit_status;
        self.recompute_trust_score();
        self
    }

    pub fn with_yanked(mut self, yanked: bool) -> Self {
        self.yanked = yanked;
        self
    }

    pub fn with_deprecated(mut self, deprecated: bool) -> Self {
        self.deprecated = deprecated;
        self
    }

    pub fn dependency_requirements(&self) -> impl Iterator<Item = PluginDep> + '_ {
        self.dependencies.iter().map(|(name, req)| PluginDep {
            name: name.clone(),
            req: req.clone(),
        })
    }

    pub fn recompute_trust_score(&mut self) {
        if self.trust_score_locked {
            return;
        }
        let downloads_component = ((self.download_count as f32 + 1.0).ln() / 10.0).clamp(0.0, 1.0);
        self.trust_score = (
            0.4 * self.community_rating.clamp(0.0, 1.0)
                + 0.3 * self.audit_status.trust_bonus()
                + 0.3 * downloads_component
        )
        .clamp(0.0, 1.0);
    }

    pub fn has_capability(&self, capability: &PluginCapability) -> bool {
        self.capabilities.contains(capability)
    }
}

/// Plugin export definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginExport {
    /// Export name
    pub name: String,
    /// Export kind
    pub kind: ExportKind,
    /// Parameter types (for functions)
    pub params: Vec<WasmType>,
    /// Return types (for functions)
    pub returns: Vec<WasmType>,
    /// Description
    pub description: Option<String>,
}

impl PluginExport {
    pub fn function(name: &str, params: Vec<WasmType>, returns: Vec<WasmType>) -> Self {
        Self {
            name: name.to_string(),
            kind: ExportKind::Function,
            params,
            returns,
            description: None,
        }
    }

    pub fn memory(name: &str) -> Self {
        Self {
            name: name.to_string(),
            kind: ExportKind::Memory,
            params: Vec::new(),
            returns: Vec::new(),
            description: None,
        }
    }

    pub fn with_description(mut self, description: &str) -> Self {
        self.description = Some(description.to_string());
        self
    }
}

/// Export kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportKind {
    Function,
    Memory,
    Table,
    Global,
}

/// Resource limits for WASM execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum memory pages (64KB each)
    pub max_memory_pages: u32,
    /// Maximum table elements
    pub max_table_elements: u32,
    /// Maximum instances per module
    pub max_instances: u32,
    /// Maximum execution time in milliseconds
    pub max_execution_time_ms: u64,
    /// Maximum fuel (instruction count)
    pub max_fuel: Option<u64>,
    /// Maximum call stack depth
    pub max_call_depth: u32,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_pages: 256, // 16MB
            max_table_elements: 10000,
            max_instances: 10,
            max_execution_time_ms: 30000, // 30 seconds
            max_fuel: Some(100_000_000),  // ~100M instructions
            max_call_depth: 1000,
        }
    }
}

impl ResourceLimits {
    pub fn unlimited() -> Self {
        Self {
            max_memory_pages: u32::MAX,
            max_table_elements: u32::MAX,
            max_instances: u32::MAX,
            max_execution_time_ms: u64::MAX,
            max_fuel: None,
            max_call_depth: u32::MAX,
        }
    }

    pub fn restrictive() -> Self {
        Self {
            max_memory_pages: 16, // 1MB
            max_table_elements: 1000,
            max_instances: 1,
            max_execution_time_ms: 5000, // 5 seconds
            max_fuel: Some(10_000_000),  // ~10M instructions
            max_call_depth: 100,
        }
    }

    pub fn max_memory_bytes(&self) -> u64 {
        self.max_memory_pages as u64 * 65536
    }
}

/// Memory configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Initial memory pages
    pub initial_pages: u32,
    /// Maximum memory pages
    pub maximum_pages: Option<u32>,
    /// Memory is shared
    pub shared: bool,
    /// Memory growth limit per call
    pub growth_limit: Option<u32>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            initial_pages: 1,         // 64KB
            maximum_pages: Some(256), // 16MB
            shared: false,
            growth_limit: Some(16), // 1MB per growth
        }
    }
}

/// Execution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    /// Enable async execution
    pub async_support: bool,
    /// Enable fuel metering
    pub fuel_metering: bool,
    /// Enable epoch interruption
    pub epoch_interruption: bool,
    /// Epoch tick interval in milliseconds
    pub epoch_tick_ms: u64,
    /// Enable debug info
    pub debug_info: bool,
    /// Enable reference types
    pub reference_types: bool,
    /// Enable SIMD
    pub simd: bool,
    /// Enable bulk memory operations
    pub bulk_memory: bool,
    /// Enable multi-value returns
    pub multi_value: bool,
    /// Enable threads
    pub threads: bool,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            async_support: true,
            fuel_metering: true,
            epoch_interruption: true,
            epoch_tick_ms: 10,
            debug_info: false,
            reference_types: true,
            simd: true,
            bulk_memory: true,
            multi_value: true,
            threads: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_value_conversions() {
        let v = WasmValue::from(42i32);
        assert_eq!(v.as_i32(), Some(42));
        assert_eq!(v.as_i64(), Some(42));

        let v = WasmValue::from("hello");
        assert_eq!(v.as_string(), Some("hello"));

        let v = WasmValue::from(true);
        assert_eq!(v.as_bool(), Some(true));
    }

    #[test]
    fn test_plugin_manifest() {
        let manifest = PluginManifest::new_from_str("test-plugin", "1.0.0")
            .unwrap()
            .with_description("A test plugin")
            .with_capability(PluginCapability::ReadConfig)
            .with_capability(PluginCapability::SendMessage);

        assert_eq!(manifest.name, "test-plugin");
        assert_eq!(manifest.version, Version::new(1, 0, 0));
        assert!(manifest.has_capability(&PluginCapability::ReadConfig));
        assert!(!manifest.has_capability(&PluginCapability::Storage));
    }

    #[test]
    fn test_resource_limits() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.max_memory_bytes(), 16 * 1024 * 1024); // 16MB

        let restrictive = ResourceLimits::restrictive();
        assert_eq!(restrictive.max_memory_bytes(), 1024 * 1024); // 1MB
    }
}
