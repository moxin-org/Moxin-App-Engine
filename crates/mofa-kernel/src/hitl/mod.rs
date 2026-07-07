//! Human-in-the-Loop (HITL) Module
//! Core abstractions for review workflows

pub mod audit;
pub mod context;
pub mod error;
pub mod policy;
pub mod types;

pub use audit::{AuditLogQuery, ReviewAuditEvent, ReviewAuditEventType};
pub use context::{
    AuditingData, Change, Diff, ExecutionStep, ExecutionTrace, PerformanceData, ReviewContext,
    TelemetrySnapshot,
};
pub use error::{HitlError, HitlResult, StoreError};

// ✅ ONE CLEAN BLOCK: This replaces the two separate policy blocks you had.
pub use policy::{
    AlwaysReviewPolicy, AuditValidationPolicy, NeverReviewPolicy, ReviewPolicy, WhaleGuardPolicy,
};

pub use types::{
    ReviewMetadata, ReviewRequest, ReviewRequestId, ReviewResponse, ReviewStatus, ReviewType,
};