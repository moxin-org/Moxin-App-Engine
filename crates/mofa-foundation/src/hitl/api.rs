//! HITL REST API
//!
//! HTTP API endpoints for review management and dashboard
//!
//! This module provides REST API endpoints for managing reviews. It's designed to work
//! with axum framework but can be adapted to other frameworks.
//!
//! # Example with axum
//!
//! ```rust,ignore
//! use axum::{Router, routing::get, extract::State};
//! use mofa_foundation::hitl::api::create_review_api_router;
//!
//! let manager = Arc::new(review_manager);
//! let app = Router::new()
//!     .nest("/api/reviews", create_review_api_router(manager));
//! ```

use crate::hitl::manager::ReviewManager;
use mofa_kernel::hitl::{AuditLogQuery, ReviewQuery, ReviewRequestId, ReviewResponse, ReviewStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// Review API state
#[derive(Clone)]
pub struct ReviewApiState {
    pub manager: Arc<ReviewManager>,
}

/// Review list response
#[derive(Debug, Serialize)]
pub struct ReviewListResponse {
    pub reviews: Vec<ReviewSummary>,
    pub total: usize,
}

/// Review summary (for list endpoints)
#[derive(Debug, Serialize)]
pub struct ReviewSummary {
    pub id: String,
    pub execution_id: String,
    pub node_id: Option<String>,
    pub status: String,
    pub review_type: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub resolved_at: Option<i64>,
    pub resolved_by: Option<String>,
    pub priority: u8,
}

/// Review detail response
#[derive(Debug, Serialize)]
pub struct ReviewDetailResponse {
    pub id: String,
    pub execution_id: String,
    pub node_id: Option<String>,
    pub review_type: String,
    pub status: String,
    pub context: serde_json::Value,
    pub metadata: serde_json::Value,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub resolved_at: Option<i64>,
    pub resolved_by: Option<String>,
    pub response: Option<serde_json::Value>,
}

/// Resolve review request
#[derive(Debug, Deserialize)]
pub struct ResolveReviewRequest {
    pub response: ReviewResponse,
    pub resolved_by: String,
}

/// Query reviews request
#[derive(Debug, Deserialize)]
pub struct QueryReviewsRequest {
    pub execution_id: Option<String>,
    pub tenant_id: Option<Uuid>,
    pub status: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

/// Audit events response
#[derive(Debug, Serialize)]
pub struct AuditEventsResponse {
    pub events: Vec<mofa_kernel::hitl::ReviewAuditEvent>,
    pub total: usize,
}

/// Query audit events request
#[derive(Debug, Deserialize)]
pub struct QueryAuditEventsRequest {
    pub review_id: Option<String>,
    pub execution_id: Option<String>,
    pub tenant_id: Option<Uuid>,
    pub event_type: Option<String>,
    pub actor: Option<String>,
    pub start_time_ms: Option<u64>,
    pub end_time_ms: Option<u64>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

impl ReviewApiState {
    /// Create new API state
    pub fn new(manager: Arc<ReviewManager>) -> Self {
        Self { manager }
    }
}

/// Create review API router (for axum)
///
/// Returns a router with the following endpoints:
/// - GET    /reviews - List reviews
/// - GET    /reviews/:id - Get review details
/// - POST   /reviews/:id/resolve - Resolve a review
/// - GET    /reviews/:id/audit - Get audit events for a review
/// - GET    /audit/events - Query audit events
///
/// # Example
///
/// ```rust,ignore
/// use axum::Router;
/// use mofa_foundation::hitl::api::create_review_api_router;
///
/// let manager = Arc::new(review_manager);
/// let app = Router::new()
///     .nest("/api/reviews", create_review_api_router(manager));
/// ```
#[cfg(feature = "http-api")]
pub fn create_review_api_router(state: Arc<ReviewApiState>) -> axum::Router {
    use axum::{
        Json,
        extract::{Path, Query, State as AxumState},
        http::StatusCode,
        response::IntoResponse,
        routing::{get, post},
    };

    axum::Router::new()
        .route("/", get(list_reviews_handler))
        .route("/:id", get(get_review_handler))
        .route("/:id/resolve", post(resolve_review_handler))
        .route("/:id/audit", get(get_review_audit_handler))
        .route("/audit/events", get(query_audit_events_handler))
        .with_state(state)
}

/// List reviews handler
#[cfg(feature = "http-api")]
async fn list_reviews_handler(
    axum::extract::State(state): axum::extract::State<Arc<ReviewApiState>>,
    axum::extract::Query(params): axum::extract::Query<QueryReviewsRequest>,
) -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse, response::Json};
    
    let query = mofa_kernel::hitl::ReviewQuery {
        execution_id: params.execution_id,
        tenant_id: params.tenant_id,
        status: params.status,
        limit: params.limit,
        offset: params.offset,
    };

    match state.manager.query_reviews(&query).await {
        Ok(reviews) => {
            let summaries: Vec<ReviewSummary> = reviews
                .into_iter()
                .map(|r| ReviewSummary {
                    id: r.id.as_str().to_string(),
                    execution_id: r.execution_id,
                    node_id: r.node_id,
                    status: format!("{:?}", r.status),
                    review_type: format!("{:?}", r.review_type),
                    created_at: r.created_at.timestamp_millis(),
                    expires_at: r.expires_at.map(|dt| dt.timestamp_millis()),
                    resolved_at: r.resolved_at.map(|dt| dt.timestamp_millis()),
                    resolved_by: r.resolved_by,
                    priority: r.metadata.priority,
                })
                .collect();

            (
                StatusCode::OK,
                Json(ReviewListResponse {
                    total: summaries.len(),
                    reviews: summaries,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// Get review handler
#[cfg(feature = "http-api")]
async fn get_review_handler(
    axum::extract::State(state): axum::extract::State<Arc<ReviewApiState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse, response::Json};
    let review_id = ReviewRequestId::new(id);

    match state.manager.get_review(&review_id).await {
        Ok(Some(review)) => {
            let detail = ReviewDetailResponse {
                id: review.id.as_str().to_string(),
                execution_id: review.execution_id,
                node_id: review.node_id,
                review_type: format!("{:?}", review.review_type),
                status: format!("{:?}", review.status),
                context: serde_json::to_value(&review.context).unwrap_or_default(),
                metadata: serde_json::to_value(&review.metadata).unwrap_or_default(),
                created_at: review.created_at.timestamp_millis(),
                expires_at: review.expires_at.map(|dt| dt.timestamp_millis()),
                resolved_at: review.resolved_at.map(|dt| dt.timestamp_millis()),
                resolved_by: review.resolved_by,
                response: review
                    .response
                    .map(|r| serde_json::to_value(&r).unwrap_or_default()),
            };

            (StatusCode::OK, Json(detail)).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Review not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// Resolve review handler
#[cfg(feature = "http-api")]
async fn resolve_review_handler(
    axum::extract::State(state): axum::extract::State<Arc<ReviewApiState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::Json(req): axum::extract::Json<ResolveReviewRequest>,
) -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse, response::Json};
    let review_id = ReviewRequestId::new(id);

    match state
        .manager
        .resolve_review(&review_id, req.response, req.resolved_by)
        .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"message": "Review resolved successfully"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// Get review audit events handler
#[cfg(feature = "http-api")]
async fn get_review_audit_handler(
    axum::extract::State(state): axum::extract::State<Arc<ReviewApiState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse, response::Json};
    match state.manager.get_review_audit_events(&id).await {
        Ok(events) => (
            StatusCode::OK,
            Json(AuditEventsResponse {
                total: events.len(),
                events,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// Query audit events handler
#[cfg(feature = "http-api")]
async fn query_audit_events_handler(
    axum::extract::State(state): axum::extract::State<Arc<ReviewApiState>>,
    axum::extract::Query(params): axum::extract::Query<QueryAuditEventsRequest>,
) -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse, response::Json};
    let mut query = AuditLogQuery {
        review_id: params.review_id,
        execution_id: params.execution_id,
        tenant_id: params.tenant_id,
        actor: params.actor,
        start_time_ms: params.start_time_ms,
        end_time_ms: params.end_time_ms,
        limit: params.limit,
        offset: params.offset,
        ..Default::default()
    };

    // Parse event type if provided
    if let Some(_event_type_str) = params.event_type {
        // This would need proper parsing - simplified here
        // In production, use a proper enum deserializer
    }

    match state.manager.query_audit_events(&query).await {
        Ok(events) => (
            StatusCode::OK,
            Json(AuditEventsResponse {
                total: events.len(),
                events,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// Helper functions for non-axum frameworks
impl ReviewApiState {
    /// List reviews (framework-agnostic)
    pub async fn list_reviews(
        &self,
        query: &ReviewQuery,
    ) -> Result<ReviewListResponse, crate::hitl::error::FoundationHitlError> {
        let reviews = self.manager.query_reviews(query).await?;

        let summaries: Vec<ReviewSummary> = reviews
            .into_iter()
            .map(|r| ReviewSummary {
                id: r.id.as_str().to_string(),
                execution_id: r.execution_id,
                node_id: r.node_id,
                status: format!("{:?}", r.status),
                review_type: format!("{:?}", r.review_type),
                created_at: r.created_at.timestamp_millis(),
                expires_at: r.expires_at.map(|dt| dt.timestamp_millis()),
                resolved_at: r.resolved_at.map(|dt| dt.timestamp_millis()),
                resolved_by: r.resolved_by,
                priority: r.metadata.priority,
            })
            .collect();

        Ok(ReviewListResponse {
            total: summaries.len(),
            reviews: summaries,
        })
    }

    /// Get review details (framework-agnostic)
    pub async fn get_review(
        &self,
        review_id: &str,
    ) -> Result<ReviewDetailResponse, crate::hitl::error::FoundationHitlError> {
        let id = ReviewRequestId::new(review_id);
        let review = self.manager.get_review(&id).await?.ok_or_else(|| {
            crate::hitl::error::FoundationHitlError::InvalidConfig("Review not found".to_string())
        })?;

        Ok(ReviewDetailResponse {
            id: review.id.as_str().to_string(),
            execution_id: review.execution_id,
            node_id: review.node_id,
            review_type: format!("{:?}", review.review_type),
            status: format!("{:?}", review.status),
            context: serde_json::to_value(&review.context).unwrap_or_default(),
            metadata: serde_json::to_value(&review.metadata).unwrap_or_default(),
            created_at: review.created_at.timestamp_millis(),
            expires_at: review.expires_at.map(|dt| dt.timestamp_millis()),
            resolved_at: review.resolved_at.map(|dt| dt.timestamp_millis()),
            resolved_by: review.resolved_by,
            response: review
                .response
                .map(|r| serde_json::to_value(&r).unwrap_or_default()),
        })
    }

    /// Resolve review (framework-agnostic)
    pub async fn resolve_review(
        &self,
        review_id: &str,
        response: ReviewResponse,
        resolved_by: String,
    ) -> Result<(), crate::hitl::error::FoundationHitlError> {
        let id = ReviewRequestId::new(review_id);
        self.manager
            .resolve_review(&id, response, resolved_by)
            .await
    }
}
