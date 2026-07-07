use super::*;
use serde_json::json;

/// JSON 处理工具 - JSON 解析和操作
/// JSON processing utilities - JSON parsing and manipulation
pub struct JsonTool {
    definition: ToolDefinition,
}

impl Default for JsonTool {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonTool {
    pub fn new() -> Self {
        Self {
            definition: ToolDefinition {
                name: "json".to_string(),
                description: "JSON operations: parse, stringify, query with JSONPath-like syntax, merge objects.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "operation": {
                            "type": "string",
                            "enum": ["parse", "stringify", "get", "set", "merge", "keys", "values"],
                            "description": "JSON operation to perform"
                        },
                        "data": {
                            "description": "JSON data (string for parse, object/array for others)"
                        },
                        "path": {
                            "type": "string",
                            "description": "Dot-notation path for get/set operations (e.g., 'user.name')"
                        },
                        "value": {
                            "description": "Value to set (for set operation)"
                        },
                        "other": {
                            "type": "object",
                            "description": "Object to merge with (for merge operation)"
                        }
                    },
                    "required": ["operation", "data"]
                }),
                requires_confirmation: false,
            },
        }
    }

    fn get_by_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = value;

        for part in parts {
            if part.is_empty() {
                continue;
            }
            // Check if it's an array index
            if let Ok(index) = part.parse::<usize>() {
                current = current.get(index)?;
            } else {
                current = current.get(part)?;
            }
        }

        Some(current)
    }

    fn set_by_path(
        value: &mut serde_json::Value,
        path: &str,
        new_value: serde_json::Value,
    ) -> PluginResult<()> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = value;

        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                // Last part - set the value
                if let Ok(index) = part.parse::<usize>() {
                    if let Some(arr) = current.as_array_mut()
                        && index < arr.len()
                    {
                        arr[index] = new_value;
                        return Ok(());
                    }
                } else if let Some(obj) = current.as_object_mut() {
                    obj.insert(part.to_string(), new_value);
                    return Ok(());
                }
                return Err(mofa_kernel::plugin::PluginError::ExecutionFailed(
                    "Cannot set value at path".to_string(),
                ));
            } else {
                // Navigate to next level
                if let Ok(index) = part.parse::<usize>() {
                    current = current.get_mut(index).ok_or_else(|| {
                        mofa_kernel::plugin::PluginError::ExecutionFailed(
                            "Invalid path".to_string(),
                        )
                    })?;
                } else {
                    current = current.get_mut(*part).ok_or_else(|| {
                        mofa_kernel::plugin::PluginError::ExecutionFailed(
                            "Invalid path".to_string(),
                        )
                    })?;
                }
            }
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl ToolExecutor for JsonTool {
    fn definition(&self) -> &ToolDefinition {
        &self.definition
    }

    async fn execute(&self, arguments: serde_json::Value) -> PluginResult<serde_json::Value> {
        let operation = arguments["operation"].as_str().ok_or_else(|| {
            mofa_kernel::plugin::PluginError::ExecutionFailed("Operation is required".to_string())
        })?;

        match operation {
            "parse" => {
                let data = arguments["data"].as_str().ok_or_else(|| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(
                        "String data is required for parse".to_string(),
                    )
                })?;
                let parsed: serde_json::Value = serde_json::from_str(data)?;
                Ok(json!({
                    "success": true,
                    "result": parsed
                }))
            }
            "stringify" => {
                let data = &arguments["data"];
                let pretty = arguments
                    .get("pretty")
                    .and_then(|p| p.as_bool())
                    .unwrap_or(true);
                let result = if pretty {
                    serde_json::to_string_pretty(data)?
                } else {
                    serde_json::to_string(data)?
                };
                Ok(json!({
                    "success": true,
                    "result": result
                }))
            }
            "get" => {
                let path = arguments["path"].as_str().ok_or_else(|| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(
                        "Path is required for get operation".to_string(),
                    )
                })?;
                let data = &arguments["data"];
                let result = Self::get_by_path(data, path);
                Ok(json!({
                    "success": result.is_some(),
                    "result": result
                }))
            }
            "set" => {
                let path = arguments["path"].as_str().ok_or_else(|| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(
                        "Path is required for set operation".to_string(),
                    )
                })?;
                let value = arguments
                    .get("value")
                    .ok_or_else(|| {
                        mofa_kernel::plugin::PluginError::ExecutionFailed(
                            "Value is required for set operation".to_string(),
                        )
                    })?
                    .clone();
                let mut data = arguments["data"].clone();
                Self::set_by_path(&mut data, path, value)?;
                Ok(json!({
                    "success": true,
                    "result": data
                }))
            }
            "merge" => {
                let mut data = arguments["data"]
                    .as_object()
                    .ok_or_else(|| {
                        mofa_kernel::plugin::PluginError::ExecutionFailed(
                            "Data must be an object for merge".to_string(),
                        )
                    })?
                    .clone();
                let other = arguments["other"].as_object().ok_or_else(|| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(
                        "Other must be an object for merge".to_string(),
                    )
                })?;
                for (k, v) in other {
                    data.insert(k.clone(), v.clone());
                }
                Ok(json!({
                    "success": true,
                    "result": data
                }))
            }
            "keys" => {
                let data = arguments["data"].as_object().ok_or_else(|| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(
                        "Data must be an object for keys".to_string(),
                    )
                })?;
                let keys: Vec<&String> = data.keys().collect();
                Ok(json!({
                    "success": true,
                    "result": keys
                }))
            }
            "values" => {
                let data = arguments["data"].as_object().ok_or_else(|| {
                    mofa_kernel::plugin::PluginError::ExecutionFailed(
                        "Data must be an object for values".to_string(),
                    )
                })?;
                let values: Vec<&serde_json::Value> = data.values().collect();
                Ok(json!({
                    "success": true,
                    "result": values
                }))
            }
            _ => Err(mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "Unknown operation: {}",
                operation
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── get_by_path ──────────────────────────────────────────────────────

    #[test]
    fn get_by_path_simple_key() {
        let data = json!({"name": "alice"});
        assert_eq!(JsonTool::get_by_path(&data, "name"), Some(&json!("alice")));
    }

    #[test]
    fn get_by_path_nested() {
        let data = json!({"user": {"name": "bob", "age": 30}});
        assert_eq!(JsonTool::get_by_path(&data, "user.name"), Some(&json!("bob")));
    }

    #[test]
    fn get_by_path_deeply_nested() {
        let data = json!({"a": {"b": {"c": {"d": 42}}}});
        assert_eq!(JsonTool::get_by_path(&data, "a.b.c.d"), Some(&json!(42)));
    }

    #[test]
    fn get_by_path_array_index() {
        let data = json!({"items": ["x", "y", "z"]});
        assert_eq!(JsonTool::get_by_path(&data, "items.1"), Some(&json!("y")));
    }

    #[test]
    fn get_by_path_missing_key_returns_none() {
        let data = json!({"name": "alice"});
        assert_eq!(JsonTool::get_by_path(&data, "age"), None);
    }

    #[test]
    fn get_by_path_missing_nested_returns_none() {
        let data = json!({"user": {"name": "alice"}});
        assert_eq!(JsonTool::get_by_path(&data, "user.email"), None);
    }

    #[test]
    fn get_by_path_empty_path_returns_root() {
        let data = json!({"key": "val"});
        assert_eq!(JsonTool::get_by_path(&data, ""), Some(&data));
    }

    #[test]
    fn get_by_path_array_out_of_bounds() {
        let data = json!({"items": [1, 2]});
        assert_eq!(JsonTool::get_by_path(&data, "items.5"), None);
    }

    // ── set_by_path ──────────────────────────────────────────────────────

    #[test]
    fn set_by_path_overwrite_existing() {
        let mut data = json!({"name": "alice"});
        JsonTool::set_by_path(&mut data, "name", json!("bob")).unwrap();
        assert_eq!(data["name"], json!("bob"));
    }

    #[test]
    fn set_by_path_nested() {
        let mut data = json!({"user": {"name": "alice"}});
        JsonTool::set_by_path(&mut data, "user.name", json!("charlie")).unwrap();
        assert_eq!(data["user"]["name"], json!("charlie"));
    }

    #[test]
    fn set_by_path_add_new_key() {
        let mut data = json!({"name": "alice"});
        JsonTool::set_by_path(&mut data, "age", json!(25)).unwrap();
        assert_eq!(data["age"], json!(25));
    }

    #[test]
    fn set_by_path_array_index() {
        let mut data = json!({"items": ["a", "b", "c"]});
        JsonTool::set_by_path(&mut data, "items.1", json!("B")).unwrap();
        assert_eq!(data["items"][1], json!("B"));
    }

    #[test]
    fn set_by_path_invalid_intermediate_returns_error() {
        let mut data = json!({"name": "alice"});
        let result = JsonTool::set_by_path(&mut data, "address.street", json!("123 Main"));
        assert!(result.is_err());
    }

    // ── execute: parse ───────────────────────────────────────────────────

    #[tokio::test]
    async fn execute_parse_valid_json() {
        let tool = JsonTool::new();
        let result = tool
            .execute(json!({"operation": "parse", "data": r#"{"key":"value"}"#}))
            .await
            .unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["result"]["key"], "value");
    }

    #[tokio::test]
    async fn execute_parse_invalid_json_returns_error() {
        let tool = JsonTool::new();
        let result = tool
            .execute(json!({"operation": "parse", "data": "not json {"}))
            .await;
        assert!(result.is_err());
    }

    // ── execute: stringify ───────────────────────────────────────────────

    #[tokio::test]
    async fn execute_stringify_compact() {
        let tool = JsonTool::new();
        let result = tool
            .execute(json!({"operation": "stringify", "data": {"k": "v"}, "pretty": false}))
            .await
            .unwrap();
        assert_eq!(result["success"], true);
        let s = result["result"].as_str().unwrap();
        // Compact has no newlines
        assert!(!s.contains('\n'));
        assert!(s.contains("\"k\""));
    }

    // ── execute: get ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn execute_get_found() {
        let tool = JsonTool::new();
        let result = tool
            .execute(json!({"operation": "get", "data": {"user": {"name": "alice"}}, "path": "user.name"}))
            .await
            .unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["result"], "alice");
    }

    #[tokio::test]
    async fn execute_get_not_found() {
        let tool = JsonTool::new();
        let result = tool
            .execute(json!({"operation": "get", "data": {"user": {}}, "path": "user.email"}))
            .await
            .unwrap();
        assert_eq!(result["success"], false);
    }

    // ── execute: set ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn execute_set_returns_modified_data() {
        let tool = JsonTool::new();
        let result = tool
            .execute(json!({"operation": "set", "data": {"a": 1}, "path": "a", "value": 99}))
            .await
            .unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["result"]["a"], 99);
    }

    // ── execute: merge ───────────────────────────────────────────────────

    #[tokio::test]
    async fn execute_merge_combines_objects() {
        let tool = JsonTool::new();
        let result = tool
            .execute(json!({"operation": "merge", "data": {"a": 1}, "other": {"b": 2, "c": 3}}))
            .await
            .unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["result"]["a"], 1);
        assert_eq!(result["result"]["b"], 2);
        assert_eq!(result["result"]["c"], 3);
    }

    #[tokio::test]
    async fn execute_merge_overwrites_existing_keys() {
        let tool = JsonTool::new();
        let result = tool
            .execute(json!({"operation": "merge", "data": {"a": 1, "b": 2}, "other": {"b": 99}}))
            .await
            .unwrap();
        assert_eq!(result["result"]["b"], 99);
    }

    // ── execute: keys / values ───────────────────────────────────────────

    #[tokio::test]
    async fn execute_keys() {
        let tool = JsonTool::new();
        let result = tool
            .execute(json!({"operation": "keys", "data": {"x": 1, "y": 2}}))
            .await
            .unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["result"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn execute_values() {
        let tool = JsonTool::new();
        let result = tool
            .execute(json!({"operation": "values", "data": {"a": 10, "b": 20}}))
            .await
            .unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["result"].as_array().unwrap().len(), 2);
    }

    // ── execute: error cases ─────────────────────────────────────────────

    #[tokio::test]
    async fn execute_unknown_operation_returns_error() {
        let tool = JsonTool::new();
        let result = tool.execute(json!({"operation": "explode", "data": {}})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn execute_missing_operation_returns_error() {
        let tool = JsonTool::new();
        let result = tool.execute(json!({"data": {}})).await;
        assert!(result.is_err());
    }

    // ── definition ───────────────────────────────────────────────────────

    #[test]
    fn definition_has_correct_name() {
        let tool = JsonTool::new();
        assert_eq!(tool.definition().name, "json");
    }

    #[test]
    fn default_creates_same_as_new() {
        let a = JsonTool::new();
        let b = JsonTool::default();
        assert_eq!(a.definition().name, b.definition().name);
    }
}
