//! HITL Core Types
//! Core types for Human-in-the-Loop functionality

use crate::hitl::context::ReviewContext;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Review request ID (newtype for type safety)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReviewRequestId(String);

impl ReviewRequestId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn generate() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl From<String> for ReviewRequestId {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl std::fmt::Display for ReviewRequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Review type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ReviewType {
    /// Simple approval/rejection
    Approval,
    /// Review with feedback
    Feedback,
    /// Review with required changes
    ChangesRequired,
    /// Information-only review (no action required)
    Informational,
}

/// Review status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ReviewStatus {
    /// Pending review
    Pending,
    /// Approved
    Approved,
    /// Rejected
    Rejected,
    /// Changes requested
    ChangesRequested,
    /// Expired
    Expired,
    /// Cancelled
    Cancelled,
}

/// Review response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ReviewResponse {
    /// Approved with optional comment
    Approved { comment: Option<String> },
    /// Rejected with reason
    Rejected {
        reason: String,
        comment: Option<String>,
    },
    /// Changes requested
    ChangesRequested {
        changes: String,
        comment: Option<String>,
    },
    /// Deferred to later
    Deferred { reason: String },
}

/// Review metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewMetadata {
    /// Priority (1-10, higher is more urgent)
    pub priority: u8,
    /// Assigned reviewer (optional)
    pub assigned_to: Option<String>,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Custom metadata
    pub custom: HashMap<String, serde_json::Value>,
    /// Tenant ID for multi-tenancy
    pub tenant_id: Option<Uuid>,
}

impl Default for ReviewMetadata {
    fn default() -> Self {
        Self {
            priority: 5,
            assigned_to: None,
            tags: Vec::new(),
            custom: HashMap::new(),
            tenant_id: None,
        }
    }
}

/// Review request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRequest {
    /// Unique request ID
    pub id: ReviewRequestId,
    /// Execution ID this review belongs to
    pub execution_id: String,
    /// Node ID (if applicable)
    pub node_id: Option<String>,
    /// Review type
    pub review_type: ReviewType,
    /// Review status
    pub status: ReviewStatus,
    /// Review context (execution state, inputs, outputs)
    pub context: ReviewContext,
    /// Review metadata
    pub metadata: ReviewMetadata,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Expiration timestamp (optional)
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Resolution timestamp (if resolved)
    pub resolved_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Resolved by (user ID)
    pub resolved_by: Option<String>,
    /// Review response (if resolved)
    pub response: Option<ReviewResponse>,
}

impl ReviewRequest {
    pub fn new(
        execution_id: impl Into<String>,
        review_type: ReviewType,
        context: ReviewContext,
    ) -> Self {
        Self {
            id: ReviewRequestId::generate(),
            execution_id: execution_id.into(),
            node_id: None,
            review_type,
            status: ReviewStatus::Pending,
            context,
            metadata: ReviewMetadata::default(),
            created_at: chrono::Utc::now(),
            expires_at: None,
            resolved_at: None,
            resolved_by: None,
            response: None,
        }
    }

    pub fn with_node_id(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = Some(node_id.into());
        self
    }

    pub fn with_metadata(mut self, metadata: ReviewMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_expiration(mut self, expires_at: chrono::DateTime<chrono::Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            chrono::Utc::now() > expires_at
        } else {
            false
        }
    }

    pub fn is_resolved(&self) -> bool {
        !matches!(self.status, ReviewStatus::Pending)
    }
}

/// Query parameters for filtering reviews
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewQuery {
    /// Filter by execution ID
    pub execution_id: Option<String>,
    /// Filter by tenant ID
    pub tenant_id: Option<Uuid>,
    /// Filter by status string mapping
    pub status: Option<String>,
    /// Maximum number of results to return
    pub limit: Option<u64>,
    /// Number of results to skip
    pub offset: Option<u64>,
}
