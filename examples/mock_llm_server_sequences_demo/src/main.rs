//! Demonstrates response sequences in the mock LLM server.

use anyhow::Result;
use mofa_testing::{MockLlmServerBuilder, ToolCallSpec};
use serde_json::json;

/// Run the mock server sequences demo.
#[tokio::main]
async fn main() -> Result<()> {
    let server = MockLlmServerBuilder::new()
        .response_sequence("seq", vec!["first", "second"]) 
        .tool_call_sequence(
            "tool-seq",
            vec![
                ToolCallSpec {
                    name: "alpha".to_string(),
                    arguments: json!({ "step": 1 }),
                    id: None,
                },
                ToolCallSpec {
                    name: "beta".to_string(),
                    arguments: json!({ "step": 2 }),
                    id: None,
                },
            ],
            Some("tool sequence"),
        )
        .start()
        .await?;

    let client = reqwest::Client::new();
    let url = format!("{}/v1/chat/completions", server.base_url());

    for _ in 0..3 {
        let response = client
            .post(&url)
            .json(&json!({
                "model": "demo-model",
                "messages": [{"role": "user", "content": "seq"}]
            }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        println!(
            "Sequence response: {}",
            serde_json::to_string_pretty(&response)?
        );
    }

    for _ in 0..2 {
        let response = client
            .post(&url)
            .json(&json!({
                "model": "demo-model",
                "messages": [{"role": "user", "content": "tool-seq"}]
            }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        println!(
            "Tool sequence response: {}",
            serde_json::to_string_pretty(&response)?
        );
    }

    server.shutdown().await;
    Ok(())
}
