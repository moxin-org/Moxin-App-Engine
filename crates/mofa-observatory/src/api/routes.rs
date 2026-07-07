use crate::tracing::{
    ingest::{ingest_trace, list_traces},
    TraceStorage,
};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde_json::Value;
use std::sync::Arc;

use crate::memory::episodic::{Episode, EpisodicMemory};

#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<TraceStorage>,
    pub episodic: Arc<EpisodicMemory>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/traces", post(ingest_trace).get(list_traces))
        .route("/v1/memory/episodes", post(add_episode))
        .route("/v1/memory/episodes/:session", get(get_session_episodes))
        .route("/v1/memory/search", get(search_memory_stub))
        .with_state(state.storage.clone())
        // Layer the episodic memory state separately
        .layer(axum::Extension(state.episodic))
}

async fn health() -> &'static str {
    "ok"
}

async fn add_episode(
    axum::Extension(episodic): axum::Extension<Arc<EpisodicMemory>>,
    Json(ep): Json<Episode>,
) -> Result<StatusCode, StatusCode> {
    episodic
        .add(&ep)
        .await
        .map(|_| StatusCode::CREATED)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_session_episodes(
    axum::Extension(episodic): axum::Extension<Arc<EpisodicMemory>>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Json<Vec<Episode>>, StatusCode> {
    episodic
        .get_session(&session_id)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn search_memory_stub(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    let query = params.get("q").map(|s| s.as_str()).unwrap_or("");
    Json(serde_json::json!({
        "query": query,
        "results": [],
        "note": "Semantic search requires API key — see SemanticMemory::search()"
    }))
}
