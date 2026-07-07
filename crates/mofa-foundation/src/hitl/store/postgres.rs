//! PostgreSQL Review Store Implementation

#[cfg(feature = "persistence-postgres")]
use async_trait::async_trait;
#[cfg(feature = "persistence-postgres")]
use mofa_kernel::hitl::{ReviewQuery, ReviewRequest, ReviewRequestId, ReviewStatus};
#[cfg(feature = "persistence-postgres")]
use serde_json;
#[cfg(feature = "persistence-postgres")]
use sqlx::{PgPool, Row};
#[cfg(feature = "persistence-postgres")]
use std::sync::Arc;
#[cfg(feature = "persistence-postgres")]
use uuid::Uuid;

#[cfg(feature = "persistence-postgres")]
use crate::hitl::store::{ReviewStore, ReviewStoreError};

#[cfg(feature = "persistence-postgres")]
/// PostgreSQL implementation of ReviewStore
pub struct PostgresReviewStore {
    pool: Arc<PgPool>,
}

#[cfg(feature = "persistence-postgres")]
impl PostgresReviewStore {
    /// Create a new PostgreSQL review store
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Initialize the database schema (create tables)
    pub async fn initialize(&self) -> Result<(), ReviewStoreError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS review_requests (
                id VARCHAR(255) PRIMARY KEY,
                execution_id VARCHAR(255) NOT NULL,
                node_id VARCHAR(255),
                review_type VARCHAR(50) NOT NULL,
                status VARCHAR(50) NOT NULL,
                context JSONB NOT NULL,
                metadata JSONB NOT NULL,
                created_at TIMESTAMPTZ NOT NULL,
                expires_at TIMESTAMPTZ,
                resolved_at TIMESTAMPTZ,
                resolved_by VARCHAR(255),
                response JSONB
            );
            
            CREATE INDEX IF NOT EXISTS idx_review_execution ON review_requests(execution_id);
            CREATE INDEX IF NOT EXISTS idx_review_status ON review_requests(status);
            CREATE INDEX IF NOT EXISTS idx_review_created ON review_requests(created_at);
            CREATE INDEX IF NOT EXISTS idx_review_expires ON review_requests(expires_at) WHERE expires_at IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_review_tenant ON review_requests((metadata->>'tenant_id'));
            "#
        )
        .execute(&*self.pool)
        .await
        .map_err(|e| ReviewStoreError::Query(e.to_string()))?;

        Ok(())
    }
}

#[cfg(feature = "persistence-postgres")]
#[async_trait]
impl ReviewStore for PostgresReviewStore {
    async fn create_review(&self, request: &ReviewRequest) -> Result<(), ReviewStoreError> {
        let context_json = serde_json::to_value(&request.context)
            .map_err(|e| ReviewStoreError::Serialization(e.to_string()))?;
        let metadata_json = serde_json::to_value(&request.metadata)
            .map_err(|e| ReviewStoreError::Serialization(e.to_string()))?;
        let response_json = request
            .response
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| ReviewStoreError::Serialization(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO review_requests (
                id, execution_id, node_id, review_type, status,
                context, metadata, created_at, expires_at,
                resolved_at, resolved_by, response
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(request.id.as_str())
        .bind(&request.execution_id)
        .bind(request.node_id.as_ref())
        .bind(format!("{:?}", request.review_type))
        .bind(format!("{:?}", request.status))
        .bind(context_json)
        .bind(metadata_json)
        .bind(request.created_at)
        .bind(request.expires_at)
        .bind(request.resolved_at)
        .bind(request.resolved_by.as_ref())
        .bind(response_json)
        .execute(&*self.pool)
        .await
        .map_err(|e| ReviewStoreError::Query(e.to_string()))?;

        Ok(())
    }

    async fn get_review(
        &self,
        id: &ReviewRequestId,
    ) -> Result<Option<ReviewRequest>, ReviewStoreError> {
        let row = sqlx::query("SELECT * FROM review_requests WHERE id = $1")
            .bind(id.as_str())
            .fetch_optional(&*self.pool)
            .await
            .map_err(|e| ReviewStoreError::Query(e.to_string()))?;

        if let Some(row) = row {
            self.row_to_review(row).await.map(Some)
        } else {
            Ok(None)
        }
    }

    async fn update_review(
        &self,
        id: &ReviewRequestId,
        status: ReviewStatus,
        response: Option<mofa_kernel::hitl::ReviewResponse>,
        resolved_by: Option<String>,
    ) -> Result<(), ReviewStoreError> {
        let response_json = response
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| ReviewStoreError::Serialization(e.to_string()))?;

        sqlx::query(
            r#"
            UPDATE review_requests
            SET status = $1, response = $2, resolved_by = $3, resolved_at = $4
            WHERE id = $5
            "#,
        )
        .bind(format!("{:?}", status))
        .bind(response_json)
        .bind(resolved_by.as_ref())
        .bind(Some(chrono::Utc::now()))
        .bind(id.as_str())
        .execute(&*self.pool)
        .await
        .map_err(|e| ReviewStoreError::Query(e.to_string()))?;

        Ok(())
    }

    async fn list_pending(
        &self,
        tenant_id: Option<Uuid>,
        limit: Option<u64>,
    ) -> Result<Vec<ReviewRequest>, ReviewStoreError> {
        let mut query = String::from("SELECT * FROM review_requests WHERE status = 'Pending'");

        if let Some(tenant_id) = tenant_id {
            query.push_str(&format!(" AND metadata->>'tenant_id' = '{}'", tenant_id));
        }

        query.push_str(" ORDER BY created_at DESC");

        if let Some(limit) = limit {
            query.push_str(&format!(" LIMIT {}", limit));
        }

        let rows = sqlx::query(&query)
            .fetch_all(&*self.pool)
            .await
            .map_err(|e| ReviewStoreError::Query(e.to_string()))?;

        let mut reviews = Vec::new();
        for row in rows {
            if let Ok(review) = self.row_to_review(row).await {
                reviews.push(review);
            }
        }

        Ok(reviews)
    }

    async fn list_by_execution(
        &self,
        execution_id: &str,
    ) -> Result<Vec<ReviewRequest>, ReviewStoreError> {
        let rows = sqlx::query(
            "SELECT * FROM review_requests WHERE execution_id = $1 ORDER BY created_at DESC",
        )
        .bind(execution_id)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| ReviewStoreError::Query(e.to_string()))?;

        let mut reviews = Vec::new();
        for row in rows {
            if let Ok(review) = self.row_to_review(row).await {
                reviews.push(review);
            }
        }

        Ok(reviews)
    }

    async fn query_reviews(
        &self,
        query: &ReviewQuery,
    ) -> Result<Vec<ReviewRequest>, ReviewStoreError> {
        let mut qb = sqlx::QueryBuilder::new("SELECT * FROM review_requests WHERE 1=1");

        if let Some(ref execution_id) = query.execution_id {
            qb.push(" AND execution_id = ");
            qb.push_bind(execution_id);
        }

        if let Some(ref tenant_id) = query.tenant_id {
            qb.push(" AND metadata->>'tenant_id' = ");
            qb.push_bind(tenant_id.to_string());
        }

        if let Some(ref status) = query.status {
            qb.push(" AND status = ");
            qb.push_bind(status);
        }

        qb.push(" ORDER BY created_at DESC");

        if let Some(limit) = query.limit {
            qb.push(" LIMIT ");
            qb.push_bind(limit as i64);
        }

        if let Some(offset) = query.offset {
            qb.push(" OFFSET ");
            qb.push_bind(offset as i64);
        }

        let rows = qb.build()
            .fetch_all(&*self.pool)
            .await
            .map_err(|e| ReviewStoreError::Query(e.to_string()))?;

        let mut reviews = Vec::new();
        for row in rows {
            if let Ok(review) = self.row_to_review(row).await {
                reviews.push(review);
            }
        }

        Ok(reviews)
    }

    async fn list_expired(&self) -> Result<Vec<ReviewRequest>, ReviewStoreError> {
        let rows = sqlx::query(
            "SELECT * FROM review_requests WHERE status = 'Pending' AND expires_at < NOW()",
        )
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| ReviewStoreError::Query(e.to_string()))?;

        let mut reviews = Vec::new();
        for row in rows {
            if let Ok(review) = self.row_to_review(row).await {
                reviews.push(review);
            }
        }

        Ok(reviews)
    }

    async fn cleanup_old_reviews(
        &self,
        before: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, ReviewStoreError> {
        let result = sqlx::query("DELETE FROM review_requests WHERE created_at < $1")
            .bind(before)
            .execute(&*self.pool)
            .await
            .map_err(|e| ReviewStoreError::Query(e.to_string()))?;

        Ok(result.rows_affected())
    }
}

#[cfg(feature = "persistence-postgres")]
impl PostgresReviewStore {
    async fn row_to_review(
        &self,
        row: sqlx::postgres::PgRow,
    ) -> Result<ReviewRequest, ReviewStoreError> {
        use mofa_kernel::hitl::{ReviewResponse, ReviewStatus, ReviewType};

        let id: String = row.get("id");
        let execution_id: String = row.get("execution_id");
        let node_id: Option<String> = row.get("node_id");
        let review_type_str: String = row.get("review_type");
        let status_str: String = row.get("status");
        let context_json: serde_json::Value = row.get("context");
        let metadata_json: serde_json::Value = row.get("metadata");
        let created_at: chrono::DateTime<chrono::Utc> = row.get("created_at");
        let expires_at: Option<chrono::DateTime<chrono::Utc>> = row.get("expires_at");
        let resolved_at: Option<chrono::DateTime<chrono::Utc>> = row.get("resolved_at");
        let resolved_by: Option<String> = row.get("resolved_by");
        let response_json: Option<serde_json::Value> = row.get("response");

        let review_type = match review_type_str.as_str() {
            "Approval" => ReviewType::Approval,
            "Feedback" => ReviewType::Feedback,
            "ChangesRequired" => ReviewType::ChangesRequired,
            "Informational" => ReviewType::Informational,
            _ => ReviewType::Approval,
        };

        let status = match status_str.as_str() {
            "Pending" => ReviewStatus::Pending,
            "Approved" => ReviewStatus::Approved,
            "Rejected" => ReviewStatus::Rejected,
            "ChangesRequested" => ReviewStatus::ChangesRequested,
            "Expired" => ReviewStatus::Expired,
            "Cancelled" => ReviewStatus::Cancelled,
            _ => ReviewStatus::Pending,
        };

        let context: mofa_kernel::hitl::ReviewContext = serde_json::from_value(context_json)
            .map_err(|e| ReviewStoreError::Serialization(e.to_string()))?;
        let metadata: mofa_kernel::hitl::ReviewMetadata = serde_json::from_value(metadata_json)
            .map_err(|e| ReviewStoreError::Serialization(e.to_string()))?;
        let response: Option<ReviewResponse> = response_json
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| ReviewStoreError::Serialization(e.to_string()))?;

        Ok(ReviewRequest {
            id: ReviewRequestId::new(id),
            execution_id,
            node_id,
            review_type,
            status,
            context,
            metadata,
            created_at,
            expires_at,
            resolved_at,
            resolved_by,
            response,
        })
    }
}
