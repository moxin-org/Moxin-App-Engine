//! Error type definitions for the dora-rs adapter layer.

use error_stack::Report;
use mofa_kernel::core::MofaError;
use thiserror::Error;

/// Error types for the dora-rs adapter layer.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum DoraError {
    #[error("MoFA error: {0}")]
    Mofa(#[from] MofaError),

    #[error("Node initialization failed: {0}")]
    NodeInitError(String),

    #[error("Node not running")]
    NodeNotRunning,

    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Dataflow error: {0}")]
    DataflowError(String),

    #[error("Dataflow not found: {0}")]
    DataflowNotFound(String),

    #[error("Channel error: {0}")]
    ChannelError(String),

    #[error("Operator error: {0}")]
    OperatorError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    #[error("Message size exceeded: {0} bytes > {1} bytes")]
    MessageSizeExceeded(usize, usize),

    #[error("Missing message bus")]
    MissingMessageBus,

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Not connected to daemon/coordinator")]
    NotConnected,

    #[error("Runtime not running")]
    RuntimeNotRunning,

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<bincode::Error> for DoraError {
    fn from(err: bincode::Error) -> Self {
        DoraError::SerializationError(err.to_string())
    }
}

impl From<serde_json::Error> for DoraError {
    fn from(err: serde_json::Error) -> Self {
        DoraError::SerializationError(err.to_string())
    }
}

impl From<tokio::sync::broadcast::error::SendError<Vec<u8>>> for DoraError {
    fn from(err: tokio::sync::broadcast::error::SendError<Vec<u8>>) -> Self {
        DoraError::ChannelError(err.to_string())
    }
}

/// Plain result alias for dora operations (backward-compatible).
pub type DoraResult<T> = Result<T, DoraError>;

/// Error-stack–backed result alias for dora operations.
pub type DoraReport<T> = ::std::result::Result<T, Report<DoraError>>;

/// Extension trait to convert [`DoraResult<T>`] into [`DoraReport<T>`].
pub trait IntoDoraReport<T> {
    /// Wrap the error in an `error_stack::Report`.
    fn into_report(self) -> DoraReport<T>;
}

impl<T> IntoDoraReport<T> for DoraResult<T> {
    #[inline]
    fn into_report(self) -> DoraReport<T> {
        self.map_err(Report::new)
    }
}
