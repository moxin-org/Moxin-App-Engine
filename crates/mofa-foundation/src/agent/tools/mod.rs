//! 工具系统 (Foundation 层)
//! Tool System (Foundation Layer)
//!
//! 提供工具适配器、内置工具与统一注册中心等具体实现。
//! Provides concrete implementations for tool adapters, built-in tools, and unified registries.
//! Kernel 仅定义 Tool 接口与基础类型；具体实现放在 Foundation 层。
//! Kernel only defines Tool interfaces and base types; concrete implementations reside in Foundation.

pub mod adapters;
pub mod builtin;
pub mod rag;
pub mod registry;

/// MCP (Model Context Protocol) 客户端实现
/// MCP (Model Context Protocol) client implementation
///
/// 需要启用 `mcp` feature flag:
/// Requires the `mcp` feature flag to be enabled:
/// ```toml
/// mofa-foundation = { version = "0.1", features = ["mcp"] }
/// ```
#[cfg(feature = "mcp")]
pub mod mcp;

pub use adapters::{BuiltinTools, ClosureTool, FunctionTool};
pub use builtin::{DateTimeTool, FileReadTool, FileWriteTool, HttpTool, JsonParseTool, ShellTool};
pub use rag::RagTool;
pub use registry::{ToolRegistry, ToolSearcher};
