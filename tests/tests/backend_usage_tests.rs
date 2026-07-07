use mofa_foundation::orchestrator::{ModelOrchestrator, OrchestratorError};
use mofa_testing::MockLLMBackend;

#[tokio::test]
async fn successful_infer_records_usage_and_cost() {
    let backend = MockLLMBackend::new();
    backend.add_response("hello", "mock reply");
    backend.set_usage_pricing(0.25, 0.5);

    let response = backend.infer("demo-model", "hello world").await.unwrap();

    assert_eq!(response, "mock reply");
    let usage = backend.last_usage().expect("usage recorded");
    assert_eq!(usage.prompt_tokens, 2);
    assert_eq!(usage.completion_tokens, 2);
    assert_eq!(usage.total_tokens, 4);
    assert!((usage.cost_usd - 0.0015).abs() < f64::EPSILON);
}

#[tokio::test]
async fn deterministic_totals_across_multiple_calls() {
    let backend = MockLLMBackend::new();
    backend.add_response("alpha", "one two");
    backend.set_usage_pricing(1.0, 2.0);

    backend.infer("demo-model", "alpha beta").await.unwrap();
    backend.infer("demo-model", "alpha beta gamma").await.unwrap();

    let history = backend.usage_history();
    let totals = backend.usage_totals();

    assert_eq!(history.len(), 2);
    assert_eq!(totals.calls, 2);
    assert_eq!(totals.prompt_tokens, 5);
    assert_eq!(totals.completion_tokens, 4);
    assert_eq!(totals.total_tokens, 9);
    assert!((totals.cost_usd - 0.013).abs() < f64::EPSILON);
}

#[tokio::test]
async fn failure_path_does_not_add_usage() {
    let backend = MockLLMBackend::new();
    backend.fail_on("boom", OrchestratorError::Other("boom".into()));

    let err = backend.infer("demo-model", "boom now").await.unwrap_err();
    assert!(matches!(err, OrchestratorError::Other(_)));
    assert!(backend.usage_history().is_empty());
    assert_eq!(backend.usage_totals().calls, 0);
}

#[tokio::test]
async fn reset_clears_usage_accounting() {
    let backend = MockLLMBackend::new();
    backend.add_response("hello", "world");
    backend.set_usage_pricing(1.0, 1.0);

    backend.infer("demo-model", "hello there").await.unwrap();
    backend.reset_usage();

    assert!(backend.usage_history().is_empty());
    assert_eq!(backend.usage_totals().calls, 0);
    assert_eq!(backend.usage_totals().total_tokens, 0);
    assert_eq!(backend.usage_totals().cost_usd, 0.0);
}