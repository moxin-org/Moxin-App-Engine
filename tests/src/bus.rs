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

    /// Number of messages captured so far.
    pub async fn message_count(&self) -> usize {
        self.captured_messages.read().await.len()
    }

    pub async fn sender_sequence(&self) -> Vec<String> {
        self.captured_messages
            .read()
            .await
            .iter()
            .map(|(sender, _, _)| sender.clone())
            .collect()
    }

    pub async fn mode_sequence(&self) -> Vec<CommunicationMode> {
        self.captured_messages
            .read()
            .await
            .iter()
            .map(|(_, mode, _)| mode.clone())
            .collect()
    }

    pub async fn sender_mode_sequence(&self) -> Vec<(String, CommunicationMode)> {
        self.captured_messages
            .read()
            .await
            .iter()
            .map(|(sender, mode, _)| (sender.clone(), mode.clone()))
            .collect()
    }

    pub async fn has_sender_sequence(&self, expected: &[&str]) -> bool {
        let actual = self.sender_sequence().await;
        actual.len() == expected.len()
            && actual
                .iter()
                .zip(expected.iter())
                .all(|(actual_sender, expected_sender)| actual_sender == expected_sender)
    }

    pub async fn has_mode_sequence(&self, expected: &[CommunicationMode]) -> bool {
        let actual = self.mode_sequence().await;
        actual.len() == expected.len()
            && actual
                .iter()
                .zip(expected.iter())
                .all(|(actual_mode, expected_mode)| actual_mode == expected_mode)
    }

    pub async fn has_sender_mode_sequence(&self, expected: &[(&str, CommunicationMode)]) -> bool {
        let actual = self.sender_mode_sequence().await;
        actual.len() == expected.len()
            && actual.iter().zip(expected.iter()).all(
                |((actual_sender, actual_mode), (expected_sender, expected_mode))| {
                    actual_sender == expected_sender && actual_mode == expected_mode
                },
            )
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
