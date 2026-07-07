use mofa_foundation::agent::components::memory::InMemoryStorage;
use mofa_kernel::agent::components::memory::{Memory, MemoryValue, Message};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("===========================================");
    println!(" MoFA Memory History & Search Demo         ");
    println!("===========================================\n");

    let mut storage = InMemoryStorage::new();
    let session_id = "example-session-001";

    println!("[1] Adding conversation history to session: {}", session_id);
    storage.add_to_history(session_id, Message::user("Can you tell me about the GSoC MoFA project?")).await?;
    storage.add_to_history(session_id, Message::assistant("MoFA has 6 great ideas for GSoC 2026. Idea 2 focuses on building a Cognitive Observatory with advanced Episodic Memory.")).await?;
    storage.add_to_history(session_id, Message::user("That sounds awesome, I will apply for Idea 2.")).await?;

    let history = storage.get_history(session_id).await?;
    println!(" -> Session contains {} messages.\n", history.len());

    println!("[2] Storing searchable documents...");
    storage.store("doc1", MemoryValue::text("The MoFA Cognitive Gateway handles request routing and telemetry.")).await?;
    storage.store("doc2", MemoryValue::text("Episodic Memory allows agents to retrieve historical context from past sessions.")).await?;
    storage.store("doc3", MemoryValue::text("The workflow engine parses custom syntax into DAGs.")).await?;

    let stats = storage.stats().await?;
    println!(" -> Current Stats: {} items, {} sessions, {} total messages\n", stats.total_items, stats.total_sessions, stats.total_messages);

    let search_term = "memory";
    println!("[3] Searching storage for keyword: '{}'", search_term);
    
    let results = storage.search(search_term, 5).await?;
    println!(" -> Found {} results.", results.len());

    for (i, result) in results.iter().enumerate() {
        println!("    [{}] Key: '{}' | Snippet: {:?}", i+1, result.key, result.value.as_text());
    }

    println!("\n===========================================");
    println!(" Demo Complete \u{2713}                        ");
    println!("===========================================");

    Ok(())
}
