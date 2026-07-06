use mofa_testing::MemoryDriftHarness;

#[tokio::test]
async fn memory_persists_across_sessions() {
    let mut harness = MemoryDriftHarness::new();

    // Session B should still be able to observe content that originated in session A
    // through the shared episodic memory timeline.
    harness
        .record_turn("session-a", "remember I prefer Rust", "Noted: Rust preference saved")
        .await
        .unwrap();
    harness
        .record_turn("session-b", "what do you remember?", "You prefer Rust")
        .await
        .unwrap();

    let session_a = harness.history("session-a").await.unwrap();
    let session_b = harness.history("session-b").await.unwrap();

    assert_eq!(session_a.len(), 2);
    assert_eq!(session_b.len(), 2);

    let recent = harness.recent_episode_texts(4).await.unwrap();
    assert!(recent.iter().any(|text| text.contains("prefer Rust")));
    assert!(recent.iter().any(|text| text.contains("You prefer Rust")));
}

#[tokio::test]
async fn clearing_one_session_does_not_affect_others() {
    let mut harness = MemoryDriftHarness::new();

    harness
        .record_turn("session-a", "alpha", "ack alpha")
        .await
        .unwrap();
    harness
        .record_turn("session-b", "beta", "ack beta")
        .await
        .unwrap();

    harness.clear_session("session-a").await.unwrap();

    let session_a = harness.history("session-a").await.unwrap();
    let session_b = harness.history("session-b").await.unwrap();

    assert!(session_a.is_empty());
    assert_eq!(session_b.len(), 2);
    assert_eq!(
        harness.session_ids(),
        vec!["session-b".to_string()]
    );
}

#[tokio::test]
async fn recent_episodes_include_multiple_sessions() {
    let mut harness = MemoryDriftHarness::new();

    harness
        .record_message("session-a", mofa_foundation::agent::components::Message::user("one"))
        .await
        .unwrap();
    harness
        .record_message("session-b", mofa_foundation::agent::components::Message::user("two"))
        .await
        .unwrap();
    harness
        .record_message(
            "session-c",
            mofa_foundation::agent::components::Message::assistant("three"),
        )
        .await
        .unwrap();

    let recent = harness.recent_episode_texts(3).await.unwrap();
    assert_eq!(recent, vec!["one", "two", "three"]);
}

#[tokio::test]
async fn clear_all_removes_all_memory() {
    let mut harness = MemoryDriftHarness::new();

    harness
        .record_turn("session-a", "persist this", "saved")
        .await
        .unwrap();
    harness
        .record_turn("session-b", "persist that", "saved too")
        .await
        .unwrap();

    harness.clear_all().await.unwrap();

    assert!(harness.history("session-a").await.unwrap().is_empty());
    assert!(harness.history("session-b").await.unwrap().is_empty());
    assert!(harness.session_ids().is_empty());
    assert!(harness.recent_episode_texts(10).await.unwrap().is_empty());
}

#[tokio::test]
async fn session_snapshot_captures_ordered_session_history() {
    let mut harness = MemoryDriftHarness::new();

    harness
        .record_turn("session-snap", "first question", "first answer")
        .await
        .unwrap();

    let snapshot = harness.session_snapshot("session-snap").await.unwrap();
    assert_eq!(snapshot.session_id, "session-snap");
    assert_eq!(snapshot.messages.len(), 2);
    assert_eq!(snapshot.messages[0].content, "first question");
    assert_eq!(snapshot.messages[1].content, "first answer");
}

#[tokio::test]
async fn stats_reflect_sessions_and_messages() {
    let mut harness = MemoryDriftHarness::new();

    harness
        .record_turn("session-a", "alpha", "ack alpha")
        .await
        .unwrap();
    harness
        .record_message(
            "session-b",
            mofa_foundation::agent::components::Message::user("beta"),
        )
        .await
        .unwrap();

    let stats = harness.stats().await.unwrap();
    assert_eq!(stats.total_sessions, 2);
    assert_eq!(stats.total_messages, 3);
}

#[tokio::test]
async fn all_session_snapshots_include_each_session_history() {
    let mut harness = MemoryDriftHarness::new();

    // Snapshots are useful for drift assertions because they preserve the full
    // ordered per-session history instead of just aggregate stats.
    harness
        .record_turn("session-a", "alpha question", "alpha answer")
        .await
        .unwrap();
    harness
        .record_message(
            "session-b",
            mofa_foundation::agent::components::Message::user("beta only"),
        )
        .await
        .unwrap();

    let snapshots = harness.all_session_snapshots().await.unwrap();
    assert_eq!(snapshots.len(), 2);

    let session_a = snapshots
        .iter()
        .find(|snapshot| snapshot.session_id == "session-a")
        .unwrap();
    let session_b = snapshots
        .iter()
        .find(|snapshot| snapshot.session_id == "session-b")
        .unwrap();

    assert_eq!(session_a.messages.len(), 2);
    assert_eq!(session_a.messages[0].content, "alpha question");
    assert_eq!(session_a.messages[1].content, "alpha answer");
    assert_eq!(session_b.messages.len(), 1);
    assert_eq!(session_b.messages[0].content, "beta only");
}

#[tokio::test]
async fn clearing_session_updates_recent_recall_without_affecting_others() {
    let mut harness = MemoryDriftHarness::new();

    harness
        .record_turn("session-a", "alpha question", "alpha answer")
        .await
        .unwrap();
    harness
        .record_turn("session-b", "beta question", "beta answer")
        .await
        .unwrap();

    let before_clear = harness.recent_episode_texts(4).await.unwrap();
    assert!(before_clear.iter().any(|text| text == "alpha question"));
    assert!(before_clear.iter().any(|text| text == "beta answer"));

    harness.clear_session("session-a").await.unwrap();

    let after_clear = harness.recent_episode_texts(4).await.unwrap();
    assert!(!after_clear.iter().any(|text| text == "alpha question"));
    assert!(!after_clear.iter().any(|text| text == "alpha answer"));
    assert!(after_clear.iter().any(|text| text == "beta question"));
    assert!(after_clear.iter().any(|text| text == "beta answer"));
}

#[tokio::test]
async fn semantic_memory_mode_supports_search() {
    let mut harness = MemoryDriftHarness::with_semantic_memory();

    // This verifies semantic retrieval through the same harness API rather than
    // only direct SemanticMemory unit tests in foundation.
    harness
        .store_text("rust-note", "Rust is a systems programming language")
        .await
        .unwrap();
    harness
        .store_text("python-note", "Python is often used for machine learning")
        .await
        .unwrap();

    let results = harness.search_texts("systems language", 2).await.unwrap();
    assert!(!results.is_empty());
    assert!(results
        .iter()
        .any(|text| text.contains("Rust is a systems programming language")));
    assert_eq!(harness.memory_type(), "semantic");
}

#[tokio::test]
async fn semantic_memory_mode_keeps_session_history_and_reset_behavior() {
    let mut harness = MemoryDriftHarness::with_semantic_memory();

    harness
        .record_turn("semantic-session", "remember this", "saved in semantic mode")
        .await
        .unwrap();

    let history = harness.history("semantic-session").await.unwrap();
    assert_eq!(history.len(), 2);

    harness.clear_all().await.unwrap();

    assert!(harness
        .history("semantic-session")
        .await
        .unwrap()
        .is_empty());
    assert!(harness.session_ids().is_empty());
}

#[tokio::test]
async fn store_text_and_retrieve_text_roundtrip() {
    let mut harness = MemoryDriftHarness::new();

    harness
        .store_text("preference", "User prefers concise Rust examples")
        .await
        .unwrap();

    let retrieved = harness.retrieve_text("preference").await.unwrap();
    assert_eq!(
        retrieved.as_deref(),
        Some("User prefers concise Rust examples")
    );
}
