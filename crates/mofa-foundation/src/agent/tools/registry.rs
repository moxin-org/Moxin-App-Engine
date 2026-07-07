//! 统一工具注册中心
//! Unified Tool Registry
//!
//! 整合内置工具、MCP 工具、自定义工具的注册中心
//! A registry that integrates builtin, MCP, and custom tools

use async_trait::async_trait;
use mofa_kernel::agent::components::tool::{
    DynTool, Tool, ToolDescriptor, ToolExt, ToolRegistry as ToolRegistryTrait,
};
use mofa_kernel::agent::error::{AgentError, AgentResult};
use std::collections::HashMap;
use std::sync::Arc;

type PluginLoader = Arc<dyn Fn(&str) -> AgentResult<Vec<Arc<dyn DynTool>>> + Send + Sync>;

/// 统一工具注册中心
/// Unified Tool Registry
///
/// 整合多种工具来源的注册中心
/// A registry integrating multiple tool sources
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// use mofa_foundation::agent::tools::ToolRegistry;
/// use mofa_foundation::agent::components::tool::EchoTool;
///
/// let mut registry = ToolRegistry::new();
///
/// // 注册内置工具
/// // Register builtin tool
/// registry.register(Arc::new(EchoTool)).unwrap();
///
/// // 注册 MCP 服务器的工具
/// // Register tools from MCP server
/// registry.load_mcp_server("http://localhost:8080").await?;
///
/// // 列出所有工具
/// // List all tools
/// for tool in registry.list() {
///     info!("{}: {}", tool.name, tool.description);
/// }
/// ```
pub struct ToolRegistry {
    /// 工具存储
    /// Tool storage
    tools: HashMap<String, Arc<dyn DynTool>>,
    /// 工具来源
    /// Tool sources
    sources: HashMap<String, ToolSource>,
    /// MCP 端点列表
    /// MCP endpoint list
    mcp_endpoints: Vec<String>,
    /// MCP 客户端管理器 (仅在 mcp feature 启用时使用)
    /// MCP client manager (only used when mcp feature is enabled)
    #[cfg(feature = "mcp")]
    mcp_client: Option<std::sync::Arc<tokio::sync::RwLock<super::mcp::McpClientManager>>>,
    /// 插件加载器（按路径映射）
    /// Plugin loaders keyed by plugin path
    plugin_loaders: HashMap<String, PluginLoader>,
}

/// 工具来源
/// Tool source
#[derive(Debug, Clone)]
pub enum ToolSource {
    /// 内置工具
    /// Builtin tool
    Builtin,
    /// MCP 服务器
    /// MCP server
    Mcp { endpoint: String },
    /// 自定义插件
    /// Custom plugin
    Plugin { path: String },
    /// 动态注册
    /// Dynamic registration
    Dynamic,
}

impl ToolRegistry {
    /// 创建新的统一注册中心
    /// Create a new unified registry
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            sources: HashMap::new(),
            mcp_endpoints: Vec::new(),
            #[cfg(feature = "mcp")]
            mcp_client: None,
            plugin_loaders: HashMap::new(),
        }
    }

    /// 注册工具并记录来源
    /// Register tool and record source
    pub fn register_with_source(
        &mut self,
        tool: Arc<dyn DynTool>,
        source: ToolSource,
    ) -> AgentResult<()> {
        let name = tool.name().to_string();
        if self.tools.contains_key(&name) {
            return Err(AgentError::RegistrationFailed(format!(
                "Tool '{name}' is already registered",
            )));
        }
        self.sources.insert(name.clone(), source);
        self.tools.insert(name, tool);
        Ok(())
    }

    /// 加载 MCP 服务器的工具
    /// Load MCP server tools
    ///
    /// 连接到 MCP 服务器，发现可用工具，并注册到工具注册中心。
    /// Connect to MCP server, discover available tools, and register them.
    ///
    /// # 参数
    /// # Parameters
    ///
    /// - `config`: MCP 服务器配置
    /// - `config`: MCP server configuration
    ///
    /// # 返回
    /// # Returns
    ///
    /// 成功注册的工具名称列表
    /// List of successfully registered tool names
    ///
    /// # 示例
    /// # Example
    ///
    /// ```rust,ignore
    /// use mofa_kernel::agent::components::mcp::McpServerConfig;
    ///
    /// let config = McpServerConfig::stdio(
    ///     "github",
    ///     "npx",
    ///     vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
    /// );
    /// let tool_names = registry.load_mcp_server(config).await?;
    /// println!("Loaded {} MCP tools", tool_names.len());
    /// ```
    #[cfg(feature = "mcp")]
    pub async fn load_mcp_server(
        &mut self,
        config: mofa_kernel::agent::components::mcp::McpServerConfig,
    ) -> AgentResult<Vec<String>> {
        use mofa_kernel::agent::components::mcp::McpClient;

        let endpoint = config.name.clone();
        self.mcp_endpoints.push(endpoint.clone());

        // Create or get the shared MCP client manager
        let client = self
            .mcp_client
            .get_or_insert_with(|| {
                std::sync::Arc::new(tokio::sync::RwLock::new(super::mcp::McpClientManager::new()))
            })
            .clone();

        // Connect to the MCP server
        {
            let mut client_guard = client.write().await;
            client_guard.connect(config).await?;
        }

        // List available tools
        let tools = {
            let client_guard = client.read().await;
            client_guard.list_tools(&endpoint).await?
        };

        // Register each MCP tool as a kernel Tool
        let mut registered_names = Vec::new();
        for tool_info in tools {
            let name = tool_info.name.clone();
            let adapter =
                super::mcp::McpToolAdapter::new(endpoint.clone(), tool_info, client.clone());
            self.register_with_source(
                adapter.into_dynamic(),
                ToolSource::Mcp {
                    endpoint: endpoint.clone(),
                },
            )?;
            registered_names.push(name);
        }

        tracing::info!(
            "Loaded {} tools from MCP server '{}'",
            registered_names.len(),
            endpoint,
        );

        Ok(registered_names)
    }

    /// 加载 MCP 服务器的工具 (存根 - 需要启用 `mcp` feature)
    /// Load MCP tools (Stub - requires `mcp` feature)
    ///
    /// # Warning
    ///
    /// This is a no-op stub. Recompile with `--features mcp` to enable real MCP support:
    ///
    /// ```toml
    /// mofa-foundation = { version = "...", features = ["mcp"] }
    /// ```
    #[cfg(not(feature = "mcp"))]
    pub async fn load_mcp_server(&mut self, endpoint: &str) -> AgentResult<Vec<String>> {
        tracing::warn!(
            endpoint = endpoint,
            "load_mcp_server called but the `mcp` feature is not enabled. \
             No tools will be loaded. Recompile with `--features mcp` to enable MCP support.",
        );
        self.mcp_endpoints.push(endpoint.to_string());
        Ok(vec![])
    }

    /// 卸载 MCP 服务器的工具
    /// Unload MCP server tools
    pub async fn unload_mcp_server(&mut self, endpoint: &str) -> AgentResult<Vec<String>> {
        self.mcp_endpoints.retain(|e| e != endpoint);

        // 移除该服务器的工具
        // Remove tools of this server
        let to_remove: Vec<String> = self
            .sources
            .iter()
            .filter_map(|(name, source)| {
                if let ToolSource::Mcp { endpoint: ep } = source
                    && ep == endpoint
                {
                    return Some(name.clone());
                }
                None
            })
            .collect();

        for name in &to_remove {
            self.tools.remove(name);
            self.sources.remove(name);
        }

        Ok(to_remove)
    }

    /// 注册插件加载器
    /// Register plugin loader callback for a path
    pub fn register_plugin_loader<F>(&mut self, path: impl Into<String>, loader: F)
    where
        F: Fn(&str) -> AgentResult<Vec<Arc<dyn DynTool>>> + Send + Sync + 'static,
    {
        self.plugin_loaders.insert(path.into(), Arc::new(loader));
    }

    /// Hot reload tools from a registered plugin path.
    ///
    /// The reload flow is transactional:
    /// 1. Resolve the plugin loader registered for `path`.
    /// 2. Remove all tools previously loaded from that plugin path.
    /// 3. Load tools from the plugin loader.
    /// 4. Re-register loaded tools with `ToolSource::Plugin`.
    ///
    /// If loading fails, or if any reloaded tool name collides with an
    /// existing source, the registry is rolled back to its original state.
    ///
    /// # Parameters
    /// - `path`: Plugin identifier/path used when registering the loader.
    ///
    /// # Returns
    /// Returns the list of tool names that were reloaded.
    ///
    /// # Errors
    /// - [`AgentError::NotFound`] if no loader is registered for `path`.
    /// - Loader-provided errors if plugin loading fails.
    /// - [`AgentError::RegistrationFailed`] if a reloaded tool name conflicts
    ///   with an existing tool.
    pub async fn hot_reload_plugin(&mut self, path: &str) -> AgentResult<Vec<String>> {
        let loader = self.plugin_loaders.get(path).cloned().ok_or_else(|| {
            AgentError::NotFound(format!("Plugin loader not registered for path: {path}"))
        })?;

        let tools_backup = self.tools.clone();
        let sources_backup = self.sources.clone();

        let to_remove: Vec<String> = self
            .sources
            .iter()
            .filter_map(|(name, source)| {
                if let ToolSource::Plugin { path: plugin_path } = source
                    && plugin_path == path
                {
                    return Some(name.clone());
                }
                None
            })
            .collect();

        for name in to_remove {
            self.tools.remove(&name);
            self.sources.remove(&name);
        }

        let loaded_tools = match loader(path) {
            Ok(tools) => tools,
            Err(err) => {
                self.tools = tools_backup;
                self.sources = sources_backup;
                return Err(err);
            }
        };

        let mut reloaded_names = Vec::new();
        for tool in loaded_tools {
            let name = tool.name().to_string();
            if self.sources.contains_key(&name) {
                self.tools = tools_backup;
                self.sources = sources_backup;
                return Err(AgentError::RegistrationFailed(format!(
                    "Tool '{name}' already exists and cannot be replaced during plugin hot reload",
                )));
            }

            self.register_with_source(
                tool,
                ToolSource::Plugin {
                    path: path.to_string(),
                },
            )?;
            reloaded_names.push(name);
        }

        Ok(reloaded_names)
    }

    /// 获取工具来源
    /// Get tool source
    pub fn get_source(&self, name: &str) -> Option<&ToolSource> {
        self.sources.get(name)
    }

    /// 按来源过滤工具
    /// Filter tools by source
    pub fn filter_by_source(&self, source_type: &str) -> Vec<ToolDescriptor> {
        self.tools
            .iter()
            .filter(|(name, _)| {
                if let Some(source) = self.sources.get(*name) {
                    match source {
                        ToolSource::Builtin => source_type == "builtin",
                        ToolSource::Mcp { .. } => source_type == "mcp",
                        ToolSource::Plugin { .. } => source_type == "plugin",
                        ToolSource::Dynamic => source_type == "dynamic",
                    }
                } else {
                    false
                }
            })
            .map(|(_, tool)| ToolDescriptor::from_dyn_tool(tool.as_ref()))
            .collect()
    }

    /// 获取 MCP 端点列表
    /// Get list of MCP endpoints
    pub fn mcp_endpoints(&self) -> &[String] {
        &self.mcp_endpoints
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolRegistryTrait for ToolRegistry {
    fn register(&mut self, tool: Arc<dyn DynTool>) -> AgentResult<()> {
        self.register_with_source(tool, ToolSource::Dynamic)
    }

    fn get(&self, name: &str) -> Option<Arc<dyn DynTool>> {
        self.tools.get(name).cloned()
    }

    fn unregister(&mut self, name: &str) -> AgentResult<bool> {
        self.sources.remove(name);
        Ok(self.tools.remove(name).is_some())
    }

    fn list(&self) -> Vec<ToolDescriptor> {
        self.tools
            .values()
            .map(|t| ToolDescriptor::from_dyn_tool(t.as_ref()))
            .collect()
    }

    fn list_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    fn count(&self) -> usize {
        self.tools.len()
    }
}

// ============================================================================
// 工具搜索
// Tool Search
// ============================================================================

/// 工具搜索器
/// Tool searcher
pub struct ToolSearcher<'a> {
    registry: &'a ToolRegistry,
}

impl<'a> ToolSearcher<'a> {
    /// 创建搜索器
    /// Create searcher
    pub fn new(registry: &'a ToolRegistry) -> Self {
        Self { registry }
    }

    /// 按名称模糊搜索
    /// Fuzzy search by name
    pub fn search_by_name(&self, pattern: &str) -> Vec<ToolDescriptor> {
        let pattern_lower = pattern.to_lowercase();
        self.registry
            .tools
            .iter()
            .filter(|(name, _)| name.to_lowercase().contains(&pattern_lower))
            .map(|(_, tool)| ToolDescriptor::from_dyn_tool(tool.as_ref()))
            .collect()
    }

    /// 按描述搜索
    /// Search by description
    pub fn search_by_description(&self, query: &str) -> Vec<ToolDescriptor> {
        let query_lower = query.to_lowercase();
        self.registry
            .tools
            .values()
            .filter(|tool| tool.description().to_lowercase().contains(&query_lower))
            .map(|tool| ToolDescriptor::from_dyn_tool(tool.as_ref()))
            .collect()
    }

    /// 按标签搜索
    /// Search by tag
    pub fn search_by_tag(&self, tag: &str) -> Vec<ToolDescriptor> {
        self.registry
            .tools
            .values()
            .filter(|tool| {
                let metadata = tool.metadata();
                metadata.tags.iter().any(|t| t == tag)
            })
            .map(|tool| ToolDescriptor::from_dyn_tool(tool.as_ref()))
            .collect()
    }

    /// 搜索需要确认的工具
    /// Search for tools requiring confirmation
    pub fn search_dangerous(&self) -> Vec<ToolDescriptor> {
        self.registry
            .tools
            .values()
            .filter(|tool| tool.metadata().is_dangerous || tool.requires_confirmation())
            .map(|tool| ToolDescriptor::from_dyn_tool(tool.as_ref()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofa_kernel::agent::components::tool::{ToolInput, ToolMetadata, ToolResult};
    use mofa_kernel::agent::context::AgentContext;
    use std::collections::HashSet;

    struct TestTool {
        name: &'static str,
    }

    impl TestTool {
        fn new(name: &'static str) -> Self {
            Self { name }
        }
    }

    struct RichTestTool {
        name: &'static str,
        description: &'static str,
        tags: Vec<&'static str>,
        is_dangerous: bool,
        needs_confirmation: bool,
    }

    impl RichTestTool {
        fn new(
            name: &'static str,
            description: &'static str,
            tags: Vec<&'static str>,
            is_dangerous: bool,
            needs_confirmation: bool,
        ) -> Self {
            Self {
                name,
                description,
                tags,
                is_dangerous,
                needs_confirmation,
            }
        }
    }

    #[async_trait]
    impl Tool for TestTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            "test tool"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {}
            })
        }

        async fn execute(
            &self,
            _input: ToolInput<serde_json::Value>,
            _ctx: &AgentContext,
        ) -> ToolResult<serde_json::Value> {
            ToolResult::success(serde_json::json!({"ok": true}))
        }
    }

    #[async_trait]
    impl Tool for RichTestTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            self.description
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {}
            })
        }

        async fn execute(
            &self,
            _input: ToolInput<serde_json::Value>,
            _ctx: &AgentContext,
        ) -> ToolResult<serde_json::Value> {
            ToolResult::success(serde_json::json!({"ok": true}))
        }

        fn metadata(&self) -> ToolMetadata {
            ToolMetadata {
                tags: self.tags.iter().map(|t| (*t).to_string()).collect(),
                is_dangerous: self.is_dangerous,
                ..Default::default()
            }
        }

        fn requires_confirmation(&self) -> bool {
            self.needs_confirmation
        }
    }

    fn names_of(descriptors: Vec<ToolDescriptor>) -> HashSet<String> {
        descriptors.into_iter().map(|d| d.name).collect()
    }

    #[tokio::test]
    async fn hot_reload_plugin_replaces_existing_plugin_tools() {
        let mut registry = ToolRegistry::new();
        let path = "/plugins/sample.rhai";

        registry
            .register_with_source(
                TestTool::new("old_plugin_tool").into_dynamic(),
                ToolSource::Plugin {
                    path: path.to_string(),
                },
            )
            .unwrap();
        registry
            .register_with_source(
                TestTool::new("keep_dynamic").into_dynamic(),
                ToolSource::Dynamic,
            )
            .unwrap();

        registry.register_plugin_loader(path, |_plugin_path| {
            Ok(vec![
                TestTool::new("new_plugin_tool_a").into_dynamic(),
                TestTool::new("new_plugin_tool_b").into_dynamic(),
            ])
        });

        let reloaded = registry.hot_reload_plugin(path).await.unwrap();
        assert_eq!(reloaded.len(), 2);
        assert!(reloaded.iter().any(|n| n == "new_plugin_tool_a"));
        assert!(reloaded.iter().any(|n| n == "new_plugin_tool_b"));

        assert!(!registry.contains("old_plugin_tool"));
        assert!(registry.contains("keep_dynamic"));
        assert!(registry.contains("new_plugin_tool_a"));
        assert!(registry.contains("new_plugin_tool_b"));

        assert!(matches!(
            registry.get_source("new_plugin_tool_a"),
            Some(ToolSource::Plugin { path: p }) if p == path
        ));
    }

    #[tokio::test]
    async fn hot_reload_plugin_rolls_back_on_loader_error() {
        let mut registry = ToolRegistry::new();
        let path = "/plugins/fail.rhai";

        registry
            .register_with_source(
                TestTool::new("existing_plugin_tool").into_dynamic(),
                ToolSource::Plugin {
                    path: path.to_string(),
                },
            )
            .unwrap();

        registry.register_plugin_loader(path, |_plugin_path| {
            Err(AgentError::Other(
                "simulated plugin load failure".to_string(),
            ))
        });

        let err = registry
            .hot_reload_plugin(path)
            .await
            .expect_err("reload should fail");
        assert!(err.to_string().contains("simulated plugin load failure"));

        assert!(registry.contains("existing_plugin_tool"));
        assert!(matches!(
            registry.get_source("existing_plugin_tool"),
            Some(ToolSource::Plugin { path: p }) if p == path
        ));
    }

    #[tokio::test]
    async fn hot_reload_plugin_fails_without_registered_loader() {
        let mut registry = ToolRegistry::new();
        let path = "/plugins/missing_loader.rhai";

        let err = registry
            .hot_reload_plugin(path)
            .await
            .expect_err("reload should fail");
        assert!(matches!(err, AgentError::NotFound(_)));
        assert!(err.to_string().contains("Plugin loader not registered"));
    }

    #[tokio::test]
    async fn register_with_source_rejects_duplicate_tool_names() {
        let mut registry = ToolRegistry::new();

        registry
            .register_with_source(
                TestTool::new("dup_tool").into_dynamic(),
                ToolSource::Builtin,
            )
            .unwrap();

        let err = registry
            .register_with_source(
                TestTool::new("dup_tool").into_dynamic(),
                ToolSource::Dynamic,
            )
            .expect_err("duplicate registration should fail");

        assert!(matches!(err, AgentError::RegistrationFailed(_)));
        assert!(err.to_string().contains("already registered"));
        assert_eq!(registry.count(), 1);
        assert!(matches!(
            registry.get_source("dup_tool"),
            Some(ToolSource::Builtin)
        ));
    }

    #[tokio::test]
    async fn filter_by_source_covers_all_source_types() {
        let mut registry = ToolRegistry::new();

        registry
            .register_with_source(
                TestTool::new("builtin_tool").into_dynamic(),
                ToolSource::Builtin,
            )
            .unwrap();
        registry
            .register_with_source(
                TestTool::new("dynamic_tool").into_dynamic(),
                ToolSource::Dynamic,
            )
            .unwrap();
        registry
            .register_with_source(
                TestTool::new("plugin_tool").into_dynamic(),
                ToolSource::Plugin {
                    path: "/plugins/sample.rhai".to_string(),
                },
            )
            .unwrap();
        registry
            .register_with_source(
                TestTool::new("mcp_tool").into_dynamic(),
                ToolSource::Mcp {
                    endpoint: "mcp://server".to_string(),
                },
            )
            .unwrap();

        let builtin = names_of(registry.filter_by_source("builtin"));
        let dynamic = names_of(registry.filter_by_source("dynamic"));
        let plugin = names_of(registry.filter_by_source("plugin"));
        let mcp = names_of(registry.filter_by_source("mcp"));

        assert_eq!(builtin, HashSet::from([String::from("builtin_tool")]));
        assert_eq!(dynamic, HashSet::from([String::from("dynamic_tool")]));
        assert_eq!(plugin, HashSet::from([String::from("plugin_tool")]));
        assert_eq!(mcp, HashSet::from([String::from("mcp_tool")]));
    }

    #[tokio::test]
    async fn tool_searcher_validates_all_search_apis() {
        let mut registry = ToolRegistry::new();

        registry
            .register_with_source(
                RichTestTool::new(
                    "WebSearch",
                    "Find GitHub repositories quickly",
                    vec!["search", "network"],
                    false,
                    false,
                )
                .into_dynamic(),
                ToolSource::Dynamic,
            )
            .unwrap();

        registry
            .register_with_source(
                RichTestTool::new(
                    "FileDelete",
                    "Delete local files",
                    vec!["filesystem"],
                    false,
                    true,
                )
                .into_dynamic(),
                ToolSource::Dynamic,
            )
            .unwrap();

        registry
            .register_with_source(
                RichTestTool::new(
                    "ShellExec",
                    "Execute shell commands",
                    vec!["shell"],
                    true,
                    false,
                )
                .into_dynamic(),
                ToolSource::Dynamic,
            )
            .unwrap();

        let searcher = ToolSearcher::new(&registry);

        let by_name = names_of(searcher.search_by_name("web"));
        let by_desc = names_of(searcher.search_by_description("github"));
        let by_tag = names_of(searcher.search_by_tag("search"));
        let dangerous = names_of(searcher.search_dangerous());

        assert_eq!(by_name, HashSet::from([String::from("WebSearch")]));
        assert_eq!(by_desc, HashSet::from([String::from("WebSearch")]));
        assert_eq!(by_tag, HashSet::from([String::from("WebSearch")]));
        assert_eq!(
            dangerous,
            HashSet::from([String::from("FileDelete"), String::from("ShellExec")])
        );
    }

    #[tokio::test]
    async fn registry_trait_invariants_hold_across_lifecycle() {
        let mut registry = ToolRegistry::new();

        registry
            .register(TestTool::new("alpha").into_dynamic())
            .unwrap();
        registry
            .register(TestTool::new("beta").into_dynamic())
            .unwrap();

        assert!(registry.contains("alpha"));
        assert!(registry.get("alpha").is_some());

        let listed_names: HashSet<String> = registry.list_names().into_iter().collect();
        let descriptor_names: HashSet<String> = names_of(registry.list());

        assert_eq!(listed_names, descriptor_names);

        registry.unregister("alpha").unwrap();
        assert!(registry.get("alpha").is_none());
        assert!(registry.get_source("alpha").is_none());
    }
}
