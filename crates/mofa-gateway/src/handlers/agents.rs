//! Agent lifecycle management endpoints
//!
//! POST   /agents              - create and register a new agent via a factory
//! GET    /agents              - list all registered agents
//! GET    /agents/{id}/status  - detailed status for one agent
//! POST   /agents/{id}/stop    - gracefully stop an agent
//! DELETE /agents/{id}         - remove agent from registry

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

use mofa_kernel::agent::config::AgentConfig;
use mofa_kernel::agent::types::AgentState;
use mofa_runtime::agent::registry::AgentRegistry;

use crate::error::{GatewayError, GatewayResult};
use crate::state::AppState;

// ─────────────────────────────────────────────────────────────────────────────
// DTOs
// ─────────────────────────────────────────────────────────────────────────────

/// Request body for POST /agents
#[derive(Debug, Deserialize)]
pub struct CreateAgentRequest {
    /// Agent ID - must be unique
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Optional description
    pub description: Option<String>,
    /// Factory type to use for construction (must be pre-registered)
    pub agent_type: String,
    /// Extra configuration forwarded to the factory as JSON
    #[serde(default)]
    pub config: Value,
}

/// Serialisable representation of an agent returned by GET /agents
#[derive(Debug, Serialize)]
pub struct AgentDto {
    /// Unique agent identifier.
    pub id: String,
    /// Human-readable agent name.
    pub name: String,
    /// Optional agent description.
    pub description: Option<String>,
    /// Optional agent version string.
    pub version: Option<String>,
    /// Current lifecycle state of the agent.
    pub state: String,
    /// Capability tags associated with the agent.
    pub tags: Vec<String>,
}

/// Serialisable status detail returned by GET /agents/{id}/status
#[derive(Debug, Serialize)]
pub struct AgentStatusDto {
    /// Unique agent identifier.
    pub id: String,
    /// Human-readable agent name.
    pub name: String,
    /// Optional agent description.
    pub description: Option<String>,
    /// Optional agent version string.
    pub version: Option<String>,
    /// Current lifecycle state of the agent.
    pub state: String,
    /// Capability tags associated with the agent.
    pub tags: Vec<String>,
    /// Reasoning strategies supported by the agent.
    pub reasoning_strategies: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper: extract client key for rate limiting
// ─────────────────────────────────────────────────────────────────────────────

fn client_key(headers: &HeaderMap) -> String {
    // Prefer X-Forwarded-For (set by load balancers) then fall back to a
    // sentinel value so rate-limiting still works in tests without a real IP.
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// POST /agents
///
/// Creates a new agent via the named factory and registers it.
/// The factory must have been pre-registered on the `AgentRegistry`.
pub async fn create_agent(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateAgentRequest>,
) -> Result<impl IntoResponse, GatewayError> {
    let client = client_key(&headers);
    if !state.rate_limiter.check(&client) {
        return Err(GatewayError::RateLimitExceeded(client));
    }

    if req.id.is_empty() {
        return Err(GatewayError::InvalidRequest(
            "agent id must not be empty".into(),
        ));
    }

    if state.registry.contains(&req.id).await {
        return Err(GatewayError::AgentAlreadyExists(req.id.clone()));
    }

    // Build AgentConfig from request fields
    // We use the JSON `config` field as the raw `custom` extension data.
    let raw = json!({
        "id": req.id,
        "name": req.name,
        "description": req.description,
        "type": req.agent_type,
        "custom": req.config,
    });
    let agent_config: AgentConfig = serde_json::from_value(raw)
        .map_err(|e| GatewayError::InvalidRequest(format!("invalid config: {}", e)))?;

    state
        .registry
        .create_and_register(&req.agent_type, agent_config)
        .await
        .map_err(|e| GatewayError::AgentOperationFailed(e.to_string()))?;

    tracing::info!(agent_id = %req.id, agent_type = %req.agent_type, "agent created and registered");

    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": req.id, "status": "registered" })),
    ))
}

/// GET /agents
///
/// Lists all agents currently in the registry.
pub async fn list_agents(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, GatewayError> {
    let client = client_key(&headers);
    if !state.rate_limiter.check(&client) {
        return Err(GatewayError::RateLimitExceeded(client));
    }

    let agents = state.registry.list().await;

    let dtos: Vec<AgentDto> = agents
        .iter()
        .map(|m| AgentDto {
            id: m.id.clone(),
            name: m.name.clone(),
            description: m.description.clone(),
            version: m.version.clone(),
            state: format!("{}", m.state),
            tags: m.capabilities.tags.iter().cloned().collect(),
        })
        .collect();

    Ok(Json(json!({
        "agents": dtos,
        "total": dtos.len(),
    })))
}

/// GET /agents/{id}/status
///
/// Returns detailed status for a single agent.
pub async fn agent_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, GatewayError> {
    let client = client_key(&headers);
    if !state.rate_limiter.check(&client) {
        return Err(GatewayError::RateLimitExceeded(client));
    }

    let metadata = state
        .registry
        .get_metadata(&id)
        .await
        .ok_or_else(|| GatewayError::AgentNotFound(id.clone()))?;

    let dto = AgentStatusDto {
        id: metadata.id.clone(),
        name: metadata.name.clone(),
        description: metadata.description.clone(),
        version: metadata.version.clone(),
        state: format!("{}", metadata.state),
        tags: metadata.capabilities.tags.iter().cloned().collect(),
        reasoning_strategies: metadata
            .capabilities
            .reasoning_strategies
            .iter()
            .map(|s| format!("{:?}", s))
            .collect(),
    };

    Ok(Json(dto))
}

/// POST /agents/{id}/stop
///
/// Gracefully stops (shuts down) an agent.
/// The agent remains in the registry with state `Shutdown` until deleted.
pub async fn stop_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<axum::response::Response, GatewayError> {
    let client = client_key(&headers);
    if !state.rate_limiter.check(&client) {
        return Err(GatewayError::RateLimitExceeded(client));
    }

    let agent_arc = state
        .registry
        .get(&id)
        .await
        .ok_or_else(|| GatewayError::AgentNotFound(id.clone()))?;

    {
        let mut agent = match agent_arc.try_write() {
            Ok(guard) => guard,
            Err(_) => {
                let err_msg = "Agent is currently executing a task and cannot be cleanly stopped at this moment.";
                return Ok((
                    StatusCode::CONFLICT,
                    [("retry-after", "5")],
                    Json(json!({ "id": id, "error": err_msg })),
                ).into_response());
            }
        };
        let current = agent.state();
        if current == AgentState::Shutdown || current == AgentState::ShuttingDown {
            return Ok((
                StatusCode::OK,
                Json(json!({ "id": id, "status": "already_stopped" })),
            ).into_response());
        }
        agent
            .shutdown()
            .await
            .map_err(|e| GatewayError::AgentOperationFailed(e.to_string()))?;
    }

    tracing::info!(agent_id = %id, "agent stopped");

    Ok((
        StatusCode::OK,
        Json(json!({ "id": id, "status": "stopped" })),
    ).into_response())
}

/// DELETE /agents/{id}
///
/// Stops and removes an agent from the registry.
pub async fn delete_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, GatewayError> {
    let client = client_key(&headers);
    if !state.rate_limiter.check(&client) {
        return Err(GatewayError::RateLimitExceeded(client));
    }

    if !state.registry.contains(&id).await {
        return Err(GatewayError::AgentNotFound(id.clone()));
    }

    // Attempt graceful stop first, ignore errors (agent may already be stopped)
    if let Some(agent_arc) = state.registry.get(&id).await {
        match agent_arc.try_write() {
            Ok(mut agent) => {
                let _ = agent.shutdown().await;
            }
            Err(_) => {
                tracing::warn!("skipping shutdown for agent {}, busy executing", id);
            }
        }
    }

    state
        .registry
        .unregister(&id)
        .await
        .map_err(|e| GatewayError::AgentOperationFailed(e.to_string()))?;

    tracing::info!(agent_id = %id, "agent deleted from registry");

    Ok((
        StatusCode::OK,
        Json(json!({ "id": id, "status": "deleted" })),
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

/// Build the agents management router sub-tree
pub fn agents_router() -> axum::Router<Arc<AppState>> {
    use axum::routing::{delete, get, post};
    axum::Router::new()
        .route("/agents", post(create_agent).get(list_agents))
        .route("/agents/{id}/status", get(agent_status))
        .route("/agents/{id}/stop", post(stop_agent))
        .route("/agents/{id}", delete(delete_agent))
}
