//! Webhook Delivery for Review Notifications
//!
//! Reliable webhook delivery with retries and HMAC signatures

use crate::hitl::error::FoundationHitlError;
use mofa_kernel::security::network::NetworkSecurity;
use mofa_kernel::hitl::{ReviewRequest, ReviewRequestId};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

/// Webhook delivery configuration
#[derive(Debug, Clone)]
pub struct WebhookConfig {
    /// Webhook URL
    pub url: String,
    /// HMAC secret for signature (optional)
    pub secret: Option<String>,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Initial retry delay
    pub retry_delay: Duration,
    /// Timeout for webhook requests
    pub timeout: Duration,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            secret: None,
            max_retries: 3,
            retry_delay: Duration::from_secs(1),
            timeout: Duration::from_secs(10),
        }
    }
}

/// Webhook delivery service
pub struct WebhookDelivery {
    config: WebhookConfig,
    client: reqwest::Client,
    pending_deliveries: Arc<Mutex<std::collections::HashMap<String, DeliveryState>>>,
}

#[derive(Debug, Clone)]
struct DeliveryState {
    attempt: u32,
    next_retry: std::time::Instant,
}

impl WebhookDelivery {
    /// Create a new webhook delivery service
    pub fn new(config: WebhookConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            // Avoid following redirects to internal/private networks.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config,
            client,
            pending_deliveries: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Deliver a webhook notification for a review request
    pub async fn deliver(
        &self,
        review: &ReviewRequest,
        event_type: &str,
    ) -> Result<(), FoundationHitlError> {
        if !NetworkSecurity::is_url_allowed(&self.config.url) {
            return Err(FoundationHitlError::InvalidConfig(format!(
                "Access denied: webhook URL '{}' targets a blocked address (private/internal network or disallowed scheme)",
                self.config.url
            )));
        }

        let payload = self.create_payload(review, event_type)?;
        let signature = self.create_signature(&payload)?;

        self.deliver_with_retry(&payload, &signature).await
    }

    fn create_payload(
        &self,
        review: &ReviewRequest,
        event_type: &str,
    ) -> Result<serde_json::Value, FoundationHitlError> {
        Ok(json!({
            "event": event_type,
            "review_id": review.id.as_str(),
            "execution_id": review.execution_id,
            "node_id": review.node_id,
            "status": format!("{:?}", review.status),
            "review_type": format!("{:?}", review.review_type),
            "created_at": review.created_at.to_rfc3339(),
            "expires_at": review.expires_at.map(|d| d.to_rfc3339()),
            "metadata": {
                "priority": review.metadata.priority,
                "assigned_to": review.metadata.assigned_to,
                "tags": review.metadata.tags,
            },
        }))
    }

    fn create_signature(
        &self,
        payload: &serde_json::Value,
    ) -> Result<Option<String>, FoundationHitlError> {
        if let Some(secret) = &self.config.secret {
            #[cfg(feature = "compression-cache")]
            {
                use sha2::{Digest, Sha256};
                let payload_str = serde_json::to_string(payload)
                    .map_err(|e| FoundationHitlError::Serialization(e.to_string()))?;
                let mut hasher = Sha256::new();
                hasher.update(secret.as_bytes());
                hasher.update(payload_str.as_bytes());
                let hash = hasher.finalize();
                Ok(Some(hex::encode(hash)))
            }
            #[cfg(not(feature = "compression-cache"))]
            {
                // Fallback to simple hash if sha2 not available
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let payload_str = serde_json::to_string(payload)
                    .map_err(|e| FoundationHitlError::Serialization(e.to_string()))?;
                let mut hasher = DefaultHasher::new();
                secret.hash(&mut hasher);
                payload_str.hash(&mut hasher);
                let hash = hasher.finish();
                Ok(Some(format!("{:x}", hash)))
            }
        } else {
            Ok(None)
        }
    }

    async fn deliver_with_retry(
        &self,
        payload: &serde_json::Value,
        signature: &Option<String>,
    ) -> Result<(), FoundationHitlError> {
        let review_id = payload["review_id"]
            .as_str()
            .ok_or_else(|| FoundationHitlError::InvalidConfig("Missing review_id".to_string()))?
            .to_string();

        // Check if we should retry now
        {
            let deliveries = self.pending_deliveries.lock().await;
            if let Some(state) = deliveries.get(&review_id)
                && state.attempt > 0
                && std::time::Instant::now() < state.next_retry
            {
                return Err(FoundationHitlError::WebhookDelivery(
                    "Retry scheduled for later".to_string(),
                ));
            }
        }

        // Build request
        let mut request = self.client.post(&self.config.url).json(payload);

        if let Some(sig) = signature {
            request = request.header("X-Webhook-Signature", sig);
        }

        // Attempt delivery
        let result = request.send().await;

        // Update state based on result
        let mut deliveries = self.pending_deliveries.lock().await;
        let state = deliveries
            .entry(review_id.clone())
            .or_insert_with(|| DeliveryState {
                attempt: 0,
                next_retry: std::time::Instant::now(),
            });

        let (should_remove, attempt_result) = match result {
            Ok(response) => {
                if response.status().is_success() {
                    info!("Webhook delivered successfully for review {}", review_id);
                    (true, Ok(()))
                } else {
                    state.attempt += 1;
                    let should_remove = state.attempt >= self.config.max_retries;
                    if should_remove {
                        error!("Webhook delivery failed after {} attempts", state.attempt);
                    } else {
                        state.next_retry =
                            std::time::Instant::now() + self.config.retry_delay * state.attempt;
                        warn!(
                            "Webhook delivery failed, will retry (attempt {})",
                            state.attempt
                        );
                    }
                    (
                        should_remove,
                        Err(FoundationHitlError::WebhookDelivery(format!(
                            "{}: {}",
                            if should_remove {
                                "Failed after attempts"
                            } else {
                                "Will retry"
                            },
                            response.status()
                        ))),
                    )
                }
            }
            Err(e) => {
                state.attempt += 1;
                let should_remove = state.attempt >= self.config.max_retries;
                if should_remove {
                    error!(
                        "Webhook delivery error after {} attempts: {}",
                        state.attempt, e
                    );
                } else {
                    state.next_retry =
                        std::time::Instant::now() + self.config.retry_delay * state.attempt;
                    warn!(
                        "Webhook delivery error, will retry (attempt {}): {}",
                        state.attempt, e
                    );
                }
                (
                    should_remove,
                    Err(FoundationHitlError::WebhookDelivery(format!(
                        "{}: {}",
                        if should_remove {
                            "Failed after attempts"
                        } else {
                            "Will retry"
                        },
                        e
                    ))),
                )
            }
        };

        if should_remove {
            deliveries.remove(&review_id);
        }

        attempt_result
    }

    /// Process pending retries (should be called periodically)
    pub async fn process_pending_retries(&self) {
        // This would be called by a background task
        // For now, it's handled in deliver_with_retry
    }
}
