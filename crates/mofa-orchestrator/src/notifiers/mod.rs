//! Pluggable notification backends for HITL governance events.
//!
//! Each notifier implements the [`Notifier`] trait. The [`HITLGovernor`] holds
//! a `Vec<Arc<dyn Notifier>>` and fans out to all registered backends on every
//! gate event.
//!
//! Shipped implementations:
//! - [`LogNotifier`]     — tracing-based, zero external dependencies (default)
//! - [`SlackNotifier`]   — Slack incoming webhook
//! - [`TelegramNotifier`] — Telegram Bot API (patterns proven in mofaclaw #54)
//! - [`FeishuNotifier`]  — Feishu webhook (patterns proven in mofaclaw #57)
//! - [`DingTalkNotifier`]   — DingTalk group robot webhook
//! - [`EmailNotifier`]      — HTTP mail relay (SendGrid / Mailgun compatible)
//! - [`WebSocketNotifier`]  — tokio broadcast channel; mofa-studio subscribes
//!                            for live approval-queue updates (no polling required)

pub mod log_notifier;
pub mod slack;
pub mod telegram;
pub mod feishu;
pub mod dingtalk;
pub mod email;
pub mod websocket;

pub use log_notifier::LogNotifier;
pub use slack::SlackNotifier;
pub use telegram::TelegramNotifier;
pub use feishu::FeishuNotifier;
pub use dingtalk::DingTalkNotifier;
pub use email::EmailNotifier;
pub use websocket::WebSocketNotifier;

use async_trait::async_trait;
use crate::error::OrchestratorResult;

/// A notification event emitted by the HITL governor when a gate fires.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GateEvent {
    /// Unique identifier for the swarm execution run.
    pub execution_id: String,
    /// The subtask that triggered the gate.
    pub task_id: String,
    /// Human-readable description of the task.
    pub task_description: String,
    /// Risk level as a string (Low / Medium / High / Critical).
    pub risk_level: String,
    /// Whether the gate is requesting approval or reporting a decision.
    pub kind: GateEventKind,
}

#[derive(Debug, Clone, serde::Serialize)]
#[non_exhaustive]
pub enum GateEventKind {
    /// Gate is waiting for human approval.
    PendingApproval,
    /// Human approved the task.
    Approved,
    /// Human rejected the task.
    Rejected { reason: String },
    /// Approval timed out.
    TimedOut,
}

/// Trait implemented by all notification backends.
#[async_trait]
pub trait Notifier: Send + Sync {
    /// Return a human-readable name for this notifier (used in logs).
    fn name(&self) -> &str;

    /// Send a gate event notification. Errors are logged but do not block
    /// the orchestrator — notifications are best-effort.
    async fn notify(&self, event: &GateEvent) -> OrchestratorResult<()>;
}
