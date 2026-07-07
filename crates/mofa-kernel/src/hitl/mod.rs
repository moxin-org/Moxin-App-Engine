//! Human-in-the-Loop (HITL) Module
//! Core abstractions for review workflows

pub mod audit;
pub mod context;
pub mod error;
pub mod policy;
pub mod types;

pub use audit::{AuditLogQuery, ReviewAuditEvent, ReviewAuditEventType};
pub use context::{
    Change, Diff, ExecutionStep, ExecutionTrace, PerformanceData, ReviewContext, TelemetrySnapshot,
};
pub use error::{HitlError, HitlResult, StoreError};
pub use policy::{AlwaysReviewPolicy, NeverReviewPolicy, ReviewPolicy};
pub use types::{
    ReviewMetadata, ReviewQuery, ReviewRequest, ReviewRequestId, ReviewResponse, ReviewStatus, ReviewType,
};
