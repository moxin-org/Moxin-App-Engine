//! Telegram Bot API notifier.
//!
//! Integration patterns ported from mofaclaw PR #54 where Telegram
//! notifications were shipped to production. This implementation adapts
//! those patterns to the [`Notifier`] trait used by the HITL governor.
//!
//! Configuration:
//! ```toml
//! [orchestrator.notifiers.telegram]
//! bot_token = "123456:ABC..."
//! chat_id   = "-100123456789"
//! ```

use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

use super::{GateEvent, GateEventKind, Notifier};
use crate::error::{OrchestratorError, OrchestratorResult};

/// Sends HITL gate events to a Telegram chat via the Bot API.
#[derive(Debug, Clone)]
pub struct TelegramNotifier {
    /// Telegram bot token (format: `{bot_id}:{secret}`).
    bot_token: String,
    /// Target chat ID — group, channel, or DM.
    chat_id: String,
}

impl TelegramNotifier {
    /// Create a new `TelegramNotifier`.
    pub fn new(bot_token: impl Into<String>, chat_id: impl Into<String>) -> Self {
        Self {
            bot_token: bot_token.into(),
            chat_id: chat_id.into(),
        }
    }

    fn format_message(&self, event: &GateEvent) -> String {
        match &event.kind {
            GateEventKind::PendingApproval => format!(
                "HITL Approval Required\n\nTask: {}\nRisk: {}\nExecution: {}\n\nPlease review and approve or reject.",
                event.task_description, event.risk_level, event.execution_id
            ),
            GateEventKind::Approved => format!(
                "HITL Approved\n\nTask: {} has been approved.\nExecution: {}",
                event.task_description, event.execution_id
            ),
            GateEventKind::Rejected { reason } => format!(
                "HITL Rejected\n\nTask: {} was rejected.\nReason: {}\nExecution: {}",
                event.task_description, reason, event.execution_id
            ),
            GateEventKind::TimedOut => format!(
                "HITL Timeout\n\nNo approval received for task: {}\nExecution: {}",
                event.task_description, event.execution_id
            ),
        }
    }
}

#[async_trait]
impl Notifier for TelegramNotifier {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn notify(&self, event: &GateEvent) -> OrchestratorResult<()> {
        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            self.bot_token
        );
        let body = json!({
            "chat_id": self.chat_id,
            "text": self.format_message(event),
            "parse_mode": "HTML"
        });

        let client = Client::new();
        let response = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| OrchestratorError::Notification(format!("telegram send error: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(OrchestratorError::Notification(format!(
                "telegram API returned {status}: {text}"
            )));
        }

        Ok(())
    }
}
