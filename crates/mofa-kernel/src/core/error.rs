//! Core structured error types used by runtime/node execution flows.

use crate::agent::error::AgentError;
use thiserror::Error;

/// Structured MoFA error classification for runtime-aware retry behavior.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum MofaError {
    #[error("Operation timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("Rate limited, retry after {retry_after_secs}s")]
    RateLimit { retry_after_secs: u64 },

    #[error("Authentication failed: {reason}")]
    AuthFailed { reason: String },

    #[error("Invalid configuration field: {field}")]
    InvalidConfig { field: String },

    #[error("Provider error: {0}")]
    Provider(String),
}

impl MofaError {
    /// Whether this error is safe and useful to retry automatically.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Timeout { .. } | Self::RateLimit { .. } | Self::Provider(_)
        )
    }
}

impl From<&AgentError> for MofaError {
    fn from(error: &AgentError) -> Self {
        match error {
            AgentError::Timeout { duration_ms } => Self::Timeout {
                timeout_ms: *duration_ms,
            },
            AgentError::ResourceUnavailable(message) => {
                if looks_like_rate_limit(message) {
                    Self::RateLimit {
                        retry_after_secs: 1,
                    }
                } else {
                    Self::Provider(message.clone())
                }
            }
            AgentError::ConfigError(message)
            | AgentError::ValidationFailed(message)
            | AgentError::InvalidInput(message)
            | AgentError::InvalidOutput(message) => {
                if looks_like_auth_error(message) {
                    Self::AuthFailed {
                        reason: message.clone(),
                    }
                } else {
                    Self::InvalidConfig {
                        field: message.clone(),
                    }
                }
            }
            AgentError::ToolExecutionFailed { message, .. } => {
                if looks_like_rate_limit(message) {
                    Self::RateLimit {
                        retry_after_secs: 1,
                    }
                } else if looks_like_auth_error(message) {
                    Self::AuthFailed {
                        reason: message.clone(),
                    }
                } else {
                    Self::Provider(message.clone())
                }
            }
            AgentError::ExecutionFailed(message)
            | AgentError::ReasoningError(message)
            | AgentError::CoordinationError(message)
            | AgentError::MemoryError(message)
            | AgentError::IoError(message)
            | AgentError::Internal(message)
            | AgentError::Other(message) => {
                if looks_like_rate_limit(message) {
                    Self::RateLimit {
                        retry_after_secs: 1,
                    }
                } else if looks_like_auth_error(message) {
                    Self::AuthFailed {
                        reason: message.clone(),
                    }
                } else {
                    Self::Provider(message.clone())
                }
            }
            AgentError::Interrupted => Self::Provider("Interrupted".to_string()),
            AgentError::NotFound(message)
            | AgentError::FactoryNotFound(message)
            | AgentError::ToolNotFound(message)
            | AgentError::RegistrationFailed(message)
            | AgentError::ShutdownFailed(message) => Self::Provider(message.clone()),
            AgentError::InitializationFailed(message) => {
                if looks_like_auth_error(message) {
                    Self::AuthFailed {
                        reason: message.clone(),
                    }
                } else {
                    Self::Provider(message.clone())
                }
            }
            AgentError::CapabilityMismatch { required, available } => Self::InvalidConfig {
                field: format!("required={required}, available={available}"),
            },
            AgentError::InvalidStateTransition { from, to } => Self::Provider(format!(
                "Invalid state transition: from {from} to {to}"
            )),
            AgentError::CircuitOpen(node) => Self::Provider(format!("Circuit open: {node}")),
        }
    }
}

impl From<AgentError> for MofaError {
    fn from(error: AgentError) -> Self {
        Self::from(&error)
    }
}

fn looks_like_auth_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("auth")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("api key")
        || lower.contains("token")
}

fn looks_like_rate_limit(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("429")
        || lower.contains("throttle")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_classification_matches_contract() {
        assert!(MofaError::Timeout { timeout_ms: 1 }.is_retryable());
        assert!(MofaError::RateLimit {
            retry_after_secs: 1
        }
        .is_retryable());
        assert!(MofaError::Provider("upstream busy".into()).is_retryable());

        assert!(!MofaError::AuthFailed {
            reason: "bad key".into()
        }
        .is_retryable());
        assert!(!MofaError::InvalidConfig {
            field: "api_url".into()
        }
        .is_retryable());
    }

    #[test]
    fn agent_timeout_maps_to_mofa_timeout() {
        let error = AgentError::Timeout { duration_ms: 321 };
        assert_eq!(
            MofaError::from(error),
            MofaError::Timeout { timeout_ms: 321 }
        );
    }
}