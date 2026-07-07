//! MCP Server Manager
//!
//! Exposes registered MoFA tools as an MCP server over HTTP/SSE, allowing
//! external systems (Claude Desktop, Cursor, other orchestrators) to discover
//! and call MoFA tools via the Model Context Protocol.
//!
//! # Architecture
//!
//! ```text
//!  ┌────────────────────────────────────────────┐
//!  │         External MCP Client                 │
//!  │  (Claude Desktop / Cursor / other agent)    │
//!  └───────────────────┬────────────────────────┘
//!                      │  HTTP/SSE (MCP protocol)
//!                      ▼
//!  ┌────────────────────────────────────────────┐
//!  │         McpServerManager                    │
//!  │  ┌──────────────────────────────────────┐  │
//!  │  │       MofaServerHandler              │  │
//!  │  │  (implements rmcp::ServerHandler)    │  │
//!  │  │                                      │  │
//!  │  │  tool registry: DynTool[]            │  │
//!  │  └──────────────────────────────────────┘  │
//!  └───────────────────┬────────────────────────┘
//!                      │  calls execute()
//!                      ▼
//!  ┌────────────────────────────────────────────┐
//!  │         MoFA Tool Registry                  │
//!  │  (any DynTool: calculator, web search, etc) │
//!  └────────────────────────────────────────────┘
//! ```

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use mofa_kernel::agent::components::mcp::McpHostConfig;
use mofa_kernel::agent::components::tool::DynTool;
use mofa_kernel::agent::context::AgentContext;
use mofa_kernel::agent::error::{AgentError, AgentResult};
use rmcp::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;
use tracing;

// ============================================================================
// Internal server handler
// ============================================================================

/// rmcp ServerHandler backed by MoFA's dynamic tool registry.
///
/// One instance is created per connection by the `StreamableHttpService` factory.
/// Tools are held behind `Arc` so cloning is cheap.
#[derive(Clone)]
struct MofaServerHandler {
    tools: Arc<HashMap<String, Arc<dyn DynTool>>>,
    config: McpHostConfig,
}

impl MofaServerHandler {
    fn new(tools: Arc<HashMap<String, Arc<dyn DynTool>>>, config: McpHostConfig) -> Self {
        Self { tools, config }
    }
}

impl ServerHandler for MofaServerHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: self.config.instructions.as_deref().map(|s| s.into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, rmcp::Error>> + Send + '_ {
        let tools: Vec<Tool> = self
            .tools
            .values()
            .map(|t| {
                let schema = t.parameters_schema();
                let input_schema = Arc::new(
                    schema
                        .as_object()
                        .cloned()
                        .unwrap_or_default(),
                );
                Tool {
                    name: Cow::Owned(t.name().to_string()),
                    description: Some(Cow::Owned(t.description().to_string())),
                    input_schema,
                    title: None,
                    output_schema: None,
                    annotations: None,
                    execution: None,
                    icons: None,
                    meta: None,
                }
            })
            .collect();

        std::future::ready(Ok(ListToolsResult {
            tools,
            next_cursor: None,
            meta: None,
        }))
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, rmcp::Error>> + Send + '_ {
        let tool_name = request.name.to_string();
        let tools = Arc::clone(&self.tools);

        async move {
            let tool = tools.get(&tool_name).ok_or_else(|| {
                rmcp::Error::invalid_params(
                    format!("Tool '{}' not found", tool_name),
                    None,
                )
            })?;

            let args = serde_json::Value::Object(
                request.arguments.unwrap_or_default(),
            );

            let ctx = AgentContext::new("mcp-server");
            let result = tool.execute_dynamic(args, &ctx).await.map_err(|e| {
                rmcp::Error::internal_error(
                    format!("Tool '{}' execution failed: {}", tool_name, e),
                    None,
                )
            })?;

            let text = match &result {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };

            Ok(CallToolResult::success(vec![Content::text(text)]))
        }
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Hosts MoFA tools as an MCP server over HTTP/SSE.
///
/// Register any number of [`DynTool`]s, then call [`serve`](McpServerManager::serve)
/// to start accepting MCP connections from Claude Desktop, Cursor, or any
/// MCP-compatible client.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_foundation::agent::tools::mcp::McpServerManager;
/// use mofa_kernel::agent::components::mcp::McpHostConfig;
///
/// let config = McpHostConfig::new("my-agent", "127.0.0.1", 3000)
///     .with_instructions("Exposes my agent's tools over MCP.");
///
/// let mut server = McpServerManager::new(config);
/// server.register_tool(Arc::new(MyTool));
///
/// server.serve().await?;
/// ```
pub struct McpServerManager {
    config: McpHostConfig,
    tools: HashMap<String, Arc<dyn DynTool>>,
}

impl McpServerManager {
    /// Create a new server manager with the given host configuration.
    pub fn new(config: McpHostConfig) -> Self {
        Self {
            config,
            tools: HashMap::new(),
        }
    }

    /// Register a tool to be exposed via MCP.
    ///
    /// Returns an error if a tool with the same name is already registered.
    pub fn register_tool(&mut self, tool: Arc<dyn DynTool>) -> AgentResult<()> {
        let name = tool.name().to_string();
        if self.tools.contains_key(&name) {
            return Err(AgentError::ConfigError(format!(
                "Tool '{}' is already registered in the MCP server",
                name
            )));
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    /// Returns the names of all registered tools.
    pub fn registered_tools(&self) -> Vec<&str> {
        self.tools.keys().map(String::as_str).collect()
    }

    /// Start the MCP HTTP/SSE server and block until the cancellation token is fired.
    ///
    /// The server listens on `config.bind_addr()` and routes all MCP traffic
    /// through `/mcp`.
    pub async fn serve(self) -> AgentResult<()> {
        self.serve_with_cancellation(CancellationToken::new()).await
    }

    /// Start the MCP server with an explicit cancellation token for graceful shutdown.
    pub async fn serve_with_cancellation(
        self,
        cancellation_token: CancellationToken,
    ) -> AgentResult<()> {
        let bind_addr = self.config.bind_addr();
        let tools = Arc::new(self.tools);
        let config = self.config;

        tracing::info!(
            "Starting MCP server '{}' on {}",
            config.name,
            bind_addr
        );

        let ct = cancellation_token.clone();
        let service: StreamableHttpService<MofaServerHandler, LocalSessionManager> =
            StreamableHttpService::new(
                {
                    let tools = Arc::clone(&tools);
                    let config = config.clone();
                    move || Ok(MofaServerHandler::new(Arc::clone(&tools), config.clone()))
                },
                Default::default(),
                StreamableHttpServerConfig {
                    cancellation_token: ct,
                    ..Default::default()
                },
            );

        let router = axum::Router::new().nest_service("/mcp", service);

        let listener = tokio::net::TcpListener::bind(&bind_addr)
            .await
            .map_err(|e| {
                AgentError::InitializationFailed(format!(
                    "MCP server failed to bind to '{}': {}",
                    bind_addr, e
                ))
            })?;

        tracing::info!("MCP server listening on http://{}/mcp", bind_addr);

        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                cancellation_token.cancelled().await;
                tracing::info!("MCP server shutting down");
            })
            .await
            .map_err(|e| {
                AgentError::ShutdownFailed(format!("MCP server error: {}", e))
            })?;

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use mofa_kernel::agent::components::mcp::McpHostConfig;

    #[test]
    fn test_server_manager_new() {
        let config = McpHostConfig::new("test-server", "127.0.0.1", 9000);
        let manager = McpServerManager::new(config);
        assert!(manager.registered_tools().is_empty());
    }

    #[test]
    fn test_register_tool_duplicate_rejected() {
        use mofa_kernel::agent::components::tool::{
            DynToolWrapper, ToolInput, ToolResult, ToolExt,
        };
        use mofa_kernel::agent::context::AgentContext;

        struct DummyTool;

        #[async_trait::async_trait]
        impl mofa_kernel::agent::components::tool::Tool for DummyTool {
            fn name(&self) -> &str {
                "dummy"
            }
            fn description(&self) -> &str {
                "A dummy tool"
            }
            fn parameters_schema(&self) -> serde_json::Value {
                serde_json::json!({ "type": "object" })
            }
            async fn execute(
                &self,
                _input: ToolInput,
                _ctx: &AgentContext,
            ) -> ToolResult {
                ToolResult::success(serde_json::json!({}))
            }
        }

        let config = McpHostConfig::new("test-server", "127.0.0.1", 9001);
        let mut manager = McpServerManager::new(config);

        let tool = DummyTool.into_dynamic();
        assert!(manager.register_tool(tool.clone()).is_ok());
        assert!(manager.register_tool(tool).is_err());
    }

    #[test]
    fn test_server_handler_get_info() {
        let config = McpHostConfig::new("test", "127.0.0.1", 9002)
            .with_instructions("Test instructions");
        let handler = MofaServerHandler::new(Arc::new(HashMap::new()), config);
        let info = handler.get_info();
        assert!(info.instructions.is_some());
    }
}
