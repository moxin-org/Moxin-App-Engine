use mofa_observatory::evaluation::{KeywordEvaluator, LatencyEvaluator, Evaluator};

#[tokio::test]
async fn test_keyword_evaluator_pass_fail() {
    let eval = KeywordEvaluator {
        required_keywords: vec!["rust".to_string(), "memory".to_string()],
        forbidden_keywords: vec!["error".to_string()],
    };

    // Should pass: both required keywords present, no forbidden
    let pass = eval
        .evaluate("What is Rust?", "Rust has great memory management", None)
        .await
        .unwrap();
    assert!(pass.score >= 0.9, "expected high score, got {}", pass.score);
    assert!(pass.passed);

    // Should fail: forbidden keyword present
    let fail = eval
        .evaluate("input", "an error occurred in memory allocation", None)
        .await
        .unwrap();
    assert!(fail.score < 0.7, "expected low score for forbidden keyword, got {}", fail.score);
}

#[tokio::test]
async fn test_keyword_evaluator_no_required() {
    let eval = KeywordEvaluator {
        required_keywords: vec![],
        forbidden_keywords: vec!["bad".to_string()],
    };

    let good = eval.evaluate("q", "great answer", None).await.unwrap();
    assert!(good.score >= 0.99, "score should be 1.0 with no required keywords and no forbidden");

    let bad = eval.evaluate("q", "bad answer", None).await.unwrap();
    assert!(bad.score < 0.7);
}

#[tokio::test]
async fn test_latency_evaluator_pass() {
    let eval = LatencyEvaluator {
        threshold_ms: 100,
        measured_ms: 50,
    };
    let result = eval.evaluate("q", "a", None).await.unwrap();
    assert!(result.score > 0.7, "score should be high for fast response");
    assert!(result.passed);
}

#[tokio::test]
async fn test_latency_evaluator_fail() {
    let eval = LatencyEvaluator {
        threshold_ms: 100,
        measured_ms: 300,
    };
    let result = eval.evaluate("q", "a", None).await.unwrap();
    assert!(result.score < 0.3, "score should be low for slow response");
    assert!(!result.passed);
}

#[tokio::test]
async fn test_latency_evaluator_at_threshold() {
    let eval = LatencyEvaluator {
        threshold_ms: 200,
        measured_ms: 200,
    };
    let result = eval.evaluate("q", "a", None).await.unwrap();
    assert!(result.passed, "exactly at threshold should pass");
    assert!(
        (result.score - 0.5).abs() < 0.01,
        "at threshold score should be ~0.5, got {}",
        result.score
    );
}
