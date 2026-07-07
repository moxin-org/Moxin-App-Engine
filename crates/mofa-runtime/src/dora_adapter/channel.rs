//! DoraChannel 封装
//! DoraChannel Wrapper
//!
//! 提供与 dora-rs 集成的跨智能体通信通道
//! Provides cross-agent communication channels integrated with dora-rs

use crate::dora_adapter::error::{DoraError, DoraResult};
use mofa_kernel::message::AgentMessage;
use mofa_kernel::bus::envelope::MessageEnvelope as KernelEnvelope;
use mofa_kernel::bus::traits::{DeliveryReceipt, ReceivedMessage};
use mofa_kernel::bus::metrics::{MessageBusCounters, MessageBusObserver, SharedCounters};
use mofa_kernel::bus::traits::{
    DeliveryGuarantee, MessageBus, MessageBusError, MessageBusResult, NackAction, ReceiveOptions,
    SubscribeOptions,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};
use tokio::time::timeout;
use tracing::{debug, info, warn};

/// Type alias for the receiver storage type to reduce complexity
type ReceiverMap = Arc<RwLock<HashMap<String, Arc<Mutex<mpsc::Receiver<MessageEnvelope>>>>>>;

/// 通道配置
/// Channel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// 通道 ID
    /// Channel ID
    pub channel_id: String,
    /// 缓冲区大小
    /// Buffer size
    pub buffer_size: usize,
    /// 消息超时时间
    /// Message timeout duration
    pub message_timeout: Duration,
    /// 是否启用持久化
    /// Whether persistence is enabled
    pub persistent: bool,
    /// Persistence directory
    #[serde(default = "default_persistence_dir")]
    pub persistence_dir: String,
}

fn default_persistence_dir() -> String {
    std::env::temp_dir()
        .join("mofa_channel")
        .to_string_lossy()
        .to_string()
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            channel_id: uuid::Uuid::now_v7().to_string(),
            buffer_size: 1024,
            message_timeout: Duration::from_secs(30),
            persistent: false,
            persistence_dir: default_persistence_dir(),
        }
    }
}

/// 消息信封 - 包含元数据的消息包装
/// Message Envelope - Message wrapper containing metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    /// 消息 ID
    /// Message ID
    pub message_id: String,
    /// 发送方 ID
    /// Sender ID
    pub sender_id: String,
    /// 接收方 ID（None 表示广播）
    /// Receiver ID (None means broadcast)
    pub receiver_id: Option<String>,
    /// 主题（用于 PubSub）
    /// Topic (for PubSub)
    pub topic: Option<String>,
    /// 时间戳
    /// Timestamp
    pub timestamp: u64,
    /// 消息内容
    /// Message payload
    pub payload: Vec<u8>,
    /// 元数据
    /// Metadata
    pub metadata: HashMap<String, String>,
}

impl MessageEnvelope {
    pub fn new(sender_id: &str, payload: Vec<u8>) -> Self {
        Self {
            message_id: uuid::Uuid::now_v7().to_string(),
            sender_id: sender_id.to_string(),
            receiver_id: None,
            topic: None,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            payload,
            metadata: HashMap::new(),
        }
    }

    /// 设置接收方（点对点）
    /// Set receiver (point-to-point)
    pub fn to(mut self, receiver_id: &str) -> Self {
        self.receiver_id = Some(receiver_id.to_string());
        self
    }

    /// 设置主题（PubSub）
    /// Set topic (PubSub)
    pub fn with_topic(mut self, topic: &str) -> Self {
        self.topic = Some(topic.to_string());
        self
    }

    /// 添加元数据
    /// Add metadata
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// 从 AgentMessage 创建
    /// Create from AgentMessage
    pub fn from_agent_message(sender_id: &str, message: &AgentMessage) -> DoraResult<Self> {
        let payload = bincode::serialize(message)?;
        Ok(Self::new(sender_id, payload))
    }

    /// 解析为 AgentMessage
    /// Parse as AgentMessage
    pub fn to_agent_message(&self) -> DoraResult<AgentMessage> {
        bincode::deserialize(&self.payload)
            .map_err(|e| DoraError::DeserializationError(e.to_string()))
    }
}

#[derive(Debug, Clone)]
struct SubscriptionInfo {
    consumer_id: String,
    options: SubscribeOptions,
}

#[derive(Debug, Clone)]
struct InFlightMessage {
    envelope: KernelEnvelope,
    subscription_opts: SubscribeOptions,
    consumer_id: String,
}

fn persist_message(dir: &str, channel_id: &str, envelope: &KernelEnvelope) {
    if let Err(e) = persist_message_inner(dir, channel_id, envelope) {
        warn!("Failed to persist message {}: {}", envelope.message_id, e);
    }
}

fn persist_message_inner(
    dir: &str,
    channel_id: &str,
    envelope: &KernelEnvelope,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = std::path::Path::new(dir).join(format!("{}.jsonl", channel_id));
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_string(envelope)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    writeln!(file, "{}", line)?;
    Ok(())
}

pub fn load_persisted_messages(dir: &str, channel_id: &str) -> Vec<KernelEnvelope> {
    let path = std::path::Path::new(dir).join(format!("{}.jsonl", channel_id));
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    contents
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// dora-rs 集成通道
/// dora-rs integrated channel
pub struct DoraChannel {
    config: ChannelConfig,
    /// 点对点通道：接收方ID -> 发送器
    /// P2P channels: Receiver ID -> Sender
    p2p_channels: Arc<RwLock<HashMap<String, mpsc::Sender<MessageEnvelope>>>>,
    /// 广播通道
    /// Broadcast channel
    broadcast_tx: broadcast::Sender<MessageEnvelope>,
    /// 主题订阅：主题 -> 订阅者ID列表
    /// Topic subscriptions: Topic -> Subscriber ID list
    topic_subscribers: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// 主题通道：主题 -> 发送器
    /// Topic channels: Topic -> Sender
    topic_channels: Arc<RwLock<HashMap<String, broadcast::Sender<MessageEnvelope>>>>,
    /// 接收器存储：智能体ID -> 接收器
    /// Receiver storage: Agent ID -> Receiver
    receivers: ReceiverMap,

    bus_senders: Arc<RwLock<HashMap<String, mpsc::Sender<KernelEnvelope>>>>,
    bus_receivers: Arc<RwLock<HashMap<String, Arc<Mutex<mpsc::Receiver<KernelEnvelope>>>>>>,
    bus_topic_subs: Arc<RwLock<HashMap<String, Vec<SubscriptionInfo>>>>,
    in_flight: Arc<RwLock<HashMap<String, InFlightMessage>>>,
    dead_letters: Arc<RwLock<HashMap<String, Vec<KernelEnvelope>>>>,
    counters: SharedCounters,
}

fn in_flight_key(consumer_id: &str, message_id: &str) -> String {
    format!("{}\0{}", consumer_id, message_id)
}

impl DoraChannel {
    /// 创建新通道
    /// Create a new channel
    pub fn new(config: ChannelConfig) -> Self {
        let (broadcast_tx, _) = broadcast::channel(config.buffer_size);
        Self {
            config,
            p2p_channels: Arc::new(RwLock::new(HashMap::new())),
            broadcast_tx,
            topic_subscribers: Arc::new(RwLock::new(HashMap::new())),
            topic_channels: Arc::new(RwLock::new(HashMap::new())),
            receivers: Arc::new(RwLock::new(HashMap::new())),
            bus_senders: Arc::new(RwLock::new(HashMap::new())),
            bus_receivers: Arc::new(RwLock::new(HashMap::new())),
            bus_topic_subs: Arc::new(RwLock::new(HashMap::new())),
            in_flight: Arc::new(RwLock::new(HashMap::new())),
            dead_letters: Arc::new(RwLock::new(HashMap::new())),
            counters: mofa_kernel::bus::new_shared_counters(),
        }
    }

    /// 获取配置
    /// Get configuration
    pub fn config(&self) -> &ChannelConfig {
        &self.config
    }

    pub fn counters(&self) -> &SharedCounters {
        &self.counters
    }

    pub async fn dead_letters(&self) -> HashMap<String, Vec<KernelEnvelope>> {
        self.dead_letters.read().await.clone()
    }

    async fn ensure_consumer_mailbox(&self, consumer_id: &str) {
        let exists = {
            let senders = self.bus_senders.read().await;
            senders.contains_key(consumer_id)
        };
        if !exists {
            let (tx, rx) = mpsc::channel(self.config.buffer_size);
            let mut senders = self.bus_senders.write().await;
            senders.insert(consumer_id.to_string(), tx);
            let mut receivers = self.bus_receivers.write().await;
            receivers.insert(consumer_id.to_string(), Arc::new(Mutex::new(rx)));
        }
    }

    /// 注册智能体
    /// Register agent
    pub async fn register_agent(&self, agent_id: &str) -> DoraResult<()> {
        let (tx, rx) = mpsc::channel(self.config.buffer_size);

        let mut p2p_channels = self.p2p_channels.write().await;
        p2p_channels.insert(agent_id.to_string(), tx);

        let mut receivers = self.receivers.write().await;
        receivers.insert(agent_id.to_string(), Arc::new(Mutex::new(rx)));

        self.ensure_consumer_mailbox(agent_id).await;

        info!(
            "Agent {} registered to channel {}",
            agent_id, self.config.channel_id
        );
        Ok(())
    }

    /// 注销智能体
    /// Unregister agent
    pub async fn unregister_agent(&self, agent_id: &str) -> DoraResult<()> {
        let mut p2p_channels = self.p2p_channels.write().await;
        p2p_channels.remove(agent_id);

        let mut receivers = self.receivers.write().await;
        receivers.remove(agent_id);

        // 从所有主题中移除
        // Remove from all topics
        let mut topic_subs = self.topic_subscribers.write().await;
        for subscribers in topic_subs.values_mut() {
            subscribers.retain(|id| id != agent_id);
        }

        self.bus_senders.write().await.remove(agent_id);
        self.bus_receivers.write().await.remove(agent_id);

        info!(
            "Agent {} unregistered from channel {}",
            agent_id, self.config.channel_id
        );
        Ok(())
    }

    /// 订阅主题
    /// Subscribe to topic
    pub async fn subscribe_legacy(&self, agent_id: &str, topic: &str) -> DoraResult<()> {
        let mut topic_subs = self.topic_subscribers.write().await;
        topic_subs
            .entry(topic.to_string())
            .or_default()
            .push(agent_id.to_string());

        // 确保主题通道存在
        // Ensure topic channel exists
        let mut topic_channels = self.topic_channels.write().await;
        if !topic_channels.contains_key(topic) {
            let (tx, _) = broadcast::channel(self.config.buffer_size);
            topic_channels.insert(topic.to_string(), tx);
        }

        debug!("Agent {} subscribed to topic {}", agent_id, topic);
        Ok(())
    }

    pub async fn subscribe_topic_legacy(
        &self,
        agent_id: &str,
        topic: &str,
    ) -> DoraResult<()> {
        self.subscribe_legacy(agent_id, topic).await
    }

    /// 取消订阅主题
    /// Unsubscribe from topic
    pub async fn unsubscribe_legacy(&self, agent_id: &str, topic: &str) -> DoraResult<()> {
        let mut topic_subs = self.topic_subscribers.write().await;
        if let Some(subscribers) = topic_subs.get_mut(topic) {
            subscribers.retain(|id| id != agent_id);
            if subscribers.is_empty() {
                topic_subs.remove(topic);
            }
        }
        debug!("Agent {} unsubscribed from topic {}", agent_id, topic);
        Ok(())
    }

    /// 发送点对点消息
    /// Send point-to-point message
    pub async fn send_p2p(&self, envelope: MessageEnvelope) -> DoraResult<()> {
        let receiver_id = envelope
            .receiver_id
            .clone()
            .ok_or_else(|| DoraError::ChannelError("No receiver specified for P2P".to_string()))?;

        // Clone the sender in a scoped block to release lock quickly
        let tx = {
            let p2p_channels = self.p2p_channels.read().await;
            p2p_channels.get(&receiver_id).cloned().ok_or_else(|| {
                DoraError::AgentNotFound(format!("Receiver {} not registered", receiver_id))
            })?
        }; // Lock released here

        // Send without holding any locks
        tx.send(envelope)
            .await
            .map_err(|e| DoraError::ChannelError(e.to_string()))?;

        debug!("P2P message sent to {}", receiver_id);
        Ok(())
    }

    /// 广播消息
    /// Broadcast message
    pub async fn broadcast(&self, envelope: MessageEnvelope) -> DoraResult<()> {
        // 如果没有接收者，send 会返回错误，但这不应该是致命错误
        // If there are no receivers, send returns an error, but this shouldn't be fatal
        match self.broadcast_tx.send(envelope) {
            Ok(receiver_count) => {
                debug!("Broadcast message sent to {} receivers", receiver_count);
            }
            Err(_) => {
                debug!("Broadcast message sent but no receivers");
            }
        }
        Ok(())
    }

    /// 发布到主题
    /// Publish to topic
    pub async fn publish_legacy(&self, envelope: MessageEnvelope) -> DoraResult<()> {
        let topic = envelope
            .topic
            .clone()
            .ok_or_else(|| DoraError::ChannelError("No topic specified".to_string()))?;

        // Clone the broadcast sender in a scoped block to release lock quickly
        let tx = {
            let topic_channels = self.topic_channels.read().await;
            topic_channels
                .get(&topic)
                .cloned()
                .ok_or_else(|| DoraError::ChannelError(format!("Topic {} not found", topic)))?
        }; // Lock released here

        // Send without holding any locks
        // 如果没有接收者，send 会返回错误，但这不应该是致命错误
        // If there are no receivers, send returns an error, but this shouldn't be fatal
        match tx.send(envelope) {
            Ok(receiver_count) => {
                debug!(
                    "Message published to topic {} with {} receivers",
                    topic, receiver_count
                );
            }
            Err(_) => {
                debug!("Message published to topic {} but no receivers", topic);
            }
        }
        Ok(())
    }

    /// 接收点对点消息（阻塞）
    /// Receive point-to-point message (blocking)
    pub async fn receive_p2p(&self, agent_id: &str) -> DoraResult<Option<MessageEnvelope>> {
        let rx = {
            let receivers = self.receivers.read().await;
            receivers.get(agent_id).cloned().ok_or_else(|| {
                DoraError::AgentNotFound(format!("Agent {} not registered", agent_id))
            })?
        };

        let mut rx_guard = rx.lock().await;
        match timeout(self.config.message_timeout, rx_guard.recv()).await {
            Ok(Some(envelope)) => Ok(Some(envelope)),
            Ok(None) => Ok(None),
            Err(_) => Err(DoraError::Timeout("Receive timeout".to_string())),
        }
    }

    /// 尝试接收点对点消息（非阻塞）
    /// Try to receive point-to-point message (non-blocking)
    pub async fn try_receive_p2p(&self, agent_id: &str) -> DoraResult<Option<MessageEnvelope>> {
        let rx = {
            let receivers = self.receivers.read().await;
            receivers.get(agent_id).cloned().ok_or_else(|| {
                DoraError::AgentNotFound(format!("Agent {} not registered", agent_id))
            })?
        };

        let mut rx_guard = rx.lock().await;
        match rx_guard.try_recv() {
            Ok(envelope) => Ok(Some(envelope)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => {
                Err(DoraError::ChannelError("Channel disconnected".to_string()))
            }
        }
    }

    /// 订阅广播（返回接收器）
    /// Subscribe to broadcast (returns receiver)
    pub fn subscribe_broadcast(&self) -> broadcast::Receiver<MessageEnvelope> {
        self.broadcast_tx.subscribe()
    }

    /// 订阅主题（返回接收器）
    /// Subscribe to topic (returns receiver)
    pub async fn subscribe_topic(
        &self,
        topic: &str,
    ) -> DoraResult<broadcast::Receiver<MessageEnvelope>> {
        let topic_channels = self.topic_channels.read().await;
        let tx = topic_channels
            .get(topic)
            .ok_or_else(|| DoraError::ChannelError(format!("Topic {} not found", topic)))?;
        Ok(tx.subscribe())
    }

    /// 获取主题订阅者列表
    /// Get topic subscriber list
    pub async fn get_topic_subscribers(&self, topic: &str) -> Vec<String> {
        let topic_subs = self.topic_subscribers.read().await;
        topic_subs.get(topic).cloned().unwrap_or_default()
    }

    /// 获取所有已注册的智能体
    /// Get all registered agents
    pub async fn registered_agents(&self) -> Vec<String> {
        let p2p_channels = self.p2p_channels.read().await;
        p2p_channels.keys().cloned().collect()
    }
}

impl MessageBusObserver for DoraChannel {
    fn counters(&self) -> &MessageBusCounters {
        &self.counters
    }
}

#[async_trait]
impl MessageBus for DoraChannel {
    async fn publish(
        &self,
        topic: &str,
        envelope: KernelEnvelope,
    ) -> MessageBusResult<()> {
        if envelope.is_expired() {
            self.counters.inc_dropped();
            return Err(MessageBusError::MessageExpired);
        }

        self.counters.inc_published();

        if self.config.persistent {
            persist_message(&self.config.persistence_dir, &self.config.channel_id, &envelope);
        }

        let topic_subs = self.bus_topic_subs.read().await;
        let subs = topic_subs.get(topic);

        if let Some(subs) = subs {
            let senders = self.bus_senders.read().await;
            for sub_info in subs {
                if let Some(tx) = senders.get(&sub_info.consumer_id) {
                    let mut env = envelope.clone();
                    env.topic = Some(topic.to_string());

                    if sub_info.options.delivery_guarantee == DeliveryGuarantee::AtLeastOnce {
                        let mut in_flight = self.in_flight.write().await;
                        in_flight.insert(
                            in_flight_key(&sub_info.consumer_id, &env.message_id),
                            InFlightMessage {
                                envelope: env.clone(),
                                subscription_opts: sub_info.options.clone(),
                                consumer_id: sub_info.consumer_id.clone(),
                            },
                        );
                    }

                    match tx.try_send(env) {
                        Ok(()) => {
                            self.counters.inc_delivered();
                        }
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            self.counters.inc_dropped();
                            warn!(
                                "Dropped message to consumer {} on topic {} (buffer full)",
                                sub_info.consumer_id, topic
                            );
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            self.counters.inc_dropped();
                        }
                    }
                }
            }
        }

        debug!(
            "Published message to topic '{}' (subs={})",
            topic,
            subs.map_or(0, |s| s.len())
        );
        Ok(())
    }

    async fn subscribe(
        &self,
        topic: &str,
        consumer_id: &str,
        options: SubscribeOptions,
    ) -> MessageBusResult<()> {
        self.ensure_consumer_mailbox(consumer_id).await;

        let mut topic_subs = self.bus_topic_subs.write().await;
        let subs = topic_subs.entry(topic.to_string()).or_default();

        if !subs.iter().any(|s| s.consumer_id == consumer_id) {
            subs.push(SubscriptionInfo {
                consumer_id: consumer_id.to_string(),
                options,
            });
        }

        debug!("Consumer '{}' subscribed to topic '{}'", consumer_id, topic);
        Ok(())
    }

    async fn unsubscribe(
        &self,
        topic: &str,
        consumer_id: &str,
    ) -> MessageBusResult<()> {
        let mut topic_subs = self.bus_topic_subs.write().await;
        if let Some(subs) = topic_subs.get_mut(topic) {
            subs.retain(|s| s.consumer_id != consumer_id);
            if subs.is_empty() {
                topic_subs.remove(topic);
            }
        }
        debug!("Consumer '{}' unsubscribed from topic '{}'", consumer_id, topic);
        Ok(())
    }

    async fn send(
        &self,
        recipient_id: &str,
        envelope: KernelEnvelope,
    ) -> MessageBusResult<()> {
        if envelope.is_expired() {
            self.counters.inc_dropped();
            return Err(MessageBusError::MessageExpired);
        }

        self.counters.inc_published();

        if self.config.persistent {
            persist_message(&self.config.persistence_dir, &self.config.channel_id, &envelope);
        }

        let senders = self.bus_senders.read().await;
        let tx = senders.get(recipient_id).ok_or_else(|| {
            MessageBusError::ConsumerNotFound(format!(
                "Recipient '{}' not registered",
                recipient_id
            ))
        })?;

        tx.send(envelope)
            .await
            .map_err(|e| MessageBusError::ChannelError(e.to_string()))?;

        self.counters.inc_delivered();
        debug!("Sent message to consumer '{}'", recipient_id);
        Ok(())
    }

    async fn receive(
        &self,
        consumer_id: &str,
        options: ReceiveOptions,
    ) -> MessageBusResult<Vec<ReceivedMessage>> {
        let rx_arc = {
            let receivers = self.bus_receivers.read().await;
            receivers.get(consumer_id).cloned().ok_or_else(|| {
                MessageBusError::ConsumerNotFound(format!(
                    "Consumer '{}' not registered",
                    consumer_id
                ))
            })?
        };

        let wait = options.timeout.unwrap_or(self.config.message_timeout);
        let max = options.max_messages.max(1);
        let mut results = Vec::with_capacity(max);

        let mut rx = rx_arc.lock().await;
        match timeout(wait, rx.recv()).await {
            Ok(Some(env)) => results.push(ReceivedMessage {
                receipt: DeliveryReceipt {
                    message_id: env.message_id.clone(),
                    consumer_id: consumer_id.to_string(),
                },
                envelope: env,
            }),
            Ok(None) => {
                return Err(MessageBusError::ChannelError(
                    "Consumer channel closed".to_string(),
                ))
            }
            Err(_) => return Ok(results),
        }

        while results.len() < max {
            match rx.try_recv() {
                Ok(env) => results.push(ReceivedMessage {
                    receipt: DeliveryReceipt {
                        message_id: env.message_id.clone(),
                        consumer_id: consumer_id.to_string(),
                    },
                    envelope: env,
                }),
                Err(_) => break,
            }
        }

        Ok(results)
    }

    async fn ack(&self, receipt: &DeliveryReceipt) -> MessageBusResult<()> {
        let mut in_flight = self.in_flight.write().await;
        let key = in_flight_key(&receipt.consumer_id, &receipt.message_id);
        if in_flight.remove(&key).is_some() {
            self.counters.inc_acked();
            debug!("Acked message '{}'", receipt.message_id);
        }
        Ok(())
    }

    async fn nack(&self, receipt: &DeliveryReceipt) -> MessageBusResult<NackAction> {
        let mut in_flight = self.in_flight.write().await;
        let key = in_flight_key(&receipt.consumer_id, &receipt.message_id);
        let Some(mut entry) = in_flight.remove(&key) else {
            return Ok(NackAction::Discarded);
        };

        self.counters.inc_nacked();
        let max_retries = entry.subscription_opts.max_retries.unwrap_or(0);

        if entry.envelope.attempt < max_retries {
            entry.envelope.increment_attempt();
            let attempt = entry.envelope.attempt;
            self.counters.inc_retries();
            in_flight.insert(key.clone(), entry.clone());

            if let Some(recipient) = entry.envelope.recipient_id.as_deref() {
                let senders = self.bus_senders.read().await;
                if let Some(tx) = senders.get(recipient) {
                    if tx.try_send(entry.envelope).is_err() {
                        self.counters.inc_dropped();
                    }
                } else {
                    self.counters.inc_dropped();
                }
            } else if entry.envelope.topic.is_some() {
                let senders = self.bus_senders.read().await;
                if let Some(tx) = senders.get(&entry.consumer_id) {
                    if tx.try_send(entry.envelope).is_err() {
                        self.counters.inc_dropped();
                    }
                } else {
                    self.counters.inc_dropped();
                }
            }

            debug!(
                "Requeued message '{}' (attempt {})",
                receipt.message_id, attempt
            );
            Ok(NackAction::Requeued)
        } else if let Some(dl_topic) = entry.subscription_opts.dead_letter_topic.as_deref() {
            let mut dead_letters = self.dead_letters.write().await;
            dead_letters
                .entry(dl_topic.to_string())
                .or_default()
                .push(entry.envelope);
            self.counters.inc_dead_lettered();
            debug!(
                "Dead-lettered message '{}' to '{}'",
                receipt.message_id, dl_topic
            );
            Ok(NackAction::DeadLettered)
        } else {
            self.counters.inc_dropped();
            debug!(
                "Discarded message '{}' (max retries exceeded)",
                receipt.message_id
            );
            Ok(NackAction::Discarded)
        }
    }
}

/// 通道管理器 - 管理多个通道
/// Channel Manager - Manages multiple channels
pub struct ChannelManager {
    channels: Arc<RwLock<HashMap<String, Arc<DoraChannel>>>>,
    default_config: ChannelConfig,
}

impl ChannelManager {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            default_config: ChannelConfig::default(),
        }
    }

    /// 创建或获取通道
    /// Create or retrieve a channel
    pub async fn get_or_create_channel(&self, channel_id: &str) -> Arc<DoraChannel> {
        let channels = self.channels.read().await;
        if let Some(channel) = channels.get(channel_id) {
            return channel.clone();
        }
        drop(channels);

        let config = ChannelConfig {
            channel_id: channel_id.to_string(),
            ..self.default_config.clone()
        };
        let channel = Arc::new(DoraChannel::new(config));

        let mut channels = self.channels.write().await;
        channels.insert(channel_id.to_string(), channel.clone());
        channel
    }

    /// 删除通道
    /// Remove a channel
    pub async fn remove_channel(&self, channel_id: &str) -> Option<Arc<DoraChannel>> {
        let mut channels = self.channels.write().await;
        channels.remove(channel_id)
    }

    /// 获取所有通道 ID
    /// Get all channel IDs
    pub async fn channel_ids(&self) -> Vec<String> {
        let channels = self.channels.read().await;
        channels.keys().cloned().collect()
    }
}

impl Default for ChannelManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofa_kernel::bus::envelope::MessageEnvelope as KernelEnvelope;
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn test_p2p_communication() {
        let channel = DoraChannel::new(ChannelConfig::default());

        // 注册两个智能体
        // Register two agents
        channel.register_agent("agent1").await.unwrap();
        channel.register_agent("agent2").await.unwrap();

        // agent1 发送消息给 agent2
        // agent1 sends message to agent2
        let envelope = MessageEnvelope::new("agent1", b"Hello agent2".to_vec()).to("agent2");
        channel.send_p2p(envelope).await.unwrap();

        // agent2 接收消息
        // agent2 receives message
        let received = channel.try_receive_p2p("agent2").await.unwrap();
        assert!(received.is_some());
        assert_eq!(received.unwrap().payload, b"Hello agent2");
    }

    #[tokio::test]
    async fn test_pubsub() {
        let channel = DoraChannel::new(ChannelConfig::default());

        channel.register_agent("publisher").await.unwrap();
        channel.register_agent("subscriber").await.unwrap();

        // 订阅主题
        // Subscribe to topic
        channel
            .subscribe_legacy("subscriber", "news")
            .await
            .unwrap();

        // 获取主题接收器
        // Get topic receiver
        let mut rx = channel.subscribe_topic("news").await.unwrap();

        // 发布消息
        // Publish message
        let envelope =
            MessageEnvelope::new("publisher", b"Breaking news".to_vec()).with_topic("news");
        channel.publish_legacy(envelope).await.unwrap();

        // 接收消息
        // Receive message
        let received = rx.recv().await.unwrap();
        assert_eq!(received.payload, b"Breaking news");
    }

    #[tokio::test]
    async fn test_channel_manager() {
        let manager = ChannelManager::new();

        let channel1 = manager.get_or_create_channel("channel1").await;
        let channel2 = manager.get_or_create_channel("channel1").await;

        // 应该返回同一个通道
        // Should return the same channel
        assert_eq!(channel1.config().channel_id, channel2.config().channel_id);

        let ids = manager.channel_ids().await;
        assert_eq!(ids.len(), 1);
        assert!(ids.contains(&"channel1".to_string()));
    }

    #[tokio::test]
    async fn test_p2p_receive_deadlock() {
        let channel = Arc::new(DoraChannel::new(ChannelConfig {
            message_timeout: Duration::from_millis(500),
            ..ChannelConfig::default()
        }));
        channel.register_agent("reader").await.unwrap();
        channel.register_agent("writer").await.unwrap();

        let channel_clone = channel.clone();

        // start a receive on reader
        let reader_task = tokio::spawn(async move {
            let _ = channel_clone.receive_p2p("reader").await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let start_time = std::time::Instant::now();

        // registering a new agent.
        channel.register_agent("new_agent").await.unwrap();
        let elapsed = start_time.elapsed();
        reader_task.await.unwrap();

        // the bug exists if register_agent was blocked
        assert!(
            elapsed < Duration::from_millis(400),
            "Deadlock reproduced: register_agent was blocked for {:?} by receive_p2p!",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_bus_send_receive() {
        let ch = DoraChannel::new(ChannelConfig::default());
        ch.register_agent("consumer-1").await.unwrap();

        let env = KernelEnvelope::new("producer-1", b"hello bus".to_vec());
        ch.send("consumer-1", env).await.unwrap();

        let msgs = ch
            .receive(
                "consumer-1",
                ReceiveOptions {
                    timeout: Some(Duration::from_secs(1)),
                    max_messages: 10,
                },
            )
            .await
            .unwrap();

        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].envelope.payload, b"hello bus");
        assert_eq!(msgs[0].envelope.sender_id, "producer-1");

        let snap = ch.counters.snapshot();
        assert_eq!(snap.total_published, 1);
        assert_eq!(snap.total_delivered, 1);
    }

    #[tokio::test]
    async fn test_bus_publish_subscribe() {
        let ch = DoraChannel::new(ChannelConfig::default());
        ch.register_agent("sub-a").await.unwrap();
        ch.register_agent("sub-b").await.unwrap();

        ch.subscribe("events.user", "sub-a", SubscribeOptions::default())
            .await
            .unwrap();
        ch.subscribe("events.user", "sub-b", SubscribeOptions::default())
            .await
            .unwrap();

        let env = KernelEnvelope::new("pub-1", b"user created".to_vec());
        ch.publish("events.user", env).await.unwrap();

        let a = ch.receive("sub-a", ReceiveOptions::default()).await.unwrap();
        let b = ch.receive("sub-b", ReceiveOptions::default()).await.unwrap();

        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(a[0].envelope.payload, b"user created");
        assert_eq!(b[0].envelope.payload, b"user created");
    }

    #[tokio::test]
    async fn test_bus_ack_nack_retry_deadletter() {
        let ch = DoraChannel::new(ChannelConfig::default());
        ch.register_agent("worker").await.unwrap();

        let opts = SubscribeOptions::at_least_once(2, "dlq.events");
        ch.subscribe("jobs", "worker", opts).await.unwrap();

        let env = KernelEnvelope::new("scheduler", b"job-1".to_vec());
        ch.publish("jobs", env).await.unwrap();

        let msgs = ch.receive("worker", ReceiveOptions::default()).await.unwrap();
        assert_eq!(msgs.len(), 1);
        let receipt = msgs[0].receipt.clone();

        let action = ch.nack(&receipt).await.unwrap();
        assert_eq!(action, NackAction::Requeued);

        let action = ch.nack(&receipt).await.unwrap();
        assert_eq!(action, NackAction::DeadLettered);

        let dl = ch.dead_letters().await;
        assert!(dl.contains_key("dlq.events"));
        assert_eq!(dl["dlq.events"].len(), 1);

        let snap = ch.counters.snapshot();
        assert_eq!(snap.total_nacked, 2);
        assert_eq!(snap.total_retries, 1);
        assert_eq!(snap.total_dead_lettered, 1);
    }

    #[tokio::test]
    async fn test_bus_ack_clears_in_flight() {
        let ch = DoraChannel::new(ChannelConfig::default());
        ch.register_agent("w").await.unwrap();

        let opts = SubscribeOptions::at_least_once(3, "dlq");
        ch.subscribe("t", "w", opts).await.unwrap();

        let env = KernelEnvelope::new("p", b"data".to_vec());
        ch.publish("t", env).await.unwrap();

        let msgs = ch.receive("w", ReceiveOptions::default()).await.unwrap();
        let receipt = msgs[0].receipt.clone();

        ch.ack(&receipt).await.unwrap();

        let action = ch.nack(&receipt).await.unwrap();
        assert_eq!(action, NackAction::Discarded);

        let snap = ch.counters.snapshot();
        assert_eq!(snap.total_acked, 1);
    }

    #[tokio::test]
    async fn test_bus_expired_message() {
        let ch = DoraChannel::new(ChannelConfig::default());
        ch.register_agent("c").await.unwrap();
        ch.subscribe("t", "c", SubscribeOptions::default()).await.unwrap();

        let mut env = KernelEnvelope::new("p", b"old".to_vec());
        env.ttl = Some(Duration::from_millis(0));
        env.timestamp_ms = env.timestamp_ms.saturating_sub(1);

        let result = ch.publish("t", env).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MessageBusError::MessageExpired));

        let snap = ch.counters.snapshot();
        assert_eq!(snap.total_dropped, 1);
    }

    #[tokio::test]
    async fn test_bus_persistence() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ChannelConfig {
            persistent: true,
            persistence_dir: tmp.path().to_string_lossy().to_string(),
            ..ChannelConfig::default()
        };
        let ch = DoraChannel::new(config.clone());
        ch.register_agent("r").await.unwrap();

        let env = KernelEnvelope::new("s", b"persistent-msg".to_vec());
        ch.send("r", env).await.unwrap();

        let loaded = load_persisted_messages(&config.persistence_dir, &config.channel_id);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].payload, b"persistent-msg");
    }

    #[tokio::test]
    async fn test_bus_unsubscribe() {
        let ch = DoraChannel::new(ChannelConfig::default());
        ch.register_agent("sub").await.unwrap();

        ch.subscribe("topic", "sub", SubscribeOptions::default()).await.unwrap();
        ch.unsubscribe("topic", "sub").await.unwrap();

        let env = KernelEnvelope::new("pub", b"msg".to_vec());
        ch.publish("topic", env).await.unwrap();

        let msgs = ch
            .receive(
                "sub",
                ReceiveOptions {
                    timeout: Some(Duration::from_millis(50)),
                    max_messages: 1,
                },
            )
            .await
            .unwrap();
        assert!(msgs.is_empty());
    }

    #[tokio::test]
    async fn test_bus_metrics_snapshot() {
        let ch = DoraChannel::new(ChannelConfig::default());
        ch.register_agent("m").await.unwrap();

        ch.subscribe("t", "m", SubscribeOptions::default()).await.unwrap();

        for i in 0..5 {
            let env = KernelEnvelope::new("p", format!("msg-{}", i).into_bytes());
            ch.publish("t", env).await.unwrap();
        }

        let snap = ch.metrics_snapshot();
        assert_eq!(snap.total_published, 5);
        assert_eq!(snap.total_delivered, 5);
    }
}
