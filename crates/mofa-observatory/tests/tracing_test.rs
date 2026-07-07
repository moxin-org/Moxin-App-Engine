use mofa_observatory::tracing::{Span, SpanStatus, TraceStorage};

#[tokio::test]
async fn test_trace_ingestion_and_retrieval() {
    let storage = TraceStorage::in_memory().await.unwrap();
    let span = Span::new_root("test-span", "agent-1");
    storage.insert_span(&span).await.unwrap();

    let spans = storage.list_spans(20, 0).await.unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].name, "test-span");
    assert_eq!(spans[0].agent_id, "agent-1");
}

#[tokio::test]
async fn test_otel_span_format_compatibility() {
    let span = Span::new_root("my-op", "agent-x");
    let json = serde_json::to_value(&span).unwrap();
    assert!(json.get("span_id").is_some(), "span_id missing");
    assert!(json.get("trace_id").is_some(), "trace_id missing");
    assert!(json.get("start_time").is_some(), "start_time missing");
    assert!(json.get("name").is_some(), "name missing");
}

#[tokio::test]
async fn test_multiple_spans_in_trace() {
    let storage = TraceStorage::in_memory().await.unwrap();

    let root = Span::new_root("root-op", "agent-1");
    let trace_id = root.trace_id.clone();
    let root_id = root.span_id.clone();

    let mut child = Span::new_root("child-op", "agent-1");
    child.trace_id = trace_id.clone();
    child.parent_span_id = Some(root_id.clone());

    storage.insert_span(&root).await.unwrap();
    storage.insert_span(&child).await.unwrap();

    let trace_spans = storage.get_trace(&trace_id).await.unwrap();
    assert_eq!(trace_spans.len(), 2);

    let child_span = trace_spans.iter().find(|s| s.parent_span_id.is_some()).unwrap();
    assert_eq!(child_span.parent_span_id.as_deref(), Some(root_id.as_str()));
}

#[tokio::test]
async fn test_span_status_roundtrip() {
    let storage = TraceStorage::in_memory().await.unwrap();
    let mut span = Span::new_root("ok-span", "agent-x");
    span.status = SpanStatus::Ok;
    storage.insert_span(&span).await.unwrap();

    let retrieved = storage.get_span(&span.span_id).await.unwrap().unwrap();
    assert_eq!(retrieved.status, SpanStatus::Ok);
}
