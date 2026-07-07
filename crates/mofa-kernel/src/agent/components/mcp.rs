//! MCP (Model Context Protocol) 组件
//! MCP (Model Context Protocol) Component
//!
//! 定义 MCP 客户端接口，用于连接外部 MCP 服务器并使用其工具。
//! Defines the MCP client interface for connecting to external MCP servers and using their tools.
//!
//! # 概述
//! # Overview
//!
//! MCP 是 Anthropic 提出的标准化协议，允许 AI Agent 与外部工具服务器通信。
//! MCP is a standardized protocol proposed by Anthropic that allows AI Agents to communicate with external tool servers.
//! 本模块定义了 MoFA 框架中的 MCP 客户端抽象。
//! This module defines the MCP client abstraction within the MoFA framework.
//!
//! # 架构
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │              MoFA Agent                      │
//! │  ┌─────────────────────────────────────────┐│
//! │  │          ToolRegistry                    ││
//! │  │  ┌───────────┐  ┌───────────────────┐   ││
//! │  │  │ Built-in  │  │  McpToolAdapter   │   ││
//! │  │  │   Tools   │  │  (per MCP tool)   │   ││
//! │  │  └───────────┘  └────────┬──────────┘   ││
//! │  └──────────────────────────┼──────────────┘│
//! └─────────────────────────────┼───────────────┘
//!                               │
//!                    ┌──────────▼──────────┐
//!                    │    McpClient        │
//!                    │  (manages MCP       │
//!                    │   connections)      │
//!                    └──────────┬──────────┘
//!                               │
//!              ┌────────────────┼────────────────┐
//!              ▼                ▼                 ▼
//!     ┌──────────────┐ ┌──────────────┐ ┌──────────────┐
//!     │ MCP Server A │ │ MCP Server B │ │ MCP Server C │
//!     │  (stdio)     │ │   (HTTP)     │ │  (stdio)     │
//!     └──────────────┘ └──────────────┘ └──────────────┘
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// MCP Transport Configuration
// ============================================================================

/// MCP 服务器传输配置
/// MCP Server Transport Configuration
///
/// 定义如何连接到 MCP 服务器
/// Defines how to connect to an MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum McpTransportConfig {
    /// 通过标准输入输出连接 (启动子进程)
    /// Connection via Standard Input/Output (spawning a sub-process)
    ///
    /// # 示例
    /// # Example
    ///
    /// ```rust,ignore
    /// let config = McpTransportConfig::Stdio {
    ///     command: "npx".to_string(),
    ///     args: vec!["-y".to_string(), "@modelcontextprotocol/server-github".to_string()],
    ///     env: HashMap::from([("GITHUB_TOKEN".to_string(), "ghp_xxx".to_string())]),
    /// };
    /// ```
    Stdio {
        /// 可执行命令
        /// Executable command
        command: String,
        /// 命令参数
        /// Command arguments
        args: Vec<String>,
        /// 环境变量
        /// Environment variables
        env: HashMap<String, String>,
    },

    /// 通过 HTTP/SSE 连接
    /// Connection via HTTP/SSE
    ///
    /// # 示例
    /// # Example
    ///
    /// ```rust,ignore
    /// let config = McpTransportConfig::Http {
    ///     url: "http://localhost:8080/mcp".to_string(),
    /// };
    /// ```
    Http {
        /// 服务器 URL
        /// Server URL
        url: String,
    },
}

// ============================================================================
// MCP Server Configuration
// ============================================================================

/// MCP 服务器配置
/// MCP Server Configuration
///
/// 定义单个 MCP 服务器的完整配置
/// Defines the full configuration for a single MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// 服务器名称 (用于标识)
    /// Server name (used for identification)
    pub name: String,
    /// 传输配置
    /// Transport configuration
    pub transport: McpTransportConfig,
    /// 是否自动连接
    /// Whether to connect automatically
    pub auto_connect: bool,
}

impl McpServerConfig {
    /// 创建 Stdio 类型的 MCP 服务器配置
    /// Creates an MCP server configuration of Stdio type
    pub fn stdio(name: impl Into<String>, command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            name: name.into(),
            transport: McpTransportConfig::Stdio {
                command: command.into(),
                args,
                env: HashMap::new(),
            },
            auto_connect: true,
        }
    }

    /// 创建 HTTP 类型的 MCP 服务器配置
    /// Creates an MCP server configuration of HTTP type
    pub fn http(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            transport: McpTransportConfig::Http { url: url.into() },
            auto_connect: true,
        }
    }

    /// 设置环境变量 (仅对 Stdio 有效)
    /// Sets environment variables (only valid for Stdio)
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if let McpTransportConfig::Stdio { ref mut env, .. } = self.transport {
            env.insert(key.into(), value.into());
        }
        self
    }

    /// 设置是否自动连接
    /// Sets whether to connect automatically
    pub fn with_auto_connect(mut self, auto_connect: bool) -> Self {
        self.auto_connect = auto_connect;
        self
    }
}

// ============================================================================
// MCP Tool Information
// ============================================================================

/// MCP 工具信息
/// MCP Tool Information
///
/// 从 MCP 服务器发现的工具元信息
/// Tool metadata discovered from an MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo<Args = serde_json::Value> {
    /// 工具名称
    /// Tool name
    pub name: String,
    /// 工具描述
    /// Tool description
    pub description: String,
    /// 输入参数 JSON Schema
    /// Input parameter JSON Schema
    pub input_schema: Args,
}

// ============================================================================
// MCP Server Information
// ============================================================================

/// MCP 服务器信息
/// MCP Server Information
///
/// MCP 服务器在初始化握手时返回的元信息
/// Metadata returned by the MCP server during initial handshake
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    /// 服务器名称
    /// Server name
    pub name: String,
    /// 服务器版本
    /// Server version
    pub version: String,
    /// 服务器说明
    /// Server instructions
    pub instructions: Option<String>,
}

// ============================================================================
// MCP Host Configuration (Server-Side)
// ============================================================================

/// Configuration for hosting a MoFA MCP server
///
/// Used when MoFA acts as an MCP server, exposing its tools and workflow
/// executions as MCP endpoints for external systems to call.
///
/// # Example
///
/// ```rust,ignore
/// let config = McpHostConfig::new("mofa-agent", "127.0.0.1", 3000)
///     .with_instructions("A MoFA agent exposing its tools over MCP.");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpHostConfig {
    /// Hostname or IP address to bind the server to
    pub host: String,
    /// Port to listen on
    pub port: u16,
    /// Server name reported during MCP handshake
    pub name: String,
    /// Server version reported during MCP handshake
    pub version: String,
    /// Optional instructions shown to connecting clients
    pub instructions: Option<String>,
}

impl McpHostConfig {
    /// Create a new host configuration
    pub fn new(name: impl Into<String>, host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            name: name.into(),
            version: "0.1.0".to_string(),
            instructions: None,
        }
    }

    /// Set instructions returned to connecting MCP clients
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Set the server version string
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Returns the `host:port` bind address string
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

impl Default for McpHostConfig {
    fn default() -> Self {
        Self::new("mofa-mcp-server", "127.0.0.1", 3000)
    }
}

// ============================================================================
// MCP Client Trait
// ============================================================================

/// MCP 客户端 Trait
/// MCP Client Trait
///
/// 定义与 MCP 服务器通信的接口。
/// Defines the interface for communicating with MCP servers.
/// 具体实现在 `mofa-foundation` 层。
/// Concrete implementation is in the `mofa-foundation` layer.
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// use mofa_kernel::agent::components::mcp::*;
///
/// let config = McpServerConfig::stdio(
///     "github",
///     "npx",
///     vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
/// );
///
/// let mut client = MyMcpClient::new();
/// client.connect(config).await?;
///
/// let tools = client.list_tools("github").await?;
/// for tool in &tools {
///     println!("{}: {}", tool.name, tool.description);
/// }
///
/// let result = client.call_tool("github", "list_repos", json!({"owner": "mofa-org"})).await?;
/// ```
#[async_trait]
pub trait McpClient: Send + Sync {
    /// 连接到 MCP 服务器
    /// Connect to an MCP server
    ///
    /// 使用给定配置建立与 MCP 服务器的连接。
    /// Establish a connection to the MCP server using the given configuration.
    /// 连接建立后，可以通过 `server_name` 引用该服务器。
    /// Once connected, the server can be referenced by `server_name`.
    async fn connect(&mut self, config: McpServerConfig) -> crate::agent::error::AgentResult<()>;

    /// 断开与 MCP 服务器的连接
    /// Disconnect from an MCP server
    async fn disconnect(&mut self, server_name: &str) -> crate::agent::error::AgentResult<()>;

    /// 列出 MCP 服务器提供的工具
    /// List tools provided by the MCP server
    async fn list_tools(
        &self,
        server_name: &str,
    ) -> crate::agent::error::AgentResult<Vec<McpToolInfo>>;

    /// 调用 MCP 服务器上的工具
    /// Call a tool on the MCP server
    async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> crate::agent::error::AgentResult<serde_json::Value>;

    /// 获取 MCP 服务器信息
    /// Get MCP server information
    async fn server_info(
        &self,
        server_name: &str,
    ) -> crate::agent::error::AgentResult<McpServerInfo>;

    /// 获取所有已连接的服务器名称
    /// Get names of all connected servers
    fn connected_servers(&self) -> Vec<String>;

    /// 检查服务器是否已连接
    /// Check if a server is connected
    fn is_connected(&self, server_name: &str) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_server_config_stdio() {
        let config = McpServerConfig::stdio("test-server", "node", vec!["server.js".to_string()])
            .with_env("API_KEY", "test-key")
            .with_auto_connect(false);

        assert_eq!(config.name, "test-server");
        assert!(!config.auto_connect);

        if let McpTransportConfig::Stdio { command, args, env } = &config.transport {
            assert_eq!(command, "node");
            assert_eq!(args, &["server.js"]);
            assert_eq!(env.get("API_KEY"), Some(&"test-key".to_string()));
        } else {
            panic!("Expected Stdio transport");
            // 应当为 Stdio 传输方式
        }
    }

    #[test]
    fn test_mcp_server_config_http() {
        let config = McpServerConfig::http("api-server", "http://localhost:8080/mcp");

        assert_eq!(config.name, "api-server");
        assert!(config.auto_connect);

        if let McpTransportConfig::Http { url } = &config.transport {
            assert_eq!(url, "http://localhost:8080/mcp");
        } else {
            panic!("Expected Http transport");
            // 应当为 Http 传输方式
        }
    }

    #[test]
    fn test_mcp_tool_info_serialization() {
        let tool = McpToolInfo {
            name: "list_repos".to_string(),
            description: "List GitHub repositories".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "owner": { "type": "string" }
                },
                "required": ["owner"]
            }),
        };

        let json = serde_json::to_string(&tool).unwrap();
        let deserialized: McpToolInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "list_repos");
    }

    #[test]
    fn test_mcp_server_info() {
        let info = McpServerInfo {
            name: "github".to_string(),
            version: "1.0.0".to_string(),
            instructions: Some("GitHub MCP Server".to_string()),
        };

        assert_eq!(info.name, "github");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.instructions, Some("GitHub MCP Server".to_string()));
    }
}
