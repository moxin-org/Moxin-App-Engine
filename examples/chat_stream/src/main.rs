use mofa_sdk::llm::{LLMAgentBuilder, OpenAIProvider};
use tokio_stream::StreamExt;
use tracing::{info, Level};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();
    info!("========================================");
    info!("  MoFA LLM Agent                       ");
    info!("========================================\n");

    // Load agent from agent.yml configuration (optional)
    // let agent = agent_from_config("chat_stream/agent.yml")
    //     .map_err(|e| format!("Failed to load config: {}", e).into())?;

    // Create OpenAI provider from environment variables
    let openai_provider = OpenAIProvider::from_env();
    // Build agent with OpenAI provider
    let agent = LLMAgentBuilder::new()
        .with_provider(std::sync::Arc::new(openai_provider))
        .build();
    info!("Agent loaded: {}", agent.config().name);
    info!("Agent ID: {}\n", agent.config().agent_id);

    // Demo: Interactive chat
    info!("--- Chat Demo ---\n");

    // Simple Q&A (no context retention)
    let response = agent
        .ask("Hello! What can you help me with?")
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { format!("LLM error: {e}").into() })?;
    info!("Q: Hello! What can you help me with?");
    info!("A: {response}\n");

    // Multi-turn conversation (with context retention)
    info!("--- Multi-turn Conversation ---\n");

    let r1 = agent
        .chat("My favorite programming language is Rust.")
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { format!("LLM error: {e}").into() })?;
    info!("User: My favorite programming language is Rust.");
    info!("AI: {r1}\n");

    let r2 = agent
        .chat("What's my favorite language?")
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { format!("LLM error: {e}").into() })?;
    info!("User: What's my favorite language?");
    info!("AI: {r2}\n");

    // 流式问答
    let mut stream = agent.ask_stream("Tell me a story").await?;
    while let Some(result) = stream.next().await {
        match result {
            Ok(text) => print!("{text}"),
            Err(e) => info!("Error: {e}"),
        }
    }
    // 流式多轮对话
    let mut stream = agent.chat_stream("Hello!").await?;
    while let Some(result) = stream.next().await {
        if let Ok(text) = result {
            print!("{text}");
        }
    }
    // 流式对话并获取完整响应
    let (mut stream, full_rx) = agent.chat_stream_with_full("What's 2+2?").await?;
    while let Some(result) = stream.next().await {
        if let Ok(text) = result {
            print!("{text}");
        }
    }
    let full_response = full_rx.await?;
    info!("\nFull: {full_response}");
    info!("========================================");
    info!("  Demo completed!                      ");
    info!("========================================");

    Ok(())
}
