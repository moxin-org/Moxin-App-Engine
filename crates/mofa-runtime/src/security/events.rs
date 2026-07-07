//! Security Events
//!
//! Event types for security governance operations.
//! These events can be emitted to the event bus for audit logging and monitoring.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Security event types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SecurityEvent {
    /// Permission check event
    PermissionCheck {
        /// Subject requesting permission
        subject: String,
        /// Action being requested
        action: String,
        /// Resource being accessed
        resource: String,
        /// Whether permission was granted
        allowed: bool,
        /// Reason if denied
        reason: Option<String>,
        /// Timestamp (milliseconds since epoch)
        timestamp_ms: u64,
    },
    /// PII detection event
    PiiDetected {
        /// Category of detected PII
        category: String,
        /// Number of detections
        count: usize,
        /// Timestamp
        timestamp_ms: u64,
    },
    /// PII redaction event
    PiiRedacted {
        /// Number of redactions performed
        count: usize,
        /// Categories redacted
        categories: Vec<String>,
        /// Timestamp
        timestamp_ms: u64,
    },
    /// Content moderation event
    ContentModerated {
        /// Verdict (allow, flag, block)
        verdict: String,
        /// Reason if flagged/blocked
        reason: Option<String>,
        /// Timestamp
        timestamp_ms: u64,
    },
    /// Prompt injection detected
    PromptInjectionDetected {
        /// Confidence score
        confidence: f64,
        /// Detected pattern
        pattern: String,
        /// Timestamp
        timestamp_ms: u64,
    },
}

impl SecurityEvent {
    /// Get the timestamp of the event
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            SecurityEvent::PermissionCheck { timestamp_ms, .. } => *timestamp_ms,
            SecurityEvent::PiiDetected { timestamp_ms, .. } => *timestamp_ms,
            SecurityEvent::PiiRedacted { timestamp_ms, .. } => *timestamp_ms,
            SecurityEvent::ContentModerated { timestamp_ms, .. } => *timestamp_ms,
            SecurityEvent::PromptInjectionDetected { timestamp_ms, .. } => *timestamp_ms,
        }
    }

    /// Create a permission check event
    pub fn permission_check(
        subject: String,
        action: String,
        resource: String,
        allowed: bool,
        reason: Option<String>,
    ) -> Self {
        Self::PermissionCheck {
            subject,
            action,
            resource,
            allowed,
            reason,
            timestamp_ms: now_ms(),
        }
    }

    /// Create a PII detected event
    pub fn pii_detected(category: String, count: usize) -> Self {
        Self::PiiDetected {
            category,
            count,
            timestamp_ms: now_ms(),
        }
    }

    /// Create a PII redacted event
    pub fn pii_redacted(count: usize, categories: Vec<String>) -> Self {
        Self::PiiRedacted {
            count,
            categories,
            timestamp_ms: now_ms(),
        }
    }

    /// Create a content moderation event
    pub fn content_moderated(verdict: String, reason: Option<String>) -> Self {
        Self::ContentModerated {
            verdict,
            reason,
            timestamp_ms: now_ms(),
        }
    }

    /// Create a prompt injection detected event
    pub fn prompt_injection_detected(confidence: f64, pattern: String) -> Self {
        Self::PromptInjectionDetected {
            confidence,
            pattern,
            timestamp_ms: now_ms(),
        }
    }
}

/// Get current timestamp in milliseconds
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_event_creation() {
        let event = SecurityEvent::permission_check(
            "agent-1".to_string(),
            "execute".to_string(),
            "tool:delete".to_string(),
            false,
            Some("insufficient permissions".to_string()),
        );

        assert!(event.timestamp_ms() > 0);
        match event {
            SecurityEvent::PermissionCheck {
                subject, allowed, ..
            } => {
                assert_eq!(subject, "agent-1");
                assert!(!allowed);
            }
            _ => panic!("Expected PermissionCheck variant, got: {:?}", event),
        }
    }
}
