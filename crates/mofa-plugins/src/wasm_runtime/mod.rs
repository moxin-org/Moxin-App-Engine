//! WASM Plugin Runtime Module
//!
//! Provides WebAssembly-based plugin execution for MoFA:
//! - Sandboxed plugin execution
//! - Host function interface for plugins
//! - Memory-safe communication between host and guest
//! - Resource limits and security controls
//! - Async plugin execution support
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      Host (MoFA)                           │
//! │  ┌──────────────────────────────────────────────────────┐   │
//! │  │              WasmPluginRuntime                       │   │
//! │  │  ┌────────────┐  ┌────────────┐  ┌────────────┐     │   │
//! │  │  │  Engine    │  │   Linker   │  │   Store    │     │   │
//! │  │  └────────────┘  └────────────┘  └────────────┘     │   │
//! │  └──────────────────────────────────────────────────────┘   │
//! │                            │                                │
//! │  ┌──────────────────────────────────────────────────────┐   │
//! │  │              Host Functions                          │   │
//! │  │  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐        │   │
//! │  │  │  log   │ │ get_   │ │ send_  │ │ call_  │        │   │
//! │  │  │        │ │ config │ │ message│ │ tool   │        │   │
//! │  │  └────────┘ └────────┘ └────────┘ └────────┘        │   │
//! │  └──────────────────────────────────────────────────────┘   │
//! │                            │                                │
//! │  ┌──────────────────────────────────────────────────────┐   │
//! │  │              WASM Sandbox                            │   │
//! │  │  ┌─────────────────────────────────────────────┐     │   │
//! │  │  │            Plugin Instance                  │     │   │
//! │  │  │  ┌─────────┐  ┌─────────┐  ┌─────────┐     │     │   │
//! │  │  │  │ Memory  │  │ Exports │  │ Imports │     │     │   │
//! │  │  │  └─────────┘  └─────────┘  └─────────┘     │     │   │
//! │  │  └─────────────────────────────────────────────┘     │   │
//! │  └──────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//! ```

mod host;
mod manager;
mod memory;
mod plugin;
pub mod runtime;
pub mod types;

pub use host::{HostCallback, HostContext, HostFunctions, LogLevel, MessageDirection};
pub use manager::{LoadedPlugin, PluginEvent, PluginHandle, PluginRegistry, WasmPluginManager};
pub use memory::{
    GuestPtr, GuestSlice, MemoryAllocator, MemoryRegion, SharedMemoryBuffer, WasmMemory,
};
pub use plugin::{PluginInstance, PluginMetrics, WasmPlugin, WasmPluginConfig, WasmPluginState};
pub use runtime::{CompiledModule, ModuleCache, RuntimeConfig, RuntimeStats, WasmRuntime};
pub use types::{
    ExecutionConfig, IntoWasmReport, MemoryConfig, PluginCapability, PluginDep, PluginExport,
    PluginManifest, ResourceLimits, WasmError, WasmReport, WasmResult, WasmType, WasmValue,
};
