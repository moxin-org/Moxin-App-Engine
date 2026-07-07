use mofa_testing::{MockLlmServer, MockLlmServerBuilder, ToolCallSpec};
use serde_json::json;

async fn post_chat(base_url: &str, body: serde_json::Value) -> (u16, serde_json::Value) {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/chat/completions", base_url);
    let resp = client.post(url).json(&body).send().await.unwrap();
    let status = resp.status().as_u16();
    let json = resp.json::<serde_json::Value>().await.unwrap();
    (status, json)
}

async fn get_models(base_url: &str) -> (u16, serde_json::Value) {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/models", base_url);
    let resp = client.get(url).send().await.unwrap();
    let status = resp.status().as_u16();
    let json = resp.json::<serde_json::Value>().await.unwrap();
    (status, json)
}

#[tokio::test]
async fn mock_llm_server_returns_default_response() {
    let server = MockLlmServer::start().await.expect("server starts");

    let (status, body) = post_chat(
        server.base_url(),
        json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "hello"}]
        }),
    )
    .await;

    assert_eq!(status, 200);
    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap();
    assert_eq!(content, "Mock fallback response.");

    server.shutdown().await;
}

#[tokio::test]
async fn mock_llm_server_matches_rules() {
    let server = MockLlmServer::start().await.expect("server starts");
    server
        .add_response_rule("ping", "pong")
        .await;

    let (status, body) = post_chat(
        server.base_url(),
        json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "please ping"}]
        }),
    )
    .await;

    assert_eq!(status, 200);
    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap();
    assert_eq!(content, "pong");

    server.shutdown().await;
}

#[tokio::test]
async fn mock_llm_server_uses_sequences() {
    let server = MockLlmServer::start().await.expect("server starts");
    server
        .add_response_sequence("seq", vec!["one", "two"]) 
        .await;

    let body = json!({
        "model": "test-model",
        "messages": [{"role": "user", "content": "seq"}]
    });

    let (_, first) = post_chat(server.base_url(), body.clone()).await;
    let (_, second) = post_chat(server.base_url(), body.clone()).await;
    let (_, third) = post_chat(server.base_url(), body).await;

    let first_content = first["choices"][0]["message"]["content"].as_str().unwrap();
    let second_content = second["choices"][0]["message"]["content"].as_str().unwrap();
    let third_content = third["choices"][0]["message"]["content"].as_str().unwrap();

    assert_eq!(first_content, "one");
    assert_eq!(second_content, "two");
    assert_eq!(third_content, "two");

    server.shutdown().await;
}

#[tokio::test]
async fn mock_llm_server_returns_errors() {
    let server = MockLlmServer::start().await.expect("server starts");
    server
        .add_error_rule("boom", 429, "rate limit")
        .await;

    let (status, body) = post_chat(
        server.base_url(),
        json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "boom"}]
        }),
    )
    .await;

    assert_eq!(status, 429);
    assert_eq!(body["error"]["message"], "rate limit");

    server.shutdown().await;
}

#[tokio::test]
async fn mock_llm_server_tracks_history() {
    let server = MockLlmServer::start().await.expect("server starts");

    let _ = post_chat(
        server.base_url(),
        json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "first"}]
        }),
    )
    .await;
    let _ = post_chat(
        server.base_url(),
        json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "second"}]
        }),
    )
    .await;

    let history = server.history().await;
    assert_eq!(history.len(), 2);
    assert!(history[0].prompt.contains("first"));
    assert!(history[1].prompt.contains("second"));

    server.shutdown().await;
}

#[tokio::test]
async fn mock_llm_server_rejects_streaming() {
    let server = MockLlmServer::start().await.expect("server starts");

    let (status, body) = post_chat(
        server.base_url(),
        json!({
            "model": "test-model",
            "stream": true,
            "messages": [{"role": "user", "content": "hello"}]
        }),
    )
    .await;

    assert_eq!(status, 400);
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("streaming"));

    server.shutdown().await;
}

#[tokio::test]
async fn mock_llm_server_exposes_models() {
    let server = MockLlmServer::start().await.expect("server starts");

    let (status, body) = get_models(server.base_url()).await;
    assert_eq!(status, 200);
    assert_eq!(body["object"], "list");
    assert_eq!(body["data"][0]["object"], "model");

    let _ = post_chat(
        server.base_url(),
        json!({
            "model": "demo-model",
            "messages": [{"role": "user", "content": "hello"}]
        }),
    )
    .await;

    let (_, body) = get_models(server.base_url()).await;
    assert_eq!(body["data"][0]["id"], "demo-model");

    server.shutdown().await;
}

#[tokio::test]
async fn mock_llm_server_returns_tool_calls() {
    let server = MockLlmServer::start().await.expect("server starts");
    server
        .add_tool_call_rule(
            "use tool",
            "echo_tool",
            json!({ "input": "ping" }),
            Some("Calling tool"),
        )
        .await;

    let (status, body) = post_chat(
        server.base_url(),
        json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "please use tool"}]
        }),
    )
    .await;

    assert_eq!(status, 200);
    assert_eq!(body["choices"][0]["finish_reason"], "tool_calls");
    let tool_call = &body["choices"][0]["message"]["tool_calls"][0];
    assert_eq!(tool_call["type"], "function");
    assert_eq!(tool_call["function"]["name"], "echo_tool");
    assert!(tool_call["function"]["arguments"]
        .as_str()
        .unwrap()
        .contains("\"input\""));

    server.shutdown().await;
}

#[tokio::test]
async fn mock_llm_server_applies_delay() {
    let server = MockLlmServer::start().await.expect("server starts");
    server
        .add_response_rule_with_delay("slow", "done", 60)
        .await;

    let start = std::time::Instant::now();
    let _ = post_chat(
        server.base_url(),
        json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "slow please"}]
        }),
    )
    .await;
    let elapsed_ms = start.elapsed().as_millis();

    assert!(elapsed_ms >= 50, "expected delay, got {elapsed_ms}ms");

    server.shutdown().await;
}

#[tokio::test]
async fn mock_llm_server_builder_configures_rules() {
    let server = MockLlmServerBuilder::new()
        .default_response("fallback")
        .response_rule("hello", "world")
        .error_rule("deny", 403, "forbidden")
        .tool_call_sequence(
            "tool-seq",
            vec![
                ToolCallSpec {
                    name: "first".to_string(),
                    arguments: json!({ "step": 1 }),
                    id: None,
                },
                ToolCallSpec {
                    name: "second".to_string(),
                    arguments: json!({ "step": 2 }),
                    id: None,
                },
            ],
            Some("tool content"),
        )
        .start()
        .await
        .expect("server starts");

    let (status, body) = post_chat(
        server.base_url(),
        json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "hello there"}]
        }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["choices"][0]["message"]["content"],
        "world"
    );

    let (status, body) = post_chat(
        server.base_url(),
        json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "deny request"}]
        }),
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["message"], "forbidden");

    let (status, body) = post_chat(
        server.base_url(),
        json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "tool-seq"}]
        }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["choices"][0]["finish_reason"], "tool_calls");

    server.shutdown().await;
}

#[tokio::test]
async fn mock_llm_server_rejects_empty_messages() {
    let server = MockLlmServer::start().await.expect("server starts");

    let (status, body) = post_chat(
        server.base_url(),
        json!({
            "model": "test-model",
            "messages": []
        }),
    )
    .await;

    assert_eq!(status, 400);
    assert_eq!(body["error"]["message"], "messages must be non-empty");

    server.shutdown().await;
}

#[tokio::test]
async fn mock_llm_server_rejects_empty_content() {
    let server = MockLlmServer::start().await.expect("server starts");

    let (status, body) = post_chat(
        server.base_url(),
        json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": ""}]
        }),
    )
    .await;

    assert_eq!(status, 400);
    assert_eq!(body["error"]["message"], "messages must include content");

    server.shutdown().await;
}
