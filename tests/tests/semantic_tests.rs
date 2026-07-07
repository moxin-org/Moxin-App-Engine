use mofa_testing::{
    ContainsAllFactsMatcher, ExcludesContentMatcher, IntentMatcher, RegexIntentMatcher,
    SemanticAssertionError, SemanticAssertionSet, SemanticExpectation, SemanticMatchResult,
    SemanticMatcher, SimilarityMatcher,
};

// ═══════════════════════════════════════════════════════════════════════════
// ContainsAllFactsMatcher tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn contains_all_facts_passes_when_all_present() {
    let matcher = ContainsAllFactsMatcher::new(vec!["Berlin", "22C", "sunny"]);
    let result = matcher.evaluate("The weather in Berlin is 22C and sunny today.");

    assert!(result.passed);
    assert_eq!(result.confidence, 1.0);
    assert!(result.explanation.contains("3 facts found"));
}

#[test]
fn contains_all_facts_fails_when_some_missing() {
    let matcher = ContainsAllFactsMatcher::new(vec!["Berlin", "22C", "snowy"]);
    let result = matcher.evaluate("The weather in Berlin is 22C and sunny.");

    assert!(!result.passed);
    assert!((result.confidence - 2.0 / 3.0).abs() < 0.01);
    assert!(result.explanation.contains("snowy"));
}

#[test]
fn contains_all_facts_case_insensitive() {
    let matcher = ContainsAllFactsMatcher::new(vec!["berlin", "sunny"]);
    let result = matcher.evaluate("BERLIN is SUNNY today");

    assert!(result.passed);
}

#[test]
fn contains_all_facts_empty_facts_passes() {
    let matcher = ContainsAllFactsMatcher::new(Vec::<String>::new());
    let result = matcher.evaluate("any response");

    assert!(result.passed);
    assert_eq!(result.confidence, 1.0);
}

#[test]
fn contains_all_facts_no_facts_found() {
    let matcher = ContainsAllFactsMatcher::new(vec!["xyzzy", "plugh"]);
    let result = matcher.evaluate("nothing matches here");

    assert!(!result.passed);
    assert_eq!(result.confidence, 0.0);
}

// ═══════════════════════════════════════════════════════════════════════════
// ExcludesContentMatcher tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn excludes_content_passes_when_clean() {
    let matcher = ExcludesContentMatcher::new(vec!["password", "SSN", "credit card"]);
    let result = matcher.evaluate("Here is your account summary for March.");

    assert!(result.passed);
    assert_eq!(result.confidence, 1.0);
}

#[test]
fn excludes_content_fails_when_banned_found() {
    let matcher = ExcludesContentMatcher::new(vec!["password", "SSN"]);
    let result = matcher.evaluate("Your password is abc123.");

    assert!(!result.passed);
    assert!(result.explanation.contains("password"));
}

#[test]
fn excludes_content_case_insensitive() {
    let matcher = ExcludesContentMatcher::new(vec!["secret"]);
    let result = matcher.evaluate("This is a SECRET document.");

    assert!(!result.passed);
}

#[test]
fn excludes_content_multiple_violations() {
    let matcher = ExcludesContentMatcher::new(vec!["password", "SSN", "credit card"]);
    let result = matcher.evaluate("Your password is X and your SSN is Y.");

    assert!(!result.passed);
    assert!(result.explanation.contains("password"));
    assert!(result.explanation.contains("SSN"));
}

#[test]
fn excludes_content_empty_banned_passes() {
    let matcher = ExcludesContentMatcher::new(Vec::<String>::new());
    let result = matcher.evaluate("anything");

    assert!(result.passed);
}

// ═══════════════════════════════════════════════════════════════════════════
// IntentMatcher tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn intent_matches_single_intent() {
    let matcher = IntentMatcher::new()
        .expect_intent("greeting", vec!["hello", "hi", "welcome"]);

    let result = matcher.evaluate("Hello! How can I help you?");
    assert!(result.passed);
    assert!(result.explanation.contains("greeting"));
}

#[test]
fn intent_fails_when_no_intent_matches() {
    let matcher = IntentMatcher::new()
        .expect_intent("farewell", vec!["goodbye", "bye"]);

    let result = matcher.evaluate("Hello there!");
    assert!(!result.passed);
}

#[test]
fn intent_any_mode_passes_with_one_match() {
    let matcher = IntentMatcher::new()
        .expect_intent("greeting", vec!["hello", "hi"])
        .expect_intent("help", vec!["assist", "help"]);

    let result = matcher.evaluate("Hi there!");
    assert!(result.passed); // greeting matched, even though help didn't
}

#[test]
fn intent_require_all_fails_with_partial() {
    let matcher = IntentMatcher::new()
        .require_all()
        .expect_intent("greeting", vec!["hello", "hi"])
        .expect_intent("help", vec!["assist", "help"]);

    let result = matcher.evaluate("Hi there!");
    assert!(!result.passed); // greeting matched, but help didn't
    assert!(result.explanation.contains("help"));
}

#[test]
fn intent_require_all_passes_when_all_match() {
    let matcher = IntentMatcher::new()
        .require_all()
        .expect_intent("greeting", vec!["hello", "hi"])
        .expect_intent("offer", vec!["help", "assist"]);

    let result = matcher.evaluate("Hello! How can I help you?");
    assert!(result.passed);
    assert_eq!(result.confidence, 1.0);
}

#[test]
fn intent_case_insensitive() {
    let matcher = IntentMatcher::new()
        .expect_intent("greeting", vec!["hello"]);

    let result = matcher.evaluate("HELLO WORLD!");
    assert!(result.passed);
}

#[test]
fn intent_empty_intents_passes() {
    let matcher = IntentMatcher::new();
    let result = matcher.evaluate("anything");
    assert!(result.passed);
}

// ═══════════════════════════════════════════════════════════════════════════
// SimilarityMatcher tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn similarity_high_overlap_passes() {
    let matcher = SimilarityMatcher::new(
        "The temperature in Berlin is 22 degrees Celsius",
        0.3,
    )
    .expect("valid threshold");

    let result = matcher.evaluate("The temperature is 22 degrees in Berlin Celsius");
    assert!(result.passed);
    assert!(result.confidence > 0.5);
}

#[test]
fn similarity_no_overlap_fails() {
    let matcher = SimilarityMatcher::new("apples oranges bananas", 0.5).expect("valid");

    let result = matcher.evaluate("cars trucks planes");
    assert!(!result.passed);
    assert_eq!(result.confidence, 0.0);
}

#[test]
fn similarity_identical_text() {
    let matcher = SimilarityMatcher::new("hello world", 0.9).expect("valid");

    let result = matcher.evaluate("hello world");
    assert!(result.passed);
    assert_eq!(result.confidence, 1.0);
}

#[test]
fn similarity_threshold_boundary() {
    let matcher = SimilarityMatcher::new("alpha beta gamma delta", 0.5).expect("valid");

    // 2 out of 4 tokens match = Jaccard depends on union
    let result = matcher.evaluate("alpha beta epsilon zeta");
    // Intersection: {alpha, beta} = 2, Union: {alpha, beta, gamma, delta, epsilon, zeta} = 6
    // Jaccard = 2/6 ≈ 0.33
    assert!(!result.passed);
}

#[test]
fn similarity_invalid_threshold_below_zero() {
    let err = SimilarityMatcher::new("test", -0.1).expect_err("should fail");
    assert!(matches!(err, SemanticAssertionError::InvalidThreshold { .. }));
}

#[test]
fn similarity_invalid_threshold_above_one() {
    let err = SimilarityMatcher::new("test", 1.5).expect_err("should fail");
    assert!(matches!(err, SemanticAssertionError::InvalidThreshold { .. }));
}

#[test]
fn similarity_zero_threshold_always_passes() {
    let matcher = SimilarityMatcher::new("reference", 0.0).expect("valid");
    let result = matcher.evaluate("completely different text");
    assert!(result.passed);
}

#[test]
fn similarity_case_insensitive() {
    let matcher = SimilarityMatcher::new("Hello World", 0.9).expect("valid");
    let result = matcher.evaluate("hello world");
    assert!(result.passed);
}

// ═══════════════════════════════════════════════════════════════════════════
// RegexIntentMatcher tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn regex_intent_matches_pattern() {
    let matcher = RegexIntentMatcher::new(
        "temperature_mentioned",
        r"\d+\s*(°?[CF]|degrees|celsius|fahrenheit)",
        "response mentions a temperature value",
    )
    .expect("valid regex");

    let result = matcher.evaluate("The temperature is 22 degrees today.");
    assert!(result.passed);
    assert_eq!(result.confidence, 1.0);
}

#[test]
fn regex_intent_fails_when_no_match() {
    let matcher = RegexIntentMatcher::new(
        "has_number",
        r"\d+",
        "response contains a number",
    )
    .expect("valid regex");

    let result = matcher.evaluate("No numbers here at all.");
    assert!(!result.passed);
}

#[test]
fn regex_intent_invalid_pattern() {
    let err = RegexIntentMatcher::new("bad", "[invalid", "test")
        .expect_err("should fail");
    assert!(matches!(err, SemanticAssertionError::InvalidPattern(_)));
}

// ═══════════════════════════════════════════════════════════════════════════
// SemanticAssertionSet tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn assertion_set_all_pass() {
    let set = SemanticAssertionSet::new()
        .add(ContainsAllFactsMatcher::new(vec!["Berlin", "sunny"]))
        .add(ExcludesContentMatcher::new(vec!["password"]))
        .add(IntentMatcher::new().expect_intent("weather", vec!["weather", "temperature"]));

    let report = set
        .evaluate("The weather in Berlin is sunny and warm.")
        .expect("evaluate");

    assert!(report.passed);
    assert_eq!(report.passed_count, 3);
    assert_eq!(report.total_count, 3);
}

#[test]
fn assertion_set_partial_failure() {
    let set = SemanticAssertionSet::new()
        .add(ContainsAllFactsMatcher::new(vec!["Berlin"]))
        .add(ExcludesContentMatcher::new(vec!["password"]));

    let report = set
        .evaluate("Your Berlin password is abc123")
        .expect("evaluate");

    assert!(!report.passed);
    assert_eq!(report.passed_count, 1); // facts passed, excludes failed
    assert_eq!(report.total_count, 2);
}

#[test]
fn assertion_set_empty_fails() {
    let set = SemanticAssertionSet::new();
    let err = set.evaluate("anything").expect_err("should fail");
    assert!(matches!(err, SemanticAssertionError::EmptyMatcherSet));
}

#[test]
fn assertion_set_single_matcher() {
    let set = SemanticAssertionSet::new()
        .add(ContainsAllFactsMatcher::new(vec!["hello"]));

    let report = set.evaluate("hello world").expect("evaluate");
    assert!(report.passed);
    assert_eq!(report.total_count, 1);
}

#[test]
fn assertion_set_average_confidence() {
    let set = SemanticAssertionSet::new()
        .add(ContainsAllFactsMatcher::new(vec!["present"])) // 1.0 confidence
        .add(ContainsAllFactsMatcher::new(vec!["missing"])); // 0.0 confidence

    let report = set
        .evaluate("present but no other fact")
        .expect("evaluate");

    assert!(!report.passed);
    assert!((report.average_confidence - 0.5).abs() < 0.01);
}

// ═══════════════════════════════════════════════════════════════════════════
// SemanticMatchResult tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn match_result_pass_constructor() {
    let result = SemanticMatchResult::pass("test", 0.95, "looks good");
    assert!(result.passed);
    assert_eq!(result.confidence, 0.95);
    assert_eq!(result.matcher_name, "test");
}

#[test]
fn match_result_fail_constructor() {
    let result = SemanticMatchResult::fail("test", 0.3, "not good");
    assert!(!result.passed);
    assert_eq!(result.confidence, 0.3);
}

#[test]
fn match_result_serialization_roundtrip() {
    let result = SemanticMatchResult::pass("contains_all_facts", 1.0, "all facts found");

    let json = serde_json::to_string(&result).expect("serialize");
    let deserialized: SemanticMatchResult = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(result, deserialized);
}

// ═══════════════════════════════════════════════════════════════════════════
// SemanticExpectation: file-backed loading
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn semantic_expectation_from_yaml() {
    let yaml = r#"
- kind: contains_all_facts
  facts:
    - Berlin
    - sunny
- kind: excludes_content
  banned:
    - password
    - secret
- kind: matches_intent
  intents:
    - name: weather
      indicators: [weather, forecast, temperature]
  require_all: false
- kind: similar_to
  reference: "The weather in Berlin is sunny"
  threshold: 0.4
- kind: matches_pattern
  name: has_temperature
  pattern: '\d+\s*degrees'
  description: "response includes a temperature"
"#;

    let expectations = SemanticExpectation::from_yaml_str(yaml).expect("parse yaml");
    assert_eq!(expectations.len(), 5);

    let set = SemanticExpectation::into_assertion_set(expectations).expect("build set");
    assert_eq!(set.len(), 5);

    let report = set
        .evaluate("The weather in Berlin is 22 degrees and sunny today.")
        .expect("evaluate");

    assert!(report.passed, "all expectations should pass: {:?}", report);
}

#[test]
fn semantic_expectation_from_json() {
    let json = r#"[
  {"kind": "contains_all_facts", "facts": ["hello"]},
  {"kind": "excludes_content", "banned": ["secret"]}
]"#;

    let expectations = SemanticExpectation::from_json_str(json).expect("parse json");
    assert_eq!(expectations.len(), 2);
}

#[test]
fn semantic_expectation_into_matcher_similarity() {
    let exp = SemanticExpectation::SimilarTo {
        reference: "hello world".to_string(),
        threshold: 0.5,
    };

    let matcher = exp.into_matcher().expect("build matcher");
    let result = matcher.evaluate("hello world");
    assert!(result.passed);
}

#[test]
fn semantic_expectation_into_matcher_invalid_pattern() {
    let exp = SemanticExpectation::MatchesPattern {
        name: "bad".to_string(),
        pattern: "[invalid".to_string(),
        description: "test".to_string(),
    };

    let err = exp.into_matcher().expect_err("should fail");
    assert!(matches!(err, SemanticAssertionError::InvalidPattern(_)));
}

// ═══════════════════════════════════════════════════════════════════════════
// Composite real-world scenarios
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn real_world_weather_agent_validation() {
    let set = SemanticAssertionSet::new()
        .add(ContainsAllFactsMatcher::new(vec!["Berlin", "temperature"]))
        .add(ExcludesContentMatcher::new(vec![
            "I don't know",
            "error",
            "unable",
        ]))
        .add(
            IntentMatcher::new()
                .require_all()
                .expect_intent("weather_info", vec!["weather", "temperature", "forecast"])
                .expect_intent("location_specific", vec!["Berlin"]),
        )
        .add(
            SimilarityMatcher::new(
                "The temperature in Berlin is around 22 degrees",
                0.3,
            )
            .expect("valid"),
        )
        .add(
            RegexIntentMatcher::new(
                "has_temp_value",
                r"\d+",
                "includes a numeric temperature value",
            )
            .expect("valid"),
        );

    let response = "The current temperature in Berlin is 22 degrees Celsius with clear skies.";
    let report = set.evaluate(response).expect("evaluate");

    assert!(report.passed, "weather response should pass all matchers");
    assert_eq!(report.passed_count, 5);
}

#[test]
fn real_world_policy_violation_detection() {
    let set = SemanticAssertionSet::new()
        .add(ExcludesContentMatcher::new(vec![
            "password",
            "SSN",
            "social security",
            "credit card number",
        ]))
        .add(ContainsAllFactsMatcher::new(vec!["account", "summary"]));

    let safe_response = "Here is your account summary for March 2026.";
    let report = set.evaluate(safe_response).expect("evaluate");
    assert!(report.passed);

    let unsafe_response = "Your account summary: password is abc123.";
    let report2 = set.evaluate(unsafe_response).expect("evaluate");
    assert!(!report2.passed);
}

#[test]
fn real_world_mixed_intent_and_facts() {
    let set = SemanticAssertionSet::new()
        .add(
            IntentMatcher::new()
                .expect_intent("acknowledgment", vec!["thank", "welcome", "glad"]),
        )
        .add(ContainsAllFactsMatcher::new(vec!["help"]));

    let result = set
        .evaluate("You're welcome! Glad I could help.")
        .expect("evaluate");

    assert!(result.passed);
    assert_eq!(result.passed_count, 2);
}
