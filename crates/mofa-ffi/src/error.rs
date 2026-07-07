//! FFI error types.
//!
//! `MoFaError` is the single error type that crosses every FFI boundary —
//! UniFFI (Python/Kotlin/Swift) and PyO3 (native Python) both map to it.
//!
//! Intentionally NOT `#[non_exhaustive]` — UniFFI generates exhaustive matches
//! across the language boundary and requires all variants to be known at
//! compile time.

/// MoFA FFI error, usable across all binding layers.
///
/// Every internal `Result<_, SomeDomainError>` is mapped to the closest
/// variant here before crossing the language boundary. The full error message
/// is forwarded as the variant's string payload so no diagnostic information
/// is silently discarded.
#[derive(Debug, thiserror::Error)]
pub enum MoFaError {
    #[error("Configuration error: {0}")]
    ConfigError(String),
    #[error("Runtime error: {0}")]
    RuntimeError(String),
    #[error("LLM error: {0}")]
    LLMError(String),
    #[error("IO error: {0}")]
    IoError(String),
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    #[error("Tool error: {0}")]
    ToolError(String),
    #[error("Session error: {0}")]
    SessionError(String),
}

/// Convenience result alias for FFI-exposed functions.
pub type MoFaResult<T> = Result<T, MoFaError>;
