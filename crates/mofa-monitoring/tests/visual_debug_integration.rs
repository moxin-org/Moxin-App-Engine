//! Visual Debugger Integration Test
//!
//! Tests the full pipeline: Executor -> Telemetry -> Recorder -> API

use std::sync::Arc;
use tokio::sync::mpsc;

use mofa_foundation::workflow::session_recorder::InMemorySessionRecorder;
use mofa_kernel::workflow::telemetry::{DebugEvent, DebugSession, SessionRecorder};
use mofa_monitoring::{DashboardConfig, DashboardServer};

/// Creates a simple 3-node workflow for testing
fn create_test_workflow_events(execution_id: &str) -> Vec<DebugEvent> {
    let base_time = 1700000000000u64;

    vec![
        DebugEvent::WorkflowStart {
            workflow_id: "test-workflow".to_string(),
            execution_id: execution_id.to_string(),
            timestamp_ms: base_time,
        },
        DebugEvent::NodeStart {
            node_id: "start".to_string(),
            timestamp_ms: base_time + 100,
            state_snapshot: serde_json::json!({"count": 0}),
        },
        DebugEvent::StateChange {
            node_id: "start".to_string(),
            timestamp_ms: base_time + 150,
            key: "count".to_string(),
            old_value: Some(serde_json::json!(0)),
            new_value: serde_json::json!(1),
        },
        DebugEvent::NodeEnd {
            node_id: "start".to_string(),
            timestamp_ms: base_time + 200,
            state_snapshot: serde_json::json!({"count": 1}),
            duration_ms: 100,
        },
        DebugEvent::NodeStart {
            node_id: "process".to_string(),
            timestamp_ms: base_time + 300,
            state_snapshot: serde_json::json!({"count": 1, "processed": false}),
        },
        DebugEvent::NodeEnd {
            node_id: "process".to_string(),
            timestamp_ms: base_time + 500,
            state_snapshot: serde_json::json!({"count": 1, "processed": true}),
            duration_ms: 200,
        },
        DebugEvent::NodeStart {
            node_id: "end".to_string(),
            timestamp_ms: base_time + 600,
            state_snapshot: serde_json::json!({"count": 1, "processed": true, "done": true}),
        },
        DebugEvent::NodeEnd {
            node_id: "end".to_string(),
            timestamp_ms: base_time + 700,
            state_snapshot: serde_json::json!({"count": 1, "processed": true, "done": true, "complete": true}),
            duration_ms: 100,
        },
        DebugEvent::WorkflowEnd {
            workflow_id: "test-workflow".to_string(),
            execution_id: execution_id.to_string(),
            timestamp_ms: base_time + 800,
            status: "completed".to_string(),
        },
    ]
}

#[tokio::test]
async fn test_debug_session_api_integration() {
    // Step 1: Create a session recorder and seed it with test data
    let recorder = Arc::new(InMemorySessionRecorder::new());
    let session_id = "test-session-1";
    let execution_id = "exec-1";

    // Create and start a session
    let session = DebugSession::new(session_id, "test-workflow", execution_id);
    recorder.start_session(&session).await.unwrap();

    // Record test events (simulating a 3-node workflow execution)
    let events = create_test_workflow_events(execution_id);
    for event in &events {
        recorder.record_event(session_id, event).await.unwrap();
    }

    // End the session
    recorder.end_session(session_id, "completed").await.unwrap();

    // Step 2: Create dashboard server with the recorder
    let config = DashboardConfig::new()
        .with_port(18080)
        .with_cors(true)
        .with_require_auth(false);

    let server = DashboardServer::new(config).with_session_recorder(recorder.clone());

    // Verify the session is recorded
    let sessions = recorder.list_sessions().await.unwrap();
    assert_eq!(sessions.len(), 1, "Should have 1 session");
    assert_eq!(sessions[0].session_id, session_id);
    assert_eq!(sessions[0].event_count, 9, "Should have 9 events");

    // Verify events can be retrieved
    let retrieved_events = recorder.get_events(session_id).await.unwrap();
    assert_eq!(retrieved_events.len(), 9, "Should retrieve all 9 events");

    // Verify events are in timestamp order
    let mut prev_timestamp = 0u64;
    for event in &retrieved_events {
        assert!(
            event.timestamp_ms() >= prev_timestamp,
            "Events should be in order"
        );
        prev_timestamp = event.timestamp_ms();
    }

    // Verify session metadata
    let retrieved_session = recorder.get_session(session_id).await.unwrap();
    assert!(retrieved_session.is_some());
    let session = retrieved_session.unwrap();
    assert_eq!(session.status, "completed");
    assert!(session.ended_at.is_some());
}

#[tokio::test]
async fn test_debug_session_not_found() {
    // Create a fresh recorder
    let recorder = Arc::new(InMemorySessionRecorder::new());

    // Try to get a non-existent session
    let result = recorder.get_session("non-existent").await.unwrap();
    assert!(result.is_none(), "Non-existent session should return None");

    // Try to get events for non-existent session
    let events = recorder.get_events("non-existent").await.unwrap();
    assert!(
        events.is_empty(),
        "Non-existent session should return empty events"
    );
}

#[tokio::test]
async fn test_multiple_sessions() {
    let recorder = Arc::new(InMemorySessionRecorder::new());

    // Create multiple sessions
    for i in 0..3 {
        let session_id = format!("session-{}", i);
        let execution_id = format!("exec-{}", i);

        let session = DebugSession::new(&session_id, "test-workflow", &execution_id);
        recorder.start_session(&session).await.unwrap();

        // Add some events
        let event = DebugEvent::WorkflowStart {
            workflow_id: "test-workflow".to_string(),
            execution_id: execution_id.clone(),
            timestamp_ms: 1700000000000u64 + (i as u64 * 1000),
        };
        recorder.record_event(&session_id, &event).await.unwrap();

        recorder
            .end_session(&session_id, "completed")
            .await
            .unwrap();
    }

    // Verify all sessions are recorded
    let sessions = recorder.list_sessions().await.unwrap();
    assert_eq!(sessions.len(), 3, "Should have 3 sessions");

    // Verify each session has events
    for i in 0..3 {
        let session_id = format!("session-{}", i);
        let events = recorder.get_events(&session_id).await.unwrap();
        assert_eq!(events.len(), 1, "Session {} should have 1 event", i);
    }
}
