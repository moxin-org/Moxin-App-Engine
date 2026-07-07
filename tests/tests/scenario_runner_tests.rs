use mofa_foundation::agent::components::tool::SimpleTool;
use mofa_kernel::agent::components::tool::ToolInput;
use mofa_kernel::bus::CommunicationMode;
use mofa_kernel::message::AgentMessage;
use mofa_testing::{ScenarioRunner, ScenarioSpec, TestStatus};
use serde_json::json;

#[test]
fn scenario_spec_parses_from_yaml() {
    let input = r#"
suite_name: yaml-scenario
clock:
  start_ms: 42000
llm:
  model_name: test-model
  responses:
    - prompt_substring: summarize
      response: compact summary
tools:
  - name: search
    description: Search docs
    schema:
      type: object
    stubbed_result:
      text: Found it
expectations:
  infer_total: 1
"#;

    let spec = ScenarioSpec::from_yaml_str(input).unwrap();
    assert_eq!(spec.suite_name, "yaml-scenario");
    assert_eq!(spec.clock.start_ms, Some(42_000));
    assert_eq!(spec.llm.model_name.as_deref(), Some("test-model"));
    assert_eq!(spec.tools.len(), 1);
    assert_eq!(spec.expectations.infer_total, Some(1));
}

#[test]
fn scenario_spec_parses_from_json() {
    let input = r#"
{
  "suite_name": "json-scenario",
  "llm": {
    "responses": [
      { "prompt_substring": "translate", "response": "bonjour" }
    ]
  }
}
"#;

    let spec = ScenarioSpec::from_json_str(input).unwrap();
    assert_eq!(spec.suite_name, "json-scenario");
    assert_eq!(spec.llm.responses.len(), 1);
    assert_eq!(spec.llm.responses[0].response, "bonjour");
}

#[tokio::test]
async fn scenario_runner_configures_fixtures_and_expectations() {
    let spec = ScenarioSpec::from_yaml_str(
        r#"
suite_name: end-to-end-scenario
clock:
  start_ms: 2500
llm:
  model_name: scenario-model
  responses:
    - prompt_substring: summarize
      response: "Short summary"
tools:
  - name: search
    description: Search docs
    schema:
      type: object
    stubbed_result:
      text: "Found result"
expectations:
  infer_total: 1
  prompt_counts:
    - substring: summarize
      expected: 1
  tool_calls:
    - tool_name: search
      expected: 1
  bus_messages_from:
    - sender_id: evaluator
      expected: 1
"#,
    )
    .unwrap();

    let report = ScenarioRunner::new(spec)
        .run(|ctx| async move {
            let response = ctx.infer("summarize this").await.unwrap();
            assert_eq!(response, "Short summary");

            let tool = ctx.tool("search").unwrap();
            let result = tool
                .execute(ToolInput::from_json(json!({"query": "rust"})))
                .await;
            assert!(result.success);

            let message = AgentMessage::TaskRequest {
                task_id: "t1".into(),
                content: "done".into(),
            };
            let _ = ctx
                .bus
                .send_and_capture("evaluator", CommunicationMode::Broadcast, message)
                .await;

            Ok(())
        })
        .await
        .unwrap();

    assert_eq!(report.timestamp, 2500);
    assert_eq!(report.total(), 5);
    assert_eq!(report.failed(), 0);
    assert_eq!(report.passed(), 5);
}

#[tokio::test]
async fn scenario_runner_records_failed_expectations() {
    let spec = ScenarioSpec::from_yaml_str(
        r#"
suite_name: failing-expectation-scenario
llm:
  responses:
    - prompt_substring: summarize
      response: "Short summary"
expectations:
  infer_total: 2
  prompt_counts:
    - substring: summarize
      expected: 2
"#,
    )
    .unwrap();

    let report = ScenarioRunner::new(spec)
        .run(|ctx| async move {
            let _ = ctx.infer("summarize this").await.unwrap();
            Ok(())
        })
        .await
        .unwrap();

    assert_eq!(report.total(), 3);
    assert_eq!(report.passed(), 1);
    assert_eq!(report.failed(), 2);
    assert_eq!(report.results[1].status, TestStatus::Failed);
    assert_eq!(report.results[2].status, TestStatus::Failed);
}

#[tokio::test]
async fn scenario_runner_applies_fail_next_and_reports_execution_failure() {
    let spec = ScenarioSpec::from_yaml_str(
        r#"
suite_name: fail-next-scenario
llm:
  fail_next:
    - count: 1
      error: "boom"
expectations:
  infer_total: 1
"#,
    )
    .unwrap();

    let report = ScenarioRunner::new(spec)
        .run(|ctx| async move {
            ctx.infer("anything")
                .await
                .map(|_| ())
                .map_err(|err| err.to_string())
        })
        .await
        .unwrap();

    assert_eq!(report.total(), 2);
    assert_eq!(report.failed(), 1);
    assert_eq!(report.passed(), 1);
    assert_eq!(report.results[0].status, TestStatus::Failed);
    assert_eq!(report.results[1].status, TestStatus::Passed);
}
