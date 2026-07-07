//! Reflection Agent Example
//!
//! Demonstrates the Reflection agentic design pattern:
//! generate → critique → refine loop.
//!
//! # Run
//!
//! ```bash
//! export OPENAI_API_KEY=your-api-key
//! # Optional: use Ollama or other compatible service
//! export OPENAI_BASE_URL=http://localhost:11434/v1
//!
//! cd examples/reflection_agent && cargo run
//! # Or from repo root:
//! cargo run --manifest-path examples/reflection_agent/Cargo.toml
//! ```

use mofa_sdk::llm::{LLMAgentBuilder, OpenAIConfig, OpenAIProvider};
use mofa_sdk::react::{ReflectionAgent, ReflectionConfig};
use std::sync::Arc;
use tracing::info;

fn create_llm_agent() -> Result<mofa_sdk::llm::LLMAgent, Box<dyn std::error::Error>> {
    let api_key = std::env::var("OPENAI_API_KEY")
        .unwrap_or_else(|_| "demo-key".to_owned());
    let base_url = std::env::var("OPENAI_BASE_URL").ok();
    let model = std::env::var("OPENAI_MODEL")
        .unwrap_or_else(|_| "gpt-4".to_owned());

    let mut config = OpenAIConfig::new(api_key).with_model(&model);
    if let Some(url) = base_url {
        config = config.with_base_url(&url);
    }

    let provider = OpenAIProvider::with_config(config);
    let agent = LLMAgentBuilder::new()
        .with_name("Reflection Demo Agent")
        .with_provider(Arc::new(provider))
        .with_system_prompt("You are a helpful assistant that thinks carefully and improves responses.")
        .with_temperature(0.7)
        .with_max_tokens(2048)
        .build();

    Ok(agent)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("==========================================");
    info!("  MoFA Reflection Agent Example");
    info!("==========================================");

    let llm_agent = Arc::new(create_llm_agent()?);

    let agent = ReflectionAgent::builder()
        .with_generator(llm_agent.clone())
        .with_config(ReflectionConfig::default().with_max_rounds(3))
        .with_verbose(true)
        .build()?;

    let task = "Explain the concept of ownership in Rust and why it matters.";
    info!("Task: {}\n", task);

    let result = agent.run(task).await?;

    info!("\n--- Reflection Result ---");
    info!("Rounds: {}", result.rounds);
    info!("Duration: {}ms", result.duration_ms);
    info!("Success: {}", result.success);

    for step in &result.steps {
        info!("\n[Round {}]", step.round + 1);
        let draft_preview = if step.draft.len() <= 100 {
            step.draft.as_str()
        } else {
            &step.draft[..100]
        };
        let critique_preview = if step.critique.len() <= 100 {
            step.critique.as_str()
        } else {
            &step.critique[..100]
        };
        info!("Draft: {}{}", draft_preview, if step.draft.len() > 100 { "..." } else { "" });
        info!("Critique: {}{}", critique_preview, if step.critique.len() > 100 { "..." } else { "" });
    }

    info!("\nFinal Answer:\n{}", result.final_answer);

    Ok(())
}