//! 工具组件
//! Tool Component
//!
//! 定义统一的工具接口，合并 ToolExecutor 和 ReActTool
//! Defines a unified tool interface, merging ToolExecutor and ReActTool

use crate::agent::context::AgentContext;
use crate::agent::error::{AgentError, AgentResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// 统一工具 Trait
/// Unified Tool Trait
///
/// 合并了 ToolExecutor 和 ReActTool 的功能
/// Merges functionalities of ToolExecutor and ReActTool
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// use mofa_kernel::agent::components::tool::{Tool, ToolInput, ToolResult, ToolMetadata};
///
/// struct Calculator;
///
/// #[async_trait]
/// impl Tool for Calculator {
///     fn name(&self) -> &str { "calculator" }
///     fn description(&self) -> &str { "Perform arithmetic operations" }
///     fn parameters_schema(&self) -> serde_json::Value {
///         serde_json::json!({
///             "type": "object",
///             "properties": {
///                 "operation": { "type": "string", "enum": ["add", "sub", "mul", "div"] },
///                 "a": { "type": "number" },
///                 "b": { "type": "number" }
///             },
///             "required": ["operation", "a", "b"]
///         })
///     }
///
///     async fn execute(&self, input: ToolInput, ctx: &CoreAgentContext) -> ToolResult {
///         // Implementation
///     }
///
///     fn metadata(&self) -> ToolMetadata {
///         ToolMetadata::default()
///     }
/// }
/// ```
#[async_trait]
pub trait Tool<Args = serde_json::Value, Out = serde_json::Value>: Send + Sync
where
    Args: serde::de::DeserializeOwned + Send + Sync + 'static,
    Out: serde::Serialize + Send + Sync + 'static,
{
    /// 工具名称 (唯一标识符)
    /// Tool name (unique identifier)
    fn name(&self) -> &str;

    /// 工具描述 (用于 LLM 理解)
    /// Tool description (for LLM understanding)
    fn description(&self) -> &str;

    /// 参数 JSON Schema
    /// Parameters JSON Schema
    fn parameters_schema(&self) -> serde_json::Value;

    /// 执行工具
    /// Execute tool
    async fn execute(&self, input: ToolInput<Args>, ctx: &AgentContext) -> ToolResult<Out>;

    /// 工具元数据
    /// Tool metadata
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::default()
    }

    /// 验证输入
    /// Validate input
    fn validate_input(&self, input: &ToolInput<Args>) -> AgentResult<()> {
        // 默认不做验证，子类可以覆盖
        // No validation by default, subclasses can override
        let _ = input;
        Ok(())
    }

    /// 是否需要确认
    /// Whether confirmation is required
    fn requires_confirmation(&self) -> bool {
        false
    }

    /// 转换为 LLM Tool 格式
    /// Convert to LLM Tool format
    fn to_llm_tool(&self) -> LLMTool {
        LLMTool {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters_schema(),
        }
    }
}

/// 工具输入
/// Tool Input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInput<Args = serde_json::Value> {
    /// 结构化参数
    /// Structured arguments
    pub arguments: Args,
    /// 原始输入 (可选)
    /// Raw input (optional)
    pub raw_input: Option<String>,
}

impl<Args> ToolInput<Args> {
    /// Create from structured arguments
    pub fn new(arguments: Args) -> Self {
        Self {
            arguments,
            raw_input: None,
        }
    }

    pub fn args(&self) -> &Args {
        &self.arguments
    }
}

impl ToolInput<serde_json::Value> {
    /// 从 JSON 参数创建
    /// Create from JSON arguments
    pub fn from_json(arguments: serde_json::Value) -> Self {
        Self {
            arguments,
            raw_input: None,
        }
    }

    /// 从原始字符串创建
    /// Create from raw string
    pub fn from_raw(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        Self {
            arguments: serde_json::Value::String(raw.clone()),
            raw_input: Some(raw),
        }
    }

    /// 获取参数值
    /// Get parameter value
    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.arguments
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// 获取字符串参数
    /// Get string parameter
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.arguments.get(key).and_then(|v| v.as_str())
    }

    /// 获取数字参数
    /// Get number parameter
    pub fn get_number(&self, key: &str) -> Option<f64> {
        self.arguments.get(key).and_then(|v| v.as_f64())
    }

    /// 获取布尔参数
    /// Get boolean parameter
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.arguments.get(key).and_then(|v| v.as_bool())
    }
}

impl From<serde_json::Value> for ToolInput<serde_json::Value> {
    fn from(v: serde_json::Value) -> Self {
        Self::from_json(v)
    }
}

impl From<String> for ToolInput<serde_json::Value> {
    fn from(s: String) -> Self {
        Self::from_raw(s)
    }
}

impl From<&str> for ToolInput<serde_json::Value> {
    fn from(s: &str) -> Self {
        Self::from_raw(s)
    }
}

/// 工具执行结果
/// Tool Execution Result
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult<Out = serde_json::Value> {
    /// 是否成功
    /// Whether successful
    pub success: bool,
    /// 输出内容
    /// Output content
    pub output: Out,
    /// 错误信息 (如果失败)
    /// Error message (if failed)
    pub error: Option<String>,
    /// 额外元数据
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl<Out> ToolResult<Out> {
    /// 创建成功结果
    /// Create success result
    pub fn success(output: Out) -> Self {
        Self {
            success: true,
            output,
            error: None,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

impl ToolResult<serde_json::Value> {
    /// 创建文本成功结果
    /// Create text success result
    pub fn success_text(text: impl Into<String>) -> Self {
        Self::success(serde_json::Value::String(text.into()))
    }

    /// 创建失败结果
    /// Create failure result
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            output: serde_json::Value::Null,
            error: Some(error.into()),
            metadata: HashMap::new(),
        }
    }

    /// 获取文本输出
    /// Get text output
    pub fn as_text(&self) -> Option<&str> {
        self.output.as_str()
    }

    /// 转换为字符串
    /// Convert to string output
    pub fn to_string_output(&self) -> String {
        if self.success {
            match &self.output {
                serde_json::Value::String(s) => s.clone(),
                v => v.to_string(),
            }
        } else {
            format!(
                "Error: {}",
                self.error.as_deref().unwrap_or("Unknown error")
            )
        }
    }
}

/// 工具元数据
/// Tool Metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolMetadata {
    /// 工具分类
    /// Tool category
    pub category: Option<String>,
    /// 工具标签
    /// Tool tags
    pub tags: Vec<String>,
    /// 是否为危险操作
    /// Whether it is a dangerous operation
    pub is_dangerous: bool,
    /// 是否需要网络
    /// Whether network is required
    pub requires_network: bool,
    /// 是否需要文件系统访问
    /// Whether filesystem access is required
    pub requires_filesystem: bool,
    /// 自定义属性
    /// Custom attributes
    pub custom: HashMap<String, serde_json::Value>,
}

impl ToolMetadata {
    /// 创建新的元数据
    /// Create new metadata
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置分类
    /// Set category
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }

    /// 添加标签
    /// Add tag
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// 标记为危险操作
    /// Mark as dangerous operation
    pub fn dangerous(mut self) -> Self {
        self.is_dangerous = true;
        self
    }

    /// 标记需要网络
    /// Mark as requiring network
    pub fn needs_network(mut self) -> Self {
        self.requires_network = true;
        self
    }

    /// 标记需要文件系统
    /// Mark as requiring filesystem
    pub fn needs_filesystem(mut self) -> Self {
        self.requires_filesystem = true;
        self
    }
}

/// 工具描述符 (用于列表展示)
/// Tool Descriptor (for list display)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    /// 工具名称
    /// Tool name
    pub name: String,
    /// 工具描述
    /// Tool description
    pub description: String,
    /// 参数 Schema
    /// Parameters Schema
    pub parameters_schema: serde_json::Value,
    /// 元数据
    /// Metadata
    pub metadata: ToolMetadata,
}

impl ToolDescriptor {
    /// 从 Tool 创建描述符
    /// Create descriptor from Tool
    pub fn from_tool<Args, Out>(tool: &dyn Tool<Args, Out>) -> Self
    where
        Args: serde::de::DeserializeOwned + Send + Sync + 'static,
        Out: serde::Serialize + Send + Sync + 'static,
    {
        Self {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            parameters_schema: tool.parameters_schema(),
            metadata: tool.metadata(),
        }
    }

    /// Create descriptor from DynTool
    pub fn from_dyn_tool(tool: &dyn DynTool) -> Self {
        Self {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            parameters_schema: tool.parameters_schema(),
            metadata: tool.metadata(),
        }
    }
}

/// LLM Tool 格式 (用于 API 调用)
/// LLM Tool format (for API calls)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMTool {
    /// 工具名称
    /// Tool name
    pub name: String,
    /// 工具描述
    /// Tool description
    pub description: String,
    /// 参数 Schema
    /// Parameters Schema
    pub parameters: serde_json::Value,
}

// ============================================================================
// 工具注册中心 Trait (接口仅在此定义)
// Tool Registry Trait (Interface defined here only)
// ============================================================================

/// Dynamic Tool Interface (for object safety and type erasure)
#[async_trait]
pub trait DynTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    async fn execute_dynamic(
        &self,
        input: serde_json::Value,
        ctx: &AgentContext,
    ) -> AgentResult<serde_json::Value>;
    fn metadata(&self) -> ToolMetadata;
    fn validate_dynamic_input(&self, input: &serde_json::Value) -> AgentResult<()>;
    fn requires_confirmation(&self) -> bool;
    fn to_llm_tool(&self) -> LLMTool;
}

pub struct DynToolWrapper<T, Args, Out> {
    pub tool: Arc<T>,
    pub _phantom: std::marker::PhantomData<(Args, Out)>,
}

pub trait ToolExt<Args, Out>: Tool<Args, Out> + Sized + Send + Sync + 'static
where
    Args: serde::de::DeserializeOwned + Send + Sync + 'static,
    Out: serde::Serialize + Send + Sync + 'static,
{
    fn into_dynamic(self) -> std::sync::Arc<dyn DynTool> {
        std::sync::Arc::new(DynToolWrapper {
            tool: std::sync::Arc::new(self),
            _phantom: std::marker::PhantomData,
        })
    }
}
impl<T, Args, Out> ToolExt<Args, Out> for T
where
    T: Tool<Args, Out> + Send + Sync + 'static,
    Args: serde::de::DeserializeOwned + Send + Sync + 'static,
    Out: serde::Serialize + Send + Sync + 'static,
{
}

#[async_trait]
impl<T, Args, Out> DynTool for DynToolWrapper<T, Args, Out>
where
    T: Tool<Args, Out> + Send + Sync,
    Args: serde::de::DeserializeOwned + Send + Sync + 'static,
    Out: serde::Serialize + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        Tool::name(&*self.tool)
    }
    fn description(&self) -> &str {
        Tool::description(&*self.tool)
    }
    fn parameters_schema(&self) -> serde_json::Value {
        Tool::parameters_schema(&*self.tool)
    }
    fn metadata(&self) -> ToolMetadata {
        Tool::metadata(&*self.tool)
    }
    fn requires_confirmation(&self) -> bool {
        Tool::requires_confirmation(&*self.tool)
    }
    fn to_llm_tool(&self) -> LLMTool {
        Tool::to_llm_tool(&*self.tool)
    }

    fn validate_dynamic_input(&self, input: &serde_json::Value) -> AgentResult<()> {
        let args: Args = serde_json::from_value(input.clone()).map_err(|e| {
            AgentError::InvalidInput(format!("Tool {} args mapping error: {}", self.name(), e))
        })?;
        (*self.tool).validate_input(&ToolInput::new(args))
    }

    async fn execute_dynamic(
        &self,
        input: serde_json::Value,
        ctx: &AgentContext,
    ) -> AgentResult<serde_json::Value> {
        let args: Args = serde_json::from_value(input).map_err(|e| {
            AgentError::InvalidInput(format!("Tool {} args mapping error: {}", self.name(), e))
        })?;
        let tool_input = ToolInput::new(args);
        let result = (*self.tool).execute(tool_input, ctx).await;
        if !result.success {
            return Err(AgentError::ToolExecutionFailed {
                tool_name: self.name().to_string(),
                message: result.error.unwrap_or_default(),
            });
        }
        serde_json::to_value(result.output).map_err(|e| {
            AgentError::ExecutionFailed(format!(
                "Tool {} output serialize error: {}",
                self.name(),
                e
            ))
        })
    }
}

/// 定义工具注册的接口，具体实现在 foundation 层。
/// Defines the tool registration interface, with concrete implementation in the foundation layer.
#[async_trait]
pub trait ToolRegistry: Send + Sync {
    /// 注册工具
    /// Register tool
    fn register(&mut self, tool: Arc<dyn DynTool>) -> AgentResult<()>;

    /// 批量注册工具
    /// Batch register tools
    fn register_all(&mut self, tools: Vec<Arc<dyn DynTool>>) -> AgentResult<()> {
        for tool in tools {
            self.register(tool)?;
        }
        Ok(())
    }

    /// 获取工具
    /// Get tool
    fn get(&self, name: &str) -> Option<Arc<dyn DynTool>>;

    /// 移除工具
    /// Remove tool
    fn unregister(&mut self, name: &str) -> AgentResult<bool>;

    /// 列出所有工具
    /// List all tools
    fn list(&self) -> Vec<ToolDescriptor>;

    /// 列出所有工具名称
    /// List all tool names
    fn list_names(&self) -> Vec<String>;

    /// 检查工具是否存在
    /// Check if tool exists
    fn contains(&self, name: &str) -> bool;

    /// 获取工具数量
    /// Get tool count
    fn count(&self) -> usize;

    /// 执行工具
    /// Execute tool
    async fn execute<Args, Out>(
        &self,
        name: &str,
        input: ToolInput<Args>,
        ctx: &AgentContext,
    ) -> AgentResult<ToolResult<Out>>
    where
        Args: serde::Serialize + Send + Sync + 'static,
        Out: serde::de::DeserializeOwned + Send + Sync + 'static,
    {
        let tool = self
            .get(name)
            .ok_or_else(|| AgentError::ToolNotFound(name.to_string()))?;

        let json_input = serde_json::to_value(&input.arguments).map_err(|e| {
            AgentError::InvalidInput(format!("Failed to serialize args for tool {}: {}", name, e))
        })?;

        let json_output = tool.execute_dynamic(json_input, ctx).await?;

        let output: Out = serde_json::from_value(json_output).map_err(|e| {
            AgentError::ExecutionFailed(format!(
                "Failed to deserialize output from tool {}: {}",
                name, e
            ))
        })?;

        Ok(ToolResult::success(output))
    }

    /// 转换为 LLM Tools
    /// Convert to LLM Tools
    fn to_llm_tools(&self) -> Vec<LLMTool> {
        self.list()
            .iter()
            .map(|d| LLMTool {
                name: d.name.clone(),
                description: d.description.clone(),
                parameters: d.parameters_schema.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::context::AgentContext;

    #[test]
    fn test_tool_input_from_json() {
        let input = ToolInput::from_json(serde_json::json!({
            "name": "test",
            "count": 42
        }));

        assert_eq!(input.get_str("name"), Some("test"));
        assert_eq!(input.get_number("count"), Some(42.0));
    }

    #[test]
    fn test_tool_result() {
        let success = ToolResult::success_text("OK");
        assert!(success.success);
        assert_eq!(success.as_text(), Some("OK"));

        let failure = ToolResult::failure("Something went wrong");
        assert!(!failure.success);
        assert!(failure.error.is_some());
    }
}
