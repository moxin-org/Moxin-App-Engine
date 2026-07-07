//! mofa-smith — swarm observability and trace reporting.
//!
//! ## SwarmTraceReporter
//!
//! Subscribes to a stream of [`AuditEvent`]s produced during swarm execution
//! and forwards structured [`SwarmSpan`]s to a pluggable [`TraceBackend`].
//!
//! ```text
//!  SwarmOrchestrator
//!       │  AuditEvent (via mpsc channel)
//!       ▼
//!  SwarmTraceReporter::run_loop()
//!       │  converts to SwarmSpan
//!       ▼
//!  TraceBackend::record_span()
//!      ├── LogTraceBackend  (tracing::info!, for development)
//!      ├── InMemoryBackend  (for tests)
//!      └── (future: OpenTelemetry, Grafana Tempo, etc.)
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use mofa_foundation::swarm::config::{AuditEvent, AuditEventKind};
use tokio::sync::mpsc;
use uuid::Uuid;

// ── span types ────────────────────────────────────────────────────────────────

/// a single structured trace span emitted by the reporter
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SwarmSpan {
    /// trace id shared across all spans in one swarm run
    pub trace_id: String,
    /// unique id for this span
    pub span_id: String,
    /// subtask id, if the event is tied to a specific task
    pub task_id: Option<String>,
    /// string representation of the originating `AuditEventKind`
    pub event_kind: String,
    /// human-readable description from the audit event
    pub description: String,
    /// unix timestamp in milliseconds
    pub timestamp_ms: u64,
    /// wall-clock duration since task started (populated for completion events)
    pub duration_ms: Option<u64>,
    /// arbitrary metadata from the audit event's `data` field
    pub metadata: serde_json::Value,
}

impl SwarmSpan {
    fn from_event(event: &AuditEvent, trace_id: &str, duration_ms: Option<u64>) -> Self {
        Self {
            trace_id: trace_id.to_string(),
            span_id: Uuid::new_v4().to_string(),
            task_id: extract_task_id(&event.data),
            event_kind: format!("{:?}", event.kind),
            description: event.description.clone(),
            timestamp_ms: event
                .timestamp
                .timestamp_millis()
                .try_into()
                .unwrap_or(now_millis()),
            duration_ms,
            metadata: event.data.clone(),
        }
    }
}

fn extract_task_id(data: &serde_json::Value) -> Option<String> {
    data.get("task_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ── backend trait ──────────────────────────────────────────────────────────────

/// pluggable sink for swarm trace spans
pub trait TraceBackend: Send + Sync {
    /// record one span — implementations should be non-blocking
    fn record_span(&self, span: SwarmSpan);
    /// flush any buffered spans (no-op for non-buffered backends)
    fn flush(&self) {}
}

// ── log backend (default, development-friendly) ───────────────────────────────

/// emits each span as a structured `tracing::info!` log line
pub struct LogTraceBackend;

impl TraceBackend for LogTraceBackend {
    fn record_span(&self, span: SwarmSpan) {
        tracing::info!(
            trace_id = %span.trace_id,
            span_id  = %span.span_id,
            task_id  = ?span.task_id,
            kind     = %span.event_kind,
            duration_ms = ?span.duration_ms,
            "{}",
            span.description
        );
    }
}

// ── in-memory backend (for tests) ─────────────────────────────────────────────

/// collects spans in memory — useful for tests and inspection
#[derive(Debug, Default, Clone)]
pub struct InMemoryBackend {
    spans: Arc<Mutex<Vec<SwarmSpan>>>,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// returns a snapshot of all collected spans
    pub fn spans(&self) -> Vec<SwarmSpan> {
        self.spans.lock().unwrap().clone()
    }

    /// returns the number of collected spans
    pub fn len(&self) -> usize {
        self.spans.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.lock().unwrap().is_empty()
    }

    /// clears all stored spans
    pub fn clear(&self) {
        self.spans.lock().unwrap().clear();
    }
}

impl TraceBackend for InMemoryBackend {
    fn record_span(&self, span: SwarmSpan) {
        self.spans.lock().unwrap().push(span);
    }
}

// ── reporter ──────────────────────────────────────────────────────────────────

/// subscribes to swarm `AuditEvent`s and reports them as `SwarmSpan`s.
///
/// # Usage — direct reporting
///
/// ```rust,ignore
/// let backend = Arc::new(LogTraceBackend);
/// let reporter = SwarmTraceReporter::new(backend);
///
/// reporter.report(&audit_event);
/// reporter.report_batch(&events);
/// ```
///
/// # Usage — channel-driven loop
///
/// ```rust,ignore
/// let (tx, rx) = tokio::sync::mpsc::channel(256);
/// let reporter = SwarmTraceReporter::new(Arc::new(LogTraceBackend));
///
/// tokio::spawn(reporter.run_loop(rx));
///
/// // during swarm execution:
/// tx.send(AuditEvent::new(AuditEventKind::SubtaskCompleted, "step done")).await?;
/// ```
pub struct SwarmTraceReporter {
    backend: Arc<dyn TraceBackend>,
    /// trace id shared across all spans for one swarm run
    trace_id: String,
    /// task_id → start timestamp (ms), used to compute durations
    task_starts: Arc<Mutex<HashMap<String, u64>>>,
}

impl SwarmTraceReporter {
    /// create a reporter with a fresh trace id
    pub fn new(backend: Arc<dyn TraceBackend>) -> Self {
        Self {
            backend,
            trace_id: Uuid::new_v4().to_string(),
            task_starts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// create a reporter tied to a specific trace id (for correlating with an existing trace)
    pub fn with_trace_id(backend: Arc<dyn TraceBackend>, trace_id: impl Into<String>) -> Self {
        Self {
            backend,
            trace_id: trace_id.into(),
            task_starts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// returns the trace id for this reporter
    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    /// report a single audit event, computing duration for completion events
    pub fn report(&self, event: &AuditEvent) {
        let duration_ms = self.compute_duration(event);
        self.track_task_start(event);
        let span = SwarmSpan::from_event(event, &self.trace_id, duration_ms);
        self.backend.record_span(span);
    }

    /// report a batch of audit events in order
    pub fn report_batch(&self, events: &[AuditEvent]) {
        for event in events {
            self.report(event);
        }
    }

    /// flush the backend (no-op for non-buffered backends)
    pub fn flush(&self) {
        self.backend.flush();
    }

    /// run an async loop consuming events from `rx` until the sender is dropped
    pub async fn run_loop(self, mut rx: mpsc::Receiver<AuditEvent>) {
        while let Some(event) = rx.recv().await {
            self.report(&event);
        }
        self.flush();
        tracing::info!(
            trace_id = %self.trace_id,
            "swarm trace reporter: channel closed, flushed"
        );
    }

    // ── helpers ────────────────────────────────────────────────────────────

    fn track_task_start(&self, event: &AuditEvent) {
        if matches!(event.kind, AuditEventKind::SubtaskStarted) {
            if let Some(task_id) = extract_task_id(&event.data) {
                let ts = event
                    .timestamp
                    .timestamp_millis()
                    .try_into()
                    .unwrap_or(now_millis());
                self.task_starts.lock().unwrap().insert(task_id, ts);
            }
        }
    }

    fn compute_duration(&self, event: &AuditEvent) -> Option<u64> {
        let is_terminal = matches!(
            event.kind,
            AuditEventKind::SubtaskCompleted
                | AuditEventKind::SubtaskFailed
                | AuditEventKind::SwarmCompleted
        );
        if !is_terminal {
            return None;
        }
        let task_id = extract_task_id(&event.data)?;
        let start = *self.task_starts.lock().unwrap().get(&task_id)?;
        let end: u64 = event
            .timestamp
            .timestamp_millis()
            .try_into()
            .unwrap_or(now_millis());
        Some(end.saturating_sub(start))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofa_foundation::swarm::config::{AuditEvent, AuditEventKind};

    fn event(kind: AuditEventKind, desc: &str) -> AuditEvent {
        AuditEvent::new(kind, desc)
    }

    fn event_with_task(kind: AuditEventKind, task_id: &str) -> AuditEvent {
        AuditEvent::new(kind, task_id).with_data(serde_json::json!({ "task_id": task_id }))
    }

    // ── InMemoryBackend ──

    #[test]
    fn in_memory_backend_collects_spans() {
        let backend = Arc::new(InMemoryBackend::new());
        let reporter = SwarmTraceReporter::new(backend.clone());

        reporter.report(&event(AuditEventKind::SwarmStarted, "started"));
        reporter.report(&event(AuditEventKind::SubtaskStarted, "t1 started"));

        assert_eq!(backend.len(), 2);
    }

    #[test]
    fn report_batch_records_all_events() {
        let backend = Arc::new(InMemoryBackend::new());
        let reporter = SwarmTraceReporter::new(backend.clone());

        let events = vec![
            event(AuditEventKind::SwarmStarted, "start"),
            event(AuditEventKind::SubtaskStarted, "t1"),
            event(AuditEventKind::SubtaskCompleted, "t1 done"),
            event(AuditEventKind::SwarmCompleted, "done"),
        ];
        reporter.report_batch(&events);

        assert_eq!(backend.len(), 4);
    }

    #[test]
    fn spans_share_trace_id() {
        let backend = Arc::new(InMemoryBackend::new());
        let reporter = SwarmTraceReporter::new(backend.clone());

        reporter.report(&event(AuditEventKind::SwarmStarted, "a"));
        reporter.report(&event(AuditEventKind::SwarmCompleted, "b"));

        let spans = backend.spans();
        assert_eq!(spans[0].trace_id, spans[1].trace_id);
        assert_eq!(spans[0].trace_id, reporter.trace_id());
    }

    #[test]
    fn span_ids_are_unique() {
        let backend = Arc::new(InMemoryBackend::new());
        let reporter = SwarmTraceReporter::new(backend.clone());

        reporter.report(&event(AuditEventKind::SwarmStarted, "a"));
        reporter.report(&event(AuditEventKind::SwarmCompleted, "b"));

        let spans = backend.spans();
        assert_ne!(spans[0].span_id, spans[1].span_id);
    }

    #[test]
    fn event_kind_serialised_correctly() {
        let backend = Arc::new(InMemoryBackend::new());
        let reporter = SwarmTraceReporter::new(backend.clone());
        reporter.report(&event(AuditEventKind::HITLRequested, "approval needed"));

        let span = &backend.spans()[0];
        assert_eq!(span.event_kind, "HITLRequested");
    }

    #[test]
    fn task_id_extracted_from_data() {
        let backend = Arc::new(InMemoryBackend::new());
        let reporter = SwarmTraceReporter::new(backend.clone());

        reporter.report(&event_with_task(AuditEventKind::SubtaskStarted, "task-42"));

        let span = &backend.spans()[0];
        assert_eq!(span.task_id.as_deref(), Some("task-42"));
    }

    #[test]
    fn duration_computed_for_completed_event() {
        let backend = Arc::new(InMemoryBackend::new());
        let reporter = SwarmTraceReporter::new(backend.clone());

        reporter.report(&event_with_task(AuditEventKind::SubtaskStarted, "t1"));
        reporter.report(&event_with_task(AuditEventKind::SubtaskCompleted, "t1"));

        let spans = backend.spans();
        // start span has no duration
        assert!(spans[0].duration_ms.is_none());
        // completed span has duration (may be 0 in fast tests, but must be Some)
        assert!(spans[1].duration_ms.is_some());
    }

    #[test]
    fn no_duration_for_non_terminal_events() {
        let backend = Arc::new(InMemoryBackend::new());
        let reporter = SwarmTraceReporter::new(backend.clone());

        reporter.report(&event(AuditEventKind::AgentAssigned, "assigned"));

        assert!(backend.spans()[0].duration_ms.is_none());
    }

    #[test]
    fn with_trace_id_uses_provided_id() {
        let backend = Arc::new(InMemoryBackend::new());
        let reporter =
            SwarmTraceReporter::with_trace_id(backend.clone(), "my-trace-123");

        reporter.report(&event(AuditEventKind::SwarmStarted, "s"));

        let span = &backend.spans()[0];
        assert_eq!(span.trace_id, "my-trace-123");
        assert_eq!(reporter.trace_id(), "my-trace-123");
    }

    #[test]
    fn clear_backend_resets_count() {
        let backend = Arc::new(InMemoryBackend::new());
        let reporter = SwarmTraceReporter::new(backend.clone());

        reporter.report(&event(AuditEventKind::SwarmStarted, "s"));
        assert_eq!(backend.len(), 1);
        backend.clear();
        assert!(backend.is_empty());
    }

    // ── run_loop ──

    #[tokio::test]
    async fn run_loop_collects_all_sent_events() {
        let backend = Arc::new(InMemoryBackend::new());
        let reporter = SwarmTraceReporter::new(backend.clone());
        let (tx, rx) = mpsc::channel(16);

        let handle = tokio::spawn(reporter.run_loop(rx));

        for i in 0..5 {
            tx.send(event(AuditEventKind::SubtaskStarted, &format!("t{i}")))
                .await
                .unwrap();
        }
        drop(tx); // close channel
        handle.await.unwrap();

        assert_eq!(backend.len(), 5);
    }

    #[tokio::test]
    async fn run_loop_exits_when_channel_closed() {
        let backend = Arc::new(InMemoryBackend::new());
        let reporter = SwarmTraceReporter::new(backend.clone());
        let (tx, rx) = mpsc::channel::<AuditEvent>(4);

        let handle = tokio::spawn(reporter.run_loop(rx));
        drop(tx); // immediately close

        // should complete without hanging
        handle.await.unwrap();
        assert!(backend.is_empty());
    }
}
