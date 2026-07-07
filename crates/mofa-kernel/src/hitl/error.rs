//! HITL Error Types
//! Human-in-the-Loop error definitions

use crate::error::KernelError;
use thiserror::Error;

/// Errors that can occur in the Human-in-the-Loop system
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HitlError {
    #[error("Review request not found: {id}")]
    ReviewNotFound { id: String },

    #[error("Review request expired: {id}")]
    ReviewExpired { id: String },

    #[error("Review request already resolved: {id}")]
    ReviewAlreadyResolved { id: String },

    #[error("Invalid review response: {reason}")]
    InvalidResponse { reason: String },

   #[error("Review policy evaluation failed: {reason}")]
    PolicyError { reason: String },

    // Fix: Added the variant name back below the attribute
    #[error("Whale Protection Triggered: Transaction value {value} exceeds the {threshold} SOL limit")]
    WhaleThresholdExceeded { value: f64, threshold: f64 },

    #[error("Missing Audit Trail: High-integrity fintech operations require an 'audit_trail' in the context")]
    MissingAuditData,

    #[error("Web3 Signature Verification Failed: The provided signature is malformed or unauthorized")]
    InvalidSignature,

    #[error("Review store error: {0}")]
    StoreError(#[from] StoreError),

    #[error("Review notification failed: {reason}")]
    NotificationError { reason: String },

    #[error("Review context serialization failed: {0}")]
    SerializationError(String),

    #[error("Review timeout: {id}")]
    ReviewTimeout { id: String },

    #[error("Rate limit exceeded for tenant {tenant_id}, retry after {retry_after_secs}s")]
    RateLimitExceeded {
        tenant_id: String,
        retry_after_secs: u64,
    },

    #[error("Webhook notification failed: {reason}")]
    WebhookError { reason: String },

    #[error("Tenant access denied: {tenant_id}")]
    TenantAccessDenied { tenant_id: String },
}

/// Errors that can occur in the review store
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StoreError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Query error: {0}")]
    Query(String),

    #[error("Record not found: {0}")]
    NotFound(String),
}

impl From<HitlError> for KernelError {
    fn from(err: HitlError) -> Self {
        KernelError::Internal(err.to_string())
    }
}

/// Result type for HITL operations
pub type HitlResult<T> = Result<T, HitlError>;

/// The "Rulebook" for the Auditing Security System.
/// This tells the AI exactly why a transaction was stopped.
#[derive(Error, Debug, PartialEq, Eq)]
pub enum AuditError {
    // 1. The "Broken Seal" Rule (Integrity)
    #[error("Audit integrity check failed: The digital seal (hash) does not match!")]
    IntegrityMismatch,

    // 2. The "Incomplete Form" Rule
    #[error("Audit failed: You forgot to fill in the '{0}' field.")]
    MissingRequiredField(String),

    // 3. The "Time Travel" Rule
    #[error("Audit failed: The clock says this happened in the future!")]
    InvalidTimestamp,

    // 4. The "VIP Only" Rule
    #[error(
        "Audit failed: This person does not have the right security key (Level {required} needed, but has Level {actual})."
    )]
    InsufficientSecurityLevel { required: u8, actual: u8 },
}
