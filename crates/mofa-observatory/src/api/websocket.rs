// WebSocket handler for real-time trace push updates.
// Clients connect to ws://host/ws and receive JSON messages of the form:
//   {"type": "span", "span": {...}}
//
// Full implementation uses tokio::sync::broadcast to fan out new spans.
// Stub here returns a placeholder message.

use axum::{
    extract::WebSocketUpgrade,
    response::IntoResponse,
};

pub async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(|mut socket| async move {
        let msg = serde_json::json!({"type": "connected", "message": "Observatory WebSocket ready"});
        let _ = socket
            .send(axum::extract::ws::Message::Text(msg.to_string()))
            .await;
        // Keep the connection open and send pings
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            if socket
                .send(axum::extract::ws::Message::Ping(vec![]))
                .await
                .is_err()
            {
                break;
            }
        }
    })
}
