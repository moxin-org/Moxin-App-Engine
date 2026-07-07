# Golden Response Testing Examples

This folder demonstrates the **golden response (snapshot) testing** capabilities of `mofa-testing`.

Golden tests record agent outputs as baselines, then compare future runs against them to detect regressions automatically.

## Included Files

| File | Description |
|------|-------------|
| `goldens/weather_agent.golden.json` | Pre-recorded golden snapshot for a weather agent scenario |
| `goldens/support_agent.golden.json` | Pre-recorded golden snapshot for a support agent scenario |

## Quick Usage

### 1. Record a golden baseline (update mode)

```rust
use mofa_testing::{
    AgentTest, GoldenStore, GoldenTestConfig, MockLLMBackend,
    MockScenarioAgent, MockTool, run_golden_test,
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scenario = AgentTest::new("weather_agent")
        .given_tool("weather_search")
        .when_user_says("What's the weather in Berlin?")
        .then_agent_should()
        .call_tool("weather_search")
        .respond_containing("Berlin")
        .build()?;

    let backend = MockLLMBackend::new();
    backend.add_response("weather", "The temperature in Berlin is 22C.");

    let tool = MockTool::new(
        "weather_search", "Lookup weather",
        json!({"type":"object","properties":{"city":{"type":"string"}}}),
    );

    let mut agent = MockScenarioAgent::new("mock-model", backend)
        .with_mock_tool(tool)
        .add_tool_rule("weather", "weather_search", json!({"city": "Berlin"}));

    // Update mode: saves actual outputs as the golden baseline
    let store = GoldenStore::new("examples/golden_response_test/goldens");
    let config = GoldenTestConfig::update(store);
    let report = run_golden_test(&config, &scenario, &mut agent).await;

    println!("Baseline recorded: passed={}", report.passed());
    Ok(())
}
```

### 2. Validate against golden (strict mode)

```rust
    // Strict mode: compares actual outputs against stored golden
    let store = GoldenStore::new("examples/golden_response_test/goldens");
    let config = GoldenTestConfig::strict(store);
    let report = run_golden_test(&config, &scenario, &mut agent).await;

    if report.failed() > 0 {
        println!("REGRESSION DETECTED:");
        for result in &report.results {
            if let Some(err) = &result.error {
                println!("  {}: {}", result.name, err);
            }
        }
    }
```

### 3. Use normalizers to ignore non-deterministic content

```rust
use mofa_testing::{NormalizerChain, RegexNormalizer};

    let config = GoldenTestConfig::strict(store)
        .with_normalizer(
            NormalizerChain::default_chain()? // whitespace + UUID + timestamps
        );
```

## How It Works

1. **Update mode**: Run the scenario, capture all turn outputs, save them as `{test_name}.golden.json`.
2. **Strict mode**: Run the scenario, load the saved golden, compare field-by-field (response text, tool call names, tool call arguments).
3. **Normalizers**: Before comparison, strip non-deterministic content (UUIDs, timestamps, whitespace) so they don't cause false failures.
4. **Reports**: Results integrate into the standard `TestReport` with clear diff messages for each mismatch.

## CI Workflow

```
# In CI: run in strict mode to catch regressions
GOLDEN_MODE=strict cargo test --test golden_tests

# Locally: update baselines when behavior intentionally changes
GOLDEN_MODE=update cargo test --test golden_tests
```
