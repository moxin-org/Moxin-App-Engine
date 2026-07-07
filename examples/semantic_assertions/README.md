# Semantic Assertion Matcher Examples

This folder demonstrates the **semantic assertion** capabilities of `mofa-testing`.

Semantic assertions validate **meaning and intent** rather than exact text, making tests resilient to harmless wording changes while maintaining strict policy checks.

## Included Files

| File | Description |
|------|-------------|
| `weather_assertions.yaml` | Weather agent assertions: facts, intents, similarity, and policy |
| `safety_assertions.yaml` | Safety-focused assertions: prohibited content and policy compliance |

## Quick Usage

### Rust Builder API

```rust
use mofa_testing::{
    ContainsAllFactsMatcher, ExcludesContentMatcher, IntentMatcher,
    SemanticAssertionSet, SimilarityMatcher,
};

let assertions = SemanticAssertionSet::new()
    // Required facts must be present
    .add(ContainsAllFactsMatcher::new(vec!["Berlin", "temperature"]))
    // Prohibited content must be absent
    .add(ExcludesContentMatcher::new(vec!["password", "error"]))
    // Expected intent via keyword indicators
    .add(IntentMatcher::new()
        .expect_intent("weather", vec!["weather", "forecast", "temperature"]))
    // Token-overlap similarity to reference
    .add(SimilarityMatcher::new(
        "The temperature in Berlin is around 22 degrees", 0.3
    ).unwrap());

let report = assertions.evaluate(
    "The current temperature in Berlin is 22 degrees Celsius."
).unwrap();

println!("passed={} ({}/{})", report.passed, report.passed_count, report.total_count);
for result in &report.results {
    println!("  [{}] {} (confidence: {:.2}): {}",
        if result.passed { "PASS" } else { "FAIL" },
        result.matcher_name, result.confidence, result.explanation
    );
}
```

### File-backed (YAML)

```rust
use mofa_testing::SemanticExpectation;

let yaml = std::fs::read_to_string(
    "examples/semantic_assertions/weather_assertions.yaml"
)?;
let expectations = SemanticExpectation::from_yaml_str(&yaml)?;
let set = SemanticExpectation::into_assertion_set(expectations)?;

let report = set.evaluate("Berlin is 22 degrees and sunny.")?;
assert!(report.passed);
```

## Available Matcher Types

| Matcher | Purpose | Confidence Score |
|---------|---------|-----------------|
| `ContainsAllFactsMatcher` | All required facts must be present | Fraction of facts found |
| `ExcludesContentMatcher` | No prohibited content may appear | Fraction of clean terms |
| `IntentMatcher` | Intent via keyword indicators | Fraction of intents matched |
| `SimilarityMatcher` | Token-overlap (Jaccard) similarity | Similarity coefficient |
| `RegexIntentMatcher` | Structural pattern via regex | 1.0 or 0.0 |

## Design Decisions

1. **All matchers are deterministic** — no external API calls required, safe for CI.
2. **Case-insensitive by default** — reduces brittle failures from capitalization differences.
3. **Confidence scores** — every result includes a [0.0, 1.0] confidence score for nuanced analysis.
4. **Composable** — combine any number of matchers into a `SemanticAssertionSet` for comprehensive validation.
5. **File-backed** — YAML/JSON definitions enable non-Rust contributors to author assertions.
