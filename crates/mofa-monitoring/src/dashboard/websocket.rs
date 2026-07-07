//! WebSocket handler for real-time monitoring updates with optional auth.

use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, broadcast, mpsc};
use tracing::{Instrument, debug, error, info, info_span, warn};

use super::auth::{AuthInfo, AuthProvider, NoopAuthProvider};
use mofa_kernel::workflow::telemetry::DebugEvent;

use super::metrics::{MetricsCollector, MetricsSnapshot};

/// WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WebSocketMessage {
    /// Full metrics snapshot
    #[serde(rename = "metrics")]
    Metrics(MetricsSnapshot),

    /// Agent update
    #[serde(rename = "agent_update")]
    AgentUpdate {
        agent_id: String,
        state: String,
        tasks_completed: u64,
    },

    /// Workflow update
    #[serde(rename = "workflow_update")]
    WorkflowUpdate {
        workflow_id: String,
        status: String,
        progress: f64,
    },

    /// Plugin update
    #[serde(rename = "plugin_update")]
    PluginUpdate {
        plugin_id: String,
        state: String,
        call_count: u64,
    },

    /// LLM model inference update - real-time metrics for model performance
    ///
    /// Aligns with GSoC Idea 2 - Studio Observability Dashboard
    /// for per-model inference monitoring (tokens/s, TTFT, etc.)
    #[serde(rename = "llm_update")]
    LLMUpdate {
        plugin_id: String,
        provider_name: String,
        model_name: String,
        total_requests: u64,
        successful_requests: u64,
        avg_latency_ms: f64,
        tokens_per_second: Option<f64>,
        error_rate: f64,
    },

    /// Debug event from workflow execution
    ///
    /// Sent in real-time when a workflow emits telemetry events
    /// during execution. Clients can subscribe to the "debug" topic
    /// to receive these events for the visual debugger.
    #[serde(rename = "debug")]
    Debug(DebugEvent),

    /// System alert
    #[serde(rename = "alert")]
    Alert {
        level: String,
        message: String,
        source: String,
    },

    /// Heartbeat
    #[serde(rename = "heartbeat")]
    Heartbeat { timestamp: u64 },

    /// Subscribe to specific updates
    #[serde(rename = "subscribe")]
    Subscribe { topics: Vec<String> },

    /// Unsubscribe from updates
    #[serde(rename = "unsubscribe")]
    Unsubscribe { topics: Vec<String> },

    /// Error message
    #[serde(rename = "error")]
    Error { message: String },

    /// Acknowledgment
    #[serde(rename = "ack")]
    Ack { message_id: String },
}

/// WebSocket client tracking
#[derive(Debug)]
pub struct WebSocketClient {
    /// Client ID
    pub id: String,
    /// Connected timestamp
    pub connected_at: u64,
    /// Subscribed topics
    pub subscriptions: HashSet<String>,
    /// Auth info (populated when auth is enabled)
    pub auth_info: Option<AuthInfo>,
    /// Message sender
    sender: mpsc::Sender<WebSocketMessage>,
}

impl WebSocketClient {
    pub fn new(id: String, sender: mpsc::Sender<WebSocketMessage>) -> Self {
        Self {
            id,
            connected_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            subscriptions: HashSet::from(["metrics".to_string()]), // Default subscription
            auth_info: None,
            sender,
        }
    }

    pub fn with_auth_info(mut self, info: AuthInfo) -> Self {
        self.auth_info = Some(info);
        self
    }

    pub async fn send(
        &self,
        msg: WebSocketMessage,
    ) -> Result<(), mpsc::error::SendError<WebSocketMessage>> {
        self.sender.send(msg).await
    }

    pub fn is_subscribed(&self, topic: &str) -> bool {
        self.subscriptions.contains(topic)
            || self.subscriptions.contains("*")
    }
}

/// Query parameters for the WebSocket upgrade request.
#[derive(Debug, Deserialize)]
pub struct WsQueryParams {
    /// Authentication token.
    token: Option<String>,
}

/// WebSocket handler state
pub struct WebSocketHandler {
    /// Connected clients
    clients: Arc<RwLock<HashMap<String, WebSocketClient>>>,
    /// Metrics collector
    collector: Arc<MetricsCollector>,
    /// Broadcast channel for updates
    broadcast_tx: broadcast::Sender<WebSocketMessage>,
    /// Update interval
    update_interval: Duration,
    /// Authentication provider
    auth: Arc<dyn AuthProvider>,
}

impl WebSocketHandler {
    pub fn new(collector: Arc<MetricsCollector>, auth: Arc<dyn AuthProvider>) -> Self {
        let (broadcast_tx, _) = broadcast::channel(1024);

        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            collector,
            broadcast_tx,
            update_interval: Duration::from_secs(1),
            auth,
        }
    }

    pub fn with_update_interval(mut self, interval: Duration) -> Self {
        self.update_interval = interval;
        self
    }

    /// Get broadcast sender for external updates
    pub fn broadcast_tx(&self) -> broadcast::Sender<WebSocketMessage> {
        self.broadcast_tx.clone()
    }

    /// Broadcast a message to all subscribed clients
    pub async fn broadcast(&self, topic: &str, msg: WebSocketMessage) {
        let clients = self.clients.read().await;
        for client in clients.values() {
            if client.is_subscribed(topic)
                && let Err(e) = client.send(msg.clone()).await
            {
                debug!("Failed to send to client {}: {}", client.id, e);
            }
        }
    }

    /// Send alert to all clients
    pub async fn send_alert(&self, level: &str, message: &str, source: &str) {
        let alert = WebSocketMessage::Alert {
            level: level.to_string(),
            message: message.to_string(),
            source: source.to_string(),
        };
        self.broadcast("alerts", alert).await;
    }

    /// Get connected client count
    pub async fn client_count(&self) -> usize {
        self.clients.read().await.len()
    }

    /// Get client IDs
    pub async fn client_ids(&self) -> Vec<String> {
        self.clients.read().await.keys().cloned().collect()
    }

    /// Start a background task to forward debug events to connected WebSocket clients.
    ///
    /// This method spawns a task that receives `DebugEvent`s from the provided receiver
    /// and broadcasts them to all WebSocket clients subscribed to the "debug" topic.
    ///
    /// # Arguments
    /// * `rx` - The receiver for debug events to broadcast
    ///
    /// # Returns
    /// A `JoinHandle` for the spawned task
    pub fn start_debug_event_forwarder(
        self: Arc<Self>,
        mut rx: mpsc::Receiver<DebugEvent>,
    ) -> tokio::task::JoinHandle<()> {
        info!("Starting debug event forwarder");

        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let msg = WebSocketMessage::Debug(event);
                self.broadcast("debug", msg).await;
            }
            info!("Debug event forwarder stopped");
        })
    }

    /// Start background update task
    pub fn start_updates(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let interval = self.update_interval;
        info!("Starting WebSocket updates with interval {:?}", interval);

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;

                // Collect and broadcast metrics to subscribed clients only
                let snapshot = self.collector.current().await;
                let msg = WebSocketMessage::Metrics(snapshot);

                self.broadcast("metrics", msg).await;
            }
        })
    }

    /// Handle WebSocket upgrade, validating `?token=` when auth is enabled.
    pub async fn handle_upgrade(
        ws: WebSocketUpgrade,
        Query(params): Query<WsQueryParams>,
        State(handler): State<Arc<WebSocketHandler>>,
    ) -> impl IntoResponse {
        if handler.auth.is_enabled() {
            let token = params.token.unwrap_or_default();
            match handler.auth.validate(&token).await {
                Ok(auth_info) => {
                    info!("WebSocket auth succeeded for {}", auth_info.client_id);
                    ws.on_upgrade(move |socket| {
                        handler.handle_socket_with_auth(socket, Some(auth_info))
                    })
                    .into_response()
                }
                Err(reason) => {
                    warn!("WebSocket auth failed: {}", reason);
                    StatusCode::FORBIDDEN.into_response()
                }
            }
        } else {
            ws.on_upgrade(move |socket| handler.handle_socket_with_auth(socket, None))
                .into_response()
        }
    }

    /// Handle individual WebSocket connection with optional auth info.
    async fn handle_socket_with_auth(
        self: Arc<Self>,
        socket: WebSocket,
        auth_info: Option<AuthInfo>,
    ) {
        let client_id = uuid::Uuid::now_v7().to_string();
        let authenticated = auth_info.is_some();

        let span = info_span!(
            "ws_connection",
            client_id = %client_id,
            authenticated = authenticated,
        );

        self.handle_socket_inner(socket, auth_info, client_id)
            .instrument(span)
            .await;
    }

    /// Inner connection handler, runs inside the `ws_connection` span.
    async fn handle_socket_inner(
        self: Arc<Self>,
        socket: WebSocket,
        auth_info: Option<AuthInfo>,
        client_id: String,
    ) {
        info!(
            client_id = %client_id,
            "WebSocket client connected"
        );

        let (mut sender, mut receiver) = socket.split();

        // Create message channel for this client
        let (tx, mut rx) = mpsc::channel::<WebSocketMessage>(256);

        // Register client
        let mut client = WebSocketClient::new(client_id.clone(), tx);
        if let Some(info) = auth_info {
            client = client.with_auth_info(info);
        }
        {
            let mut clients = self.clients.write().await;
            clients.insert(client_id.clone(), client);
        }

        // Subscribe to broadcast channel
        let mut broadcast_rx = self.broadcast_tx.subscribe();

        // Task to send messages to client
        let mut send_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Messages from direct send
                    Some(msg) = rx.recv() => {
                        let json = serde_json::to_string(&msg).unwrap_or_default();
                        if sender.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                    // Messages from broadcast
                    result = broadcast_rx.recv() => {
                        match result {
                            Ok(msg) => {
                                let json = serde_json::to_string(&msg).unwrap_or_default();
                                if sender.send(Message::Text(json)).await.is_err() {
                                    break;
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                warn!("WebSocket broadcast lagged by {} messages", n);
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                break;
                            }
                        }
                    }
                }
            }
        });

        // Task to receive messages from client
        let clients = self.clients.clone();
        let client_id_clone = client_id.clone();
        let mut receive_task = tokio::spawn(async move {
            while let Some(result) = receiver.next().await {
                match result {
                    Ok(Message::Text(text)) => {
                        // Parse incoming message
                        if let Ok(msg) = serde_json::from_str::<WebSocketMessage>(&text) {
                            match msg {
                                WebSocketMessage::Subscribe { topics } => {
                                    let mut clients = clients.write().await;
                                    if let Some(client) = clients.get_mut(&client_id_clone) {
                                        client.subscriptions.extend(topics);
                                    }
                                }
                                WebSocketMessage::Unsubscribe { topics } => {
                                    let mut clients = clients.write().await;
                                    if let Some(client) = clients.get_mut(&client_id_clone) {
                                        for topic in &topics {
                                            client.subscriptions.remove(topic);
                                        }
                                    }
                                }
                                WebSocketMessage::Heartbeat { .. } => {
                                    // Just acknowledge heartbeats
                                    debug!("Heartbeat from client {}", client_id_clone);
                                }
                                _ => {
                                    debug!("Received message from client: {:?}", msg);
                                }
                            }
                        }
                    }
                    Ok(Message::Close(_)) => {
                        break;
                    }
                    Ok(Message::Ping(_data)) => {
                        debug!("Ping from client {}", client_id_clone);
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });

        // Wait for either task to complete, then abort the other.
        // Dropping a JoinHandle only detaches the task — it does NOT
        // cancel it, so the "losing" task would keep running as an
        // orphan (sending to a closed socket or processing messages
        // for a disconnected client).
        tokio::select! {
            _ = &mut send_task => {
                receive_task.abort();
            }
            _ = &mut receive_task => {
                send_task.abort();
            }
        }

        // Cleanup
        let subscription_count;
        {
            let mut clients = self.clients.write().await;
            subscription_count = clients
                .get(&client_id)
                .map(|c| c.subscriptions.len())
                .unwrap_or(0);
            clients.remove(&client_id);
        }
        info!(
            client_id = %client_id,
            subscriptions = subscription_count,
            "WebSocket client disconnected"
        );
    }
}

/// Create WebSocket route handler with the given auth provider.
pub fn create_websocket_handler(
    collector: Arc<MetricsCollector>,
    auth: Arc<dyn AuthProvider>,
) -> (Arc<WebSocketHandler>, axum::routing::MethodRouter) {
    let handler = Arc::new(WebSocketHandler::new(collector, auth));
    let handler_clone = handler.clone();

    let route = axum::routing::get(move |ws: WebSocketUpgrade, query: Query<WsQueryParams>| {
        let h = handler_clone.clone();
        async move { WebSocketHandler::handle_upgrade(ws, query, State(h)).await }
    });

    (handler, route)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_message_serialize() {
        let msg = WebSocketMessage::Heartbeat { timestamp: 12345 };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("heartbeat"));
        assert!(json.contains("12345"));
    }

    #[test]
    fn test_websocket_client_subscription() {
        let (tx, _) = mpsc::channel(16);
        let mut client = WebSocketClient::new("test-1".to_string(), tx);

        assert!(client.is_subscribed("metrics")); // Default subscription

        client.subscriptions.insert("alerts".to_string());
        assert!(client.is_subscribed("alerts"));
        assert!(!client.is_subscribed("other"));

        client.subscriptions.insert("*".to_string());
        assert!(client.is_subscribed("anything")); // Wildcard matches all
    }

    #[tokio::test]
    async fn test_websocket_handler_client_count() {
        let collector = Arc::new(MetricsCollector::new(Default::default()));
        let auth: Arc<dyn AuthProvider> = Arc::new(NoopAuthProvider);
        let handler = WebSocketHandler::new(collector, auth);

        assert_eq!(handler.client_count().await, 0);
    }

    /// Integration test: verifies that a WebSocket client connected to the
    /// real server receives live metric updates via the broadcast channel.
    #[tokio::test]
    async fn test_websocket_receives_updates() {
        use crate::dashboard::server::DashboardServer;
        use futures_util::StreamExt;

        let config = crate::dashboard::server::DashboardConfig::new()
            .with_host("127.0.0.1")
            .with_port(0); // OS picks a free port

        let mut server = DashboardServer::new(config);
        let router = server.build_router();
        let ws_handler = server
            .ws_handler()
            .expect("ws_handler should be set after build_router");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind");
        let addr = listener.local_addr().unwrap();

        // Serve in background
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let ws_url = format!("ws://{}/ws", addr);
        let (mut ws_stream, _response) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("failed to connect WebSocket");

        // Start broadcasting updates
        let ws_for_updates = Arc::clone(&ws_handler);
        let update_handle = tokio::spawn(async move {
            ws_for_updates.start_updates();
        });

        let received =
            tokio::time::timeout(std::time::Duration::from_secs(5), ws_stream.next()).await;

        assert!(received.is_ok(), "Timed out waiting for WebSocket message");
        let frame = received.unwrap();
        assert!(frame.is_some(), "WebSocket stream ended unexpectedly");

        let msg = frame.unwrap().expect("WebSocket error");
        let text = msg.into_text().expect("expected text frame");

        // Verify it deserializes as a Metrics message
        let parsed: WebSocketMessage =
            serde_json::from_str(&text).expect("failed to parse WebSocket message");
        assert!(
            matches!(parsed, WebSocketMessage::Metrics(_)),
            "Expected Metrics message, got: {:?}",
            parsed
        );

        update_handle.abort();
    }

    /// Integration test: verifies that a WebSocket connection is rejected
    /// with 403 when auth is enabled and no valid token is provided.
    #[tokio::test]
    async fn test_websocket_auth_reject() {
        use crate::dashboard::auth::TokenAuthProvider;
        use crate::dashboard::server::DashboardServer;

        let auth: Arc<dyn AuthProvider> = Arc::new(TokenAuthProvider::new("test-secret"));
        let config = crate::dashboard::server::DashboardConfig::new()
            .with_host("127.0.0.1")
            .with_port(0)
            .with_auth(auth);

        let mut server = DashboardServer::new(config);
        let router = server.build_router();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind");
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Attempt connection without a token — should fail
        let ws_url = format!("ws://{}/ws", addr);
        let result = tokio_tungstenite::connect_async(&ws_url).await;
        assert!(
            result.is_err(),
            "Expected WebSocket connection to be rejected without token"
        );
    }
}
