//! Memory drift testing example
//!
//! Demonstrates:
//! - episodic cross-session retention
//! - session isolation after clearing one session
//! - semantic search and reset behavior
//!
//! Run:
//! `cargo run --manifest-path examples/Cargo.toml -p memory_drift_testing`

use mofa_testing::MemoryDriftHarness;

async fn run_episodic_flow() {
    let mut harness = MemoryDriftHarness::new();

    harness
        .record_turn("session-a", "remember I prefer Rust", "Saved your Rust preference")
        .await
        .expect("record session-a");
    harness
        .record_turn("session-b", "what do you remember?", "You prefer Rust")
        .await
        .expect("record session-b");

    let session_a = harness.history("session-a").await.expect("history session-a");
    let recent = harness
        .recent_episode_texts(4)
        .await
        .expect("recent episodes");
    let stats = harness.stats().await.expect("episodic stats");

    println!("== Episodic Memory ==");
    println!("session_ids: {:?}", harness.session_ids());
    println!("session-a messages: {}", session_a.len());
    println!("recent episodes: {:?}", recent);
    println!(
        "stats: sessions={} messages={}",
        stats.total_sessions, stats.total_messages
    );
    println!();
}

async fn run_clear_flow() {
    let mut harness = MemoryDriftHarness::new();

    harness
        .record_turn("session-a", "alpha", "ack alpha")
        .await
        .expect("record session-a");
    harness
        .record_turn("session-b", "beta", "ack beta")
        .await
        .expect("record session-b");

    harness
        .clear_session("session-a")
        .await
        .expect("clear session-a");

    println!("== Isolation After Clear ==");
    println!("session_ids: {:?}", harness.session_ids());
    println!(
        "session-a history: {:?}",
        harness.history("session-a").await.expect("history session-a")
    );
    println!(
        "session-b history len: {}",
        harness.history("session-b")
            .await
            .expect("history session-b")
            .len()
    );
    println!();
}

async fn run_semantic_flow() {
    let mut harness = MemoryDriftHarness::with_semantic_memory();

    harness
        .store_text("rust-note", "Rust is a systems programming language")
        .await
        .expect("store rust note");
    harness
        .store_text("python-note", "Python is common in machine learning")
        .await
        .expect("store python note");

    let search_results = harness
        .search_texts("systems language", 2)
        .await
        .expect("semantic search");

    println!("== Semantic Memory ==");
    println!("memory_type: {}", harness.memory_type());
    println!("search results: {:?}", search_results);

    harness.clear_all().await.expect("clear semantic memory");
    let after_clear = harness
        .search_texts("systems language", 2)
        .await
        .expect("semantic search after clear");
    println!("after clear: {:?}", after_clear);
    println!();
}

#[tokio::main]
async fn main() {
    run_episodic_flow().await;
    run_clear_flow().await;
    run_semantic_flow().await;
}
