//! Typed error and result types for the agent sub-system.
//!
//! # Result types
//!
//! Two result aliases are provided:
//!
//! - [`AgentResult<T>`] — `Result<T, AgentError>`.  Used by the **existing**
//!   public trait methods ([`MoFAAgent`][crate::agent::core::MoFAAgent], etc.)
//!   to preserve backward compatibility for all downstream implementors.
//!
//! - [`AgentReport<T>`] — `Result<T, error_stack::Report<AgentError>>`.  Use
//!   this in **new** code and internal helpers that want a full causal chain.
//!
//! ## Converting between the two
//!
//! Use the [`IntoAgentReport`] extension trait at module boundaries:
//!
//! ```rust,ignore
//! use mofa_kernel::agent::error::IntoAgentReport as _;
//! use error_stack::ResultExt as _;
//!
//! fn foo() -> AgentReport<()> {
//!     some_fallible_op()
//!         .into_report()               // AgentResult<T> → AgentReport<T>
//!         .attach("doing foo")
//! }
//! ```

use std::fmt;
use thiserror::Error;

/// Backward-compatible result alias for existing public trait methods.
///
/// Equivalent to `Result<T, AgentError>`.  All [`MoFAAgent`][crate::agent::core::MoFAAgent]
/// trait methods return this type to preserve compatibility for downstream
/// implementors.
///
/// For **new** internal code, prefer [`AgentReport<T>`] which carries a full
/// `error_stack` causal chain.
pub type AgentResult<T> = Result<T, AgentError>;

/// Error-stack–backed result alias for new agent code.
///
/// Equivalent to `Result<T, error_stack::Report<AgentError>>`.
/// Convert from [`AgentResult<T>`] using [`IntoAgentReport`].
pub type AgentReport<T> = ::std::result::Result<T, error_stack::Report<AgentError>>;

/// Extension trait to convert [`AgentResult<T>`] into [`AgentReport<T>`].
///
/// ```rust,ignore
/// use mofa_kernel::agent::error::IntoAgentReport as _;
/// use error_stack::ResultExt as _;
///
/// agent_op()
///     .into_report()
///     .attach("calling agent op")?;
/// ```
pub trait IntoAgentReport<T> {
    /// Wrap the error in an `error_stack::Report`, capturing the current
    /// location as the first stack frame.
    fn into_report(self) -> AgentReport<T>;
}

impl<T> IntoAgentReport<T> for AgentResult<T> {
    #[inline]
    fn into_report(self) -> AgentReport<T> {
        self.map_err(error_stack::Report::new)
    }
}

/// Agent 错误类型
/// Agent error types
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AgentError {
    /// Agent not found.
    #[error("Agent not found: {0}")]
    NotFound(String),

    /// Agent initialization failed.
    #[error("Agent initialization failed: {0}")]
    InitializationFailed(String),

    /// Agent validation failed.
    #[error("Agent validation failed: {0}")]
    ValidationFailed(String),

    /// Agent execution failed.
    #[error("Agent execution failed: {0}")]
    ExecutionFailed(String),

    /// A specific tool's execution failed.
    #[error("Tool execution failed: {tool_name}: {message}")]
    ToolExecutionFailed { tool_name: String, message: String },

    /// The requested tool could not be found.
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    /// Agent configuration is invalid or missing.
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Agent shutdown failed.
    #[error("Shutdown Failed: {0}")]
    ShutdownFailed(String),

    /// The input provided to the agent was invalid.
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// The output produced by the agent was invalid.
    #[error("Invalid output: {0}")]
    InvalidOutput(String),

    /// An illegal agent state transition was attempted.
    #[error("Invalid state transition: from {from:?} to {to:?}")]
    InvalidStateTransition { from: String, to: String },

    /// An operation timed out.
    #[error("Operation timed out after {duration_ms}ms")]
    Timeout { duration_ms: u64 },

    /// An operation was cancelled or interrupted.
    #[error("Operation was interrupted")]
    Interrupted,

    /// A required resource is temporarily unavailable.
    #[error("Resource unavailable: {0}")]
    ResourceUnavailable(String),

    /// The agent does not satisfy the required capability set.
    #[error("Capability mismatch: required {required}, available {available}")]
    CapabilityMismatch { required: String, available: String },

    /// The requested agent factory was not found in the registry.
    #[error("Agent factory not found: {0}")]
    FactoryNotFound(String),

    /// Registering an agent or tool in the registry failed.
    #[error("Registration failed: {0}")]
    RegistrationFailed(String),

    /// A memory sub-system operation failed.
    #[error("Memory error: {0}")]
    MemoryError(String),

    /// A reasoning / inference step failed.
    #[error("Reasoning error: {0}")]
    ReasoningError(String),

    /// A multi-agent coordination step failed.
    #[error("Coordination error: {0}")]
    CoordinationError(String),

    /// A (de)serialization error at the agent boundary.
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Protocol version mismatch — the sender used a version not supported
    /// by this receiver. Returned by [`crate::llm::MessageEnvelope::check_version`].
    #[error(
        "Protocol version mismatch: received \"{}\" but this build only supports \"{}\"",
        received,
        supported
    )]
    ProtocolVersionMismatch {
        /// The version tag found in the received message.
        received: String,
        /// The version(s) this build accepts (e.g., `"1"`).
        supported: String,
    },

    /// An I/O error (file, network, etc.).
    #[error("IO error: {0}")]
    IoError(String),

    /// An unexpected internal error.
    #[error("Internal error: {0}")]
    Internal(String),

    /// A catch-all for errors that do not fit the above categories.
    #[error("{0}")]
    Other(String),

    /// Circuit breaker open — the node is currently blocked due to repeated failures.
    /// Route to a fallback or retry later.
    #[error("circuit breaker open for node '{0}'")]
    CircuitOpen(String),
}

impl AgentError {
    /// Create a tool execution failure error.
    pub fn tool_execution_failed(tool_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ToolExecutionFailed {
            tool_name: tool_name.into(),
            message: message.into(),
        }
    }

    /// Create an invalid state transition error.
    pub fn invalid_state_transition(from: impl fmt::Debug, to: impl fmt::Debug) -> Self {
        Self::InvalidStateTransition {
            from: format!("{:?}", from),
            to: format!("{:?}", to),
        }
    }

    /// Create a timeout error.
    pub fn timeout(duration_ms: u64) -> Self {
        Self::Timeout { duration_ms }
    }

    /// Create a capability mismatch error.
    pub fn capability_mismatch(required: impl Into<String>, available: impl Into<String>) -> Self {
        Self::CapabilityMismatch {
            required: required.into(),
            available: available.into(),
        }
    }

    /// Returns `true` for transient errors that *may* succeed on retry.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            AgentError::Timeout { .. }
                | AgentError::ResourceUnavailable(_)
                | AgentError::ExecutionFailed(_)
                | AgentError::ToolExecutionFailed { .. }
                | AgentError::CoordinationError(_)
                | AgentError::Internal(_)
                | AgentError::IoError(_)
                | AgentError::ReasoningError(_)
                | AgentError::MemoryError(_)
        )
    }

    /// Whether this error is transient and the operation may succeed on retry.
    ///
    /// Transient errors: timeouts, resource unavailability, IO failures, and
    /// generic execution/coordination failures.
    /// Permanent errors (config, not-found, invalid input, interruption,
    /// validation) should fail fast without retry.
    ///
    /// NOTE: Stricter than [`is_retryable`] — excludes `Internal`,
    /// `ToolExecutionFailed`, `ReasoningError`, and `MemoryError` which may
    /// indicate bugs rather than transient infrastructure issues.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Timeout { .. }
                | Self::ResourceUnavailable(_)
                | Self::IoError(_)
                | Self::ExecutionFailed(_)
                | Self::CoordinationError(_)
        )
    }
}

impl From<std::io::Error> for AgentError {
    fn from(err: std::io::Error) -> Self {
        AgentError::IoError(err.to_string())
    }
}

impl From<serde_json::Error> for AgentError {
    fn from(err: serde_json::Error) -> Self {
        AgentError::SerializationError(err.to_string())
    }
}

impl From<crate::plugin::PluginError> for AgentError {
    fn from(err: crate::plugin::PluginError) -> Self {
        AgentError::Internal(err.to_string())
    }
}

impl From<crate::bus::BusError> for AgentError {
    fn from(err: crate::bus::BusError) -> Self {
        AgentError::Internal(err.to_string())
    }
}

impl From<crate::agent::secretary::ConnectionError> for AgentError {
    fn from(err: crate::agent::secretary::ConnectionError) -> Self {
        AgentError::Internal(err.to_string())
    }
}

impl From<crate::agent::secretary::SecretaryError> for AgentError {
    fn from(err: crate::agent::secretary::SecretaryError) -> Self {
        AgentError::Internal(err.to_string())
    }
}

#[cfg(feature = "config")]
impl From<crate::config::ConfigError> for AgentError {
    fn from(err: crate::config::ConfigError) -> Self {
        match err {
            crate::config::ConfigError::Io(e) => {
                AgentError::ConfigError(format!("Failed to read config file: {}", e))
            }
            crate::config::ConfigError::Parse(e) => {
                AgentError::ConfigError(format!("Failed to parse config: {}", e))
            }
            crate::config::ConfigError::UnsupportedFormat(e) => {
                AgentError::ConfigError(format!("Unsupported config format: {}", e))
            }
            crate::config::ConfigError::Serialization(e) => {
                AgentError::ConfigError(format!("Failed to deserialize config: {}", e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        /// 测试错误显示
        /// Test error display
        let err = AgentError::NotFound("test-agent".to_string());
        assert_eq!(err.to_string(), "Agent not found: test-agent");
    }

    #[test]
    fn test_tool_execution_failed() {
        /// 测试工具执行失败
        /// Test tool execution failure
        let err = AgentError::tool_execution_failed("calculator", "division by zero");
        assert!(err.to_string().contains("calculator"));
        assert!(err.to_string().contains("division by zero"));
    }

    #[cfg(feature = "config")]
    #[test]
    fn config_error_converts_via_from() {
        let config_err = crate::config::ConfigError::Parse("bad yaml".into());
        let agent_err: AgentError = config_err.into();

        assert!(matches!(agent_err, AgentError::ConfigError(_)));
        if let AgentError::ConfigError(msg) = agent_err {
            assert!(msg.contains("bad yaml"));
        }
    }

    #[test]
    fn test_is_transient_classification() {
        // Transient errors — should be retried
        assert!(AgentError::Timeout { duration_ms: 5000 }.is_transient());
        assert!(AgentError::ResourceUnavailable("gpu".into()).is_transient());
        assert!(AgentError::IoError("connection reset".into()).is_transient());
        assert!(AgentError::ExecutionFailed("LLM timeout".into()).is_transient());
        assert!(AgentError::CoordinationError("peer unreachable".into()).is_transient());

        // Permanent errors — should NOT be retried
        assert!(!AgentError::ConfigError("bad yaml".into()).is_transient());
        assert!(!AgentError::NotFound("agent-x".into()).is_transient());
        assert!(!AgentError::InvalidInput("missing field".into()).is_transient());
        assert!(!AgentError::Interrupted.is_transient());
        assert!(!AgentError::ValidationFailed("schema".into()).is_transient());
        assert!(!AgentError::Internal("panic".into()).is_transient());
    }
}
