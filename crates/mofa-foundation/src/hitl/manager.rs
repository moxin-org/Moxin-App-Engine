//! Review Manager
//!
//! Central orchestration for review requests with timeout handling and policy evaluation

use crate::hitl::audit::AuditStore;
use crate::hitl::error::{FoundationHitlError, HitlResult};
use crate::hitl::notifier::ReviewNotifier;
use crate::hitl::policy_engine::ReviewPolicyEngine;
use crate::hitl::rate_limiter::RateLimiter;
use crate::hitl::store::ReviewStore;
use mofa_kernel::hitl::{
    AuditLogQuery, HitlError, ReviewAuditEvent, ReviewAuditEventType, ReviewContext, ReviewQuery, ReviewRequest,
    ReviewRequestId, ReviewResponse, ReviewStatus,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{error, info, warn};
use uuid::Uuid;

/// Review manager configuration
#[derive(Debug, Clone)]
pub struct ReviewManagerConfig {
    /// Default expiration time for reviews
    pub default_expiration: Duration,
    /// Interval for checking expired reviews
    pub expiration_check_interval: Duration,
    /// Enable rate limiting
    pub enable_rate_limiting: bool,
}

impl Default for ReviewManagerConfig {
    fn default() -> Self {
        Self {
            default_expiration: Duration::from_secs(3600), // 1 hour
            expiration_check_interval: Duration::from_secs(60), // 1 minute
            enable_rate_limiting: true,
        }
    }
}

/// Review manager - central orchestration
pub struct ReviewManager {
    store: Arc<dyn ReviewStore>,
    notifier: Arc<ReviewNotifier>,
    policy_engine: Arc<ReviewPolicyEngine>,
    rate_limiter: Option<Arc<RateLimiter>>,
    audit_store: Option<Arc<dyn AuditStore>>,
    config: ReviewManagerConfig,
}

impl ReviewManager {
    /// Create a new review manager
    pub fn new(
        store: Arc<dyn ReviewStore>,
        notifier: Arc<ReviewNotifier>,
        policy_engine: Arc<ReviewPolicyEngine>,
        rate_limiter: Option<Arc<RateLimiter>>,
        config: ReviewManagerConfig,
    ) -> Self {
        Self {
            store,
            notifier,
            policy_engine,
            rate_limiter,
            audit_store: None,
            config,
        }
    }

    /// Create a new review manager with audit trail
    pub fn with_audit_store(
        store: Arc<dyn ReviewStore>,
        notifier: Arc<ReviewNotifier>,
        policy_engine: Arc<ReviewPolicyEngine>,
        rate_limiter: Option<Arc<RateLimiter>>,
        audit_store: Arc<dyn AuditStore>,
        config: ReviewManagerConfig,
    ) -> Self {
        Self {
            store,
            notifier,
            policy_engine,
            rate_limiter,
            audit_store: Some(audit_store),
            config,
        }
    }

    /// Set audit store (for builder pattern)
    pub fn set_audit_store(&mut self, audit_store: Arc<dyn AuditStore>) {
        self.audit_store = Some(audit_store);
    }

    /// Request a review (creates and stores review request)
    pub async fn request_review(&self, mut request: ReviewRequest) -> HitlResult<ReviewRequestId> {
        // Check rate limiting
        if let Some(ref limiter) = self.rate_limiter {
            let tenant_id = request
                .metadata
                .tenant_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "default".to_string());
            limiter
                .check(&tenant_id)
                .await
                .map_err(FoundationHitlError::from)?;
        }

        // Set expiration if not set
        if request.expires_at.is_none() {
            request.expires_at = Some(
                chrono::Utc::now()
                    + chrono::Duration::from_std(self.config.default_expiration)
                        .unwrap_or(chrono::Duration::hours(1)),
            );
        }

        // Store review request
        self.store
            .create_review(&request)
            .await
            .map_err(FoundationHitlError::Store)?;

        // Record audit event
        if let Some(ref audit_store) = self.audit_store {
            let mut audit_event =
                ReviewAuditEvent::new(request.id.as_str(), ReviewAuditEventType::Created, None)
                    .with_execution_id(&request.execution_id);

            if let Some(ref node_id) = request.node_id {
                audit_event = audit_event.with_node_id(node_id);
            }

            if let Some(tenant_id) = request.metadata.tenant_id {
                audit_event = audit_event.with_tenant_id(tenant_id);
            }

            audit_event = audit_event.with_data(
                "review_type".to_string(),
                serde_json::json!(format!("{:?}", request.review_type)),
            );

            if let Err(e) = audit_store.record_event(&audit_event).await {
                warn!("Failed to record audit event: {}", e);
                // Don't fail the request if audit fails
            }
        }

        // Notify about new review
        if let Err(e) = self.notifier.notify_review_created(&request).await {
            warn!("Failed to notify about review creation: {}", e);
            // Don't fail the request if notification fails
        }

        info!(
            "Review request created: {} (execution: {})",
            request.id.as_str(),
            request.execution_id
        );

        Ok(request.id)
    }

    /// Get a review request by ID
    pub async fn get_review(&self, id: &ReviewRequestId) -> HitlResult<Option<ReviewRequest>> {
        self.store
            .get_review(id)
            .await
            .map_err(FoundationHitlError::Store)
    }

    /// Resolve a review (approve/reject/changes requested)
    pub async fn resolve_review(
        &self,
        id: &ReviewRequestId,
        response: ReviewResponse,
        resolved_by: String,
    ) -> HitlResult<()> {
        // Get current review
        let review = self
            .store
            .get_review(id)
            .await
            .map_err(FoundationHitlError::Store)?
            .ok_or_else(|| {
                FoundationHitlError::Store(crate::hitl::store::ReviewStoreError::NotFound(
                    id.as_str().to_string(),
                ))
            })?;

        // Check if already resolved
        if review.is_resolved() {
            return Err(FoundationHitlError::InvalidConfig(format!(
                "Review {} already resolved",
                id.as_str()
            )));
        }

        // Determine status from response
        let status = match &response {
            ReviewResponse::Approved { .. } => ReviewStatus::Approved,
            ReviewResponse::Rejected { .. } => ReviewStatus::Rejected,
            ReviewResponse::ChangesRequested { .. } => ReviewStatus::ChangesRequested,
            ReviewResponse::Deferred { .. } => ReviewStatus::Pending, // Keep pending for deferred
            // Handle future variants added to non_exhaustive enum
            _ => ReviewStatus::Pending,
        };

        // Update review
        self.store
            .update_review(
                id,
                status.clone(),
                Some(response.clone()),
                Some(resolved_by.clone()),
            )
            .await
            .map_err(FoundationHitlError::Store)?;

        // Record audit event
        if let Some(ref audit_store) = self.audit_store {
            let mut audit_event = ReviewAuditEvent::new(
                id.as_str(),
                ReviewAuditEventType::Resolved,
                Some(resolved_by.clone()),
            )
            .with_execution_id(&review.execution_id);

            if let Some(ref node_id) = review.node_id {
                audit_event = audit_event.with_node_id(node_id);
            }

            if let Some(tenant_id) = review.metadata.tenant_id {
                audit_event = audit_event.with_tenant_id(tenant_id);
            }

            audit_event = audit_event
                .with_data(
                    "status".to_string(),
                    serde_json::json!(format!("{:?}", status)),
                )
                .with_data(
                    "response".to_string(),
                    serde_json::to_value(&response).unwrap_or_default(),
                );

            if let Err(e) = audit_store.record_event(&audit_event).await {
                warn!("Failed to record audit event: {}", e);
                // Don't fail the resolution if audit fails
            }
        }

        // Get updated review for notification
        let updated_review = self
            .store
            .get_review(id)
            .await
            .map_err(FoundationHitlError::Store)?
            .ok_or_else(|| {
                FoundationHitlError::Store(crate::hitl::store::ReviewStoreError::NotFound(
                    id.as_str().to_string(),
                ))
            })?;

        // Notify about resolution
        if let Err(e) = self.notifier.notify_review_resolved(&updated_review).await {
            warn!("Failed to notify about review resolution: {}", e);
        }

        info!("Review resolved: {} (status: {:?})", id.as_str(), status);

        Ok(())
    }

    /// Query audit events
    pub async fn query_audit_events(
        &self,
        query: &AuditLogQuery,
    ) -> HitlResult<Vec<ReviewAuditEvent>> {
        if let Some(ref audit_store) = self.audit_store {
            audit_store
                .query_events(query)
                .await
                .map_err(|e| FoundationHitlError::Audit(e.to_string()))
        } else {
            Err(FoundationHitlError::Audit(
                "Audit store not configured".to_string(),
            ))
        }
    }

    /// Get audit events for a review
    pub async fn get_review_audit_events(
        &self,
        review_id: &str,
    ) -> HitlResult<Vec<ReviewAuditEvent>> {
        if let Some(ref audit_store) = self.audit_store {
            audit_store
                .get_review_events(review_id)
                .await
                .map_err(|e| FoundationHitlError::Audit(e.to_string()))
        } else {
            Err(FoundationHitlError::Audit(
                "Audit store not configured".to_string(),
            ))
        }
    }

    /// Wait for a review to be resolved (with timeout)
    pub async fn wait_for_review(
        &self,
        id: &ReviewRequestId,
        timeout: Option<Duration>,
    ) -> HitlResult<ReviewResponse> {
        let timeout = timeout.unwrap_or(self.config.default_expiration);
        let start = std::time::Instant::now();
        let check_interval = Duration::from_millis(100);

        loop {
            // Check timeout
            if start.elapsed() > timeout {
                return Err(FoundationHitlError::InvalidConfig(format!(
                    "Review {} timed out",
                    id.as_str()
                )));
            }

            // Check review status
            if let Some(review) = self
                .store
                .get_review(id)
                .await
                .map_err(FoundationHitlError::Store)?
            {
                if review.is_expired() {
                    return Err(FoundationHitlError::InvalidConfig(format!(
                        "Review {} expired",
                        id.as_str()
                    )));
                }

                if let Some(response) = review.response {
                    return Ok(response);
                }
            }

            // Wait before next check
            tokio::time::sleep(check_interval).await;
        }
    }

    /// List pending reviews
    pub async fn list_pending(
        &self,
        tenant_id: Option<Uuid>,
        limit: Option<u64>,
    ) -> HitlResult<Vec<ReviewRequest>> {
        self.store
            .list_pending(tenant_id, limit)
            .await
            .map_err(FoundationHitlError::Store)
    }

    /// Query reviews with filters and pagination
    pub async fn query_reviews(
        &self,
        query: &ReviewQuery,
    ) -> HitlResult<Vec<ReviewRequest>> {
        self.store
            .query_reviews(query)
            .await
            .map_err(FoundationHitlError::Store)
    }

    /// Start background task for expiration checking
    pub fn start_expiration_checker(&self) -> tokio::task::JoinHandle<()> {
        let store = Arc::clone(&self.store);
        let interval_duration = self.config.expiration_check_interval;

        tokio::spawn(async move {
            let mut interval = interval(interval_duration);
            interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                // Check for expired reviews
                match store.list_expired().await {
                    Ok(expired) => {
                        for review in expired {
                            info!("Marking review {} as expired", review.id.as_str());
                            let _ = store
                                .update_review(
                                    &review.id,
                                    ReviewStatus::Expired,
                                    None,
                                    Some("system".to_string()),
                                )
                                .await;

                            // Record audit event for expiration (if audit store available)
                            // Note: We can't access audit_store here since it's in self
                            // This would need to be passed to the background task or handled differently
                        }
                    }
                    Err(e) => {
                        error!("Error checking expired reviews: {}", e);
                    }
                }
            }
        })
    }
}
