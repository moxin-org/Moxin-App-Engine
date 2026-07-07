use std::collections::VecDeque;

use async_trait::async_trait;
use mofa_testing::{
    AgentTest, AgentTestScenario, MockLLMBackend, MockScenarioAgent, MockTool, ScenarioAgent,
    ScenarioTurnOutput, TurnExpectation,
};
use serde_json::json;

struct ScriptedAgent {
    outputs: VecDeque<Result<ScenarioTurnOutput, String>>,
}

impl ScriptedAgent {
    fn new(outputs: Vec<Result<ScenarioTurnOutput, String>>) -> Self {
        Self {
            outputs: outputs.into(),
        }
    }
}

#[async_trait]
impl ScenarioAgent for ScriptedAgent {
    async fn execute_turn(
        &mut self,
        _system_prompt: Option<&str>,
        _user_input: &str,
    ) -> Result<ScenarioTurnOutput, String> {
        self.outputs
            .pop_front()
            .unwrap_or_else(|| Err("no scripted output available".to_string()))
    }
}

#[tokio::test]
async fn agent_test_builder_runs_multi_turn_scenario() {
    let scenario = AgentTest::new("customer_support_agent")
        .given_system_prompt("You are a helpful assistant")
        .given_tool("weather_search")
        .when_user_says("What's the weather?")
        .then_agent_should()
        .call_tool("weather_search")
        .respond_containing("temperature")
        .when_user_says("Thanks!")
        .then_agent_should()
        .not_call_any_tool()
        .respond_matching_regex("(?i)welcome|happy to help")
        .build()
        .expect("scenario should build");

    let mut agent = ScriptedAgent::new(vec![
        Ok(ScenarioTurnOutput::new("The temperature is 22C.")
            .with_tool_call("weather_search", json!({"city":"Berlin"}))),
        Ok(ScenarioTurnOutput::new("You're welcome, happy to help.")),
    ]);

    let report = scenario.run_with_agent(&mut agent).await;

    assert_eq!(report.total(), 2);
    assert_eq!(report.failed(), 0);
    assert_eq!(report.passed(), 2);
}

#[test]
fn builder_reports_invalid_ordering_errors() {
    let err = AgentTest::new("agent")
        .then_agent_should()
        .call_tool("search")
        .build()
        .expect_err("build should fail when expectations are declared before turn");

    assert!(
        err.errors
            .iter()
            .any(|e| e.contains("call_tool() must be called after when_user_says()"))
    );
}

#[test]
fn load_scenario_from_yaml() {
    let yaml = r#"
agent_id: customer_support_agent
system_prompt: You are a helpful assistant
tools:
  - weather_search
turns:
  - user: What's the weather?
    expect:
      - kind: call_tool
        name: weather_search
      - kind: respond_containing
        text: temperature
  - user: Thanks!
    expect:
      - kind: not_call_any_tool
      - kind: respond_matching_regex
        pattern: "(?i)welcome|help"
"#;

    let scenario = AgentTestScenario::from_yaml_str(yaml).expect("yaml scenario should load");

    assert_eq!(scenario.agent_id, "customer_support_agent");
    assert_eq!(scenario.turns.len(), 2);
    assert!(matches!(
        scenario.turns[0].expectations[0],
        TurnExpectation::CallTool { .. }
    ));
}

#[test]
fn load_scenario_from_toml() {
    let toml_input = r#"
agent_id = "customer_support_agent"
system_prompt = "You are a helpful assistant"
tools = ["weather_search"]

[[turns]]
user = "What's the weather?"

[[turns.expect]]
kind = "call_tool"
name = "weather_search"

[[turns.expect]]
kind = "respond_containing"
text = "temperature"
"#;

    let scenario = AgentTestScenario::from_toml_str(toml_input).expect("toml scenario should load");

    assert_eq!(scenario.turns.len(), 1);
    assert_eq!(scenario.turns[0].expectations.len(), 2);
}

#[tokio::test]
async fn mock_scenario_agent_integrates_backend_and_tools() {
    let backend = MockLLMBackend::new();
    backend.add_response("weather", "The temperature is 22C in Berlin.");

    let tool = MockTool::new(
        "weather_search",
        "Lookup weather",
        json!({
            "type": "object",
            "properties": {
                "city": {"type": "string"}
            },
            "required": ["city"]
        }),
    );

    let mut agent = MockScenarioAgent::new("mock-model", backend)
        .with_mock_tool(tool.clone())
        .add_tool_rule("weather", "weather_search", json!({"city": "Berlin"}));

    let scenario = AgentTest::new("weather_agent")
        .given_mock_tool(&tool)
        .when_user_says("Can you check the weather?")
        .then_agent_should()
        .call_tool_with("weather_search", json!({"city": "Berlin"}))
        .respond_containing("temperature")
        .build()
        .expect("scenario should build");

    let report = scenario.run_with_agent(&mut agent).await;

    assert_eq!(report.failed(), 0);
    assert_eq!(tool.call_count().await, 1);
    let first_call = tool.nth_call(0).await.expect("first call should exist");
    assert_eq!(first_call.arguments, json!({"city": "Berlin"}));
}

#[tokio::test]
async fn scenario_expectation_failure_is_reported() {
    let scenario = AgentTest::new("agent")
        .when_user_says("hello")
        .then_agent_should()
        .respond_exact("expected")
        .build()
        .expect("scenario should build");

    let mut agent = ScriptedAgent::new(vec![Ok(ScenarioTurnOutput::new("actual"))]);

    let report = scenario.run_with_agent(&mut agent).await;

    assert_eq!(report.total(), 1);
    assert_eq!(report.failed(), 1);
    assert!(
        report.results[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("expected exact response")
    );
}
