use anyhow::Result;
use mofa_testing::{AgentTestRunner, MockTool};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    let mut runner = AgentTestRunner::new().await?;

    let tool = MockTool::new(
        "echo_tool",
        "Echo the provided input",
        json!({
            "type": "object",
            "properties": {
                "input": { "type": "string" }
            },
            "required": ["input"]
        }),
    );

    runner.register_mock_tool(tool).await?;

    runner
        .mock_llm()
        .add_tool_call_response("echo_tool", json!({ "input": "ping" }), None)
        .await;
    runner
        .mock_llm()
        .add_response("Tool response completed")
        .await;

    let result = runner.run_text("use the tool").await?;
    println!("Output: {}", result.output_text().unwrap_or_default());

    for record in &result.metadata.tool_calls {
        println!(
            "Tool call: name={} input={} output={} duration_ms={:?}",
            record.tool_name,
            record.input,
            record.output.as_ref().unwrap_or(&serde_json::Value::Null),
            record.duration_ms
        );
    }

    runner.shutdown().await?;
    Ok(())
}
