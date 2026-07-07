//! Time-Travel Debugger Telemetry
//!
//! Defines the core telemetry types and traits for the visual time-travel debugger.
//! The kernel layer defines only trait interfaces and data types — concrete
//! implementations are provided in `mofa-foundation`.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────┐
//! │                  Telemetry Pipeline                   │
//! ├──────────────────────────────────────────────────────┤
//! │                                                       │
//! │  WorkflowExecutor ──emit──▶ TelemetryEmitter         │
//! │                                  │                    │
//! │                          ┌───────┴───────┐           │
//! │                          ▼               ▼           │
//! │                    Channel          SessionRecorder   │
//! │                  (real-time)        (persistence)     │
//! │                                                       │
//! └──────────────────────────────────────────────────────┘
//! ```
//!
//! # Event Types
//!
//! - `DebugEvent::WorkflowStart` — emitted when workflow execution begins
//! - `DebugEvent::NodeStart` — emitted when a node begins execution
//! - `DebugEvent::StateChange` — emitted on state mutations within a node
//! - `DebugEvent::NodeEnd` — emitted when a node finishes execution
//! - `DebugEvent::WorkflowEnd` — emitted when workflow execution completes
//! - `DebugEvent::Error` — emitted on errors during execution
//! - `DebugEvent::LLMCall` — emitted when an LLM API call completes

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::agent::error::AgentResult;

// ============================================================================
// DebugEvent — Telemetry event types
// ============================================================================

/// Debug telemetry event emitted during workflow execution.
///
/// These events form the backbone of the time-travel debugger, capturing
/// the complete execution trace of a workflow including state mutations.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_kernel::workflow::telemetry::DebugEvent;
///
/// let event = DebugEvent::NodeStart {
///     node_id: "process".to_string(),
///     timestamp_ms: 1700000000000,
///     state_snapshot: serde_json::json!({"messages": ["hello"]}),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DebugEvent {
    /// Workflow execution started
    WorkflowStart {
        /// Workflow graph ID
        workflow_id: String,
        /// Unique execution ID
        execution_id: String,
        /// Timestamp in milliseconds since epoch
        timestamp_ms: u64,
    },

    /// A node started executing
    NodeStart {
        /// Node identifier
        node_id: String,
        /// Timestamp in milliseconds since epoch
        timestamp_ms: u64,
        /// Snapshot of the state when the node started
        state_snapshot: serde_json::Value,
    },

    /// A state key was mutated during node execution
    StateChange {
        /// Node that caused the change
        node_id: String,
        /// Timestamp in milliseconds since epoch
        timestamp_ms: u64,
        /// State key that changed
        key: String,
        /// Previous value (None if key was new)
        old_value: Option<serde_json::Value>,
        /// New value
        new_value: serde_json::Value,
    },

    /// A node finished executing
    NodeEnd {
        /// Node identifier
        node_id: String,
        /// Timestamp in milliseconds since epoch
        timestamp_ms: u64,
        /// Snapshot of the state after node execution
        state_snapshot: serde_json::Value,
        /// Duration of node execution in milliseconds
        duration_ms: u64,
    },

    /// Workflow execution completed
    WorkflowEnd {
        /// Workflow graph ID
        workflow_id: String,
        /// Unique execution ID
        execution_id: String,
        /// Timestamp in milliseconds since epoch
        timestamp_ms: u64,
        /// Final status description
        status: String,
    },

    /// Error during execution
    Error {
        /// Node where the error occurred (if applicable)
        node_id: Option<String>,
        /// Timestamp in milliseconds since epoch
        timestamp_ms: u64,
        /// Error description
        error: String,
    },

    /// An LLM API call completed (success or failure).
    ///
    /// Captures per-request telemetry for LLM interactions, enabling
    /// debugging of individual model calls within a workflow node.
    LLMCall {
        /// Node that triggered this LLM call (if within a workflow)
        node_id: Option<String>,
        /// Timestamp in milliseconds since epoch
        timestamp_ms: u64,
        /// LLM provider name (e.g. "openai", "anthropic")
        provider: String,
        /// Model identifier (e.g. "gpt-4", "claude-3-opus")
        model: String,
        /// Input messages sent to the model (serialized ChatMessages)
        prompt_messages: Vec<serde_json::Value>,
        /// Model response payload
        response: serde_json::Value,
        /// Number of tokens in the prompt
        prompt_tokens: u64,
        /// Number of tokens in the completion
        completion_tokens: u64,
        /// Wall-clock latency of the API call in milliseconds
        latency_ms: u64,
        /// Estimated cost in USD (None if pricing is unknown)
        cost_usd: Option<f64>,
        /// Call outcome: "ok" or "error"
        status: String,
        /// Error message if status is "error"
        error: Option<String>,
    },
}

impl DebugEvent {
    /// Get the timestamp of this event in milliseconds since epoch
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            Self::WorkflowStart { timestamp_ms, .. } => *timestamp_ms,
            Self::NodeStart { timestamp_ms, .. } => *timestamp_ms,
            Self::StateChange { timestamp_ms, .. } => *timestamp_ms,
            Self::NodeEnd { timestamp_ms, .. } => *timestamp_ms,
            Self::WorkflowEnd { timestamp_ms, .. } => *timestamp_ms,
            Self::Error { timestamp_ms, .. } => *timestamp_ms,
            Self::LLMCall { timestamp_ms, .. } => *timestamp_ms,
        }
    }

    /// Get the node ID associated with this event, if any
    pub fn node_id(&self) -> Option<&str> {
        match self {
            Self::WorkflowStart { .. } => None,
            Self::NodeStart { node_id, .. } => Some(node_id),
            Self::StateChange { node_id, .. } => Some(node_id),
            Self::NodeEnd { node_id, .. } => Some(node_id),
            Self::WorkflowEnd { .. } => None,
            Self::Error { node_id, .. } => node_id.as_deref(),
            Self::LLMCall { node_id, .. } => node_id.as_deref(),
        }
    }

    /// Get the current timestamp in milliseconds since epoch
    pub fn now_ms() -> u64 {
        crate::utils::now_ms()
    }

    /// Returns a human-readable event type name
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::WorkflowStart { .. } => "workflow_start",
            Self::NodeStart { .. } => "node_start",
            Self::StateChange { .. } => "state_change",
            Self::NodeEnd { .. } => "node_end",
            Self::WorkflowEnd { .. } => "workflow_end",
            Self::Error { .. } => "error",
            Self::LLMCall { .. } => "llm_call",
        }
    }
}

// ============================================================================
// DebugSession — Session metadata
// ============================================================================

/// Metadata for a recorded debugging session.
///
/// A session corresponds to a single workflow execution, capturing
/// all telemetry events from start to finish.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugSession {
    /// Unique session identifier
    pub session_id: String,
    /// Workflow graph ID
    pub workflow_id: String,
    /// Execution ID (from RuntimeContext)
    pub execution_id: String,
    /// Session start timestamp (ms since epoch)
    pub started_at: u64,
    /// Session end timestamp (ms since epoch), None if still running
    pub ended_at: Option<u64>,
    /// Final status ("running", "completed", "failed")
    pub status: String,
    /// Total number of events recorded
    pub event_count: u64,
}

impl DebugSession {
    /// Create a new debug session
    pub fn new(
        session_id: impl Into<String>,
        workflow_id: impl Into<String>,
        execution_id: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            workflow_id: workflow_id.into(),
            execution_id: execution_id.into(),
            started_at: DebugEvent::now_ms(),
            ended_at: None,
            status: "running".to_string(),
            event_count: 0,
        }
    }
}

// ============================================================================
// SessionQuery — Filter parameters for session queries
// ============================================================================

/// Query parameters for filtering debug sessions.
///
/// All fields are optional — unset fields impose no constraint.
/// Results are ordered by `started_at` descending (newest first).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionQuery {
    /// Filter by workflow graph ID
    pub workflow_id: Option<String>,
    /// Filter by session status ("running", "completed", "failed")
    pub status: Option<String>,
    /// Only include sessions started at or after this timestamp (ms since epoch)
    pub from: Option<u64>,
    /// Only include sessions started at or before this timestamp (ms since epoch)
    pub to: Option<u64>,
    /// Only include sessions whose duration exceeds this threshold
    pub min_duration_ms: Option<u64>,
    /// Maximum number of sessions to return
    pub limit: Option<usize>,
    /// Number of sessions to skip (for pagination)
    pub offset: Option<usize>,
}

impl SessionQuery {
    /// Returns true if the given session matches all active filters.
    pub fn matches(&self, session: &DebugSession) -> bool {
        if let Some(ref wf) = self.workflow_id
            && session.workflow_id != *wf
        {
            return false;
        }
        if let Some(ref status) = self.status
            && session.status != *status
        {
            return false;
        }
        if let Some(from) = self.from
            && session.started_at < from
        {
            return false;
        }
        if let Some(to) = self.to
            && session.started_at > to
        {
            return false;
        }
        if let Some(min_dur) = self.min_duration_ms {
            let duration = match session.ended_at {
                Some(ended) => ended.saturating_sub(session.started_at),
                None => return false, // still running, duration unknown
            };
            if duration < min_dur {
                return false;
            }
        }
        true
    }

    /// Sort sessions by `started_at` descending, then apply offset and limit.
    pub fn paginate(&self, mut sessions: Vec<DebugSession>) -> Vec<DebugSession> {
        sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        let offset = self.offset.unwrap_or(0);
        let iter = sessions.into_iter().skip(offset);
        match self.limit {
            Some(limit) => iter.take(limit).collect(),
            None => iter.collect(),
        }
    }
}

// ============================================================================
// TelemetryEmitter — Trait for emitting debug events
// ============================================================================

/// Trait for emitting telemetry events during workflow execution.
///
/// Implementations can forward events to channels, recorders, loggers, etc.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_kernel::workflow::telemetry::{TelemetryEmitter, DebugEvent};
///
/// struct LoggingEmitter;
///
/// #[async_trait]
/// impl TelemetryEmitter for LoggingEmitter {
///     async fn emit(&self, event: DebugEvent) {
///         println!("[{}] {:?}", event.event_type(), event);
///     }
/// }
/// ```
#[async_trait]
pub trait TelemetryEmitter: Send + Sync {
    /// Emit a debug event
    async fn emit(&self, event: DebugEvent);

    /// Check if telemetry collection is enabled
    ///
    /// When false, the executor may skip expensive operations like
    /// state snapshot serialization.
    fn is_enabled(&self) -> bool {
        true
    }
}

// ============================================================================
// SessionRecorder — Trait for persisting debug sessions
// ============================================================================

/// Trait for persisting and querying debug sessions.
///
/// Implementations store the complete execution trace to enable
/// time-travel debugging: replaying the exact sequence of events
/// and state mutations step-by-step.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_kernel::workflow::telemetry::{SessionRecorder, DebugSession, DebugEvent};
///
/// // Start a session
/// let session = DebugSession::new("session-1", "workflow-1", "exec-1");
/// recorder.start_session(&session).await?;
///
/// // Record events during execution
/// recorder.record_event("session-1", &event).await?;
///
/// // End the session
/// recorder.end_session("session-1").await?;
///
/// // Later: replay the session
/// let events = recorder.get_events("session-1").await?;
/// for event in events {
///     // Step through each event in the time-travel UI
/// }
/// ```
#[async_trait]
pub trait SessionRecorder: Send + Sync {
    /// Start recording a new debug session
    async fn start_session(&self, session: &DebugSession) -> AgentResult<()>;

    /// Record a telemetry event within a session
    async fn record_event(&self, session_id: &str, event: &DebugEvent) -> AgentResult<()>;

    /// End a recording session (sets ended_at and final status)
    async fn end_session(&self, session_id: &str, status: &str) -> AgentResult<()>;

    /// Retrieve session metadata by ID
    async fn get_session(&self, session_id: &str) -> AgentResult<Option<DebugSession>>;

    /// Retrieve all events for a session, ordered by timestamp
    async fn get_events(&self, session_id: &str) -> AgentResult<Vec<DebugEvent>>;

    /// List all recorded sessions
    async fn list_sessions(&self) -> AgentResult<Vec<DebugSession>>;

    /// Query sessions with filtering, pagination, and ordering.
    ///
    /// The default implementation falls back to `list_sessions()` with
    /// in-memory filtering. Backends with indexed storage (e.g. PostgreSQL)
    /// should override this for efficient server-side queries.
    async fn query_sessions(&self, query: &SessionQuery) -> AgentResult<Vec<DebugSession>> {
        let sessions = self.list_sessions().await?;
        let filtered: Vec<DebugSession> =
            sessions.into_iter().filter(|s| query.matches(s)).collect();
        Ok(query.paginate(filtered))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_debug_event_serialization() {
        let event = DebugEvent::NodeStart {
            node_id: "process".to_string(),
            timestamp_ms: 1700000000000,
            state_snapshot: json!({"messages": ["hello"]}),
        };

        let serialized = serde_json::to_string(&event).unwrap();
        let deserialized: DebugEvent = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.event_type(), "node_start");
        assert_eq!(deserialized.node_id(), Some("process"));
        assert_eq!(deserialized.timestamp_ms(), 1700000000000);
    }

    #[test]
    fn test_debug_event_all_variants_serialize() {
        let events = vec![
            DebugEvent::WorkflowStart {
                workflow_id: "wf-1".to_string(),
                execution_id: "exec-1".to_string(),
                timestamp_ms: 1000,
            },
            DebugEvent::NodeStart {
                node_id: "n1".to_string(),
                timestamp_ms: 1001,
                state_snapshot: json!({}),
            },
            DebugEvent::StateChange {
                node_id: "n1".to_string(),
                timestamp_ms: 1002,
                key: "count".to_string(),
                old_value: Some(json!(0)),
                new_value: json!(1),
            },
            DebugEvent::NodeEnd {
                node_id: "n1".to_string(),
                timestamp_ms: 1003,
                state_snapshot: json!({"count": 1}),
                duration_ms: 2,
            },
            DebugEvent::WorkflowEnd {
                workflow_id: "wf-1".to_string(),
                execution_id: "exec-1".to_string(),
                timestamp_ms: 1004,
                status: "completed".to_string(),
            },
            DebugEvent::Error {
                node_id: Some("n2".to_string()),
                timestamp_ms: 1005,
                error: "something failed".to_string(),
            },
        ];

        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            let round_trip: DebugEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event.event_type(), round_trip.event_type());
            assert_eq!(event.timestamp_ms(), round_trip.timestamp_ms());
        }
    }

    #[test]
    fn test_debug_event_node_id() {
        let start = DebugEvent::WorkflowStart {
            workflow_id: "w".to_string(),
            execution_id: "e".to_string(),
            timestamp_ms: 0,
        };
        assert_eq!(start.node_id(), None);

        let node = DebugEvent::NodeStart {
            node_id: "n1".to_string(),
            timestamp_ms: 0,
            state_snapshot: json!(null),
        };
        assert_eq!(node.node_id(), Some("n1"));

        let error_with_node = DebugEvent::Error {
            node_id: Some("n2".to_string()),
            timestamp_ms: 0,
            error: "err".to_string(),
        };
        assert_eq!(error_with_node.node_id(), Some("n2"));

        let error_without = DebugEvent::Error {
            node_id: None,
            timestamp_ms: 0,
            error: "err".to_string(),
        };
        assert_eq!(error_without.node_id(), None);
    }

    #[test]
    fn test_debug_session_creation() {
        let session = DebugSession::new("s-1", "wf-1", "exec-1");
        assert_eq!(session.session_id, "s-1");
        assert_eq!(session.workflow_id, "wf-1");
        assert_eq!(session.execution_id, "exec-1");
        assert_eq!(session.status, "running");
        assert!(session.ended_at.is_none());
        assert_eq!(session.event_count, 0);
        assert!(session.started_at > 0);
    }

    #[test]
    fn test_debug_session_serialization() {
        let session = DebugSession::new("s-1", "wf-1", "exec-1");
        let json = serde_json::to_string(&session).unwrap();
        let round_trip: DebugSession = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.session_id, "s-1");
        assert_eq!(round_trip.status, "running");
    }

    #[test]
    fn test_now_ms() {
        let ts = DebugEvent::now_ms();
        // Should be a reasonable timestamp (after 2020)
        assert!(ts > 1_577_836_800_000);
    }

    fn make_session(
        id: &str,
        wf: &str,
        status: &str,
        start: u64,
        end: Option<u64>,
    ) -> DebugSession {
        DebugSession {
            session_id: id.to_string(),
            workflow_id: wf.to_string(),
            execution_id: format!("exec-{id}"),
            started_at: start,
            ended_at: end,
            status: status.to_string(),
            event_count: 0,
        }
    }

    #[test]
    fn test_session_query_matches_workflow_id() {
        let query = SessionQuery {
            workflow_id: Some("wf-a".to_string()),
            ..Default::default()
        };
        let s1 = make_session("s1", "wf-a", "completed", 1000, Some(2000));
        let s2 = make_session("s2", "wf-b", "completed", 1000, Some(2000));
        assert!(query.matches(&s1));
        assert!(!query.matches(&s2));
    }

    #[test]
    fn test_session_query_matches_status() {
        let query = SessionQuery {
            status: Some("failed".to_string()),
            ..Default::default()
        };
        let s1 = make_session("s1", "wf", "failed", 1000, Some(2000));
        let s2 = make_session("s2", "wf", "completed", 1000, Some(2000));
        assert!(query.matches(&s1));
        assert!(!query.matches(&s2));
    }

    #[test]
    fn test_session_query_matches_time_range() {
        let query = SessionQuery {
            from: Some(500),
            to: Some(1500),
            ..Default::default()
        };
        let before = make_session("s1", "wf", "completed", 100, Some(200));
        let inside = make_session("s2", "wf", "completed", 1000, Some(2000));
        let after = make_session("s3", "wf", "completed", 2000, Some(3000));
        assert!(!query.matches(&before));
        assert!(query.matches(&inside));
        assert!(!query.matches(&after));
    }

    #[test]
    fn test_session_query_matches_min_duration() {
        let query = SessionQuery {
            min_duration_ms: Some(500),
            ..Default::default()
        };
        let short = make_session("s1", "wf", "completed", 1000, Some(1200));
        let long = make_session("s2", "wf", "completed", 1000, Some(2000));
        let running = make_session("s3", "wf", "running", 1000, None);
        assert!(!query.matches(&short)); // 200ms < 500ms
        assert!(query.matches(&long)); // 1000ms >= 500ms
        assert!(!query.matches(&running)); // still running, duration unknown
    }

    #[test]
    fn test_session_query_combined_filters() {
        let query = SessionQuery {
            workflow_id: Some("wf-x".to_string()),
            status: Some("failed".to_string()),
            from: Some(1000),
            ..Default::default()
        };
        let match_all = make_session("s1", "wf-x", "failed", 1500, Some(2000));
        let wrong_wf = make_session("s2", "wf-y", "failed", 1500, Some(2000));
        let wrong_status = make_session("s3", "wf-x", "completed", 1500, Some(2000));
        let too_early = make_session("s4", "wf-x", "failed", 500, Some(600));
        assert!(query.matches(&match_all));
        assert!(!query.matches(&wrong_wf));
        assert!(!query.matches(&wrong_status));
        assert!(!query.matches(&too_early));
    }

    #[test]
    fn test_session_query_paginate() {
        // sessions created in ascending order: s0(0), s1(100), ..., s9(900)
        let sessions: Vec<DebugSession> = (0..10)
            .map(|i| {
                make_session(
                    &format!("s{i}"),
                    "wf",
                    "completed",
                    i * 100,
                    Some(i * 100 + 50),
                )
            })
            .collect();

        // limit only — sorted descending, so newest (s9) comes first
        let q = SessionQuery {
            limit: Some(3),
            ..Default::default()
        };
        let result = q.paginate(sessions.clone());
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].session_id, "s9");
        assert_eq!(result[1].session_id, "s8");
        assert_eq!(result[2].session_id, "s7");

        // offset only — skip 7 newest, leaving 3 oldest
        let q = SessionQuery {
            offset: Some(7),
            ..Default::default()
        };
        assert_eq!(q.paginate(sessions.clone()).len(), 3);

        // limit + offset — skip 5 newest, take next 2
        let q = SessionQuery {
            limit: Some(2),
            offset: Some(5),
            ..Default::default()
        };
        let result = q.paginate(sessions.clone());
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].session_id, "s4");
        assert_eq!(result[1].session_id, "s3");

        // no limit or offset returns all
        let q = SessionQuery::default();
        assert_eq!(q.paginate(sessions).len(), 10);
    }

    #[test]
    fn test_paginate_sorts_descending_regardless_of_input_order() {
        // feed sessions in random order
        let sessions = vec![
            make_session("mid", "wf", "completed", 500, Some(550)),
            make_session("old", "wf", "completed", 100, Some(150)),
            make_session("new", "wf", "completed", 900, Some(950)),
        ];
        let q = SessionQuery::default();
        let result = q.paginate(sessions);
        assert_eq!(result[0].session_id, "new");
        assert_eq!(result[1].session_id, "mid");
        assert_eq!(result[2].session_id, "old");
    }

    #[test]
    fn test_session_query_empty_matches_all() {
        let query = SessionQuery::default();
        let s = make_session("s1", "wf", "completed", 1000, Some(2000));
        assert!(query.matches(&s));
    }

    #[test]
    fn test_llm_call_event_serialization() {
        let event = DebugEvent::LLMCall {
            node_id: Some("process".to_string()),
            timestamp_ms: 1700000001000,
            provider: "openai".to_string(),
            model: "gpt-4".to_string(),
            prompt_messages: vec![json!({"role": "user", "content": "hello"})],
            response: json!({"choices": [{"message": {"content": "hi"}}]}),
            prompt_tokens: 5,
            completion_tokens: 3,
            latency_ms: 230,
            cost_usd: Some(0.002),
            status: "ok".to_string(),
            error: None,
        };

        let serialized = serde_json::to_string(&event).unwrap();
        let deserialized: DebugEvent = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.event_type(), "llm_call");
        assert_eq!(deserialized.node_id(), Some("process"));
        assert_eq!(deserialized.timestamp_ms(), 1700000001000);
    }

    #[test]
    fn test_llm_call_event_without_node() {
        let event = DebugEvent::LLMCall {
            node_id: None,
            timestamp_ms: 1000,
            provider: "anthropic".to_string(),
            model: "claude-3-opus".to_string(),
            prompt_messages: vec![],
            response: json!(null),
            prompt_tokens: 0,
            completion_tokens: 0,
            latency_ms: 0,
            cost_usd: None,
            status: "error".to_string(),
            error: Some("rate limited".to_string()),
        };

        assert_eq!(event.event_type(), "llm_call");
        assert_eq!(event.node_id(), None);
        assert_eq!(event.timestamp_ms(), 1000);

        // Verify round-trip serialization
        let json = serde_json::to_string(&event).unwrap();
        let round_trip: DebugEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.event_type(), "llm_call");
    }

    #[test]
    fn test_llm_call_in_all_variants_serialize() {
        // Ensure LLMCall is included in the full variant coverage test
        let llm_event = DebugEvent::LLMCall {
            node_id: Some("n1".to_string()),
            timestamp_ms: 2000,
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            prompt_messages: vec![
                json!({"role": "system", "content": "you are helpful"}),
                json!({"role": "user", "content": "what is 2+2?"}),
            ],
            response: json!({"choices": [{"message": {"content": "4"}}]}),
            prompt_tokens: 15,
            completion_tokens: 1,
            latency_ms: 150,
            cost_usd: Some(0.001),
            status: "ok".to_string(),
            error: None,
        };

        let json = serde_json::to_string(&llm_event).unwrap();
        let round_trip: DebugEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(llm_event.event_type(), round_trip.event_type());
        assert_eq!(llm_event.timestamp_ms(), round_trip.timestamp_ms());
        assert_eq!(llm_event.node_id(), round_trip.node_id());
    }
}
