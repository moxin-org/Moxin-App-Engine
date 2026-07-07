use std::collections::VecDeque;

use async_trait::async_trait;
use mofa_testing::{
    AgentTest, AgentTestScenario, ParameterExpansionError, ParameterMatrix, ParameterSet,
    ParameterizedScenario, ParameterizedScenarioFile, ScenarioAgent, ScenarioTurnOutput,
    TurnExpectation,
};
use serde_json::json;

// ─── Helper: scripted agent ─────────────────────────────────────────────────

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

// ═══════════════════════════════════════════════════════════════════════════
// ParameterSet tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parameter_set_builder_works() {
    let ps = ParameterSet::new("berlin_celsius")
        .with_var("city", "Berlin")
        .with_var("unit", "celsius");

    assert_eq!(ps.name, "berlin_celsius");
    assert_eq!(ps.get("city"), Some("Berlin"));
    assert_eq!(ps.get("unit"), Some("celsius"));
    assert_eq!(ps.get("missing"), None);
}

#[test]
fn parameter_set_variables_are_ordered() {
    let ps = ParameterSet::new("test")
        .with_var("zebra", "z")
        .with_var("alpha", "a")
        .with_var("mid", "m");

    let keys: Vec<&String> = ps.variables.keys().collect();
    assert_eq!(keys, vec!["alpha", "mid", "zebra"]);
}

// ═══════════════════════════════════════════════════════════════════════════
// ParameterMatrix tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn matrix_expands_two_dimensions() {
    let sets = ParameterMatrix::new()
        .dimension("city", vec!["Berlin", "Tokyo"])
        .dimension("unit", vec!["C", "F"])
        .expand()
        .expect("expansion should succeed");

    assert_eq!(sets.len(), 4);

    // Verify all combinations exist
    let names: Vec<&str> = sets.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Berlin_C"));
    assert!(names.contains(&"Berlin_F"));
    assert!(names.contains(&"Tokyo_C"));
    assert!(names.contains(&"Tokyo_F"));

    // Verify variable bindings
    let berlin_c = sets.iter().find(|s| s.name == "Berlin_C").unwrap();
    assert_eq!(berlin_c.get("city"), Some("Berlin"));
    assert_eq!(berlin_c.get("unit"), Some("C"));
}

#[test]
fn matrix_single_dimension() {
    let sets = ParameterMatrix::new()
        .dimension("lang", vec!["en", "de", "ja"])
        .expand()
        .expect("expansion should succeed");

    assert_eq!(sets.len(), 3);
    assert_eq!(sets[0].name, "en");
    assert_eq!(sets[1].name, "de");
    assert_eq!(sets[2].name, "ja");
}

#[test]
fn matrix_three_dimensions() {
    let sets = ParameterMatrix::new()
        .dimension("a", vec!["1", "2"])
        .dimension("b", vec!["x", "y"])
        .dimension("c", vec!["p", "q"])
        .expand()
        .expect("expansion should succeed");

    assert_eq!(sets.len(), 8); // 2 * 2 * 2
}

#[test]
fn matrix_empty_dimension_fails() {
    let err = ParameterMatrix::new()
        .dimension("city", vec!["Berlin"])
        .dimension("unit", Vec::<String>::new())
        .expand()
        .expect_err("should fail with empty dimension");

    assert!(matches!(
        err,
        ParameterExpansionError::EmptyMatrixDimension { variable } if variable == "unit"
    ));
}

#[test]
fn matrix_no_dimensions_fails() {
    let err = ParameterMatrix::new()
        .expand()
        .expect_err("should fail with no dimensions");

    assert!(matches!(err, ParameterExpansionError::EmptyParameterSets));
}

#[test]
fn matrix_exceeds_limit() {
    let err = ParameterMatrix::new()
        .with_limit(5)
        .dimension("a", vec!["1", "2", "3"])
        .dimension("b", vec!["x", "y", "z"])
        .expand()
        .expect_err("should fail exceeding limit");

    assert!(matches!(
        err,
        ParameterExpansionError::MatrixExpansionLimit {
            requested: 9,
            limit: 5,
        }
    ));
}

#[test]
fn matrix_combination_count() {
    let matrix = ParameterMatrix::new()
        .dimension("a", vec!["1", "2", "3"])
        .dimension("b", vec!["x", "y"]);

    assert_eq!(matrix.combination_count(), 6);
}

// ═══════════════════════════════════════════════════════════════════════════
// ParameterizedScenario: expansion and substitution tests
// ═══════════════════════════════════════════════════════════════════════════

fn make_template_scenario() -> AgentTestScenario {
    AgentTest::new("weather_agent")
        .given_tool("weather_search")
        .when_user_says("What's the weather in {{city}}?")
        .then_agent_should()
        .call_tool("weather_search")
        .respond_containing("{{city}}")
        .build()
        .expect("template should build")
}

#[test]
fn parameterized_expansion_substitutes_placeholders() {
    let template = make_template_scenario();
    let sets = vec![
        ParameterSet::new("berlin").with_var("city", "Berlin"),
        ParameterSet::new("tokyo").with_var("city", "Tokyo"),
    ];

    let param = ParameterizedScenario::new(template, sets);
    let expanded = param.expand().expect("expansion should succeed");

    assert_eq!(expanded.len(), 2);

    // First variant
    assert_eq!(expanded[0].agent_id, "weather_agent[berlin]");
    assert_eq!(
        expanded[0].turns[0].user_input,
        "What's the weather in Berlin?"
    );
    assert!(matches!(
        &expanded[0].turns[0].expectations[1],
        TurnExpectation::RespondContaining { text } if text == "Berlin"
    ));

    // Second variant
    assert_eq!(expanded[1].agent_id, "weather_agent[tokyo]");
    assert_eq!(
        expanded[1].turns[0].user_input,
        "What's the weather in Tokyo?"
    );
}

#[test]
fn parameterized_expansion_with_matrix() {
    let template = AgentTest::new("agent")
        .when_user_says("Hello {{name}} from {{city}}")
        .then_agent_should()
        .respond_containing("{{name}}")
        .build()
        .expect("template should build");

    let sets = ParameterMatrix::new()
        .dimension("name", vec!["Alice", "Bob"])
        .dimension("city", vec!["Berlin", "Tokyo"])
        .expand()
        .expect("matrix should expand");

    let param = ParameterizedScenario::new(template, sets);
    let expanded = param.expand().expect("expansion should succeed");

    assert_eq!(expanded.len(), 4);
}

#[test]
fn parameterized_empty_sets_fails() {
    let template = make_template_scenario();
    let param = ParameterizedScenario::new(template, vec![]);
    let err = param.expand().expect_err("should fail with empty sets");

    assert!(matches!(err, ParameterExpansionError::EmptyParameterSets));
}

#[test]
fn parameterized_missing_variable_fails() {
    let template = make_template_scenario();
    let sets = vec![
        ParameterSet::new("incomplete"), // missing "city"
    ];

    let param = ParameterizedScenario::new(template, sets);
    let err = param.expand().expect_err("should fail with missing variable");

    assert!(matches!(
        err,
        ParameterExpansionError::MissingVariable {
            set_name,
            variable,
        } if set_name == "incomplete" && variable == "city"
    ));
}

#[test]
fn parameterized_case_count() {
    let template = make_template_scenario();
    let sets = vec![
        ParameterSet::new("a").with_var("city", "A"),
        ParameterSet::new("b").with_var("city", "B"),
        ParameterSet::new("c").with_var("city", "C"),
    ];

    let param = ParameterizedScenario::new(template, sets);
    assert_eq!(param.case_count(), 3);
}

#[test]
fn expanded_scenario_names_are_stable() {
    let template = make_template_scenario();
    let sets = vec![
        ParameterSet::new("first").with_var("city", "Berlin"),
        ParameterSet::new("second").with_var("city", "Tokyo"),
    ];

    let param = ParameterizedScenario::new(template.clone(), sets.clone());
    let expanded1 = param.expand().expect("first expansion");

    let param2 = ParameterizedScenario::new(template, sets);
    let expanded2 = param2.expand().expect("second expansion");

    for (a, b) in expanded1.iter().zip(expanded2.iter()) {
        assert_eq!(a.agent_id, b.agent_id);
    }
}

#[test]
fn substitution_works_in_regex_patterns() {
    let template = AgentTest::new("agent")
        .when_user_says("test")
        .then_agent_should()
        .respond_matching_regex("(?i){{keyword}}")
        .build()
        .expect("template should build");

    let sets = vec![ParameterSet::new("greeting").with_var("keyword", "hello")];

    let param = ParameterizedScenario::new(template, sets);
    let expanded = param.expand().expect("expansion should succeed");

    assert!(matches!(
        &expanded[0].turns[0].expectations[0],
        TurnExpectation::RespondMatchingRegex { pattern } if pattern == "(?i)hello"
    ));
}

#[test]
fn substitution_works_in_exact_response() {
    let template = AgentTest::new("agent")
        .when_user_says("test")
        .then_agent_should()
        .respond_exact("Hello, {{name}}!")
        .build()
        .expect("template should build");

    let sets = vec![ParameterSet::new("alice").with_var("name", "Alice")];

    let param = ParameterizedScenario::new(template, sets);
    let expanded = param.expand().expect("expansion should succeed");

    assert!(matches!(
        &expanded[0].turns[0].expectations[0],
        TurnExpectation::RespondExact { text } if text == "Hello, Alice!"
    ));
}

#[test]
fn substitution_works_in_call_tool_with_arguments() {
    let template = AgentTest::new("agent")
        .when_user_says("check {{city}}")
        .then_agent_should()
        .call_tool_with("search", json!({"query": "{{city}}"}))
        .build()
        .expect("template should build");

    let sets = vec![ParameterSet::new("berlin").with_var("city", "Berlin")];

    let param = ParameterizedScenario::new(template, sets);
    let expanded = param.expand().expect("expansion should succeed");

    assert!(matches!(
        &expanded[0].turns[0].expectations[0],
        TurnExpectation::CallToolWith { arguments, .. }
            if arguments == &json!({"query": "Berlin"})
    ));
}

#[test]
fn substitution_works_in_system_prompt() {
    let template = AgentTest::new("agent")
        .given_system_prompt("You help with {{topic}}")
        .when_user_says("help")
        .then_agent_should()
        .respond_containing("{{topic}}")
        .build()
        .expect("template should build");

    let sets = vec![ParameterSet::new("math").with_var("topic", "mathematics")];

    let param = ParameterizedScenario::new(template, sets);
    let expanded = param.expand().expect("expansion should succeed");

    assert_eq!(
        expanded[0].system_prompt.as_deref(),
        Some("You help with mathematics")
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ParameterizedScenario: execution tests
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn parameterized_scenarios_run_and_produce_reports() {
    let template = make_template_scenario();
    let sets = vec![
        ParameterSet::new("berlin").with_var("city", "Berlin"),
        ParameterSet::new("tokyo").with_var("city", "Tokyo"),
    ];

    let param = ParameterizedScenario::new(template, sets);
    let expanded = param.expand().expect("expansion should succeed");

    // Run first variant
    let mut agent1 = ScriptedAgent::new(vec![Ok(
        ScenarioTurnOutput::new("The weather in Berlin is sunny.")
            .with_tool_call("weather_search", json!({})),
    )]);
    let report1 = expanded[0].run_with_agent(&mut agent1).await;
    assert_eq!(report1.passed(), 1);
    assert_eq!(report1.failed(), 0);

    // Run second variant
    let mut agent2 = ScriptedAgent::new(vec![Ok(
        ScenarioTurnOutput::new("The weather in Tokyo is rainy.")
            .with_tool_call("weather_search", json!({})),
    )]);
    let report2 = expanded[1].run_with_agent(&mut agent2).await;
    assert_eq!(report2.passed(), 1);
    assert_eq!(report2.failed(), 0);
}

#[tokio::test]
async fn parameterized_scenario_detects_failure_in_variant() {
    let template = make_template_scenario();
    let sets = vec![ParameterSet::new("berlin").with_var("city", "Berlin")];

    let param = ParameterizedScenario::new(template, sets);
    let expanded = param.expand().expect("expansion should succeed");

    // Agent doesn't call the expected tool
    let mut agent = ScriptedAgent::new(vec![Ok(ScenarioTurnOutput::new(
        "I don't know about Berlin.",
    ))]);

    let report = expanded[0].run_with_agent(&mut agent).await;
    assert_eq!(report.failed(), 1);
    assert!(report.results[0]
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("weather_search"));
}

// ═══════════════════════════════════════════════════════════════════════════
// File-backed parameterized scenario loading
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn load_parameterized_scenario_from_yaml() {
    let yaml = r#"
template:
  agent_id: weather_agent
  tools:
    - weather_search
  turns:
    - user: "What's the weather in {{city}}?"
      expect:
        - kind: call_tool
          name: weather_search
        - kind: respond_containing
          text: "{{city}}"

parameters:
  - name: berlin
    vars:
      city: Berlin
  - name: tokyo
    vars:
      city: Tokyo
"#;

    let param = ParameterizedScenarioFile::from_yaml_str(yaml)
        .expect("yaml parameterized scenario should load");

    assert_eq!(param.case_count(), 2);

    let expanded = param.expand().expect("expansion should succeed");
    assert_eq!(expanded[0].agent_id, "weather_agent[berlin]");
    assert_eq!(expanded[1].agent_id, "weather_agent[tokyo]");
}

#[test]
fn load_parameterized_scenario_with_matrix_from_yaml() {
    let yaml = r#"
template:
  agent_id: weather_agent
  turns:
    - user: "Weather in {{city}} in {{unit}}"
      expect:
        - kind: respond_containing
          text: "{{city}}"

matrix:
  dimensions:
    city:
      - Berlin
      - Tokyo
    unit:
      - celsius
      - fahrenheit
"#;

    let param = ParameterizedScenarioFile::from_yaml_str(yaml)
        .expect("yaml matrix scenario should load");

    assert_eq!(param.case_count(), 4);

    let expanded = param.expand().expect("expansion should succeed");
    assert_eq!(expanded.len(), 4);
}

#[test]
fn load_parameterized_scenario_from_json() {
    let json = r#"{
  "template": {
    "agent_id": "agent",
    "turns": [
      {
        "user": "Hello {{name}}",
        "expect": [
          {"kind": "respond_containing", "text": "{{name}}"}
        ]
      }
    ]
  },
  "parameters": [
    {"name": "alice", "vars": {"name": "Alice"}},
    {"name": "bob", "vars": {"name": "Bob"}}
  ]
}"#;

    let param = ParameterizedScenarioFile::from_json_str(json)
        .expect("json parameterized scenario should load");

    assert_eq!(param.case_count(), 2);
}

#[test]
fn load_parameterized_scenario_from_toml() {
    let toml_input = r#"
[template]
agent_id = "support_agent"

[[template.turns]]
user = "Help with {{topic}}"

[[template.turns.expect]]
kind = "respond_containing"
text = "{{topic}}"

[[parameters]]
name = "billing"

[parameters.vars]
topic = "billing"

[[parameters]]
name = "shipping"

[parameters.vars]
topic = "shipping"
"#;

    let param = ParameterizedScenarioFile::from_toml_str(toml_input)
        .expect("toml parameterized scenario should load");

    assert_eq!(param.case_count(), 2);

    let expanded = param.expand().expect("expansion should succeed");
    assert_eq!(expanded[0].turns[0].user_input, "Help with billing");
    assert_eq!(expanded[1].turns[0].user_input, "Help with shipping");
}

#[test]
fn parameterized_yaml_with_mixed_explicit_and_matrix() {
    let yaml = r#"
template:
  agent_id: agent
  turns:
    - user: "Test {{x}}_{{y}}"
      expect:
        - kind: respond_containing
          text: "{{x}}"

parameters:
  - name: manual_case
    vars:
      x: manual
      y: override

matrix:
  dimensions:
    x:
      - a
      - b
    y:
      - "1"
      - "2"
"#;

    let param = ParameterizedScenarioFile::from_yaml_str(yaml)
        .expect("mixed scenario should load");

    // 1 explicit + 4 matrix = 5
    assert_eq!(param.case_count(), 5);
}

#[test]
fn matrix_limit_in_file_is_respected() {
    let yaml = r#"
template:
  agent_id: agent
  turns:
    - user: "Test {{x}} {{y}} {{z}}"
      expect:
        - kind: respond_containing
          text: "ok"

matrix:
  limit: 5
  dimensions:
    x: ["a", "b", "c"]
    y: ["1", "2", "3"]
    z: ["p", "q", "r"]
"#;

    let err = ParameterizedScenarioFile::from_yaml_str(yaml)
        .expect_err("should fail with matrix limit exceeded");

    assert!(matches!(
        err,
        ParameterExpansionError::MatrixExpansionLimit { .. }
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge cases and robustness
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_placeholders_still_works() {
    let template = AgentTest::new("agent")
        .when_user_says("Hello")
        .then_agent_should()
        .respond_containing("Hi")
        .build()
        .expect("template should build");

    let sets = vec![
        ParameterSet::new("case_a"),
        ParameterSet::new("case_b"),
    ];

    let param = ParameterizedScenario::new(template, sets);
    let expanded = param.expand().expect("expansion without placeholders should succeed");

    assert_eq!(expanded.len(), 2);
    assert_eq!(expanded[0].turns[0].user_input, "Hello");
    assert_eq!(expanded[1].turns[0].user_input, "Hello");
}

#[test]
fn multiple_placeholders_in_single_field() {
    let template = AgentTest::new("agent")
        .when_user_says("From {{city}} to {{destination}} via {{mode}}")
        .then_agent_should()
        .respond_containing("route")
        .build()
        .expect("template should build");

    let sets = vec![ParameterSet::new("trip")
        .with_var("city", "Berlin")
        .with_var("destination", "Tokyo")
        .with_var("mode", "train")];

    let param = ParameterizedScenario::new(template, sets);
    let expanded = param.expand().expect("expansion should succeed");

    assert_eq!(
        expanded[0].turns[0].user_input,
        "From Berlin to Tokyo via train"
    );
}

#[test]
fn same_placeholder_used_multiple_times() {
    let template = AgentTest::new("agent")
        .when_user_says("Tell me about {{city}}")
        .then_agent_should()
        .respond_containing("{{city}}")
        .build()
        .expect("template should build");

    let sets = vec![ParameterSet::new("berlin").with_var("city", "Berlin")];

    let param = ParameterizedScenario::new(template, sets);
    let expanded = param.expand().expect("expansion should succeed");

    assert_eq!(expanded[0].turns[0].user_input, "Tell me about Berlin");
    assert!(matches!(
        &expanded[0].turns[0].expectations[0],
        TurnExpectation::RespondContaining { text } if text == "Berlin"
    ));
}

#[test]
fn parameter_set_serialization_roundtrip() {
    let ps = ParameterSet::new("test")
        .with_var("city", "Berlin")
        .with_var("unit", "C");

    let json = serde_json::to_string(&ps).expect("serialize");
    let deserialized: ParameterSet = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(ps.name, deserialized.name);
    assert_eq!(ps.variables, deserialized.variables);
}
