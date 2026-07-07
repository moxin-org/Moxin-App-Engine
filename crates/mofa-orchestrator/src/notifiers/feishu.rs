//! Feishu (Lark) webhook notifier.
//!
//! Integration patterns ported from mofaclaw PR #57 where Feishu
//! notifications were shipped to production. This implementation adapts
//! those patterns to the [`Notifier`] trait used by the HITL governor.
//!
//! Configuration:
//! ```toml
//! [orchestrator.notifiers.feishu]
//! webhook_url = "https://open.feishu.cn/open-apis/bot/v2/hook/..."
//! ```

use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

use super::{GateEvent, GateEventKind, Notifier};
use crate::error::{OrchestratorError, OrchestratorResult};

/// Sends HITL gate events to a Feishu group chat via incoming webhook.
#[derive(Debug, Clone)]
pub struct FeishuNotifier {
    /// Feishu incoming webhook URL.
    webhook_url: String,
}

impl FeishuNotifier {
    /// Create a new `FeishuNotifier`.
    pub fn new(webhook_url: impl Into<String>) -> Self {
        Self {
            webhook_url: webhook_url.into(),
        }
    }

    fn format_message(&self, event: &GateEvent) -> String {
        match &event.kind {
            GateEventKind::PendingApproval => format!(
                "[HITL] Approval required for task '{}' (risk: {}, execution: {})",
                event.task_description, event.risk_level, event.execution_id
            ),
            GateEventKind::Approved => format!(
                "[HITL] Task '{}' approved (execution: {})",
                event.task_description, event.execution_id
            ),
            GateEventKind::Rejected { reason } => format!(
                "[HITL] Task '{}' rejected — {} (execution: {})",
                event.task_description, reason, event.execution_id
            ),
            GateEventKind::TimedOut => format!(
                "[HITL] Approval timeout for task '{}' (execution: {})",
                event.task_description, event.execution_id
            ),
        }
    }
}

#[async_trait]
impl Notifier for FeishuNotifier {
    fn name(&self) -> &str {
        "feishu"
    }

    async fn notify(&self, event: &GateEvent) -> OrchestratorResult<()> {
        let body = json!({
            "msg_type": "text",
            "content": {
                "text": self.format_message(event)
            }
        });

        let client = Client::new();
        let response = client
            .post(&self.webhook_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| OrchestratorError::Notification(format!("feishu send error: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(OrchestratorError::Notification(format!(
                "feishu webhook returned {status}: {text}"
            )));
        }

        Ok(())
    }
}
