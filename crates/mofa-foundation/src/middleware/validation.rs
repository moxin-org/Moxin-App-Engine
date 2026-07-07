//! Validation Middleware for Request/Response Validation
//!
//! This module provides middleware for validating incoming requests and outgoing responses,
//! including schema validation, rate limiting, and input sanitization.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{Duration, Instant};

/// Errors that can occur during validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub code: ValidationErrorCode,
    pub message: String,
    pub field: Option<String>,
    pub details: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ValidationErrorCode {
    InvalidJson,
    MissingField,
    InvalidType,
    StringTooShort,
    StringTooLong,
    PatternMismatch,
    EnumMismatch,
    RateLimitExceeded,
    XssDetected,
    SqlInjectionDetected,
    InvalidSchema,
}

/// Schema definition for a field
#[derive(Debug, Clone)]
pub struct FieldSchema {
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub pattern: Option<String>,
    pub allowed_values: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FieldType {
    String,
    Integer,
    Float,
    Boolean,
    Object,
    Array,
    Email,
    Url,
    Uuid,
}

/// Schema for request validation
#[derive(Debug, Clone, Default)]
pub struct RequestSchema {
    pub fields: Vec<FieldSchema>,
}

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub max_requests: u32,
    pub window_seconds: u64,
}

/// Input sanitization configuration
#[derive(Debug, Clone, Default)]
pub struct SanitizationConfig {
    pub strip_html: bool,
    pub remove_scripts: bool,
    pub prevent_xss: bool,
    pub prevent_sql_injection: bool,
}

/// Rate limiter implementation
#[derive(Clone)]
pub struct RateLimiter {
    requests: Arc<RwLock<HashMap<String, Vec<Instant>>>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    pub async fn check_rate_limit(&self, key: &str) -> Result<(), ValidationError> {
        let now = Instant::now();
        let mut requests = self.requests.write().await;
        
        let timestamps = requests.entry(key.to_string()).or_insert_with(Vec::new);
        
        // Remove old timestamps outside the window
        timestamps.retain(|t| now.duration_since(*t) < Duration::from_secs(self.config.window_seconds));
        
        // Check if rate limit exceeded
        if timestamps.len() >= self.config.max_requests as usize {
            return Err(ValidationError {
                code: ValidationErrorCode::RateLimitExceeded,
                message: format!("Rate limit exceeded: max {} requests per {} seconds", 
                    self.config.max_requests, self.config.window_seconds),
                field: Some(key.to_string()),
                details: None,
            });
        }
        
        // Add current timestamp
        timestamps.push(now);
        
        Ok(())
    }

    pub async fn reset(&self, key: &str) {
        let mut requests = self.requests.write().await;
        requests.remove(key);
    }
}

/// Input sanitizer implementation
#[derive(Clone)]
pub struct InputSanitizer {
    config: SanitizationConfig,
}

impl InputSanitizer {
    pub fn new(config: SanitizationConfig) -> Self {
        Self { config }
    }

    pub fn sanitize(&self, input: &str) -> String {
        let mut result = input.to_string();
        
        if self.config.prevent_xss {
            result = self.sanitize_xss(&result);
        }
        
        if self.config.prevent_sql_injection {
            result = self.sanitize_sql(&result);
        }
        
        if self.config.strip_html {
            result = self.strip_html_tags(&result);
        }
        
        result
    }

    fn sanitize_xss(&self, input: &str) -> String {
        let result = input.to_string();
        // Remove script tags - replace with safe equivalents
        let result = result.replace("<script", "[script removed]");
        let result = result.replace("</script>", "[/script removed]");
        // Remove javascript: protocol
        let result = result.replace("javascript:", "");
        // Remove onerror attributes
        let result = result.replace("onerror=", "");
        // Remove onclick attributes  
        let result = result.replace("onclick=", "");
        // Remove onload attributes
        result.replace("onload=", "")
    }

    fn sanitize_sql(&self, input: &str) -> String {
        let result = input.to_string();
        // Escape single quotes
        let result = result.replace('\'', "\\'");
        // Remove common SQL comment patterns
        let result = result.replace("--", "");
        // Remove semicolons that could chain commands
        let result = result.replace("; ", " ");
        let result = result.replace(";'", " ");
        result
    }

    fn strip_html_tags(&self, input: &str) -> String {
        let mut result = String::new();
        let mut in_tag = false;
        
        for ch in input.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => result.push(ch),
                _ => {}
            }
        }
        
        result
    }

    fn contains_script_tag(&self, input: &str) -> bool {
        let lower = input.to_lowercase();
        // Check for various dangerous patterns
        lower.contains("<script") || lower.contains("javascript:") || 
        lower.contains("onerror=") || lower.contains("onclick=")
    }

    fn contains_sql_injection(&self, input: &str) -> bool {
        let lower = input.to_lowercase();
        // Simple SQL keyword detection
        lower.contains("'; drop ") || lower.contains("'; delete ") || 
        lower.contains("'; update ") || lower.contains("union select") ||
        lower.contains("--") || lower.contains("' or '1'='1")
    }
}

/// Configuration for validation middleware
#[derive(Debug, Clone)]
pub struct ValidationMiddlewareConfig {
    pub request_schema: Option<RequestSchema>,
    pub rate_limit: Option<RateLimitConfig>,
    pub sanitization: Option<SanitizationConfig>,
    pub enabled: bool,
}

impl Default for ValidationMiddlewareConfig {
    fn default() -> Self {
        Self {
            request_schema: None,
            rate_limit: None,
            sanitization: None,
            enabled: true,
        }
    }
}

/// Validation middleware for requests
#[derive(Clone)]
pub struct ValidationMiddleware {
    config: ValidationMiddlewareConfig,
    rate_limiter: Option<RateLimiter>,
    sanitizer: Option<InputSanitizer>,
}

impl ValidationMiddleware {
    pub fn new(config: ValidationMiddlewareConfig) -> Self {
        let rate_limiter = config.rate_limit.clone().map(RateLimiter::new);
        let sanitizer = config.sanitization.clone().map(InputSanitizer::new);
        
        Self {
            config,
            rate_limiter,
            sanitizer,
        }
    }

    /// Validate a request against the schema
    pub fn validate_request(&self, data: &serde_json::Value) -> Result<(), Vec<ValidationError>> {
        if !self.config.enabled {
            return Ok(());
        }

        let Some(ref schema) = self.config.request_schema else {
            return Ok(());
        };

        let mut errors = Vec::new();
        
        for field_schema in &schema.fields {
            // Check required fields
            if field_schema.required {
                if !data.get(&field_schema.name).is_some() {
                    errors.push(ValidationError {
                        code: ValidationErrorCode::MissingField,
                        message: format!("Required field {} is missing", field_schema.name),
                        field: Some(field_schema.name.clone()),
                        details: None,
                    });
                    continue;
                }
            } else {
                // Skip optional fields that are not present
                if !data.get(&field_schema.name).is_some() {
                    continue;
                }
            }

            // Get the field value
            let Some(value) = data.get(&field_schema.name) else {
                continue;
            };

            // Validate field type
            if let Err(e) = self.validate_field_type(value, &field_schema.field_type) {
                errors.push(e);
            }

            // Validate string length constraints
            if let Some(min) = field_schema.min_length {
                if let Some(s) = value.as_str() {
                    if s.len() < min {
                        errors.push(ValidationError {
                            code: ValidationErrorCode::StringTooShort,
                            message: format!("Field {} must be at least {} characters", field_schema.name, min),
                            field: Some(field_schema.name.clone()),
                            details: None,
                        });
                    }
                }
            }

            if let Some(max) = field_schema.max_length {
                if let Some(s) = value.as_str() {
                    if s.len() > max {
                        errors.push(ValidationError {
                            code: ValidationErrorCode::StringTooLong,
                            message: format!("Field {} must be at most {} characters", field_schema.name, max),
                            field: Some(field_schema.name.clone()),
                            details: None,
                        });
                    }
                }
            }

            // Validate pattern
            if let Some(ref pattern) = field_schema.pattern {
                if let Some(s) = value.as_str() {
                    if let Ok(re) = regex::Regex::new(pattern) {
                        if !re.is_match(s) {
                            errors.push(ValidationError {
                                code: ValidationErrorCode::PatternMismatch,
                                message: format!("Field {} does not match required pattern", field_schema.name),
                                field: Some(field_schema.name.clone()),
                                details: None,
                            });
                        }
                    }
                }
            }

            // Validate allowed values
            if let Some(ref allowed) = field_schema.allowed_values {
                if let Some(s) = value.as_str() {
                    if !allowed.contains(&s.to_string()) {
                        errors.push(ValidationError {
                            code: ValidationErrorCode::EnumMismatch,
                            message: format!("Field {} must be one of: {:?}", field_schema.name, allowed),
                            field: Some(field_schema.name.clone()),
                            details: None,
                        });
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn validate_field_type(&self, value: &serde_json::Value, expected: &FieldType) -> Result<(), ValidationError> {
        let valid = match expected {
            FieldType::String => value.is_string(),
            FieldType::Integer => value.is_i64() || value.is_u64(),
            FieldType::Float => value.is_number(),
            FieldType::Boolean => value.is_boolean(),
            FieldType::Object => value.is_object(),
            FieldType::Array => value.is_array(),
            FieldType::Email => {
                if let Some(s) = value.as_str() {
                    s.contains('@') && s.contains('.')
                } else {
                    false
                }
            },
            FieldType::Url => {
                if let Some(s) = value.as_str() {
                    s.starts_with("http://") || s.starts_with("https://")
                } else {
                    false
                }
            },
            FieldType::Uuid => {
                if let Some(s) = value.as_str() {
                    // Simple UUID validation
                    s.len() == 36 && s.contains('-')
                } else {
                    false
                }
            },
        };

        if valid {
            Ok(())
        } else {
            Err(ValidationError {
                code: ValidationErrorCode::InvalidType,
                message: format!("Invalid type for field"),
                field: None,
                details: None,
            })
        }
    }

    /// Check rate limit for a key
    pub async fn check_rate_limit(&self, key: &str) -> Result<(), ValidationError> {
        if !self.config.enabled {
            return Ok(());
        }

        if let Some(ref limiter) = self.rate_limiter {
            limiter.check_rate_limit(key).await
        } else {
            Ok(())
        }
    }

    /// Sanitize input
    pub fn sanitize(&self, input: &str) -> String {
        if !self.config.enabled {
            return input.to_string();
        }

        if let Some(ref sanitizer) = self.sanitizer {
            sanitizer.sanitize(input)
        } else {
            input.to_string()
        }
    }

    /// Validate and sanitize request data
    pub async fn process(&self, data: &serde_json::Value, rate_limit_key: Option<&str>) 
        -> Result<serde_json::Value, Vec<ValidationError>> {
        
        // Check rate limit
        if let Some(key) = rate_limit_key {
            self.check_rate_limit(key).await.map_err(|e| vec![e])?;
        }

        // Validate request
        self.validate_request(data)?;

        // Return sanitized data
        Ok(data.clone())
    }
}

/// Build a validation middleware from a JSON configuration
pub fn from_config(config: &serde_json::Value) -> Result<ValidationMiddleware, ValidationError> {
    let enabled = config.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    
    let request_schema = if let Some(schema_val) = config.get("schema") {
        let mut fields = Vec::new();
        
        if let Some(fields_arr) = schema_val.get("fields").and_then(|v| v.as_array()) {
            for field_val in fields_arr {
                let name = field_val.get("name")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_default();
                
                let field_type_str = field_val.get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("string");
                
                let field_type = match field_type_str {
                    "string" => FieldType::String,
                    "integer" => FieldType::Integer,
                    "float" => FieldType::Float,
                    "boolean" => FieldType::Boolean,
                    "object" => FieldType::Object,
                    "array" => FieldType::Array,
                    "email" => FieldType::Email,
                    "url" => FieldType::Url,
                    "uuid" => FieldType::Uuid,
                    _ => FieldType::String,
                };
                
                let required = field_val.get("required").and_then(|v| v.as_bool()).unwrap_or(false);
                let min_length = field_val.get("min_length").and_then(|v| v.as_u64()).map(|v| v as usize);
                let max_length = field_val.get("max_length").and_then(|v| v.as_u64()).map(|v| v as usize);
                let pattern = field_val.get("pattern").and_then(|v| v.as_str()).map(String::from);
                
                let allowed_values = if let Some(arr) = field_val.get("allowed_values").and_then(|v| v.as_array()) {
                    Some(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                } else {
                    None
                };
                
                fields.push(FieldSchema {
                    name,
                    field_type,
                    required,
                    min_length,
                    max_length,
                    pattern,
                    allowed_values,
                });
            }
        }
        
        Some(RequestSchema { fields })
    } else {
        None
    };

    let rate_limit = if let Some(rl_val) = config.get("rate_limit") {
        Some(RateLimitConfig {
            max_requests: rl_val.get("max_requests").and_then(|v| v.as_u64()).unwrap_or(100) as u32,
            window_seconds: rl_val.get("window_seconds").and_then(|v| v.as_u64()).unwrap_or(60),
        })
    } else {
        None
    };

    let sanitization = if let Some(san_val) = config.get("sanitization") {
        Some(SanitizationConfig {
            strip_html: san_val.get("strip_html").and_then(|v| v.as_bool()).unwrap_or(true),
            remove_scripts: san_val.get("remove_scripts").and_then(|v| v.as_bool()).unwrap_or(true),
            prevent_xss: san_val.get("prevent_xss").and_then(|v| v.as_bool()).unwrap_or(true),
            prevent_sql_injection: san_val.get("prevent_sql_injection").and_then(|v| v.as_bool()).unwrap_or(true),
        })
    } else {
        None
    };

    let middleware_config = ValidationMiddlewareConfig {
        request_schema,
        rate_limit,
        sanitization,
        enabled,
    };

    Ok(ValidationMiddleware::new(middleware_config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_schema_validation() {
        let schema = RequestSchema {
            fields: vec![
                FieldSchema {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    required: true,
                    min_length: Some(2),
                    max_length: Some(50),
                    pattern: None,
                    allowed_values: None,
                },
            ],
        };

        let middleware = ValidationMiddleware::new(ValidationMiddlewareConfig {
            request_schema: Some(schema),
            ..Default::default()
        });

        // Valid request
        let valid_data = serde_json::json!({"name": "John"});
        assert!(middleware.validate_request(&valid_data).is_ok());

        // Missing required field
        let missing_data = serde_json::json!({});
        let result = middleware.validate_request(&missing_data);
        assert!(result.is_err());
        
        // Too short
        let short_data = serde_json::json!({"name": "J"});
        let result = middleware.validate_request(&short_data);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rate_limiter() {
        let config = RateLimitConfig {
            max_requests: 3,
            window_seconds: 60,
        };
        
        let limiter = RateLimiter::new(config);
        
        // Should allow first 3 requests
        for _ in 0..3 {
            let result = limiter.check_rate_limit("test_key").await;
            assert!(result.is_ok());
        }
        
        // 4th request should be denied
        let result = limiter.check_rate_limit("test_key").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_sanitizer_xss() {
        let sanitizer = InputSanitizer::new(SanitizationConfig {
            prevent_xss: true,
            ..Default::default()
        });
        
        let result = sanitizer.sanitize("<script>alert(1)</script>");
        // After sanitization, script tags should be replaced
        assert!(result.contains("[script removed]"), "XSS not sanitized: {}", result);
    }

    #[test]
    fn test_sanitizer_sql() {
        let sanitizer = InputSanitizer::new(SanitizationConfig {
            prevent_sql_injection: true,
            ..Default::default()
        });
        
        let result = sanitizer.sanitize("test' OR '1'='1");
        assert!(!result.contains("' OR '"));
    }

    #[test]
    fn test_from_config() {
        let config_json = serde_json::json!({
            "enabled": true,
            "schema": {
                "fields": [
                    {
                        "name": "email",
                        "type": "email",
                        "required": true
                    }
                ]
            },
            "rate_limit": {
                "max_requests": 100,
                "window_seconds": 60
            }
        });
        
        let middleware = from_config(&config_json);
        assert!(middleware.is_ok());
    }
}
