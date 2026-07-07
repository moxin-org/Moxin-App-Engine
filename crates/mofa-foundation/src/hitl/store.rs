//! Review Store Trait and Implementations
//!
//! Persistent storage for review requests

use async_trait::async_trait;
use mofa_kernel::hitl::{ReviewQuery, ReviewRequest, ReviewRequestId, ReviewStatus};
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ReviewStoreError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Query error: {0}")]
    Query(String),

    #[error("Record not found: {0}")]
    NotFound(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Constraint violation: {0}")]
    Constraint(String),
}

/// Review store trait for persistent storage
#[async_trait]
pub trait ReviewStore: Send + Sync {
    /// Create a new review request
    async fn create_review(&self, request: &ReviewRequest) -> Result<(), ReviewStoreError>;

    /// Get a review request by ID
    async fn get_review(
        &self,
        id: &ReviewRequestId,
    ) -> Result<Option<ReviewRequest>, ReviewStoreError>;

    /// Update review status and response
    async fn update_review(
        &self,
        id: &ReviewRequestId,
        status: ReviewStatus,
        response: Option<mofa_kernel::hitl::ReviewResponse>,
        resolved_by: Option<String>,
    ) -> Result<(), ReviewStoreError>;

    /// List pending reviews for a tenant
    async fn list_pending(
        &self,
        tenant_id: Option<Uuid>,
        limit: Option<u64>,
    ) -> Result<Vec<ReviewRequest>, ReviewStoreError>;

    /// List reviews by execution ID
    async fn list_by_execution(
        &self,
        execution_id: &str,
    ) -> Result<Vec<ReviewRequest>, ReviewStoreError>;

    /// Query reviews with filters and pagination
    async fn query_reviews(
        &self,
        query: &ReviewQuery,
    ) -> Result<Vec<ReviewRequest>, ReviewStoreError>;

    /// List expired reviews
    async fn list_expired(&self) -> Result<Vec<ReviewRequest>, ReviewStoreError>;

    /// Delete old reviews (cleanup)
    async fn cleanup_old_reviews(
        &self,
        before: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, ReviewStoreError>;
}

/// In-memory review store (for testing)
pub struct InMemoryReviewStore {
    reviews: Arc<parking_lot::RwLock<std::collections::HashMap<String, ReviewRequest>>>,
}

impl InMemoryReviewStore {
    pub fn new() -> Self {
        Self {
            reviews: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
        }
    }
}

#[async_trait]
impl ReviewStore for InMemoryReviewStore {
    async fn create_review(&self, request: &ReviewRequest) -> Result<(), ReviewStoreError> {
        let mut reviews = self.reviews.write();
        reviews.insert(request.id.as_str().to_string(), request.clone());
        Ok(())
    }

    async fn get_review(
        &self,
        id: &ReviewRequestId,
    ) -> Result<Option<ReviewRequest>, ReviewStoreError> {
        let reviews = self.reviews.read();
        Ok(reviews.get(id.as_str()).cloned())
    }

    async fn update_review(
        &self,
        id: &ReviewRequestId,
        status: ReviewStatus,
        response: Option<mofa_kernel::hitl::ReviewResponse>,
        resolved_by: Option<String>,
    ) -> Result<(), ReviewStoreError> {
        let mut reviews = self.reviews.write();
        if let Some(mut review) = reviews.get(id.as_str()).cloned() {
            review.status = status.clone();
            review.response = response;
            review.resolved_by = resolved_by;
            review.resolved_at = Some(chrono::Utc::now());
            reviews.insert(id.as_str().to_string(), review);
            Ok(())
        } else {
            Err(ReviewStoreError::NotFound(id.as_str().to_string()))
        }
    }

    async fn list_pending(
        &self,
        tenant_id: Option<Uuid>,
        limit: Option<u64>,
    ) -> Result<Vec<ReviewRequest>, ReviewStoreError> {
        let reviews = self.reviews.read();
        let mut pending: Vec<_> = reviews
            .values()
            .filter(|r| matches!(r.status, ReviewStatus::Pending))
            .filter(|r| tenant_id.is_none() || r.metadata.tenant_id == tenant_id)
            .cloned()
            .collect();

        pending.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        if let Some(limit) = limit {
            pending.truncate(limit as usize);
        }

        Ok(pending)
    }

    async fn list_by_execution(
        &self,
        execution_id: &str,
    ) -> Result<Vec<ReviewRequest>, ReviewStoreError> {
        let reviews = self.reviews.read();
        Ok(reviews
            .values()
            .filter(|r| r.execution_id == execution_id)
            .cloned()
            .collect())
    }

    async fn query_reviews(
        &self,
        query: &ReviewQuery,
    ) -> Result<Vec<ReviewRequest>, ReviewStoreError> {
        let reviews = self.reviews.read();
        let mut results: Vec<_> = reviews
            .values()
            .filter(|r| {
                if let Some(ref execution_id) = query.execution_id
                    && r.execution_id != *execution_id {
                        return false;
                    }
                if let Some(ref tenant_id) = query.tenant_id
                    && r.metadata.tenant_id != Some(*tenant_id) {
                        return false;
                    }
                if let Some(ref status_str) = query.status {
                    let r_status_str = format!("{:?}", r.status).to_lowercase();
                    if r_status_str != status_str.to_lowercase() {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();

        // Sort descending by creation date
        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Apply offset
        if let Some(offset) = query.offset {
            let offset_usize = offset as usize;
            if offset_usize >= results.len() {
                return Ok(Vec::new());
            }
            results = results.split_off(offset_usize);
        }

        // Apply limit
        if let Some(limit) = query.limit {
            results.truncate(limit as usize);
        }

        Ok(results)
    }

    async fn list_expired(&self) -> Result<Vec<ReviewRequest>, ReviewStoreError> {
        let now = chrono::Utc::now();
        let reviews = self.reviews.read();
        Ok(reviews
            .values()
            .filter(|r| r.is_expired() && matches!(r.status, ReviewStatus::Pending))
            .cloned()
            .collect())
    }

    async fn cleanup_old_reviews(
        &self,
        before: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, ReviewStoreError> {
        let mut reviews = self.reviews.write();
        let mut count = 0;
        reviews.retain(|_, r| {
            if r.created_at < before {
                count += 1;
                false
            } else {
                true
            }
        });
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofa_kernel::hitl::{ReviewRequest, ReviewResponse, ReviewStatus, ReviewType};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn create_test_review(execution_id: &str) -> ReviewRequest {
        use mofa_kernel::hitl::{ExecutionStep, ExecutionTrace, ReviewContext};

        let trace = ExecutionTrace {
            steps: vec![ExecutionStep {
                step_id: "test_step".to_string(),
                step_type: "test".to_string(),
                timestamp_ms: 0,
                input: None,
                output: None,
                metadata: HashMap::new(),
            }],
            duration_ms: 100,
        };

        let context = ReviewContext::new(trace, serde_json::json!({}));
        ReviewRequest::new(execution_id, ReviewType::Approval, context)
    }

    #[tokio::test]
    async fn test_create_and_get_review() {
        let store = InMemoryReviewStore::new();
        let review = create_test_review("exec-1");
        let review_id = review.id.clone();

        // Create review
        store.create_review(&review).await.unwrap();

        // Get review
        let retrieved = store.get_review(&review_id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().execution_id, "exec-1");
    }

    #[tokio::test]
    async fn test_update_review_status() {
        let store = InMemoryReviewStore::new();
        let review = create_test_review("exec-1");
        let review_id = review.id.clone();

        store.create_review(&review).await.unwrap();

        // Update status
        store
            .update_review(
                &review_id,
                ReviewStatus::Approved,
                Some(ReviewResponse::Approved { comment: None }),
                Some("reviewer@test.com".to_string()),
            )
            .await
            .unwrap();

        // Verify update
        let updated = store.get_review(&review_id).await.unwrap().unwrap();
        assert_eq!(updated.status, ReviewStatus::Approved);
        assert_eq!(updated.resolved_by, Some("reviewer@test.com".to_string()));
    }

    #[tokio::test]
    async fn test_list_pending_reviews() {
        let store = InMemoryReviewStore::new();

        // Create multiple reviews
        let review1 = create_test_review("exec-1");
        let review2 = create_test_review("exec-2");
        let review3 = create_test_review("exec-3");

        store.create_review(&review1).await.unwrap();
        store.create_review(&review2).await.unwrap();
        store.create_review(&review3).await.unwrap();

        // List pending
        let pending = store.list_pending(None, None).await.unwrap();
        assert_eq!(pending.len(), 3);

        // List with limit
        let limited = store.list_pending(None, Some(2)).await.unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[tokio::test]
    async fn test_list_pending_by_tenant() {
        let store = InMemoryReviewStore::new();
        let tenant_1 = Uuid::new_v4();
        let tenant_2 = Uuid::new_v4();

        // Create reviews for different tenants
        let mut review1 = create_test_review("exec-1");
        review1.metadata.tenant_id = Some(tenant_1);

        let mut review2 = create_test_review("exec-2");
        review2.metadata.tenant_id = Some(tenant_2);

        let mut review3 = create_test_review("exec-3");
        review3.metadata.tenant_id = Some(tenant_1);

        store.create_review(&review1).await.unwrap();
        store.create_review(&review2).await.unwrap();
        store.create_review(&review3).await.unwrap();

        // List for tenant_1
        let tenant_1_reviews = store.list_pending(Some(tenant_1), None).await.unwrap();
        assert_eq!(tenant_1_reviews.len(), 2);

        // List for tenant_2
        let tenant_2_reviews = store.list_pending(Some(tenant_2), None).await.unwrap();
        assert_eq!(tenant_2_reviews.len(), 1);
    }

    #[tokio::test]
    async fn test_list_by_execution_id() {
        let store = InMemoryReviewStore::new();

        let review1 = create_test_review("exec-1");
        let review2 = create_test_review("exec-1");
        let review3 = create_test_review("exec-2");

        store.create_review(&review1).await.unwrap();
        store.create_review(&review2).await.unwrap();
        store.create_review(&review3).await.unwrap();

        // List by execution ID
        let exec_1_reviews = store.list_by_execution("exec-1").await.unwrap();
        assert_eq!(exec_1_reviews.len(), 2);

        let exec_2_reviews = store.list_by_execution("exec-2").await.unwrap();
        assert_eq!(exec_2_reviews.len(), 1);
    }

    #[tokio::test]
    async fn test_update_nonexistent_review() {
        let store = InMemoryReviewStore::new();
        let review_id = ReviewRequestId::new("nonexistent-review-id");

        // Try to update non-existent review
        let result = store
            .update_review(&review_id, ReviewStatus::Approved, None, None)
            .await;

        assert!(result.is_err());
        if let Err(ReviewStoreError::NotFound(id)) = result {
            assert_eq!(id, review_id.as_str());
        } else {
            panic!("Expected NotFound error");
        }
    }

    #[tokio::test]
    async fn test_query_reviews() {
        let store = InMemoryReviewStore::new();
        let tenant_1 = Uuid::new_v4();

        // Create reviews
        let mut review1 = create_test_review("exec-1");
        review1.metadata.tenant_id = Some(tenant_1);
        
        let mut review2 = create_test_review("exec-1");
        review2.status = ReviewStatus::Approved;
        review2.metadata.tenant_id = Some(tenant_1);

        let mut review3 = create_test_review("exec-2");
        review3.status = ReviewStatus::Rejected;
        review3.metadata.tenant_id = Some(tenant_1);

        let review4 = create_test_review("exec-3");

        store.create_review(&review1).await.unwrap();
        // fake small delay so created_at differs if they rely on it for sorting
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        store.create_review(&review2).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        store.create_review(&review3).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        store.create_review(&review4).await.unwrap();

        // 1. Filter by tenant_id
        let query = ReviewQuery {
            tenant_id: Some(tenant_1),
            ..Default::default()
        };
        let res = store.query_reviews(&query).await.unwrap();
        assert_eq!(res.len(), 3);

        // 2. Filter by status
        let query = ReviewQuery {
            status: Some("Approved".to_string()),
            ..Default::default()
        };
        let res = store.query_reviews(&query).await.unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].execution_id, "exec-1");

        // 3. Filter by execution_id
        let query = ReviewQuery {
            execution_id: Some("exec-1".to_string()),
            ..Default::default()
        };
        let res = store.query_reviews(&query).await.unwrap();
        assert_eq!(res.len(), 2);

        // 4. Test pagination (limit and offset)
        // All reviews, sorted by created_at desc
        let query = ReviewQuery {
            limit: Some(2),
            offset: Some(1),
            ..Default::default()
        };
        let res = store.query_reviews(&query).await.unwrap();
        assert_eq!(res.len(), 2);
        // Latest is review4, so offset 1 is review3, offset 2 is review2
        assert_eq!(res[0].execution_id, "exec-2"); // review3
        assert_eq!(res[1].status, ReviewStatus::Approved); // review2
    }
}

impl Default for InMemoryReviewStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "persistence-postgres")]
mod postgres;
#[cfg(feature = "persistence-postgres")]
pub use postgres::PostgresReviewStore;
