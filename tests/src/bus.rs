//! Mock message bus for inspecting agent-to-agent communication.

use mofa_kernel::bus::{AgentBus, BusError, CommunicationMode};
use mofa_kernel::message::AgentMessage;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A message-bus spy that records all sent messages for later assertions.
/// Supports failure injection via [`fail_next_send`](Self::fail_next_send).
#[derive(Clone)]
pub struct MockAgentBus {
    pub inner: AgentBus,
    pub captured_messages: Arc<RwLock<Vec<(String, CommunicationMode, AgentMessage)>>>,
    failure_queue: Arc<RwLock<VecDeque<String>>>,
}

impl MockAgentBus {
    /// Create a new mock bus backed by a real [`AgentBus`].
    pub fn new() -> Self {
        Self {
            inner: AgentBus::new(),
            captured_messages: Arc::new(RwLock::new(Vec::new())),
            failure_queue: Arc::new(RwLock::new(VecDeque::new())),
        }
    }

    /// Queue send failures for the next N calls.
    /// Each failure returns `BusError::SendFailed` with the given message.
    pub async fn fail_next_send(&self, count: usize, error_msg: &str) {
        let mut queue = self.failure_queue.write().await;
        for _ in 0..count {
            queue.push_back(error_msg.to_string());
        }
    }

    /// Send a message through the inner bus **and** record it.
    /// If failures are queued, the message is still captured but the send returns an error.
    pub async fn send_and_capture(
        &self,
        sender_id: &str,
        mode: CommunicationMode,
        message: AgentMessage,
    ) -> Result<(), BusError> {
        self.captured_messages.write().await.push((
            sender_id.to_string(),
            mode.clone(),
            message.clone(),
        ));

        // Check failure queue before delegating
        {
            let mut queue = self.failure_queue.write().await;
            if let Some(err_msg) = queue.pop_front() {
                return Err(BusError::SendFailed(err_msg));
            }
        }

        self.inner.send_message(sender_id, mode, &message).await
    }

    /// All captured messages sent by the given sender.
    pub async fn messages_from(
        &self,
        sender_id: &str,
    ) -> Vec<(String, CommunicationMode, AgentMessage)> {
        self.captured_messages
            .read()
            .await
            .iter()
            .filter(|(sid, _, _)| sid == sender_id)
            .cloned()
            .collect()
    }

    /// All captured messages addressed to the given recipient.
    pub async fn messages_to(
        &self,
        recipient_id: &str,
    ) -> Vec<(String, CommunicationMode, AgentMessage)> {
        self.captured_messages
            .read()
            .await
            .iter()
            .filter(|(_, mode, _)| match mode {
                CommunicationMode::PointToPoint(target) => target == recipient_id,
                _ => false,
            })
            .cloned()
            .collect()
    }

    /// All captured messages matching a predicate.
    pub async fn messages_matching<F>(
        &self,
        predicate: F,
    ) -> Vec<(String, CommunicationMode, AgentMessage)>
    where
        F: Fn(&str, &CommunicationMode, &AgentMessage) -> bool,
    {
        self.captured_messages
            .read()
            .await
            .iter()
            .filter(|(s, m, msg)| predicate(s, m, msg))
            .cloned()
            .collect()
    }

    /// Number of messages captured so far.
    pub async fn message_count(&self) -> usize {
        self.captured_messages.read().await.len()
    }

    /// Clears the capture history.
    pub async fn clear_history(&self) {
        self.captured_messages.write().await.clear();
    }
}

impl Default for MockAgentBus {
    fn default() -> Self {
        Self::new()
    }
}
