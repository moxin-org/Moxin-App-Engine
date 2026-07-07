# Agent Test DSL Examples

This folder provides practical, copy-paste scenarios for the `mofa-testing` Agent Test DSL.

## Included Scenarios

- `scenario_weather.yaml`: Multi-turn support flow with tool call + follow-up acknowledgment
- `scenario_follow_up.toml`: TOML scenario with tool expectation and summary follow-up

## Minimal Usage

```rust
use mofa_testing::{
    AgentTestScenario, MockLLMBackend, MockScenarioAgent, MockTool,
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let yaml = std::fs::read_to_string("examples/agent_test_dsl/scenario_weather.yaml")?;
    let scenario = AgentTestScenario::from_yaml_str(&yaml)?;

    let backend = MockLLMBackend::new();
    backend.add_response("weather", "The temperature in Berlin is 22C.");

    let weather_tool = MockTool::new(
        "weather_search",
        "Lookup weather",
        json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"]
        }),
    );

    let mut agent = MockScenarioAgent::new("mock-model", backend)
        .with_mock_tool(weather_tool)
        .add_tool_rule("weather", "weather_search", json!({"city": "Berlin"}));

    let report = scenario.run_with_agent(&mut agent).await;
    println!("total={}, passed={}, failed={}", report.total(), report.passed(), report.failed());

    Ok(())
}
```

## Why these examples exist

The DSL introduces structural testing capabilities. These example scenarios are included so new users can quickly understand practical usage without reverse-engineering test internals.
