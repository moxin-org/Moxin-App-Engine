//! Demonstrates the mock LLM server with OpenAI-compatible requests.

use anyhow::Result;
use mofa_testing::MockLlmServerBuilder;
use serde_json::json;

/// Run the mock server demo.
#[tokio::main]
async fn main() -> Result<()> {
    let server = MockLlmServerBuilder::new()
        .response_rule("ping", "pong")
        .response_rule_with_delay("slow", "done", 50)
        .tool_call_rule(
            "use tool",
            "echo_tool",
            json!({ "input": "ping" }),
            Some("Calling tool"),
        )
        .error_rule("deny", 403, "forbidden")
        .start()
        .await?;

    let client = reqwest::Client::new();
    let url = format!("{}/v1/chat/completions", server.base_url());

    let response = client
        .post(url)
        .json(&json!({
            "model": "demo-model",
            "messages": [{"role": "user", "content": "ping"}]
        }))
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;

    println!(
        "Response: {}",
        serde_json::to_string_pretty(&response)?
    );

    // /v1/models example
    let models_url = format!("{}/v1/models", server.base_url());
    let models = client
        .get(models_url)
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;
    println!("Models: {}", serde_json::to_string_pretty(&models)?);

    // Tool-call example
    let tool_call = client
        .post(format!("{}/v1/chat/completions", server.base_url()))
        .json(&json!({
            "model": "demo-model",
            "messages": [{"role": "user", "content": "please use tool"}]
        }))
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;
    println!("Tool call: {}", serde_json::to_string_pretty(&tool_call)?);

    // Delay example
    let start = std::time::Instant::now();
    let _ = client
        .post(format!("{}/v1/chat/completions", server.base_url()))
        .json(&json!({
            "model": "demo-model",
            "messages": [{"role": "user", "content": "slow please"}]
        }))
        .send()
        .await?;
    println!("Delayed response took ~{}ms", start.elapsed().as_millis());

    // Validation error example
    let invalid = client
        .post(format!("{}/v1/chat/completions", server.base_url()))
        .json(&json!({
            "model": "demo-model",
            "messages": []
        }))
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;
    println!("Validation error: {}", serde_json::to_string_pretty(&invalid)?);

    // Error rule example
    let denied = client
        .post(format!("{}/v1/chat/completions", server.base_url()))
        .json(&json!({
            "model": "demo-model",
            "messages": [{"role": "user", "content": "deny this"}]
        }))
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;
    println!("Rule error: {}", serde_json::to_string_pretty(&denied)?);

    server.shutdown().await;
    Ok(())
}
