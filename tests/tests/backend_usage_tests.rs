use mofa_foundation::orchestrator::ModelOrchestrator;
use mofa_testing::backend::MockLLMBackend;

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[tokio::test]
async fn usage_records_tokens_and_cost_for_successful_infer() {
    let backend = MockLLMBackend::new();
    backend.add_response("hello", "world response");
    backend.set_token_cost_rates(0.5, 1.0); // USD per 1k tokens

    let _ = backend.infer("m", "hello there").await.unwrap();

    let usage = backend.last_usage().expect("usage must exist");
    assert_eq!(usage.prompt_tokens, 2);
    assert_eq!(usage.completion_tokens, 2);
    assert_eq!(usage.total_tokens, 4);
    assert!(approx_eq(usage.cost_usd, 0.003));
}

#[tokio::test]
async fn usage_totals_are_deterministic_across_calls() {
    let backend = MockLLMBackend::new();
    backend.add_response("a", "x y");
    backend.add_response("b", "z");
    backend.set_token_cost_rates(1.0, 2.0);

    let _ = backend.infer("m", "a a").await.unwrap(); // in=2, out=2 => 0.006
    let _ = backend.infer("m", "b").await.unwrap(); // in=1, out=1 => 0.003

    let totals = backend.get_usage_totals();
    assert_eq!(totals.successful_calls, 2);
    assert_eq!(totals.prompt_tokens, 3);
    assert_eq!(totals.completion_tokens, 3);
    assert_eq!(totals.total_tokens, 6);
    assert!(approx_eq(totals.total_cost_usd, 0.009));

    let history = backend.get_usage_history();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].0, "a a");
    assert_eq!(history[1].0, "b");
}

#[tokio::test]
async fn failed_infer_does_not_add_usage_record() {
    let backend = MockLLMBackend::new();
    backend.fail_next(
        1,
        mofa_foundation::orchestrator::OrchestratorError::Other("boom".into()),
    );
    backend.set_token_cost_rates(1.0, 1.0);

    let _ = backend.infer("m", "hello").await;

    assert!(backend.last_usage().is_none());
    assert_eq!(backend.get_usage_history().len(), 0);
    let totals = backend.get_usage_totals();
    assert_eq!(totals.successful_calls, 0);
    assert_eq!(totals.total_tokens, 0);
    assert!(approx_eq(totals.total_cost_usd, 0.0));
}

#[tokio::test]
async fn reset_usage_accounting_clears_history_and_totals() {
    let backend = MockLLMBackend::new();
    backend.add_response("hello", "world");
    backend.set_token_cost_rates(1.0, 1.0);

    let _ = backend.infer("m", "hello world").await.unwrap();
    assert_eq!(backend.get_usage_totals().successful_calls, 1);

    backend.reset_usage_accounting();

    assert!(backend.last_usage().is_none());
    assert_eq!(backend.get_usage_history().len(), 0);
    let totals = backend.get_usage_totals();
    assert_eq!(totals.successful_calls, 0);
    assert_eq!(totals.total_tokens, 0);
    assert!(approx_eq(totals.total_cost_usd, 0.0));
}
