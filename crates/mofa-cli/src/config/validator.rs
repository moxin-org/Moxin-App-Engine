//! Configuration validation

use super::AgentConfig;
use std::collections::HashSet;

/// Configuration validation result
#[must_use]
#[derive(Debug, Clone)]
#[must_use]
pub struct ConfigValidationResult {
    /// Whether validation passed
    pub is_valid: bool,
    /// Validation errors
    pub errors: Vec<ValidationError>,
    /// Validation warnings
    pub warnings: Vec<ValidationWarning>,
}

impl ConfigValidationResult {
    /// Create a successful validation result
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Check if validation passed
    pub fn ok(&self) -> bool {
        self.is_valid && self.errors.is_empty()
    }

    /// Add an error
    pub fn add_error(&mut self, error: ValidationError) {
        self.is_valid = false;
        self.errors.push(error);
    }

    /// Add a warning
    pub fn add_warning(&mut self, warning: ValidationWarning) {
        self.warnings.push(warning);
    }

    /// Get formatted error message
    pub fn error_message(&self) -> String {
        if self.errors.is_empty() {
            return String::new();
        }

        self.errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get formatted warning message
    pub fn warning_message(&self) -> String {
        if self.warnings.is_empty() {
            return String::new();
        }

        self.warnings
            .iter()
            .map(|w| w.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for ConfigValidationResult {
    fn default() -> Self {
        Self::valid()
    }
}

/// Validation error
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// Field path
    pub field: String,
    /// Error message
    pub message: String,
    /// Suggestion for fixing
    pub suggestion: Option<String>,
}

impl ValidationError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            suggestion: None,
        }
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "✗ {}. {}", self.field, self.message)?;
        if let Some(ref suggestion) = self.suggestion {
            write!(f, "\n  Suggestion: {}", suggestion)?;
        }
        Ok(())
    }
}

/// Validation warning
#[derive(Debug, Clone)]
pub struct ValidationWarning {
    /// Field path
    pub field: String,
    /// Warning message
    pub message: String,
}

impl ValidationWarning {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ValidationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "⚠ {}. {}", self.field, self.message)
    }
}

/// Configuration validator
#[derive(Debug, Clone)]
pub struct ConfigValidator {
    /// Strict mode (warnings become errors)
    strict: bool,
}

impl ConfigValidator {
    /// Create a new validator
    pub fn new() -> Self {
        Self { strict: false }
    }

    /// Enable strict mode
    pub fn strict(mut self) -> Self {
        self.strict = true;
        self
    }

    /// Validate a configuration
    #[must_use]
    pub fn validate(&self, config: &AgentConfig) -> ConfigValidationResult {
        let mut result = ConfigValidationResult::valid();

        // Validate agent identity
        self.validate_agent_identity(config, &mut result);

        // Validate LLM configuration
        if let Some(ref llm) = config.llm {
            self.validate_llm_config(llm, &mut result);
        }

        // Validate runtime configuration
        if let Some(ref runtime) = config.runtime {
            self.validate_runtime_config(runtime, &mut result);
        }

        result
    }

    fn validate_agent_identity(&self, config: &AgentConfig, result: &mut ConfigValidationResult) {
        let agent = &config.agent;

        // Validate agent ID
        if agent.id.is_empty() {
            result.add_error(
                ValidationError::new("agent.id", "Agent ID cannot be empty")
                    .with_suggestion("Set a unique identifier for your agent"),
            );
        }

        // Validate agent name
        if agent.name.is_empty() {
            result.add_error(
                ValidationError::new("agent.name", "Agent name cannot be empty")
                    .with_suggestion("Set a human-readable name for your agent"),
            );
        }

        // Validate capabilities
        if let Some(ref caps) = agent.capabilities {
            let valid_caps = HashSet::from([
                "llm".to_string(),
                "chat".to_string(),
                "tool_call".to_string(),
                "memory".to_string(),
                "storage".to_string(),
                "workflow".to_string(),
            ]);

            for cap in caps {
                if !valid_caps.contains(cap) {
                    result.add_warning(ValidationWarning::new(
                        format!("agent.capabilities.{}", cap),
                        format!("Unknown capability: {}", cap),
                    ));
                }
            }
        }
    }

    fn validate_llm_config(&self, llm: &super::LlmConfig, result: &mut ConfigValidationResult) {
        let valid_providers = HashSet::from([
            "openai".to_string(),
            "ollama".to_string(),
            "azure".to_string(),
            "compatible".to_string(),
        ]);

        // Validate provider
        if !valid_providers.contains(&llm.provider) {
            result.add_error(
                ValidationError::new(
                    "llm.provider",
                    format!("Unknown provider: {}", llm.provider),
                )
                .with_suggestion("Use: openai, ollama, azure, or compatible"),
            );
        }

        // Validate model
        if llm.model.is_empty() {
            result.add_error(
                ValidationError::new("llm.model", "Model cannot be empty")
                    .with_suggestion("Specify the model to use (e.g., gpt-4o, llama2)"),
            );
        }

        // Validate API key for providers that require it
        if llm.provider != "ollama"
            && (llm.api_key.is_none() || llm.api_key.as_ref().is_none_or(|k| k.is_empty()))
        {
            result.add_warning(ValidationWarning::new(
                "llm.api_key",
                format!(
                    "API key not set for {} provider (may be in environment variable)",
                    llm.provider
                ),
            ));
        }

        // Validate temperature
        if let Some(temp) = llm.temperature
            && (!(0.0..=2.0).contains(&temp))
        {
            result.add_error(
                ValidationError::new(
                    "llm.temperature",
                    format!("Temperature out of range: {}", temp),
                )
                .with_suggestion("Temperature must be between 0.0 and 2.0"),
            );
        }

        // Validate max_tokens
        if let Some(max_tokens) = llm.max_tokens
            && max_tokens == 0
        {
            result.add_error(
                ValidationError::new("llm.max_tokens", "max_tokens cannot be zero")
                    .with_suggestion("Set a positive value or remove to use default"),
            );
        }
    }

    fn validate_runtime_config(
        &self,
        runtime: &super::RuntimeConfig,
        result: &mut ConfigValidationResult,
    ) {
        // Validate max_concurrent_tasks
        if let Some(max_tasks) = runtime.max_concurrent_tasks
            && max_tasks == 0
        {
            result.add_error(
                ValidationError::new("runtime.max_concurrent_tasks", "Cannot be zero")
                    .with_suggestion("Set a positive value or remove to use default"),
            );
        }

        // Validate default_timeout_secs
        if let Some(timeout) = runtime.default_timeout_secs
            && timeout == 0
        {
            result.add_warning(ValidationWarning::new(
                "runtime.default_timeout_secs",
                "Zero timeout may cause operations to hang",
            ));
        }

        // Validate persistence
        if let Some(ref persistence) = runtime.persistence
            && persistence.enabled == Some(true)
            && (persistence.database_url.is_none()
                || persistence
                    .database_url
                    .as_ref()
                    .is_none_or(|u| u.is_empty()))
        {
            result.add_warning(ValidationWarning::new(
                "runtime.persistence.database_url",
                "Persistence enabled but no database URL configured",
            ));
        }
    }
}

impl Default for ConfigValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_config() {
        let config = AgentConfig {
            agent: super::super::AgentIdentity {
                id: "test-agent".to_string(),
                name: "Test Agent".to_string(),
                description: Some("A test agent".to_string()),
                capabilities: Some(vec!["llm".to_string(), "chat".to_string()]),
            },
            llm: Some(super::super::LlmConfig {
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                api_key: Some("${OPENAI_API_KEY}".to_string()),
                base_url: None,
                temperature: Some(0.7),
                max_tokens: Some(4096),
                system_prompt: Some("You are a helpful assistant.".to_string()),
            }),
            runtime: Some(super::super::RuntimeConfig {
                max_concurrent_tasks: Some(10),
                default_timeout_secs: Some(60),
                dora_enabled: None,
                persistence: None,
            }),
            node_config: std::collections::HashMap::new(),
        };

        let validator = ConfigValidator::new();
        let result = validator.validate(&config);

        assert!(result.ok());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_invalid_agent_id() {
        let config = AgentConfig {
            agent: super::super::AgentIdentity {
                id: "".to_string(),
                name: "Test".to_string(),
                description: None,
                capabilities: None,
            },
            llm: None,
            runtime: None,
            node_config: std::collections::HashMap::new(),
        };

        let validator = ConfigValidator::new();
        let result = validator.validate(&config);

        assert!(!result.ok());
        assert!(result.errors.iter().any(|e| e.field == "agent.id"));
    }

    #[test]
    fn test_invalid_provider() {
        let config = AgentConfig {
            agent: super::super::AgentIdentity {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: None,
                capabilities: None,
            },
            llm: Some(super::super::LlmConfig {
                provider: "invalid".to_string(),
                model: "gpt-4o".to_string(),
                api_key: Some("test".to_string()),
                base_url: None,
                temperature: None,
                max_tokens: None,
                system_prompt: None,
            }),
            runtime: None,
            node_config: std::collections::HashMap::new(),
        };

        let validator = ConfigValidator::new();
        let result = validator.validate(&config);

        assert!(!result.ok());
        assert!(result.errors.iter().any(|e| e.field == "llm.provider"));
    }

    #[test]
    fn test_temperature_out_of_range() {
        let config = AgentConfig {
            agent: super::super::AgentIdentity {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: None,
                capabilities: None,
            },
            llm: Some(super::super::LlmConfig {
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                api_key: Some("test".to_string()),
                base_url: None,
                temperature: Some(3.0),
                max_tokens: None,
                system_prompt: None,
            }),
            runtime: None,
            node_config: std::collections::HashMap::new(),
        };

        let validator = ConfigValidator::new();
        let result = validator.validate(&config);

        assert!(!result.ok());
        assert!(result.errors.iter().any(|e| e.field == "llm.temperature"));
    }
}
