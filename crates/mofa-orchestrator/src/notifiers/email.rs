//! Email notifier for HITL governance events.
//!
//! Sends gate events via an HTTP email delivery API (SendGrid, Mailgun, or any
//! compatible relay). This keeps the implementation dependency-free beyond the
//! `reqwest` client that is already required for other notifiers.
//!
//! Configuration:
//! ```toml
//! [orchestrator.notifiers.email]
//! api_url    = "https://api.sendgrid.com/v3/mail/send"
//! api_key    = "SG...."        # Bearer token
//! from       = "mofa@example.com"
//! to         = ["ops@example.com", "sre@example.com"]
//! ```
//!
//! The payload follows the SendGrid v3 / Mailgun message schema, which most
//! HTTP mail relays accept with minor field renaming.

use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

use super::{GateEvent, GateEventKind, Notifier};
use crate::error::{OrchestratorError, OrchestratorResult};

/// Sends HITL gate events via an HTTP email delivery API.
///
/// Uses a SendGrid-compatible JSON payload so the same struct works with
/// SendGrid, Mailgun (via their compatibility endpoint), Postmark, and
/// self-hosted relays such as Postal or Haraka.
#[derive(Debug, Clone)]
pub struct EmailNotifier {
    /// Base URL of the HTTP mail API (e.g. `https://api.sendgrid.com/v3/mail/send`).
    api_url: String,
    /// Bearer token or API key passed in the `Authorization` header.
    api_key: String,
    /// Sender address shown in the `From` field.
    from: String,
    /// One or more recipient addresses.
    to: Vec<String>,
}

impl EmailNotifier {
    /// Create a new `EmailNotifier`.
    ///
    /// # Arguments
    /// * `api_url` — HTTP mail API endpoint
    /// * `api_key` — Bearer token for `Authorization: Bearer <key>`
    /// * `from`    — sender email address
    /// * `to`      — list of recipient email addresses (at least one required)
    pub fn new(
        api_url: impl Into<String>,
        api_key: impl Into<String>,
        from: impl Into<String>,
        to: Vec<String>,
    ) -> Self {
        Self {
            api_url: api_url.into(),
            api_key: api_key.into(),
            from: from.into(),
            to,
        }
    }

    fn subject(&self, event: &GateEvent) -> String {
        match &event.kind {
            GateEventKind::PendingApproval => format!(
                "[MoFA HITL] Approval Required — {} ({})",
                event.task_id, event.risk_level
            ),
            GateEventKind::Approved => format!(
                "[MoFA HITL] Approved — {} ({})",
                event.task_id, event.execution_id
            ),
            GateEventKind::Rejected { .. } => format!(
                "[MoFA HITL] Rejected — {} ({})",
                event.task_id, event.execution_id
            ),
            GateEventKind::TimedOut => format!(
                "[MoFA HITL] Timed Out — {} ({})",
                event.task_id, event.execution_id
            ),
        }
    }

    fn body(&self, event: &GateEvent) -> String {
        match &event.kind {
            GateEventKind::PendingApproval => format!(
                "MoFA Swarm Orchestrator — HITL Approval Required\n\n\
                 Execution ID : {}\n\
                 Task ID      : {}\n\
                 Task         : {}\n\
                 Risk Level   : {}\n\n\
                 Please log in and approve or reject this task.",
                event.execution_id,
                event.task_id,
                event.task_description,
                event.risk_level,
            ),
            GateEventKind::Approved => format!(
                "MoFA Swarm Orchestrator — Task Approved\n\n\
                 Execution ID : {}\n\
                 Task ID      : {}\n\
                 Task         : {}\n\n\
                 Execution continues.",
                event.execution_id, event.task_id, event.task_description,
            ),
            GateEventKind::Rejected { reason } => format!(
                "MoFA Swarm Orchestrator — Task Rejected\n\n\
                 Execution ID : {}\n\
                 Task ID      : {}\n\
                 Task         : {}\n\
                 Reason       : {}\n\n\
                 The task has been halted.",
                event.execution_id, event.task_id, event.task_description, reason,
            ),
            GateEventKind::TimedOut => format!(
                "MoFA Swarm Orchestrator — Approval Timed Out\n\n\
                 Execution ID : {}\n\
                 Task ID      : {}\n\
                 Task         : {}\n\n\
                 No approval was received within the deadline. The task has been escalated.",
                event.execution_id, event.task_id, event.task_description,
            ),
        }
    }
}

#[async_trait]
impl Notifier for EmailNotifier {
    fn name(&self) -> &str {
        "email"
    }

    async fn notify(&self, event: &GateEvent) -> OrchestratorResult<()> {
        if self.to.is_empty() {
            return Err(OrchestratorError::Notification(
                "email notifier: no recipients configured".to_string(),
            ));
        }

        // Build a SendGrid v3 compatible payload.
        // Most HTTP mail relays accept this schema or a near-identical variant.
        let personalizations: Vec<serde_json::Value> = self
            .to
            .iter()
            .map(|addr| json!({ "to": [{ "email": addr }] }))
            .collect();

        let body = json!({
            "personalizations": personalizations,
            "from": { "email": self.from },
            "subject": self.subject(event),
            "content": [{
                "type": "text/plain",
                "value": self.body(event)
            }]
        });

        let client = Client::new();
        let response = client
            .post(&self.api_url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| OrchestratorError::Notification(format!("email send error: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(OrchestratorError::Notification(format!(
                "email API returned {status}: {text}"
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifiers::GateEventKind;

    fn make_event(kind: GateEventKind) -> GateEvent {
        GateEvent {
            execution_id: "exec-001".to_string(),
            task_id: "task-audit".to_string(),
            task_description: "Run financial compliance audit".to_string(),
            risk_level: "High".to_string(),
            kind,
        }
    }

    fn notifier() -> EmailNotifier {
        EmailNotifier::new(
            "https://api.sendgrid.com/v3/mail/send",
            "SG.test",
            "mofa@example.com",
            vec!["ops@example.com".to_string()],
        )
    }

    #[test]
    fn subject_contains_task_id_for_pending() {
        let n = notifier();
        let s = n.subject(&make_event(GateEventKind::PendingApproval));
        assert!(s.contains("task-audit"), "subject: {s}");
        assert!(s.contains("Approval Required"), "subject: {s}");
    }

    #[test]
    fn subject_contains_execution_id_for_approved() {
        let n = notifier();
        let s = n.subject(&make_event(GateEventKind::Approved));
        assert!(s.contains("exec-001"), "subject: {s}");
    }

    #[test]
    fn body_contains_reason_for_rejected() {
        let n = notifier();
        let b = n.body(&make_event(GateEventKind::Rejected {
            reason: "policy violation".to_string(),
        }));
        assert!(b.contains("policy violation"), "body: {b}");
    }

    #[test]
    fn body_contains_escalation_text_for_timeout() {
        let n = notifier();
        let b = n.body(&make_event(GateEventKind::TimedOut));
        assert!(b.contains("escalated"), "body: {b}");
    }

    #[test]
    fn notifier_name_is_email() {
        let n = notifier();
        assert_eq!(n.name(), "email");
    }

    #[tokio::test]
    async fn returns_error_when_no_recipients() {
        let n = EmailNotifier::new(
            "https://api.sendgrid.com/v3/mail/send",
            "key",
            "from@example.com",
            vec![],
        );
        let result = n.notify(&make_event(GateEventKind::PendingApproval)).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("no recipients"), "err: {msg}");
    }
}
