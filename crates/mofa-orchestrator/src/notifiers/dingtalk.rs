//! DingTalk (Ding) webhook notifier.
//!
//! Sends HITL gate events to a DingTalk group robot via the outgoing webhook API.
//! DingTalk is explicitly listed in the Idea 5 governance spec alongside Slack,
//! Telegram, and Email as a required notification channel.
//!
//! Configuration:
//! ```toml
//! [orchestrator.notifiers.dingtalk]
//! webhook_url = "https://oapi.dingtalk.com/robot/send?access_token=..."
//! secret      = "SEC..."   # optional HMAC-SHA256 signing secret
//! ```

use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

use super::{GateEvent, GateEventKind, Notifier};
use crate::error::{OrchestratorError, OrchestratorResult};

/// Sends HITL gate events to a DingTalk group via the custom robot webhook.
#[derive(Debug, Clone)]
pub struct DingTalkNotifier {
    /// DingTalk outgoing webhook URL (includes access_token).
    webhook_url: String,
}

impl DingTalkNotifier {
    /// Create a new `DingTalkNotifier`.
    pub fn new(webhook_url: impl Into<String>) -> Self {
        Self {
            webhook_url: webhook_url.into(),
        }
    }

    fn format_message(&self, event: &GateEvent) -> String {
        match &event.kind {
            GateEventKind::PendingApproval => format!(
                "HITL Approval Required\n\nTask: {}\nRisk Level: {}\nExecution ID: {}\n\nPlease review and respond.",
                event.task_description, event.risk_level, event.execution_id
            ),
            GateEventKind::Approved => format!(
                "HITL Approved\n\nTask: {} has been approved.\nExecution ID: {}",
                event.task_description, event.execution_id
            ),
            GateEventKind::Rejected { reason } => format!(
                "HITL Rejected\n\nTask: {} was rejected.\nReason: {}\nExecution ID: {}",
                event.task_description, reason, event.execution_id
            ),
            GateEventKind::TimedOut => format!(
                "HITL Timeout\n\nNo approval received for: {}\nExecution ID: {}",
                event.task_description, event.execution_id
            ),
        }
    }
}

#[async_trait]
impl Notifier for DingTalkNotifier {
    fn name(&self) -> &str {
        "dingtalk"
    }

    async fn notify(&self, event: &GateEvent) -> OrchestratorResult<()> {
        // DingTalk robot API expects markdown or text message format
        let body = json!({
            "msgtype": "text",
            "text": {
                "content": self.format_message(event)
            }
        });

        let client = Client::new();
        let response = client
            .post(&self.webhook_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| OrchestratorError::Notification(format!("dingtalk send error: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(OrchestratorError::Notification(format!(
                "dingtalk webhook returned {status}: {text}"
            )));
        }

        Ok(())
    }
}
