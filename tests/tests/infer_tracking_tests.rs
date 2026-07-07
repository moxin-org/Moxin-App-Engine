//! Tests for MockLLMBackend per-prompt tracking: infer_history(), infer_count_for(),
//! clear_infer_history().

use mofa_foundation::orchestrator::{
    ModelOrchestrator, ModelProviderConfig, ModelType, OrchestratorError,
};
use mofa_testing::backend::MockLLMBackend;
use std::collections::HashMap;

fn make_config(name: &str) -> ModelProviderConfig {
    ModelProviderConfig {
        model_name: name.into(),
        model_path: "/mock".into(),
        device: "cpu".into(),
        model_type: ModelType::Llm,
        max_context_length: None,
        quantization: None,
        extra_config: HashMap::new(),
    }
}

async fn setup_backend() -> MockLLMBackend {
    let backend = MockLLMBackend::new();
    backend.register_model(make_config("m")).await.unwrap();
    backend.load_model("m").await.unwrap();
    backend
}

#[tokio::test]
async fn infer_history_is_empty_initially() {
    let backend = MockLLMBackend::new();
    assert!(backend.infer_history().is_empty());
}

#[tokio::test]
async fn infer_history_records_all_prompts_in_order() {
    let backend = setup_backend().await;

    backend.infer("m", "summarize this").await.unwrap();
    backend.infer("m", "translate to French").await.unwrap();
    backend.infer("m", "summarize again").await.unwrap();

    let history = backend.infer_history();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0], "summarize this");
    assert_eq!(history[1], "translate to French");
    assert_eq!(history[2], "summarize again");
}

#[tokio::test]
async fn infer_count_for_returns_zero_for_unmatched() {
    let backend = setup_backend().await;

    backend.infer("m", "hello world").await.unwrap();

    assert_eq!(backend.infer_count_for("nonexistent"), 0);
}

#[tokio::test]
async fn infer_count_for_counts_only_matching_prompts() {
    let backend = setup_backend().await;

    backend.infer("m", "summarize this").await.unwrap();
    backend.infer("m", "translate to French").await.unwrap();
    backend.infer("m", "summarize again").await.unwrap();

    assert_eq!(backend.infer_count_for("summarize"), 2);
    assert_eq!(backend.infer_count_for("translate"), 1);
}

#[tokio::test]
async fn infer_count_for_works_after_fail_next() {
    let backend = setup_backend().await;

    backend.fail_next(1, OrchestratorError::InferenceFailed("boom".into()));

    let _ = backend.infer("m", "summarize this").await;
    backend.infer("m", "translate to French").await.unwrap();

    assert_eq!(backend.infer_count_for("summarize"), 1);
    assert_eq!(backend.infer_count_for("translate"), 1);
    assert_eq!(backend.infer_history().len(), 2);
}

#[tokio::test]
async fn clear_infer_history_resets_history_but_not_call_count() {
    let backend = setup_backend().await;

    backend.infer("m", "hello").await.unwrap();
    backend.infer("m", "world").await.unwrap();

    assert_eq!(backend.call_count(), 2);
    assert_eq!(backend.infer_history().len(), 2);

    backend.clear_infer_history();

    assert!(backend.infer_history().is_empty());
    assert_eq!(backend.call_count(), 2);
}

#[tokio::test]
async fn overlapping_substrings_counted_correctly() {
    let backend = setup_backend().await;

    backend.infer("m", "summarize the text").await.unwrap();
    backend.infer("m", "summarize and translate").await.unwrap();
    backend.infer("m", "just translate").await.unwrap();

    assert_eq!(backend.infer_count_for("summarize"), 2);
    assert_eq!(backend.infer_count_for("translate"), 2);
    assert_eq!(backend.infer_count_for("summarize and translate"), 1);
}
