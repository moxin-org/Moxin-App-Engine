//! Review Policy
//! Trait for defining review policies

use crate::hitl::{HitlResult, ReviewContext, ReviewRequest, ReviewType};
use async_trait::async_trait;

/// Review policy trait
#[async_trait]
pub trait ReviewPolicy: Send + Sync {
    async fn should_request_review(
        &self,
        context: &ReviewContext,
    ) -> HitlResult<Option<ReviewRequest>>;

    async fn can_auto_approve(&self, request: &ReviewRequest) -> HitlResult<bool>;

    fn name(&self) -> &str;
}

/// Always request review policy
pub struct AlwaysReviewPolicy;

#[async_trait]
impl ReviewPolicy for AlwaysReviewPolicy {
    async fn should_request_review(
        &self,
        context: &ReviewContext,
    ) -> HitlResult<Option<ReviewRequest>> {
        let request = ReviewRequest::new("unknown", ReviewType::Approval, context.clone());
        Ok(Some(request))
    }

    async fn can_auto_approve(&self, _request: &ReviewRequest) -> HitlResult<bool> {
        Ok(false)
    }

    fn name(&self) -> &str {
        "AlwaysReviewPolicy"
    }
}

/// Never request review policy
pub struct NeverReviewPolicy;

#[async_trait]
impl ReviewPolicy for NeverReviewPolicy {
    async fn should_request_review(
        &self,
        _context: &ReviewContext,
    ) -> HitlResult<Option<ReviewRequest>> {
        Ok(None)
    }

    async fn can_auto_approve(&self, _request: &ReviewRequest) -> HitlResult<bool> {
        Ok(true)
    }

    fn name(&self) -> &str {
        "NeverReviewPolicy"
    }
}

/// NEW: Audit-Aware Policy
pub struct AuditValidationPolicy;

#[async_trait]
impl ReviewPolicy for AuditValidationPolicy {
    async fn should_request_review(
        &self,
        context: &ReviewContext,
    ) -> HitlResult<Option<ReviewRequest>> {
        // Match the key name used in context.rs ("audit_trail")
        if let Some(_audit_val) = context.additional.get("audit_trail") {
            let request = ReviewRequest::new(
                "audit_check".to_string(),
                ReviewType::Approval,
                context.clone(),
            );
            return Ok(Some(request));
        }

        Ok(None)
    }

    async fn can_auto_approve(&self, _request: &ReviewRequest) -> HitlResult<bool> {
        Ok(false) // Humans must sign off on luxury/fintech audits
    }

    fn name(&self) -> &str {
        "AuditValidationPolicy"
    }
}

pub struct WhaleGuardPolicy {
    pub sol_threshold: f64,
}

#[async_trait]
impl ReviewPolicy for WhaleGuardPolicy {
    async fn should_request_review(
        &self,
        context: &ReviewContext,
    ) -> HitlResult<Option<ReviewRequest>> {
        // Look into the 'audit_trail' stored in context.additional
        if let Some(audit_val) = context.additional.get("audit_trail") {
            // Extract the amount from the blockchain metadata
            let amount = audit_val["metadata"]["amount"].as_f64().unwrap_or(0.0);

            // If it's a "Whale" amount, trigger the pause button
            if amount >= self.sol_threshold {
                let request = ReviewRequest::new(
                    "whale_protection_trigger".to_string(),
                    ReviewType::Approval,
                    context.clone(),
                );
                return Ok(Some(request));
            }
        }
        Ok(None)
    }

    async fn can_auto_approve(&self, _request: &ReviewRequest) -> HitlResult<bool> {
        Ok(false) // High-value transfers ALWAYS need a human signature
    }

    fn name(&self) -> &str {
        "WhaleGuardPolicy"
    }
}





#[cfg(test)]
mod policy_tests {
    use super::*;
    use crate::hitl::ReviewContext;
    use crate::hitl::context::{AuditingData, ExecutionTrace};
    use serde_json::json;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_audit_validation_policy_triggers() {
        let policy = AuditValidationPolicy;

        let trace = ExecutionTrace {
            steps: vec![],
            duration_ms: 0,
        };
        let audit = AuditingData {
            intent: "Luxury Purchase".to_string(),
            result: "Approved".to_string(),
            relevant_trace_steps: vec![],
            metadata: HashMap::new(),
            policy_status: "Pass".to_string(),
        };

        let context = ReviewContext::new(trace, json!({})).with_auditing_data(audit);

        let result = policy.should_request_review(&context).await.unwrap();

        assert!(result.is_some());
        let request = result.unwrap();
        assert_eq!(request.execution_id, "audit_check");

        println!("✅ Audit Guard successfully caught the transaction!");
    } // <--- Added this to close the first test

    #[tokio::test]
    async fn test_whale_guard_triggers_on_large_amount() {
        let policy = WhaleGuardPolicy { sol_threshold: 100.0 };
        let trace = ExecutionTrace { steps: vec![], duration_ms: 0 };
        
        // Simulate a 150 SOL Whale Trade
        let audit = AuditingData {
            intent: "Whale Trade".to_string(),
            result: "Execute".to_string(),
            relevant_trace_steps: vec![],
            metadata: HashMap::from([("amount".to_string(), json!(150.0))]),
            policy_status: "Pending".to_string(),
        };

        let context = ReviewContext::new(trace, json!({})).with_auditing_data(audit);
        let result = policy.should_request_review(&context).await.unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap().execution_id, "whale_protection_trigger");
        
        println!("✅ Whale Guard successfully blocked the 150 SOL transaction!");
    }
} // <--- Module ends here