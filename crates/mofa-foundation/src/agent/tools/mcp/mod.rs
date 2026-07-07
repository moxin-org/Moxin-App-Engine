//! MCP 客户端实现
//! MCP Client Implementation
//!
//! 提供 MCP 客户端的具体实现，使用 `rmcp` 库连接 MCP 服务器。
//! Provides concrete MCP client implementation using `rmcp` to connect to MCP servers.
//!
//! # 模块结构
//! # Module Structure
//!
//! - `McpClientManager` — 管理多个 MCP 服务器连接
//! - `McpClientManager` — Manages multiple MCP server connections
//! - `McpToolAdapter` — 将 MCP 工具包装为内核 `Tool` trait
//! - `McpToolAdapter` — Wraps MCP tools as kernel `Tool` traits

mod client;
mod tool_adapter;
#[cfg(feature = "mcp-server")]
mod server;

pub use client::McpClientManager;
pub use tool_adapter::McpToolAdapter;
#[cfg(feature = "mcp-server")]
pub use server::McpServerManager;
