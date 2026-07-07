use super::*;
use mofa_kernel::security::network::NetworkSecurity;
use reqwest::Client;
use serde_json::json;

/// HTTP 请求工具 - 发送网络请求
/// HTTP request utilities - Send network requests
pub struct HttpRequestTool {
    definition: ToolDefinition,
    client: Client,
}

impl Default for HttpRequestTool {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpRequestTool {
    pub fn new() -> Self {
        Self {
            definition: ToolDefinition {
                name: "http_request".to_string(),
                description: "Send HTTP requests (GET, POST, PUT, DELETE) to URLs. Useful for fetching web content or calling APIs.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "method": {
                            "type": "string",
                            "enum": ["GET", "POST", "PUT", "DELETE"],
                            "description": "HTTP method"
                        },
                        "url": {
                            "type": "string",
                            "description": "The URL to send the request to"
                        },
                        "headers": {
                            "type": "object",
                            "description": "Optional HTTP headers as key-value pairs",
                            "additionalProperties": { "type": "string" }
                        },
                        "body": {
                            "type": "string",
                            "description": "Optional request body for POST/PUT requests"
                        }
                    },
                    "required": ["method", "url"]
                }),
                requires_confirmation: true,
            },
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                // Avoid following redirects to internal/private networks.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    /// Truncate a UTF-8 string to at most `max_bytes` bytes without splitting
    /// a multi-byte character. Returns the longest prefix that fits.
    fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
        if s.len() <= max_bytes {
            return s;
        }
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

#[async_trait::async_trait]
impl ToolExecutor for HttpRequestTool {
    fn definition(&self) -> &ToolDefinition {
        &self.definition
    }

    async fn execute(&self, arguments: serde_json::Value) -> PluginResult<serde_json::Value> {
        let method = arguments["method"].as_str().unwrap_or("GET");
        let url = arguments["url"].as_str().ok_or_else(|| {
            mofa_kernel::plugin::PluginError::ExecutionFailed("URL is required".to_string())
        })?;

        // Validate URL to prevent SSRF attacks
        if !NetworkSecurity::is_url_allowed(url) {
            return Err(mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                "Access denied: URL '{}' targets a blocked address (private/internal network or disallowed scheme)",
                url
            )));
        }

        let mut request = match method {
            "GET" => self.client.get(url),
            "POST" => self.client.post(url),
            "PUT" => self.client.put(url),
            "DELETE" => self.client.delete(url),
            _ => {
                return Err(mofa_kernel::plugin::PluginError::ExecutionFailed(format!(
                    "Unsupported HTTP method: {}",
                    method
                )));
            }
        };

        // Add headers if provided
        if let Some(headers) = arguments.get("headers").and_then(|h| h.as_object()) {
            for (key, value) in headers {
                if let Some(v) = value.as_str() {
                    request = request.header(key.as_str(), v);
                }
            }
        }

        // Add body if provided
        if let Some(body) = arguments.get("body").and_then(|b| b.as_str()) {
            request = request.body(body.to_string());
        }

        let response = request
            .send()
            .await
            .map_err(|e| mofa_kernel::plugin::PluginError::ExecutionFailed(e.to_string()))?;
        let status = response.status().as_u16();
        let headers: std::collections::HashMap<String, String> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let body = response
            .text()
            .await
            .map_err(|e| mofa_kernel::plugin::PluginError::ExecutionFailed(e.to_string()))?;

        // Truncate body if too long
        let truncated_body = if body.len() > 5000 {
            format!(
                "{}... [truncated, total {} bytes]",
                Self::truncate_utf8(&body, 5000),
                body.len()
            )
        } else {
            body
        };

        Ok(json!({
            "status": status,
            "headers": headers,
            "body": truncated_body
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_utf8_ascii() {
        let s = "a".repeat(6000);
        let result = HttpRequestTool::truncate_utf8(&s, 5000);
        assert_eq!(result.len(), 5000);
    }

    #[test]
    fn test_truncate_utf8_multibyte() {
        // '中' is 3 bytes in UTF-8. Build a string where byte 5000 lands
        // inside a character.
        let s = "中".repeat(2000); // 6000 bytes total
        let result = HttpRequestTool::truncate_utf8(&s, 5000);
        // Should back up to nearest char boundary (4998 = 1666 × 3)
        assert!(result.len() <= 5000);
        assert!(result.is_char_boundary(result.len()));
    }

    #[test]
    fn test_truncate_utf8_short_string() {
        let s = "hello";
        let result = HttpRequestTool::truncate_utf8(s, 5000);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_utf8_emoji() {
        // '😀' is 4 bytes in UTF-8
        let s = "😀".repeat(1500); // 6000 bytes
        let result = HttpRequestTool::truncate_utf8(&s, 5000);
        assert!(result.len() <= 5000);
        assert!(result.is_char_boundary(result.len()));
    }

    // This test checks if `HttpRequestTool` actually enforces the shared ssrf policy .
    #[tokio::test]
    async fn denies_blocked_url_via_execute() {
        let tool = HttpRequestTool::new();
        let err = tool
            .execute(json!({
                "method": "GET",
                "url": "http://127.0.0.1:8080/"
            }))
            .await
            .unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("access denied"),
            "err={err:?}"
        );
    }

    #[test]
    fn truncates_utf8_without_panicking() {
        let value = "A🙂B";
        assert_eq!(HttpRequestTool::truncate_utf8(value, 2), "A");
    }
}
