// MessageBus trait

use crate::bus::envelope::MessageEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum MessageBusError {
    #[error("Topic not found: {0}")]
    TopicNotFound(String),
    #[error("Consumer not found: {0}")]
    ConsumerNotFound(String),
    #[error("Message not found: {0}")]
    MessageNotFound(String),
    #[error("Channel error: {0}")]
    ChannelError(String),
    #[error("Timeout waiting for message")]
    Timeout,
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Persistence error: {0}")]
    PersistenceError(String),
    #[error("Message expired (TTL exceeded)")]
    MessageExpired,
    #[error("Max retries exceeded for message {0}")]
    MaxRetriesExceeded(String),
    #[error("Dead-letter: {reason}")]
    DeadLetter { message_id: String, reason: String },
    #[error("Internal error: {0}")]
    Internal(String),
}

pub type MessageBusResult<T> = Result<T, MessageBusError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryGuarantee {
    AtMostOnce,
    AtLeastOnce,
}

impl Default for DeliveryGuarantee {
    fn default() -> Self {
        Self::AtMostOnce
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeOptions {
    pub delivery_guarantee: DeliveryGuarantee,
    pub max_retries: Option<u32>,
    pub dead_letter_topic: Option<String>,
}

impl Default for SubscribeOptions {
    fn default() -> Self {
        Self {
            delivery_guarantee: DeliveryGuarantee::AtMostOnce,
            max_retries: None,
            dead_letter_topic: None,
        }
    }
}

impl SubscribeOptions {
    pub fn at_least_once(max_retries: u32, dead_letter_topic: &str) -> Self {
        Self {
            delivery_guarantee: DeliveryGuarantee::AtLeastOnce,
            max_retries: Some(max_retries),
            dead_letter_topic: Some(dead_letter_topic.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiveOptions {
    #[serde(
        serialize_with = "crate::bus::envelope::serialize_opt_duration_pub",
        deserialize_with = "crate::bus::envelope::deserialize_opt_duration_pub"
    )]
    pub timeout: Option<Duration>,
    pub max_messages: usize,
}

impl Default for ReceiveOptions {
    fn default() -> Self {
        Self {
            timeout: None,
            max_messages: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NackAction {
    Requeued,
    DeadLettered,
    Discarded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryReceipt {
    pub message_id: String,
    pub consumer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceivedMessage {
    pub envelope: MessageEnvelope,
    pub receipt: DeliveryReceipt,
}

#[async_trait]
pub trait MessageBus: Send + Sync + 'static {
    async fn publish(&self, topic: &str, envelope: MessageEnvelope) -> MessageBusResult<()>;

    async fn subscribe(
        &self,
        topic: &str,
        consumer_id: &str,
        options: SubscribeOptions,
    ) -> MessageBusResult<()>;

    async fn unsubscribe(&self, topic: &str, consumer_id: &str) -> MessageBusResult<()>;

    async fn send(&self, recipient_id: &str, envelope: MessageEnvelope) -> MessageBusResult<()>;

    async fn receive(
        &self,
        consumer_id: &str,
        options: ReceiveOptions,
    ) -> MessageBusResult<Vec<ReceivedMessage>>;

    async fn ack(&self, receipt: &DeliveryReceipt) -> MessageBusResult<()>;

    async fn nack(&self, receipt: &DeliveryReceipt) -> MessageBusResult<NackAction>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscribe_options_at_least_once() {
        let opts = SubscribeOptions::at_least_once(5, "dlq.topic");
        assert_eq!(opts.delivery_guarantee, DeliveryGuarantee::AtLeastOnce);
        assert_eq!(opts.max_retries, Some(5));
        assert_eq!(opts.dead_letter_topic.as_deref(), Some("dlq.topic"));
    }

    #[test]
    fn test_receive_options_default() {
        let opts = ReceiveOptions::default();
        assert!(opts.timeout.is_none());
        assert_eq!(opts.max_messages, 1);
    }
}
