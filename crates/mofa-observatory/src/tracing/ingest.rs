use super::{Span, TraceStorage};
use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use std::sync::Arc;

/// POST /v1/traces — ingest a batch of spans.
pub async fn ingest_trace(
    State(storage): State<Arc<TraceStorage>>,
    Json(spans): Json<Vec<Span>>,
) -> Result<StatusCode, StatusCode> {
    for span in &spans {
        storage
            .insert_span(span)
            .await
            .map_err(|e| {
                tracing::error!("Failed to insert span {}: {e}", span.span_id);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }
    Ok(StatusCode::ACCEPTED)
}

/// GET /v1/traces — list spans with optional limit/offset.
pub async fn list_traces(
    State(storage): State<Arc<TraceStorage>>,
    axum::extract::Query(params): axum::extract::Query<ListParams>,
) -> Result<Json<Vec<Span>>, StatusCode> {
    let limit = params.limit.unwrap_or(100).min(500);
    let offset = params.offset.unwrap_or(0);
    storage
        .list_spans(limit, offset)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(serde::Deserialize)]
pub struct ListParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
