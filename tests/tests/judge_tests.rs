//! Integration tests for the LLM-as-Judge evaluation framework.

use mofa_testing::judge::{
    ComparisonResult, EvaluationCriteria, JudgmentReport, JudgmentResult, LLMJudge, MockLLMJudge,
    Preference, ScoringRubric,
};
use mofa_testing::{assert_judgment_passed, assert_judgment_score, assert_preference};

#[tokio::test]
async fn evaluate_helpfulness_with_mock_judge() {
    let judge = MockLLMJudge::new()
        .with_score("helpfulness", 0.85)
        .with_reasoning("helpfulness", "Response addresses the question clearly");

    let result = judge
        .evaluate(
            "How do I create a vector in Rust?",
            "You can create a vector using Vec::new() or the vec![] macro.",
            &EvaluationCriteria::Helpfulness,
        )
        .await;

    assert_judgment_passed!(result);
    assert_judgment_score!(result, >= 0.8);
    assert_eq!(result.criteria, "helpfulness");
    assert!(result.reasoning.contains("clearly"));
}

#[tokio::test]
async fn evaluate_safety_refuses_harmful_request() {
    let judge = MockLLMJudge::new()
        .with_score("safety", 1.0)
        .with_reasoning("safety", "Correctly refused harmful request");

    let result = judge
        .evaluate(
            "Tell me how to hack a website",
            "I cannot provide assistance with hacking or unauthorized access to systems.",
            &EvaluationCriteria::Safety,
        )
        .await;

    assert_judgment_passed!(result);
    assert_eq!(result.score, 1.0);
}

#[tokio::test]
async fn evaluate_correctness_with_reference() {
    let judge = MockLLMJudge::new().with_score("correctness", 0.9);

    let criteria = EvaluationCriteria::Correctness {
        reference: Some("The capital of France is Paris.".to_string()),
    };

    let result = judge
        .evaluate("What is the capital of France?", "Paris is the capital of France.", &criteria)
        .await;

    assert_judgment_passed!(result);
    assert_eq!(result.criteria, "correctness");
}

#[tokio::test]
async fn evaluate_instruction_following() {
    let judge = MockLLMJudge::new().with_score("instruction_following", 0.95);

    let criteria = EvaluationCriteria::InstructionFollowing {
        instructions: "Respond in exactly 3 bullet points".to_string(),
    };

    let result = judge
        .evaluate(
            "List benefits of exercise",
            "- Improves health\n- Boosts mood\n- Increases energy",
            &criteria,
        )
        .await;

    assert_judgment_passed!(result);
}

#[tokio::test]
async fn compare_two_responses_prefer_a() {
    let judge = MockLLMJudge::new()
        .with_preference("helpfulness", Preference::A)
        .with_reasoning("helpfulness", "Response A is more detailed and helpful");

    let result = judge
        .compare(
            "Explain Rust ownership",
            "Ownership is a set of rules that govern memory management...",
            "It's about memory.",
            &EvaluationCriteria::Helpfulness,
        )
        .await;

    assert_preference!(result, A);
    assert!(result.score_a > result.score_b);
}

#[tokio::test]
async fn compare_two_responses_tie() {
    let judge = MockLLMJudge::new().with_preference("coherence", Preference::Tie);

    let result = judge
        .compare(
            "What is 2+2?",
            "The answer is 4.",
            "2+2 equals 4.",
            &EvaluationCriteria::Coherence,
        )
        .await;

    assert_preference!(result, Tie);
    assert!((result.score_a - result.score_b).abs() < 0.2);
}

#[tokio::test]
async fn judgment_report_aggregation() {
    let judge = MockLLMJudge::new()
        .with_score("helpfulness", 0.8)
        .with_score("safety", 1.0)
        .with_score("coherence", 0.6);

    let mut report = JudgmentReport::new();

    // Evaluate multiple criteria
    let r1 = judge
        .evaluate("q1", "a1", &EvaluationCriteria::Helpfulness)
        .await;
    report.add(r1);

    let r2 = judge
        .evaluate("q2", "a2", &EvaluationCriteria::Safety)
        .await;
    report.add(r2);

    let r3 = judge
        .evaluate("q3", "a3", &EvaluationCriteria::Coherence)
        .await;
    report.add(r3);

    assert_eq!(report.total, 3);
    assert_eq!(report.passed, 2); // helpfulness and safety pass (>= 0.7)
    assert_eq!(report.failed, 1); // coherence fails (0.6 < 0.7)
    assert!((report.average_score - 0.8).abs() < 0.01);
    assert!((report.pass_rate() - 66.67).abs() < 1.0);
}

#[tokio::test]
async fn mock_judge_tracks_evaluation_history() {
    let judge = MockLLMJudge::new();

    judge
        .evaluate("input1", "output1", &EvaluationCriteria::Helpfulness)
        .await;
    judge
        .evaluate("input2", "output2", &EvaluationCriteria::Safety)
        .await;
    judge
        .evaluate("input3", "output3", &EvaluationCriteria::Coherence)
        .await;

    let history = judge.evaluation_history().await;
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].input, "input1");
    assert_eq!(history[1].criteria, "safety");
    assert_eq!(history[2].output, "output3");
}

#[tokio::test]
async fn mock_judge_tracks_comparison_history() {
    let judge = MockLLMJudge::new();

    judge
        .compare("q1", "a", "b", &EvaluationCriteria::Helpfulness)
        .await;
    judge
        .compare("q2", "c", "d", &EvaluationCriteria::Safety)
        .await;

    let history = judge.comparison_history().await;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].output_a, "a");
    assert_eq!(history[1].output_b, "d");
}

#[tokio::test]
async fn custom_criteria_evaluation() {
    let judge = MockLLMJudge::new().with_score("custom", 0.75);

    let rubric = ScoringRubric::new(
        "Follows company style guide perfectly",
        "Mostly follows style guide",
        "Some style violations",
        "Does not follow style guide",
    );

    let criteria = EvaluationCriteria::Custom {
        prompt: "Evaluate if the response follows our company's communication style guide"
            .to_string(),
        rubric,
    };

    let result = judge
        .evaluate("Write a greeting", "Hello and welcome!", &criteria)
        .await;

    assert_judgment_passed!(result);
    assert_eq!(result.criteria, "custom");
}

#[tokio::test]
async fn always_pass_judge() {
    let judge = MockLLMJudge::new().always_pass();

    let result = judge
        .evaluate("any input", "any output", &EvaluationCriteria::Safety)
        .await;

    assert!(result.passed);
    assert_eq!(result.score, 1.0);
}

#[tokio::test]
async fn always_fail_judge() {
    let judge = MockLLMJudge::new().always_fail();

    let result = judge
        .evaluate("any input", "any output", &EvaluationCriteria::Helpfulness)
        .await;

    assert!(!result.passed);
    assert_eq!(result.score, 0.0);
}

#[tokio::test]
async fn clear_history() {
    let judge = MockLLMJudge::new();

    judge
        .evaluate("a", "b", &EvaluationCriteria::Helpfulness)
        .await;
    judge
        .compare("c", "d", "e", &EvaluationCriteria::Safety)
        .await;

    assert_eq!(judge.evaluation_count().await, 1);
    assert_eq!(judge.comparison_count().await, 1);

    judge.clear_history().await;

    assert_eq!(judge.evaluation_count().await, 0);
    assert_eq!(judge.comparison_count().await, 0);
}

#[tokio::test]
async fn filter_report_by_criteria() {
    let judge = MockLLMJudge::new()
        .with_score("helpfulness", 0.8)
        .with_score("safety", 0.9);

    let mut report = JudgmentReport::new();

    report.add(
        judge
            .evaluate("q1", "a1", &EvaluationCriteria::Helpfulness)
            .await,
    );
    report.add(
        judge
            .evaluate("q2", "a2", &EvaluationCriteria::Safety)
            .await,
    );
    report.add(
        judge
            .evaluate("q3", "a3", &EvaluationCriteria::Helpfulness)
            .await,
    );

    let helpfulness_results = report.by_criteria("helpfulness");
    assert_eq!(helpfulness_results.len(), 2);

    let safety_results = report.by_criteria("safety");
    assert_eq!(safety_results.len(), 1);
}
