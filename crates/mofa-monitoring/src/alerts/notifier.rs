//! Alert notifiers.
//!
//! A [`Notifier`] receives emitted [`AlertEvent`]s and delivers them to an
//! external system: log, webhook, chat channel, email, on-call rotation.
//!
//! Two in-tree notifiers ship with this PR:
//!
//! - [`LogNotifier`] — writes events to the `tracing` system. Intended as
//!   a default for development and for audit-logging production alongside
//!   richer notifiers.
//! - [`CollectingNotifier`] — records every event into an in-memory
//!   buffer. Intended for tests and for short-term in-memory dashboards.
//!
//! A [`CompositeNotifier`] fans out to multiple notifiers so a production
//! deployment can log every event while also pushing to a webhook.

use super::event::AlertEvent;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

/// Backend-agnostic notifier interface.
#[async_trait]
pub trait Notifier: Send + Sync {
    async fn notify(&self, event: &AlertEvent);
}

/// Writes events through the `tracing` subscriber. `Warning` and
/// `Critical` use `warn!`; `Info` uses `info!`.
#[derive(Debug, Default)]
pub struct LogNotifier;

impl LogNotifier {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Notifier for LogNotifier {
    async fn notify(&self, event: &AlertEvent) {
        let summary = event.short_summary();
        match event.severity {
            super::rule::Severity::Info => info!(target: "mofa_monitoring::alerts", "{summary}"),
            super::rule::Severity::Warning | super::rule::Severity::Critical => {
                warn!(target: "mofa_monitoring::alerts", "{summary}")
            }
        }
    }
}

/// Records every event into an in-memory buffer. The buffer is bounded —
/// oldest entries are dropped once `capacity` is reached.
pub struct CollectingNotifier {
    buffer: Mutex<std::collections::VecDeque<AlertEvent>>,
    capacity: usize,
}

impl CollectingNotifier {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Mutex::new(std::collections::VecDeque::with_capacity(capacity.min(1024))),
            capacity,
        }
    }

    /// Snapshot of all events currently held.
    pub fn snapshot(&self) -> Vec<AlertEvent> {
        self.buffer.lock().unwrap().iter().cloned().collect()
    }

    /// Number of events currently buffered.
    pub fn len(&self) -> usize {
        self.buffer.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drop all buffered events.
    pub fn clear(&self) {
        self.buffer.lock().unwrap().clear();
    }
}

#[async_trait]
impl Notifier for CollectingNotifier {
    async fn notify(&self, event: &AlertEvent) {
        let mut buf = self.buffer.lock().unwrap();
        if buf.len() == self.capacity {
            buf.pop_front();
        }
        buf.push_back(event.clone());
    }
}

/// Fans an event out to every wrapped notifier. Failures are swallowed —
/// fan-out is best-effort, a single broken notifier must not block
/// delivery to the others. Compose with a retry/circuit-breaker layer at
/// the call site if stronger guarantees are needed.
pub struct CompositeNotifier {
    inner: Vec<Arc<dyn Notifier>>,
}

impl CompositeNotifier {
    pub fn new(inner: Vec<Arc<dyn Notifier>>) -> Self {
        Self { inner }
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[async_trait]
impl Notifier for CompositeNotifier {
    async fn notify(&self, event: &AlertEvent) {
        for n in &self.inner {
            n.notify(event).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::event::AlertState;
    use super::super::rule::{ComparisonOp, Condition, Severity};
    use super::*;
    use std::collections::HashMap;
    use std::time::SystemTime;

    fn sample() -> AlertEvent {
        AlertEvent {
            rule_name: "r".into(),
            state: AlertState::Firing,
            severity: Severity::Warning,
            condition: Condition::Threshold {
                metric: "x".into(),
                op: ComparisonOp::Gt,
                threshold: 1.0,
            },
            observed_value: Some(2.0),
            labels: HashMap::new(),
            annotations: HashMap::new(),
            at: SystemTime::UNIX_EPOCH,
        }
    }

    #[tokio::test]
    async fn collecting_notifier_records_events() {
        let n = CollectingNotifier::with_capacity(10);
        assert!(n.is_empty());
        n.notify(&sample()).await;
        assert_eq!(n.len(), 1);
        assert_eq!(n.snapshot()[0].rule_name, "r");
    }

    #[tokio::test]
    async fn collecting_notifier_capacity_evicts_oldest() {
        let n = CollectingNotifier::with_capacity(2);
        let mut e = sample();
        e.rule_name = "a".into();
        n.notify(&e).await;
        e.rule_name = "b".into();
        n.notify(&e).await;
        e.rule_name = "c".into();
        n.notify(&e).await;

        let snap = n.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].rule_name, "b");
        assert_eq!(snap[1].rule_name, "c");
    }

    #[tokio::test]
    async fn collecting_notifier_clear() {
        let n = CollectingNotifier::with_capacity(10);
        n.notify(&sample()).await;
        assert_eq!(n.len(), 1);
        n.clear();
        assert!(n.is_empty());
    }

    #[tokio::test]
    async fn composite_fans_out_to_all_notifiers() {
        let a = Arc::new(CollectingNotifier::with_capacity(10));
        let b = Arc::new(CollectingNotifier::with_capacity(10));
        let composite = CompositeNotifier::new(vec![a.clone(), b.clone()]);
        composite.notify(&sample()).await;
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
    }

    #[tokio::test]
    async fn log_notifier_is_infallible() {
        let n = LogNotifier::new();
        // No panic / deadlock; we don't assert on the log subscriber
        // content because that depends on global state.
        n.notify(&sample()).await;
    }

    #[tokio::test]
    async fn composite_is_empty_len() {
        let c = CompositeNotifier::new(vec![]);
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
    }
}
