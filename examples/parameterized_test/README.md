# Parameterized Agent Test Examples

This folder demonstrates the **parameterized test** capabilities of `mofa-testing`.

A parameterized test allows one scenario template to expand into multiple concrete test cases by substituting `{{variable}}` placeholders with values from parameter sets.

## Included Files

| File | Description |
|------|-------------|
| `scenario_weather_parameterized.yaml` | Explicit parameter list: tests the same weather lookup across multiple cities |
| `scenario_greetings_matrix.yaml` | Matrix expansion: generates Cartesian product of languages × tones |
| `scenario_support_parameterized.toml` | TOML format: parameterized support ticket lookup |

## Quick Usage

```rust
use mofa_testing::{
    ParameterizedScenarioFile, MockLLMBackend, MockScenarioAgent, MockTool,
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load the parameterized scenario
    let yaml = std::fs::read_to_string(
        "examples/parameterized_test/scenario_weather_parameterized.yaml",
    )?;
    let parameterized = ParameterizedScenarioFile::from_yaml_str(&yaml)?;

    println!("Expanding {} test variants...", parameterized.case_count());

    let expanded = parameterized.expand()?;

    for scenario in &expanded {
        // Set up a fresh mock agent per variant
        let backend = MockLLMBackend::new();
        // The response includes the city name from the scenario
        backend.add_response("weather", "The temperature is 22C.");

        let tool = MockTool::new(
            "weather_search",
            "Lookup weather",
            json!({"type": "object", "properties": {"city": {"type": "string"}}}),
        );

        let mut agent = MockScenarioAgent::new("mock-model", backend)
            .with_mock_tool(tool)
            .add_tool_rule("weather", "weather_search", json!({"city": "any"}));

        let report = scenario.run_with_agent(&mut agent).await;
        println!(
            "[{}] total={} passed={} failed={}",
            scenario.agent_id,
            report.total(),
            report.passed(),
            report.failed()
        );
    }

    Ok(())
}
```

## How It Works

1. **Template**: Write a scenario with `{{variable}}` placeholders in user prompts, expected responses, and tool arguments.

2. **Parameters**: Provide either:
   - **Explicit list**: Named parameter sets with variable bindings
   - **Matrix**: Dimensions with values; the Cartesian product is computed automatically
   - **Both**: Explicit sets + matrix sets are combined

3. **Expansion**: Each parameter set produces one concrete scenario with all `{{variable}}` placeholders replaced.

4. **Execution**: Each expanded scenario runs independently with isolated mock agents.

## Placeholder Syntax

- Placeholders use `{{variable_name}}` (double curly braces).
- They are supported in: `user` input, `text` expectations, `pattern` (regex), tool `name`, tool `arguments`, and `system_prompt`.
- The same variable can be used in multiple places and will be substituted everywhere.
