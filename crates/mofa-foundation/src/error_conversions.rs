//! Cross-crate error conversions for mofa-foundation
//!
//! Implements `From<DomainError> for GlobalError` so that domain-specific
//! errors from this crate can be converted to the unified `GlobalError`
//! type defined in `mofa-kernel` using the `?` operator.

use mofa_kernel::agent::types::error::GlobalError;

// ============================================================================
// LLMError → GlobalError
// ============================================================================

impl From<crate::llm::types::LLMError> for GlobalError {
    fn from(err: crate::llm::types::LLMError) -> Self {
        GlobalError::LLM(err.to_string())
    }
}

// ============================================================================
// PersistenceError → GlobalError
// ============================================================================

impl From<crate::persistence::PersistenceError> for GlobalError {
    fn from(err: crate::persistence::PersistenceError) -> Self {
        GlobalError::Persistence(err.to_string())
    }
}

// ============================================================================
// PromptError → GlobalError
// ============================================================================

impl From<crate::prompt::PromptError> for GlobalError {
    fn from(err: crate::prompt::PromptError) -> Self {
        GlobalError::Prompt(err.to_string())
    }
}

// ============================================================================
// DslError → GlobalError
// ============================================================================

impl From<crate::workflow::dsl::DslError> for GlobalError {
    fn from(err: crate::workflow::dsl::DslError) -> Self {
        GlobalError::Dsl(err.to_string())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::LLMError;
    use crate::persistence::PersistenceError;
    use crate::prompt::PromptError;
    use crate::workflow::dsl::DslError;
    use mofa_kernel::agent::types::error::ErrorCategory;

    #[test]
    fn test_llm_error_to_global() {
        let llm_err = LLMError::NetworkError("connection timeout".to_string());
        let global: GlobalError = llm_err.into();

        assert_eq!(global.category(), ErrorCategory::LLM);
        assert!(global.to_string().contains("connection timeout"));
        assert!(global.is_retryable());
    }

    #[test]
    fn test_llm_error_variants() {
        let errors = vec![
            LLMError::AuthError("bad key".to_string()),
            LLMError::RateLimited("too many requests".to_string()),
            LLMError::ModelNotFound("gpt-5".to_string()),
            LLMError::Timeout("30s".to_string()),
        ];

        for err in errors {
            let msg = err.to_string();
            let global: GlobalError = err.into();
            assert_eq!(global.category(), ErrorCategory::LLM);
            assert!(global.to_string().contains(&msg.split(": ").last().unwrap_or("")));
        }
    }

    #[test]
    fn test_persistence_error_to_global() {
        let pers_err = PersistenceError::Connection("refused".to_string());
        let global: GlobalError = pers_err.into();

        assert_eq!(global.category(), ErrorCategory::Persistence);
        assert!(global.to_string().contains("refused"));
    }

    #[test]
    fn test_persistence_error_variants() {
        let not_found = PersistenceError::NotFound("user-123".to_string());
        let global: GlobalError = not_found.into();
        assert!(global.to_string().contains("user-123"));

        let constraint = PersistenceError::Constraint("unique violation".to_string());
        let global: GlobalError = constraint.into();
        assert!(global.to_string().contains("unique violation"));
    }

    #[test]
    fn test_prompt_error_to_global() {
        let prompt_err = PromptError::MissingVariable("name".to_string());
        let global: GlobalError = prompt_err.into();

        assert_eq!(global.category(), ErrorCategory::Workflow);
        assert!(global.to_string().contains("name"));
    }

    #[test]
    fn test_dsl_error_to_global() {
        let dsl_err = DslError::MissingStartNode;
        let global: GlobalError = dsl_err.into();

        assert_eq!(global.category(), ErrorCategory::Workflow);
        assert!(global.to_string().contains("Missing Start node"));
    }
}
