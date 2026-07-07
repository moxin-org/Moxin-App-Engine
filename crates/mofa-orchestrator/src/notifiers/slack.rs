//! Slack incoming webhook notifier.

use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

use super::{GateEvent, GateEventKind, Notifier};
use crate::error::{OrchestratorError, OrchestratorResult};

/// Sends HITL gate events to a Slack channel via incoming webhook.
#[derive(Debug, Clone)]
pub struct SlackNotifier {
    webhook_url: String,
}

impl SlackNotifier {
    /// Create a new `SlackNotifier`.
    pub fn new(webhook_url: impl Into<String>) -> Self {
        Self {
            webhook_url: webhook_url.into(),
        }
    }

    fn format_message(&self, event: &GateEvent) -> String {
        match &event.kind {
            GateEventKind::PendingApproval => format!(
                ":warning: *HITL Approval Required*\nTask: `{}`\nRisk: `{}`\nExecution: `{}`",
                event.task_description, event.risk_level, event.execution_id
            ),
            GateEventKind::Approved => format!(
                ":white_check_mark: *HITL Approved*\nTask: `{}` — execution: `{}`",
                event.task_description, event.execution_id
            ),
            GateEventKind::Rejected { reason } => format!(
                ":x: *HITL Rejected*\nTask: `{}`\nReason: {}\nExecution: `{}`",
                event.task_description, reason, event.execution_id
            ),
            GateEventKind::TimedOut => format!(
                ":hourglass_flowing_sand: *HITL Timeout*\nTask: `{}` — no approval received\nExecution: `{}`",
                event.task_description, event.execution_id
            ),
        }
    }
}

#[async_trait]
impl Notifier for SlackNotifier {
    fn name(&self) -> &str {
        "slack"
    }

    async fn notify(&self, event: &GateEvent) -> OrchestratorResult<()> {
        let body = json!({ "text": self.format_message(event) });

        let client = Client::new();
        let response = client
            .post(&self.webhook_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| OrchestratorError::Notification(format!("slack send error: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(OrchestratorError::Notification(format!(
                "slack webhook returned {status}: {text}"
            )));
        }

        Ok(())
    }
}
