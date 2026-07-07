//! WebSocket streaming endpoint for OpenAI-compatible chat completions.
//!
//! Provides a `/ws/v1/chat/completions` endpoint that accepts a JSON request
//! and streams response chunks as WebSocket text messages. This is an
//! alternative to SSE for clients that need bidirectional communication
//! (e.g., mid-stream cancellation via client close frame).
//!
//! # Protocol
//!
//! 1. Client connects via WebSocket upgrade.
//! 2. Client sends one JSON text message: `ChatCompletionRequest`.
//! 3. Server sends one chunk per WebSocket text message (same JSON format as SSE `data:`).
//! 4. Server sends `"[DONE]"` as the final message.
//! 5. Either side can close the connection to cancel the stream.
//!    When the client closes, the next `socket.send()` call fails and the
//!    server drops the [`BoxTokenStream`], stopping the producer immediately.
//!
//! # Example (JavaScript)
//!
//! ```js
//! const ws = new WebSocket("ws://localhost:8080/ws/v1/chat/completions");
//! ws.onopen = () => {
//!   ws.send(JSON.stringify({
//!     model: "mofa-local",
//!     messages: [{role: "user", content: "Hello!"}],
//!     stream: true,
//!   }));
//! };
//! ws.onmessage = ({data}) => {
//!   if (data === "[DONE]") { ws.close(); return; }
//!   const chunk = JSON.parse(data);
//!   console.log(chunk.choices[0].delta.content);
//! };
//! ```

use axum::extract::{State, WebSocketUpgrade, ws::Message};
use axum::response::Response;
use futures::StreamExt;
use mofa_kernel::llm::streaming::{StreamChunk, StreamError};
use mofa_kernel::llm::types::FinishReason;
use tracing::{debug, error, info, warn};

use crate::openai_compat::handler::AppState;
use crate::openai_compat::types::{ChatCompletionChunk, ChatCompletionRequest, ChunkChoice, Delta};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers (mirror openai_compat/handler.rs helpers)
// ─────────────────────────────────────────────────────────────────────────────

fn completion_id() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("chatcmpl-mofa{ts}")
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn finish_reason_str(fr: &FinishReason) -> &'static str {
    match fr {
        FinishReason::Stop => "stop",
        FinishReason::Length => "length",
        FinishReason::ToolCalls => "tool_calls",
        FinishReason::ContentFilter => "content_filter",
        _ => "stop",
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WebSocket upgrade handler
// ─────────────────────────────────────────────────────────────────────────────

/// Handler for `GET /ws/v1/chat/completions`.
///
/// Upgrades the connection to WebSocket and streams chat completion chunks
/// as JSON text messages.
pub async fn ws_chat_completions(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(|socket| handle_ws_session(socket, state))
}

/// Main WebSocket session handler.
///
/// Waits for the client's initial JSON request message, then drives the
/// inference stream, forwarding chunks until the stream ends or the client
/// disconnects.
async fn handle_ws_session(mut socket: axum::extract::ws::WebSocket, state: AppState) {
    // ── 1. Receive the chat completion request ────────────────────────────
    let req: ChatCompletionRequest = match socket.recv().await {
        Some(Ok(Message::Text(text))) => match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "WebSocket: invalid request JSON");
                let err = serde_json::json!({
                    "error": {"message": format!("Invalid request: {e}"), "type": "invalid_request_error"}
                });
                let _ = socket.send(Message::Text(err.to_string())).await;
                return;
            }
        },
        Some(Ok(Message::Close(_))) => {
            debug!("WebSocket: client closed before sending request");
            return;
        }
        Some(Ok(_)) => {
            warn!("WebSocket: expected Text message, got non-text frame");
            return;
        }
        Some(Err(e)) => {
            error!(error = %e, "WebSocket: receive error");
            return;
        }
        None => {
            debug!("WebSocket: connection closed immediately");
            return;
        }
    };

    info!(model = %req.model, "WebSocket streaming request received");

    // ── 2. Validate ───────────────────────────────────────────────────────
    if req.messages.is_empty() {
        let err = serde_json::json!({
            "error": {"message": "messages must not be empty", "type": "invalid_request_error"}
        });
        let _ = socket.send(Message::Text(err.to_string())).await;
        return;
    }

    // ── 3. Run the orchestrator ───────────────────────────────────────────
    let inference_req = req.to_inference_request(7168);

    let (_result, token_stream) = {
        let mut orch = state.orchestrator.write().await;
        orch.infer_stream(&inference_req)
    };

    let id = completion_id();
    let model = req.model.clone();
    let created = unix_now();

    // ── 4. Stream chunks to WebSocket ────────────────────────────────────
    // Cancellation is handled implicitly: stream_chunks_to_ws returns early
    // whenever socket.send() fails (i.e. the client closed the connection).
    // The BoxTokenStream is dropped at that point, stopping the producer.
    stream_chunks_to_ws(&mut socket, token_stream, &id, &model, created).await;

    let _ = socket.close().await;
}

/// Drive the token stream and send each chunk as a WebSocket text message.
///
/// Consumes a [`BoxTokenStream`] (emitting [`StreamChunk`] items) and
/// translates them into OpenAI-compatible JSON WebSocket text frames.
async fn stream_chunks_to_ws(
    socket: &mut axum::extract::ws::WebSocket,
    mut token_stream: mofa_kernel::llm::streaming::BoxTokenStream,
    id: &str,
    model: &str,
    created: u64,
) {
    // Role chunk
    let role_chunk = ChatCompletionChunk {
        id: id.to_string(),
        object: "chat.completion.chunk".to_string(),
        created,
        model: model.to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: Delta {
                role: Some("assistant".to_string()),
                content: None,
            },
            finish_reason: None,
        }],
    };

    if let Ok(json) = serde_json::to_string(&role_chunk)
        && socket.send(Message::Text(json)).await.is_err()
    {
        return; // client disconnected
    }

    // Content / control chunks
    while let Some(item) = token_stream.next().await {
        match item {
            Err(StreamError::Provider { message, .. }) => {
                error!(error = %message, "WebSocket: upstream stream error");
                let err = serde_json::json!({
                    "error": {"message": message, "type": "stream_error"}
                });
                let _ = socket.send(Message::Text(err.to_string())).await;
                break;
            }
            Err(e) => {
                error!(error = %e, "WebSocket: stream error");
                break;
            }
            Ok(sc) if sc.is_done() => {
                // Emit the stop chunk with finish_reason then break
                let finish = sc
                    .finish_reason
                    .as_ref()
                    .map(finish_reason_str)
                    .unwrap_or("stop");

                let stop_chunk = ChatCompletionChunk {
                    id: id.to_string(),
                    object: "chat.completion.chunk".to_string(),
                    created,
                    model: model.to_string(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: Delta::default(),
                        finish_reason: Some(finish.to_string()),
                    }],
                };

                if let Ok(json) = serde_json::to_string(&stop_chunk) {
                    let _ = socket.send(Message::Text(json)).await;
                }
                break;
            }
            Ok(sc) => {
                // Regular content chunk
                let chunk = ChatCompletionChunk {
                    id: id.to_string(),
                    object: "chat.completion.chunk".to_string(),
                    created,
                    model: model.to_string(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: Delta {
                            role: None,
                            content: Some(sc.delta),
                        },
                        finish_reason: None,
                    }],
                };

                match serde_json::to_string(&chunk) {
                    Ok(json) => {
                        if socket.send(Message::Text(json)).await.is_err() {
                            debug!("WebSocket: client disconnected during stream");
                            return;
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "WebSocket: chunk serialization failed");
                    }
                }
            }
        }
    }

    // [DONE] terminator
    let _ = socket.send(Message::Text("[DONE]".to_string())).await;
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_finish_reason_str() {
        assert_eq!(finish_reason_str(&FinishReason::Stop), "stop");
        assert_eq!(finish_reason_str(&FinishReason::Length), "length");
        assert_eq!(finish_reason_str(&FinishReason::ToolCalls), "tool_calls");
        assert_eq!(
            finish_reason_str(&FinishReason::ContentFilter),
            "content_filter"
        );
    }

    #[test]
    fn test_completion_id_format() {
        let id = completion_id();
        assert!(
            id.starts_with("chatcmpl-mofa"),
            "ID should start with chatcmpl-mofa"
        );
    }
}
