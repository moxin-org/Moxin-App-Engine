//! Built-in tools for MoFA agents.
//!
//! Provides a library of practical tools that agents can use out of the box:
//!
//! - [`HttpTool`]: make HTTP GET/POST requests and return the response body.
//! - [`FileReadTool`]: read the contents of a local file.
//! - [`FileWriteTool`]: write or append text to a local file.
//! - [`ShellTool`]: execute a shell command and capture stdout/stderr.
//! - [`JsonParseTool`]: parse JSON, query nested keys, and list keys/values.
//! - [`DateTimeTool`]: get the current time, format timestamps, and calculate differences.
//!
//! All tools implement the kernel [`Tool`] trait through the [`SimpleTool`] convenience
//! trait (via [`SimpleToolAdapter`]) so they slot into any [`ToolRegistry`].
//!
//! # Example
//!
//! ```rust,ignore
//! use mofa_foundation::agent::tools::builtin::{DateTimeTool, FileReadTool, HttpTool};
//! use mofa_foundation::agent::{SimpleToolRegistry, as_tool};
//! use mofa_kernel::agent::components::tool::ToolRegistry;
//!
//! let mut registry = SimpleToolRegistry::new();
//! registry.register(as_tool(DateTimeTool)).unwrap();
//! registry.register(as_tool(FileReadTool)).unwrap();
//! registry.register(as_tool(HttpTool)).unwrap();
//! ```

use async_trait::async_trait;
use chrono::{DateTime, Duration, TimeZone, Utc};
use mofa_kernel::agent::components::tool::{ToolInput, ToolMetadata, ToolResult};
use serde_json::json;
use tokio::io::AsyncWriteExt as _;

use crate::agent::components::tool::{SimpleTool, ToolCategory};

// ============================================================================
// HttpTool
// ============================================================================

/// Shared HTTP client for connection pool reuse across all invocations.
/// reqwest::Client holds an internal connection pool, DNS resolver, and TLS cache
/// that are expensive to recreate. This static ensures keep-alive reuse and efficient
/// resource management across the lifetime of the agent.
static HTTP_CLIENT: once_cell::sync::Lazy<reqwest::Client> =
    once_cell::sync::Lazy::new(|| reqwest::Client::new());

/// Make HTTP GET or POST requests and return the response body.
///
/// Requires network access. The LLM supplies the URL, method, optional headers,
/// and an optional request body.
///
/// | Parameter | Type   | Required | Description                            |
/// |-----------|--------|----------|----------------------------------------|
/// | `url`     | string | yes      | Full URL to request                    |
/// | `method`  | string | no       | `"GET"` (default) or `"POST"` etc.     |
/// | `body`    | string | no       | Request body for POST/PUT/PATCH        |
/// | `headers` | object | no       | Additional headers as key→value pairs  |
#[derive(Debug)]
pub struct HttpTool;

#[async_trait]
impl SimpleTool for HttpTool {
    fn name(&self) -> &str {
        "http_request"
    }

    fn description(&self) -> &str {
        "Make an HTTP GET or POST request to a URL and return the response body. \
         Use this to call external APIs or fetch web content."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The full URL to request (e.g. https://api.example.com/data)"
                },
                "method": {
                    "type": "string",
                    "enum": ["GET", "POST", "PUT", "DELETE", "PATCH"],
                    "description": "HTTP method. Defaults to GET."
                },
                "body": {
                    "type": "string",
                    "description": "Request body (for POST/PUT/PATCH)"
                },
                "headers": {
                    "type": "object",
                    "description": "Additional request headers as key-value pairs",
                    "additionalProperties": { "type": "string" }
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let url = match input.get_str("url") {
            Some(u) => u.to_string(),
            None => return ToolResult::failure("missing required parameter: url"),
        };

        let method = input.get_str("method").unwrap_or("GET").to_uppercase();
        let client = &*HTTP_CLIENT;

        let mut builder = match method.as_str() {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            "PATCH" => client.patch(&url),
            other => return ToolResult::failure(format!("unsupported HTTP method: {other}")),
        };

        // Optional headers
        if let Some(headers) = input.arguments.get("headers").and_then(|v| v.as_object()) {
            for (key, val) in headers {
                if let Some(v) = val.as_str()
                    && let (Ok(name), Ok(value)) = (
                        reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                        reqwest::header::HeaderValue::from_str(v),
                    )
                {
                    builder = builder.header(name, value);
                }
            }
        }

        // Optional request body
        if let Some(body) = input.get_str("body") {
            builder = builder.body(body.to_string());
        }

        match builder.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let text = resp.text().await.unwrap_or_default();
                if status < 400 {
                    ToolResult::success(json!({ "status": status, "body": text }))
                } else {
                    ToolResult::failure(format!("HTTP {status}: {text}"))
                }
            }
            Err(e) => ToolResult::failure(format!("request failed: {e}")),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::new().needs_network()
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Web
    }
}

// ============================================================================
// FileReadTool
// ============================================================================

/// Read the entire contents of a local file and return them as text.
///
/// | Parameter | Type   | Required | Description         |
/// |-----------|--------|----------|---------------------|
/// | `path`    | string | yes      | Path to the file    |
#[derive(Debug)]
pub struct FileReadTool;

#[async_trait]
impl SimpleTool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read the contents of a local file and return them as text."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file to read"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let path = match input.get_str("path") {
            Some(p) => p.to_string(),
            None => return ToolResult::failure("missing required parameter: path"),
        };

        match tokio::fs::read_to_string(&path).await {
            Ok(contents) => {
                let bytes = contents.len();
                ToolResult::success(json!({
                    "path": path,
                    "content": contents,
                    "bytes": bytes
                }))
            }
            Err(e) => ToolResult::failure(format!("failed to read '{path}': {e}")),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::new().needs_filesystem()
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::File
    }
}

// ============================================================================
// FileWriteTool
// ============================================================================

/// Write or append text to a local file.
///
/// | Parameter  | Type    | Required | Description                                   |
/// |------------|---------|----------|-----------------------------------------------|
/// | `path`     | string  | yes      | Path to the file                              |
/// | `content`  | string  | yes      | Text to write                                 |
/// | `append`   | boolean | no       | If `true`, appends instead of overwriting     |
#[derive(Debug)]
pub struct FileWriteTool;

#[async_trait]
impl SimpleTool for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Write or append text to a local file. \
         Set append=true to add to the end of an existing file rather than overwriting it."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file"
                },
                "content": {
                    "type": "string",
                    "description": "Text content to write"
                },
                "append": {
                    "type": "boolean",
                    "description": "If true, append to the file instead of overwriting. Defaults to false."
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let path = match input.get_str("path") {
            Some(p) => p.to_string(),
            None => return ToolResult::failure("missing required parameter: path"),
        };
        let content = match input.get_str("content") {
            Some(c) => c.to_string(),
            None => return ToolResult::failure("missing required parameter: content"),
        };
        let append = input.get_bool("append").unwrap_or(false);
        let bytes_written = content.len();

        let write_result: std::io::Result<()> = if append {
            match tokio::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&path)
                .await
            {
                Ok(mut file) => {
                    // Ensure appended bytes are flushed and committed before returning success.
                    if let Err(e) = file.write_all(content.as_bytes()).await {
                        Err(e)
                    } else if let Err(e) = file.flush().await {
                        Err(e)
                    } else {
                        file.sync_all().await
                    }
                }
                Err(e) => Err(e),
            }
        } else {
            tokio::fs::write(&path, content.as_bytes()).await
        };

        match write_result {
            Ok(()) => ToolResult::success(json!({
                "path": path,
                "bytes_written": bytes_written,
                "mode": if append { "append" } else { "overwrite" }
            })),
            Err(e) => ToolResult::failure(format!("failed to write '{path}': {e}")),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::new().needs_filesystem()
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::File
    }
}

// ============================================================================
// ShellTool
// ============================================================================

/// Execute a shell command and return stdout and stderr.
///
/// Marked `is_dangerous = true` in metadata so callers can gate on confirmation
/// before invoking.
///
/// | Parameter | Type            | Required | Description                          |
/// |-----------|-----------------|----------|--------------------------------------|
/// | `command` | string          | yes      | The executable to run                |
/// | `args`    | array of string | no       | Arguments to pass to the command     |
/// | `cwd`     | string          | no       | Working directory for the process    |
#[derive(Debug)]
pub struct ShellTool;

#[async_trait]
impl SimpleTool for ShellTool {
    fn name(&self) -> &str {
        "shell_exec"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return its stdout and stderr. \
         This tool is marked dangerous — only call it when explicitly required."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command or executable to run"
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Command-line arguments (optional)"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory in which to run the command (optional)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let command = match input.get_str("command") {
            Some(c) => c.to_string(),
            None => return ToolResult::failure("missing required parameter: command"),
        };

        let mut cmd = tokio::process::Command::new(&command);

        if let Some(args) = input.arguments.get("args").and_then(|v| v.as_array()) {
            for arg in args {
                if let Some(s) = arg.as_str() {
                    cmd.arg(s);
                }
            }
        }

        if let Some(cwd) = input.get_str("cwd") {
            cmd.current_dir(cwd);
        }

        match cmd.output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                if output.status.success() {
                    ToolResult::success(json!({
                        "exit_code": exit_code,
                        "stdout": stdout,
                        "stderr": stderr
                    }))
                } else {
                    ToolResult::failure(format!(
                        "command exited with code {exit_code}. stderr: {stderr}"
                    ))
                    .with_metadata("stdout", stdout)
                    .with_metadata("exit_code", exit_code.to_string())
                }
            }
            Err(e) => ToolResult::failure(format!("failed to run '{command}': {e}")),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::new().needs_filesystem().dangerous()
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }
}

// ============================================================================
// JsonParseTool
// ============================================================================

/// Parse JSON and optionally query nested values using dot-notation paths.
///
/// | Parameter   | Type   | Required | Description                                          |
/// |-------------|--------|----------|------------------------------------------------------|
/// | `json`      | string | yes      | Raw JSON string to parse                             |
/// | `operation` | string | no       | `"parse"` (default), `"query"`, `"keys"`, `"values"` |
/// | `path`      | string | no       | Dot-notation path for `"query"` (e.g. `"a.b.c"`)    |
///
/// ### Operations
///
/// - **parse** — return the parsed JSON value as-is.
/// - **query** — traverse with a dot-notation path and return the value at that node.
/// - **keys** — return an array of top-level keys (object only).
/// - **values** — return an array of top-level values (object only).
#[derive(Debug)]
pub struct JsonParseTool;

#[async_trait]
impl SimpleTool for JsonParseTool {
    fn name(&self) -> &str {
        "json_parse"
    }

    fn description(&self) -> &str {
        "Parse a JSON string and optionally query it using a dot-notation path \
         (e.g. \"user.address.city\"). \
         Supported operations: parse, query, keys, values."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "json": {
                    "type": "string",
                    "description": "The JSON string to parse"
                },
                "operation": {
                    "type": "string",
                    "enum": ["parse", "query", "keys", "values"],
                    "description": "What to do with the parsed JSON. Defaults to 'parse'."
                },
                "path": {
                    "type": "string",
                    "description": "Dot-notation path for 'query' (e.g. \"user.name\")"
                }
            },
            "required": ["json"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let json_str = match input.get_str("json") {
            Some(s) => s,
            None => return ToolResult::failure("missing required parameter: json"),
        };

        let parsed: serde_json::Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(e) => return ToolResult::failure(format!("invalid JSON: {e}")),
        };

        let operation = input.get_str("operation").unwrap_or("parse");

        match operation {
            "parse" => ToolResult::success(parsed),

            "query" => {
                let path = match input.get_str("path") {
                    Some(p) => p,
                    None => return ToolResult::failure("'query' operation requires 'path'"),
                };

                let mut current = &parsed;
                for key in path.split('.') {
                    current = match current.get(key) {
                        Some(v) => v,
                        None => {
                            return ToolResult::failure(format!("key not found at path '{path}'"));
                        }
                    };
                }
                ToolResult::success(current.clone())
            }

            "keys" => match parsed.as_object() {
                Some(obj) => {
                    let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
                    ToolResult::success(json!(keys))
                }
                None => ToolResult::failure("'keys' requires the JSON to be an object"),
            },

            "values" => match parsed.as_object() {
                Some(obj) => {
                    let values: Vec<&serde_json::Value> = obj.values().collect();
                    ToolResult::success(json!(values))
                }
                None => ToolResult::failure("'values' requires the JSON to be an object"),
            },

            other => ToolResult::failure(format!("unknown operation: '{other}'")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::General
    }
}

// ============================================================================
// DateTimeTool
// ============================================================================

/// Date/time utilities: current time, formatting, and difference calculations.
///
/// | Parameter    | Type    | Required | Description                                           |
/// |--------------|---------|----------|-------------------------------------------------------|
/// | `operation`  | string  | yes      | `"now"`, `"format"`, `"diff"`, `"add"`               |
/// | `timestamp`  | integer | depends  | Unix timestamp in seconds (required for format/diff/add) |
/// | `timestamp2` | integer | no       | Second timestamp for `"diff"`                         |
/// | `format`     | string  | no       | strftime format string. Defaults to RFC 3339.         |
/// | `unit`       | string  | no       | `"seconds"`, `"minutes"`, `"hours"`, `"days"`         |
/// | `amount`     | integer | no       | Amount to add (for `"add"` operation)                 |
///
/// ### Operations
///
/// - **now** — return current UTC timestamp and ISO 8601 string.
/// - **format** — format a Unix timestamp with a strftime format string.
/// - **diff** — difference between two timestamps in the given unit.
/// - **add** — add an amount of time to a timestamp.
#[derive(Debug)]
pub struct DateTimeTool;

#[async_trait]
impl SimpleTool for DateTimeTool {
    fn name(&self) -> &str {
        "datetime"
    }

    fn description(&self) -> &str {
        "Date and time utilities. \
         Get current time (now), format a Unix timestamp, calculate the difference \
         between two timestamps, or add an offset to a timestamp."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["now", "format", "diff", "add"],
                    "description": "The date/time operation to perform"
                },
                "timestamp": {
                    "type": "integer",
                    "description": "Unix timestamp in seconds (required for format, diff, add)"
                },
                "timestamp2": {
                    "type": "integer",
                    "description": "Second Unix timestamp for the 'diff' operation"
                },
                "format": {
                    "type": "string",
                    "description": "strftime format string (e.g. \"%Y-%m-%d %H:%M:%S\"). Defaults to RFC 3339."
                },
                "unit": {
                    "type": "string",
                    "enum": ["seconds", "minutes", "hours", "days"],
                    "description": "Time unit for diff/add. Defaults to 'seconds'."
                },
                "amount": {
                    "type": "integer",
                    "description": "Amount to add (positive or negative) for the 'add' operation"
                }
            },
            "required": ["operation"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let operation = match input.get_str("operation") {
            Some(op) => op.to_string(),
            None => return ToolResult::failure("missing required parameter: operation"),
        };

        match operation.as_str() {
            "now" => {
                let now = Utc::now();
                ToolResult::success(json!({
                    "timestamp": now.timestamp(),
                    "iso8601": now.to_rfc3339(),
                    "date": now.format("%Y-%m-%d").to_string(),
                    "time": now.format("%H:%M:%S").to_string()
                }))
            }

            "format" => {
                let ts = match input.get_number("timestamp") {
                    Some(t) => t as i64,
                    None => return ToolResult::failure("'format' requires 'timestamp'"),
                };
                let dt: DateTime<Utc> = match Utc.timestamp_opt(ts, 0) {
                    chrono::LocalResult::Single(d) => d,
                    _ => return ToolResult::failure(format!("invalid timestamp: {ts}")),
                };
                let formatted = if let Some(fmt) = input.get_str("format") {
                    dt.format(fmt).to_string()
                } else {
                    dt.to_rfc3339()
                };
                ToolResult::success(json!({ "timestamp": ts, "formatted": formatted }))
            }

            "diff" => {
                let ts1 = match input.get_number("timestamp") {
                    Some(t) => t as i64,
                    None => return ToolResult::failure("'diff' requires 'timestamp'"),
                };
                let ts2 = match input.get_number("timestamp2") {
                    Some(t) => t as i64,
                    None => return ToolResult::failure("'diff' requires 'timestamp2'"),
                };
                let diff_secs = (ts2 - ts1).abs();
                let unit = input.get_str("unit").unwrap_or("seconds");
                let value = match unit {
                    "seconds" => diff_secs,
                    "minutes" => diff_secs / 60,
                    "hours" => diff_secs / 3600,
                    "days" => diff_secs / 86400,
                    other => return ToolResult::failure(format!("unknown unit: '{other}'")),
                };
                ToolResult::success(json!({
                    "diff": value,
                    "unit": unit,
                    "diff_seconds": diff_secs
                }))
            }

            "add" => {
                let ts = match input.get_number("timestamp") {
                    Some(t) => t as i64,
                    None => return ToolResult::failure("'add' requires 'timestamp'"),
                };
                let amount = match input.get_number("amount") {
                    Some(a) => a as i64,
                    None => return ToolResult::failure("'add' requires 'amount'"),
                };
                let unit = input.get_str("unit").unwrap_or("seconds");
                let duration = match unit {
                    "seconds" => Duration::try_seconds(amount),
                    "minutes" => Duration::try_minutes(amount),
                    "hours" => Duration::try_hours(amount),
                    "days" => Duration::try_days(amount),
                    other => return ToolResult::failure(format!("unknown unit: '{other}'")),
                };
                let duration = match duration {
                    Some(d) => d,
                    None => return ToolResult::failure("duration overflow"),
                };
                let dt: DateTime<Utc> = match Utc.timestamp_opt(ts, 0) {
                    chrono::LocalResult::Single(d) => d,
                    _ => return ToolResult::failure(format!("invalid timestamp: {ts}")),
                };
                let result = dt + duration;
                ToolResult::success(json!({
                    "original_timestamp": ts,
                    "result_timestamp": result.timestamp(),
                    "result_iso8601": result.to_rfc3339(),
                    "added": amount,
                    "unit": unit
                }))
            }

            other => ToolResult::failure(format!("unknown operation: '{other}'")),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::General
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- DateTimeTool ----

    #[tokio::test]
    async fn test_datetime_now() {
        let result = DateTimeTool
            .execute(ToolInput::from_json(json!({"operation": "now"})))
            .await;
        assert!(result.success, "expected success, got: {:?}", result.error);
        assert!(result.output.get("timestamp").is_some());
        assert!(result.output.get("iso8601").is_some());
    }

    #[tokio::test]
    async fn test_datetime_format_epoch() {
        let result = DateTimeTool
            .execute(ToolInput::from_json(json!({
                "operation": "format",
                "timestamp": 0,
                "format": "%Y-%m-%d"
            })))
            .await;
        assert!(result.success, "{:?}", result.error);
        assert_eq!(result.output["formatted"], "1970-01-01");
    }

    #[tokio::test]
    async fn test_datetime_diff_days() {
        let result = DateTimeTool
            .execute(ToolInput::from_json(json!({
                "operation": "diff",
                "timestamp": 0,
                "timestamp2": 86400,
                "unit": "days"
            })))
            .await;
        assert!(result.success, "{:?}", result.error);
        assert_eq!(result.output["diff"], 1);
    }

    #[tokio::test]
    async fn test_datetime_add_hours() {
        let result = DateTimeTool
            .execute(ToolInput::from_json(json!({
                "operation": "add",
                "timestamp": 0,
                "amount": 2,
                "unit": "hours"
            })))
            .await;
        assert!(result.success, "{:?}", result.error);
        assert_eq!(result.output["result_timestamp"], 7200);
    }

    #[tokio::test]
    async fn test_datetime_missing_operation() {
        let result = DateTimeTool.execute(ToolInput::from_json(json!({}))).await;
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_datetime_unknown_operation() {
        let result = DateTimeTool
            .execute(ToolInput::from_json(json!({"operation": "invalid"})))
            .await;
        assert!(!result.success);
    }

    // ---- JsonParseTool ----

    #[tokio::test]
    async fn test_json_parse_basic() {
        let result = JsonParseTool
            .execute(ToolInput::from_json(json!({
                "json": "{\"name\": \"Alice\", \"age\": 30}"
            })))
            .await;
        assert!(result.success, "{:?}", result.error);
        assert_eq!(result.output["name"], "Alice");
        assert_eq!(result.output["age"], 30);
    }

    #[tokio::test]
    async fn test_json_query_dot_path() {
        let result = JsonParseTool
            .execute(ToolInput::from_json(json!({
                "json": "{\"user\": {\"address\": {\"city\": \"Berlin\"}}}",
                "operation": "query",
                "path": "user.address.city"
            })))
            .await;
        assert!(result.success, "{:?}", result.error);
        assert_eq!(result.output, "Berlin");
    }

    #[tokio::test]
    async fn test_json_query_missing_key() {
        let result = JsonParseTool
            .execute(ToolInput::from_json(json!({
                "json": "{\"a\": 1}",
                "operation": "query",
                "path": "a.b.c"
            })))
            .await;
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_json_keys() {
        let result = JsonParseTool
            .execute(ToolInput::from_json(json!({
                "json": "{\"x\": 1, \"y\": 2}",
                "operation": "keys"
            })))
            .await;
        assert!(result.success, "{:?}", result.error);
        let keys: Vec<String> = serde_json::from_value(result.output).unwrap();
        assert!(keys.contains(&"x".to_string()));
        assert!(keys.contains(&"y".to_string()));
    }

    #[tokio::test]
    async fn test_json_invalid() {
        let result = JsonParseTool
            .execute(ToolInput::from_json(json!({"json": "not-json!!"})))
            .await;
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap().contains("invalid JSON"));
    }

    #[tokio::test]
    async fn test_json_query_without_path() {
        let result = JsonParseTool
            .execute(ToolInput::from_json(json!({
                "json": "{\"a\": 1}",
                "operation": "query"
            })))
            .await;
        assert!(!result.success);
    }

    // ---- FileReadTool / FileWriteTool ----

    #[tokio::test]
    async fn test_file_write_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt").to_string_lossy().to_string();

        let write = FileWriteTool
            .execute(ToolInput::from_json(json!({
                "path": path,
                "content": "hello from MoFA"
            })))
            .await;
        assert!(write.success, "{:?}", write.error);

        let read = FileReadTool
            .execute(ToolInput::from_json(json!({"path": path})))
            .await;
        assert!(read.success, "{:?}", read.error);
        assert_eq!(read.output["content"], "hello from MoFA");
    }

    #[tokio::test]
    async fn test_file_write_append() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("append.txt").to_string_lossy().to_string();

        let w1 = FileWriteTool
            .execute(ToolInput::from_json(
                json!({"path": path, "content": "line1\n"}),
            ))
            .await;
        assert!(w1.success, "{:?}", w1.error);

        let w2 = FileWriteTool
            .execute(ToolInput::from_json(
                json!({"path": path, "content": "line2\n", "append": true}),
            ))
            .await;
        assert!(w2.success, "{:?}", w2.error);

        // Retry briefly in case slower filesystems delay visible updates.
        let mut content = String::new();
        let mut ok = false;
        for _ in 0..8 {
            let read = FileReadTool
                .execute(ToolInput::from_json(json!({"path": path})))
                .await;
            assert!(read.success);
            content = read.output["content"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            if content.contains("line2") {
                ok = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(ok, "content={content:?}");
        assert!(content.contains("line1"), "content={content:?}");
        assert!(content.contains("line2"), "content={content:?}");
    }

    #[tokio::test]
    async fn test_file_read_nonexistent() {
        let result = FileReadTool
            .execute(ToolInput::from_json(
                json!({"path": "/nonexistent/path/xyz.txt"}),
            ))
            .await;
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_file_write_missing_params() {
        let result = FileWriteTool
            .execute(ToolInput::from_json(json!({"path": "/tmp/x"})))
            .await;
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap().contains("content"));
    }

    // ---- ShellTool ----

    #[tokio::test]
    async fn test_shell_echo() {
        let input = if cfg!(windows) {
            json!({
                "command": "cmd",
                "args": ["/c", "echo hello shell"]
            })
        } else {
            json!({
                "command": "echo",
                "args": ["hello shell"]
            })
        };

        let result = ShellTool.execute(ToolInput::from_json(input)).await;
        assert!(result.success, "{:?}", result.error);
        assert!(
            result.output["stdout"]
                .as_str()
                .unwrap()
                .contains("hello shell")
        );
    }

    #[tokio::test]
    async fn test_shell_nonzero_exit() {
        let input = if cfg!(windows) {
            json!({
                "command": "cmd",
                "args": ["/c", "exit 1"]
            })
        } else {
            json!({
                "command": "sh",
                "args": ["-c", "exit 1"]
            })
        };

        let result = ShellTool.execute(ToolInput::from_json(input)).await;
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap().contains("exited"));
    }

    #[tokio::test]
    async fn test_shell_nonexistent_command() {
        let result = ShellTool
            .execute(ToolInput::from_json(
                json!({"command": "nonexistent_mofa_tool_xyz"}),
            ))
            .await;
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_shell_missing_command() {
        let result = ShellTool.execute(ToolInput::from_json(json!({}))).await;
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap().contains("command"));
    }
}
