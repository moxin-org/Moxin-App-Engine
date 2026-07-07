use chrono::Utc;
use mofa_observatory::memory::episodic::{Episode, EpisodicMemory};
use mofa_observatory::memory::semantic::SemanticMemory;
use uuid::Uuid;

#[tokio::test]
async fn test_episodic_memory_roundtrip() {
    let mem = EpisodicMemory::in_memory().await.unwrap();
    let ep = Episode {
        id: Uuid::new_v4(),
        session_id: "sess-1".to_string(),
        timestamp: Utc::now(),
        role: "user".to_string(),
        content: "What is Rust?".to_string(),
        metadata: Default::default(),
    };
    mem.add(&ep).await.unwrap();

    let episodes = mem.get_session("sess-1").await.unwrap();
    assert_eq!(episodes.len(), 1);
    assert_eq!(episodes[0].content, "What is Rust?");
    assert_eq!(episodes[0].role, "user");
}

#[tokio::test]
async fn test_episodic_memory_multiple_sessions() {
    let mem = EpisodicMemory::in_memory().await.unwrap();

    for i in 0..5 {
        mem.add(&Episode {
            id: Uuid::new_v4(),
            session_id: "sess-a".to_string(),
            timestamp: Utc::now(),
            role: "user".to_string(),
            content: format!("message {i}"),
            metadata: Default::default(),
        })
        .await
        .unwrap();
    }

    mem.add(&Episode {
        id: Uuid::new_v4(),
        session_id: "sess-b".to_string(),
        timestamp: Utc::now(),
        role: "assistant".to_string(),
        content: "different session".to_string(),
        metadata: Default::default(),
    })
    .await
    .unwrap();

    let sess_a = mem.get_session("sess-a").await.unwrap();
    let sess_b = mem.get_session("sess-b").await.unwrap();
    assert_eq!(sess_a.len(), 5);
    assert_eq!(sess_b.len(), 1);
}

#[tokio::test]
async fn test_semantic_search_latency_under_100ms() {
    // Uses stub search (no API call) — inserts pre-computed embeddings
    let mem = SemanticMemory::new("http://localhost:9999", "test-key");
    const DIM: usize = 64;
    const N: usize = 1000;

    // Bulk-insert N facts then build the index once (O(n log n), not O(n²))
    for i in 0..N {
        let embedding: Vec<f32> = (0..DIM)
            .map(|j| ((i * DIM + j) as f32).sin())
            .collect();
        mem.insert_with_embedding(format!("fact {i}"), 0.5, embedding);
    }
    mem.finalize_index();

    let query_emb: Vec<f32> = (0..DIM).map(|j| (j as f32 / DIM as f32).sin()).collect();

    let start = std::time::Instant::now();
    let results = mem.search_with_embedding(query_emb, 5);
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 100,
        "search took {}ms — should be under 100ms",
        elapsed.as_millis()
    );
    assert!(!results.is_empty(), "search should return results");
}

#[tokio::test]
async fn test_episodic_memory_recent() {
    let mem = EpisodicMemory::in_memory().await.unwrap();

    for i in 0..10 {
        mem.add(&Episode {
            id: Uuid::new_v4(),
            session_id: format!("sess-{i}"),
            timestamp: Utc::now(),
            role: "user".to_string(),
            content: format!("episode {i}"),
            metadata: Default::default(),
        })
        .await
        .unwrap();
    }

    let recent = mem.recent(5).await.unwrap();
    assert_eq!(recent.len(), 5);
}
