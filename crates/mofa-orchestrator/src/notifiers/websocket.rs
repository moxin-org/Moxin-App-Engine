//! WebSocket broadcast notifier for HITL governance events.
//!
//! [`WebSocketNotifier`] distributes gate events over a `tokio::sync::broadcast`
//! channel. Any number of consumers — most notably **mofa-studio** (the Makepad
//! desktop Observatory) — can call [`WebSocketNotifier::subscribe`] to receive
//! a live stream of serialized gate events without polling.
//!
//! # Design
//!
//! The notifier itself holds only a `broadcast::Sender<String>`. Each gate event
//! is serialized to a JSON string and sent to the channel. Downstream consumers
//! receive the JSON over whatever transport they prefer (WebSocket, SSE, stdout).
//! Wiring to an Axum WebSocket endpoint is done in the orchestrator layer, keeping
//! this struct transport-agnostic and trivially testable without a running server.
//!
//! # mofa-studio integration
//!
//! ```text
//! SwarmOrchestrator
//!   -> HITLGovernor.notify(event)
//!      -> WebSocketNotifier.notify(event)   // serialises + sends to broadcast channel
//!
//! mofa-studio (Makepad UI)
//!   <- Axum WS handler                      // subscribes to broadcast channel
//!      <- WebSocketNotifier.subscribe()      // receives JSON gate event frames
//! ```
//!
//! This gives mofa-studio real-time approval-queue updates and live swarm graph
//! state without any additional infrastructure beyond the orchestrator process.
//!
//! # Configuration
//!
//! ```toml
//! [orchestrator.notifiers.websocket]
//! channel_capacity = 128   # broadcast ring-buffer depth (default: 128)
//! ```

use async_trait::async_trait;
use tokio::sync::broadcast;

use super::{GateEvent, Notifier};
use crate::error::{OrchestratorError, OrchestratorResult};

/// Default broadcast channel capacity (ring-buffer depth).
///
/// Slow consumers lag behind by at most this many events before their receiver
/// returns [`broadcast::error::RecvError::Lagged`]. Tune upward for high-volume
/// swarms or downstream consumers that do significant per-event work.
const DEFAULT_CAPACITY: usize = 128;

/// Broadcasts serialized HITL gate events to all subscribed WebSocket consumers.
///
/// Create one instance per orchestrator and share it via [`Arc`]. Each connected
/// mofa-studio client holds one [`broadcast::Receiver<String>`] obtained from
/// [`WebSocketNotifier::subscribe`].
///
/// [`Arc`]: std::sync::Arc
#[derive(Clone)]
pub struct WebSocketNotifier {
    sender: broadcast::Sender<String>,
}

impl std::fmt::Debug for WebSocketNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSocketNotifier")
            .field("receiver_count", &self.sender.receiver_count())
            .finish()
    }
}

impl WebSocketNotifier {
    /// Create a new `WebSocketNotifier` with the default channel capacity.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// Create a new `WebSocketNotifier` with a custom ring-buffer capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Subscribe to the gate-event stream.
    ///
    /// Each call returns an independent receiver. Receivers that fall more than
    /// `capacity` events behind will receive [`broadcast::error::RecvError::Lagged`]
    /// and must resync.
    ///
    /// # Typical usage (Axum WebSocket handler)
    ///
    /// ```rust,no_run
    /// use mofa_orchestrator::notifiers::websocket::WebSocketNotifier;
    /// use std::sync::Arc;
    ///
    /// async fn ws_handler(notifier: Arc<WebSocketNotifier>) {
    ///     let mut rx = notifier.subscribe();
    ///     while let Ok(json) = rx.recv().await {
    ///         // forward `json` frame to the connected WebSocket client
    ///         println!("event: {json}");
    ///     }
    /// }
    /// ```
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.sender.subscribe()
    }

    /// Number of active receivers currently subscribed to this notifier.
    pub fn receiver_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for WebSocketNotifier {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Notifier for WebSocketNotifier {
    fn name(&self) -> &str {
        "websocket"
    }

    async fn notify(&self, event: &GateEvent) -> OrchestratorResult<()> {
        // Serialize the gate event to JSON. Downstream consumers parse the
        // frame according to their own schema (mofa-studio uses the full struct).
        let json = serde_json::to_string(event).map_err(|e| {
            OrchestratorError::Notification(format!("websocket serialise error: {e}"))
        })?;

        // `send` returns the number of active receivers. An error means there
        // are no receivers at the moment — this is expected when no studio client
        // is connected. Log at trace level and continue; notifications are
        // best-effort.
        match self.sender.send(json) {
            Ok(n) => {
                tracing::trace!(receivers = n, "websocket gate event delivered");
            }
            Err(_) => {
                tracing::trace!("websocket notifier: no active receivers, event dropped");
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifiers::{GateEvent, GateEventKind};

    fn make_event() -> GateEvent {
        GateEvent {
            execution_id: "exec-ws-001".to_string(),
            task_id: "task-compliance".to_string(),
            task_description: "Run PII audit on customer records".to_string(),
            risk_level: "Critical".to_string(),
            kind: GateEventKind::PendingApproval,
        }
    }

    #[tokio::test]
    async fn subscriber_receives_event() {
        let notifier = WebSocketNotifier::new();
        let mut rx = notifier.subscribe();

        notifier.notify(&make_event()).await.unwrap();

        let json = rx.recv().await.unwrap();
        assert!(json.contains("exec-ws-001"), "json: {json}");
        assert!(json.contains("task-compliance"), "json: {json}");
    }

    #[tokio::test]
    async fn multiple_subscribers_each_receive_event() {
        let notifier = WebSocketNotifier::new();
        let mut rx1 = notifier.subscribe();
        let mut rx2 = notifier.subscribe();

        notifier.notify(&make_event()).await.unwrap();

        let j1 = rx1.recv().await.unwrap();
        let j2 = rx2.recv().await.unwrap();
        assert_eq!(j1, j2, "both subscribers should receive the same payload");
    }

    #[tokio::test]
    async fn no_receiver_does_not_error() {
        // When no studio client is connected the notifier must still succeed.
        let notifier = WebSocketNotifier::new();
        let result = notifier.notify(&make_event()).await;
        assert!(result.is_ok(), "no-receiver case must not fail: {result:?}");
    }

    #[tokio::test]
    async fn receiver_count_reflects_subscriptions() {
        let notifier = WebSocketNotifier::new();
        assert_eq!(notifier.receiver_count(), 0);
        let _rx1 = notifier.subscribe();
        assert_eq!(notifier.receiver_count(), 1);
        let _rx2 = notifier.subscribe();
        assert_eq!(notifier.receiver_count(), 2);
    }

    #[test]
    fn notifier_name_is_websocket() {
        let n = WebSocketNotifier::new();
        assert_eq!(n.name(), "websocket");
    }

    #[tokio::test]
    async fn event_json_is_valid_json() {
        let notifier = WebSocketNotifier::new();
        let mut rx = notifier.subscribe();
        notifier.notify(&make_event()).await.unwrap();
        let json = rx.recv().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("must be valid JSON");
        assert_eq!(parsed["execution_id"], "exec-ws-001");
        assert_eq!(parsed["risk_level"], "Critical");
    }
}
