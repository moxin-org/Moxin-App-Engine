//! Workflow DSL (Domain Specific Language)
//!
//! Provides declarative configuration for workflows using YAML/TOML.
//!
//! # Example
//!
//! ```yaml
//! metadata:
//!   id: customer_support
//!   name: Customer Support Workflow
//!
//! agents:
//!   classifier:
//!     model: gpt-4
//!     system_prompt: "Classify queries into: billing, technical, general"
//!     temperature: 0.3
//!
//! nodes:
//!   - type: start
//!     id: start
//!
//!   - type: llm_agent
//!     id: classify
//!     name: Classify Query
//!     agent:
//!       agent_id: classifier
//!
//!   - type: end
//!     id: end
//!
//! edges:
//!   - from: start
//!     to: classify
//!   - from: classify
//!     to: end
//! ```

mod env;
mod parser;
mod schema;
pub mod validation;
pub use parser::*;
pub use schema::*;

/// Plain result alias for DSL operations (backward-compatible).
pub type DslResult<T> = Result<T, DslError>;

/// Error-stack–backed result alias for DSL operations.
pub type DslReport<T> = ::std::result::Result<T, error_stack::Report<DslError>>;

/// Extension trait to convert [`DslResult<T>`] into [`DslReport<T>`].
pub trait IntoDslReport<T> {
    /// Wrap the error in an `error_stack::Report`.
    fn into_report(self) -> DslReport<T>;
}

impl<T> IntoDslReport<T> for DslResult<T> {
    #[inline]
    fn into_report(self) -> DslReport<T> {
        self.map_err(error_stack::Report::new)
    }
}

/// Errors that can occur during DSL parsing.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DslError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parse error: {0}")]
    YamlParse(#[from] serde_yaml::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("JSON parse error: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    #[error("Invalid node type: {0}")]
    InvalidNodeType(String),

    #[error("Invalid edge: from '{from}' to '{to}'")]
    InvalidEdge { from: String, to: String },

    #[error("Build error: {0}")]
    Build(String),
}
