//! Context Compression Example
//!
//! Demonstrates how MoFA prevents agents from hitting LLM token limits during
//! long conversations using the built-in context compression module.
//!
//! Two strategies are shown:
//!
//! - SlidingWindowCompressor: discards old messages, keeps the N most recent.
//!   Zero latency, no external calls required.
//!
//! - SummarizingCompressor: uses the LLM to condense old turns into a summary.
//!   Requires OPENAI_API_KEY (or a compatible endpoint via OPENAI_BASE_URL).
//!
//! # Running
//!
//! Sliding window demo (no credentials needed):
//! ```bash
//! cargo run -p context_compression
//! ```
//!
//! Full demo including summarization:
//! ```bash
//! export OPENAI_API_KEY=your-key
//! cargo run -p context_compression -- summarize
//! ```

use mofa_foundation::agent::{
    AgentExecutorConfig, ContextCompressor, SlidingWindowCompressor, SummarizingCompressor,
    TokenCounter,
};
use mofa_kernel::agent::types::ChatMessage;
use std::sync::Arc;
use tracing::info;

// ============================================================================
// Helpers
// ============================================================================

fn make_msg(role: &str, content: &str) -> ChatMessage {
    ChatMessage {
        role: role.to_string(),
        content: Some(content.to_string()),
        tool_call_id: None,
        tool_calls: None,
    }
}

/// Build a simulated long conversation with a system prompt and n back-and-forth turns.
fn build_long_conversation(turns: usize) -> Vec<ChatMessage> {
    let mut msgs = vec![make_msg(
        "system",
        "You are a helpful assistant specialising in Rust and distributed systems.",
    )];

    let topics = [
        ("What is ownership in Rust?", "Ownership is Rust's central feature for memory management without a garbage collector. Each value has a single owner, and when the owner goes out of scope the value is dropped."),
        ("Explain borrowing.", "Borrowing lets you reference a value without taking ownership. You can have many immutable borrows or exactly one mutable borrow at a time."),
        ("What are lifetimes?", "Lifetimes annotate how long references are valid. The compiler uses them to ensure references never outlive the data they point to."),
        ("How does async work in Rust?", "Rust async uses a poll-based model. Futures are state machines that yield control when waiting for I/O and are driven by an executor like Tokio."),
        ("What is Arc?", "Arc is an atomically reference-counted smart pointer for sharing ownership across threads. When the last Arc drops, the inner value is freed."),
    ];

    for i in 0..turns {
        let (q, a) = &topics[i % topics.len()];
        msgs.push(make_msg("user", q));
        msgs.push(make_msg("assistant", a));
    }

    msgs
}

// ============================================================================
// Demo 1: Token counting
// ============================================================================

fn demo_token_counter() {
    println!("\n========================================");
    println!("  Demo 1: Token Counter");
    println!("========================================\n");

    let conversation = build_long_conversation(10);

    let total = TokenCounter::count(&conversation);
    println!("Conversation length:  {} messages", conversation.len());
    println!("Estimated token count: {total}");

    let single = TokenCounter::count_str(
        "The quick brown fox jumps over the lazy dog.",
    );
    println!(
        "\n'The quick brown fox...' → approximately {single} tokens (chars/4 heuristic)"
    );

    println!("\nWhen total exceeds the configured max_context_tokens, the executor");
    println!("triggers compression automatically before calling the LLM.");
}

// ============================================================================
// Demo 2: Sliding window compressor
// ============================================================================

async fn demo_sliding_window() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n========================================");
    println!("  Demo 2: Sliding Window Compressor");
    println!("========================================\n");

    // Build a long conversation: 1 system + 30 conversation messages (15 turns).
    let messages = build_long_conversation(15);
    println!("Original conversation: {} messages", messages.len());

    let compressor = SlidingWindowCompressor::new(6); // keep 6 most-recent non-system messages
    let tokens_before = compressor.count_tokens(&messages);
    println!("Estimated tokens before compression: {tokens_before}");

    // Trigger compression with a tight budget.
    let budget = tokens_before / 3;
    let compressed = compressor.compress(messages, budget).await?;

    println!("\nAfter compression (window_size=6, budget={budget} tokens):");
    println!("Messages kept: {}", compressed.len());
    for msg in &compressed {
        let preview = msg
            .content
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(60)
            .collect::<String>();
        println!("  [{:9}] {}...", msg.role, preview);
    }

    let tokens_after = compressor.count_tokens(&compressed);
    println!("\nEstimated tokens after: {tokens_after}");
    println!(
        "Token reduction: {:.0}%",
        100.0 * (1.0 - tokens_after as f64 / tokens_before as f64)
    );

    Ok(())
}

// ============================================================================
// Demo 3: AgentExecutor with compressor attached
// ============================================================================

async fn demo_agent_executor() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n========================================");
    println!("  Demo 3: AgentExecutor Integration");
    println!("========================================\n");

    // Normally you would use a real provider here.
    // This demo shows the API surface without making actual LLM calls.
    println!("Creating AgentExecutor with a SlidingWindowCompressor attached...\n");
    println!(
        r#"  let compressor = Arc::new(SlidingWindowCompressor::new(10));

  let executor = AgentExecutor::with_config(
      llm_provider,
      workspace_path,
      AgentExecutorConfig::new()
          .with_max_context_tokens(3000)
          .with_model("gpt-4o-mini"),
  )
  .await?
  .with_compressor(compressor);

  // Each call to process_message now automatically compresses context
  // when estimated token count exceeds 3000.
  let reply = executor.process_message("session-1", "Hello!").await?;"#
    );

    println!("\nThe compressor runs transparently inside process_message:");
    println!("  1. Build messages (system prompt + history + current message)");
    println!("  2. Count tokens using ContextCompressor::count_tokens()");
    println!("  3. If count > max_context_tokens → call compress()");
    println!("  4. Pass compressed messages to LLM");

    // Demonstrate the config builder API directly.
    let config = AgentExecutorConfig::new()
        .with_max_context_tokens(3_000)
        .with_max_iterations(10);

    println!("\nConfig: max_context_tokens={}", config.max_context_tokens);
    println!("Config: max_iterations={}", config.max_iterations);

    Ok(())
}

// ============================================================================
// Demo 4: Summarizing compressor (requires OPENAI_API_KEY)
// ============================================================================

async fn demo_summarizing_compressor() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n========================================");
    println!("  Demo 4: Summarizing Compressor");
    println!("========================================\n");

    let api_key = match std::env::var("OPENAI_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            println!("OPENAI_API_KEY not set. Skipping live summarization demo.");
            println!(
                "\nTo try it:\n  export OPENAI_API_KEY=your-key\n  cargo run -p context_compression -- summarize"
            );
            return Ok(());
        }
    };

    use mofa_foundation::llm::openai::{OpenAIConfig, OpenAIProvider};

    let base_url = std::env::var("OPENAI_BASE_URL").ok();
    let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());

    let mut cfg = OpenAIConfig::new(api_key).with_model(&model);
    if let Some(url) = base_url {
        cfg = cfg.with_base_url(&url);
    }
    let provider = Arc::new(OpenAIProvider::with_config(cfg));

    let compressor = SummarizingCompressor::new(provider).with_keep_recent(4);

    let messages = build_long_conversation(8); // 1 system + 16 conversation messages
    let tokens_before = compressor.count_tokens(&messages);

    println!("Original conversation: {} messages ({tokens_before} est. tokens)", messages.len());

    let budget = tokens_before / 2;
    println!("Compressing to budget: {budget} tokens (keep_recent=4)...\n");

    let compressed = compressor.compress(messages, budget).await?;

    println!("After compression: {} messages", compressed.len());
    for msg in &compressed {
        let preview = msg
            .content
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(80)
            .collect::<String>();
        println!("  [{:9}] {}", msg.role, preview);
    }

    let tokens_after = compressor.count_tokens(&compressed);
    println!(
        "\nTokens: {tokens_before} → {tokens_after} ({:.0}% reduction)",
        100.0 * (1.0 - tokens_after as f64 / tokens_before as f64)
    );

    Ok(())
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    info!("Starting context compression demos");

    let mode = std::env::args().nth(1).unwrap_or_default();

    println!("==========================================");
    println!("  MoFA Context Compression Demo");
    println!("==========================================");
    println!("\nAgents running long conversations eventually exceed LLM token");
    println!("limits. MoFA's context compression module handles this automatically.");

    demo_token_counter();
    demo_sliding_window().await?;
    demo_agent_executor().await?;

    if mode == "summarize" {
        demo_summarizing_compressor().await?;
    } else {
        println!("\n========================================");
        println!("  Demo 4: Summarizing Compressor");
        println!("========================================");
        println!("\nRun with 'summarize' argument and OPENAI_API_KEY set to try the");
        println!("LLM-based summarization strategy:");
        println!("  cargo run -p context_compression -- summarize");
    }

    println!("\n==========================================");
    println!("  Demo complete!");
    println!("==========================================");

    Ok(())
}
