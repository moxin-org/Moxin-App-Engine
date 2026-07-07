use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

// Standardized message envelope with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub message_id: String,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub schema_version: Option<String>,
    pub sender_id: String,
    pub recipient_id: Option<String>,
    pub topic: Option<String>,
    pub timestamp_ms: u64,
    pub attempt: u32,
    #[serde(
        serialize_with = "serialize_opt_duration",
        deserialize_with = "deserialize_opt_duration"
    )]
    pub ttl: Option<Duration>,
    pub payload: Vec<u8>,
    pub headers: HashMap<String, String>,
}

fn serialize_opt_duration<S>(dur: &Option<Duration>, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match dur {
        Some(d) => s.serialize_some(&d.as_millis()),
        None => s.serialize_none(),
    }
}

fn deserialize_opt_duration<'de, D>(d: D) -> Result<Option<Duration>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<u128> = Option::deserialize(d)?;
    Ok(opt.map(|ms| Duration::from_millis(ms as u64)))
}

pub fn serialize_opt_duration_pub<S>(dur: &Option<Duration>, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serialize_opt_duration(dur, s)
}

pub fn deserialize_opt_duration_pub<'de, D>(d: D) -> Result<Option<Duration>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_opt_duration(d)
}

impl MessageEnvelope {
    pub fn new(sender_id: &str, payload: Vec<u8>) -> Self {
        Self {
            message_id: uuid::Uuid::now_v7().to_string(),
            correlation_id: None,
            causation_id: None,
            trace_id: None,
            span_id: None,
            schema_version: None,
            sender_id: sender_id.to_string(),
            recipient_id: None,
            topic: None,
            timestamp_ms: now_epoch_ms(),
            attempt: 1,
            ttl: None,
            payload,
            headers: HashMap::new(),
        }
    }

    pub fn with_recipient(mut self, recipient_id: &str) -> Self {
        self.recipient_id = Some(recipient_id.to_string());
        self
    }

    pub fn with_topic(mut self, topic: &str) -> Self {
        self.topic = Some(topic.to_string());
        self
    }

    pub fn with_correlation_id(mut self, id: &str) -> Self {
        self.correlation_id = Some(id.to_string());
        self
    }

    pub fn with_causation_id(mut self, id: &str) -> Self {
        self.causation_id = Some(id.to_string());
        self
    }

    pub fn with_tracing(mut self, trace_id: &str, span_id: &str) -> Self {
        self.trace_id = Some(trace_id.to_string());
        self.span_id = Some(span_id.to_string());
        self
    }

    pub fn with_schema_version(mut self, version: &str) -> Self {
        self.schema_version = Some(version.to_string());
        self
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }

    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers.extend(headers);
        self
    }

    pub fn is_expired(&self) -> bool {
        if let Some(ttl) = self.ttl {
            let age_ms = now_epoch_ms().saturating_sub(self.timestamp_ms);
            age_ms > ttl.as_millis() as u64
        } else {
            false
        }
    }

    pub fn increment_attempt(&mut self) {
        self.attempt += 1;
    }
}

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_construction() {
        let env = MessageEnvelope::new("sender-1", b"hello".to_vec())
            .with_topic("events.test")
            .with_recipient("receiver-1")
            .with_correlation_id("corr-1")
            .with_causation_id("cause-1")
            .with_tracing("trace-abc", "span-def")
            .with_schema_version("2.0.0")
            .with_ttl(Duration::from_secs(60))
            .with_header("priority", "high");

        assert_eq!(env.sender_id, "sender-1");
        assert_eq!(env.recipient_id.as_deref(), Some("receiver-1"));
        assert_eq!(env.topic.as_deref(), Some("events.test"));
        assert_eq!(env.correlation_id.as_deref(), Some("corr-1"));
        assert_eq!(env.causation_id.as_deref(), Some("cause-1"));
        assert_eq!(env.trace_id.as_deref(), Some("trace-abc"));
        assert_eq!(env.span_id.as_deref(), Some("span-def"));
        assert_eq!(env.schema_version.as_deref(), Some("2.0.0"));
        assert_eq!(env.ttl, Some(Duration::from_secs(60)));
        assert_eq!(env.headers.get("priority").map(|s| s.as_str()), Some("high"));
        assert_eq!(env.attempt, 1);
        assert_eq!(env.payload, b"hello");
    }

    #[test]
    fn test_is_expired() {
        let mut env = MessageEnvelope::new("s", vec![]);
        assert!(!env.is_expired());

        env.ttl = Some(Duration::from_millis(0));
        env.timestamp_ms = now_epoch_ms().saturating_sub(1);
        assert!(env.is_expired());
    }

    #[test]
    fn test_increment_attempt() {
        let mut env = MessageEnvelope::new("s", vec![]);
        assert_eq!(env.attempt, 1);
        env.increment_attempt();
        assert_eq!(env.attempt, 2);
    }

    #[test]
    fn test_serde_roundtrip() {
        let env = MessageEnvelope::new("s", b"data".to_vec())
            .with_ttl(Duration::from_secs(120));
        let json = serde_json::to_string(&env).unwrap();
        let restored: MessageEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.message_id, env.message_id);
        assert_eq!(restored.ttl, env.ttl);
        assert_eq!(restored.payload, b"data");
    }
}
