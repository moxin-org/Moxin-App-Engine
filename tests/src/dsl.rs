//! Declarative agent testing DSL and scenario runner.
//!
//! This module provides:
//! - A fluent builder (`AgentTest`) for multi-turn scenarios
//! - YAML/TOML scenario loading
//! - Expectation evaluation for responses and tool calls
//! - A mock harness (`MockScenarioAgent`) that integrates with `MockLLMBackend`
//!   and `MockTool`

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::Instant;

use async_trait::async_trait;
use mofa_foundation::agent::components::tool::SimpleTool;
use mofa_foundation::orchestrator::ModelOrchestrator;
use mofa_kernel::agent::components::tool::ToolInput;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::backend::MockLLMBackend;
use crate::report::{TestCaseResult, TestReport, TestReportBuilder, TestStatus};
use crate::tools::MockTool;

/// Builder-level validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioBuildError {
    pub errors: Vec<String>,
}

impl ScenarioBuildError {
    pub fn new(errors: Vec<String>) -> Self {
        Self { errors }
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }
}

impl Display for ScenarioBuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "scenario build failed: {}", self.errors.join("; "))
    }
}

impl Error for ScenarioBuildError {}

/// Scenario loading error (JSON/YAML/TOML parse + validation).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScenarioLoadError {
    Json(String),
    Yaml(String),
    Toml(String),
    Validation(Vec<String>),
}

impl Display for ScenarioLoadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(msg) => write!(f, "json parse error: {msg}"),
            Self::Yaml(msg) => write!(f, "yaml parse error: {msg}"),
            Self::Toml(msg) => write!(f, "toml parse error: {msg}"),
            Self::Validation(errors) => {
                write!(f, "scenario validation error: {}", errors.join("; "))
            }
        }
    }
}

impl Error for ScenarioLoadError {}

/// Single tool call record observed in a turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub name: String,
    pub arguments: Value,
}

/// Agent output observed for one scenario turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioTurnOutput {
    pub response: String,
    pub tool_calls: Vec<ToolCallRecord>,
}

impl ScenarioTurnOutput {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            tool_calls: Vec::new(),
        }
    }

    pub fn with_tool_call(mut self, name: impl Into<String>, arguments: Value) -> Self {
        self.tool_calls.push(ToolCallRecord {
            name: name.into(),
            arguments,
        });
        self
    }
}

/// Agent adapter trait used by the scenario runner.
#[async_trait]
pub trait ScenarioAgent: Send {
    async fn execute_turn(
        &mut self,
        system_prompt: Option<&str>,
        user_input: &str,
    ) -> Result<ScenarioTurnOutput, String>;
}

/// Turn expectation primitives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TurnExpectation {
    CallTool { name: String },
    CallToolWith { name: String, arguments: Value },
    NotCallAnyTool,
    RespondContaining { text: String },
    RespondExact { text: String },
    RespondMatchingRegex { pattern: String },
}

/// One conversational turn in a scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioTurn {
    pub user_input: String,
    pub expectations: Vec<TurnExpectation>,
}

/// Declarative scenario for testing an agent's behavior.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTestScenario {
    pub agent_id: String,
    pub system_prompt: Option<String>,
    pub tools: Vec<String>,
    pub turns: Vec<ScenarioTurn>,
}

impl AgentTestScenario {
    /// Parse a scenario from JSON.
    pub fn from_json_str(input: &str) -> Result<Self, ScenarioLoadError> {
        let parsed: ScenarioFile =
            serde_json::from_str(input).map_err(|err| ScenarioLoadError::Json(err.to_string()))?;
        let scenario = Self::from(parsed);
        scenario
            .validate()
            .map_err(|err| ScenarioLoadError::Validation(err.errors))?;
        Ok(scenario)
    }

    /// Parse a scenario from YAML.
    pub fn from_yaml_str(input: &str) -> Result<Self, ScenarioLoadError> {
        let parsed: ScenarioFile =
            serde_yaml::from_str(input).map_err(|err| ScenarioLoadError::Yaml(err.to_string()))?;
        let scenario = Self::from(parsed);
        scenario
            .validate()
            .map_err(|err| ScenarioLoadError::Validation(err.errors))?;
        Ok(scenario)
    }

    /// Parse a scenario from TOML.
    pub fn from_toml_str(input: &str) -> Result<Self, ScenarioLoadError> {
        let parsed: ScenarioFile =
            toml::from_str(input).map_err(|err| ScenarioLoadError::Toml(err.to_string()))?;
        let scenario = Self::from(parsed);
        scenario
            .validate()
            .map_err(|err| ScenarioLoadError::Validation(err.errors))?;
        Ok(scenario)
    }

    /// Validate scenario shape and expectation payloads.
    pub fn validate(&self) -> Result<(), ScenarioBuildError> {
        let mut errors = Vec::new();

        if self.agent_id.trim().is_empty() {
            errors.push("agent_id must not be empty".to_string());
        }

        if self.turns.is_empty() {
            errors.push("scenario must include at least one turn".to_string());
        }

        for (idx, turn) in self.turns.iter().enumerate() {
            if turn.user_input.trim().is_empty() {
                errors.push(format!("turn {} has empty user_input", idx + 1));
            }

            for expectation in &turn.expectations {
                match expectation {
                    TurnExpectation::CallTool { name }
                    | TurnExpectation::CallToolWith { name, .. } => {
                        if name.trim().is_empty() {
                            errors.push(format!(
                                "turn {} contains a tool expectation with empty tool name",
                                idx + 1
                            ));
                        }
                    }
                    TurnExpectation::RespondContaining { text }
                    | TurnExpectation::RespondExact { text } => {
                        if text.is_empty() {
                            errors.push(format!(
                                "turn {} contains an empty response text expectation",
                                idx + 1
                            ));
                        }
                    }
                    TurnExpectation::RespondMatchingRegex { pattern } => {
                        if let Err(err) = Regex::new(pattern) {
                            errors.push(format!(
                                "turn {} has invalid regex '{}': {}",
                                idx + 1,
                                pattern,
                                err
                            ));
                        }
                    }
                    TurnExpectation::NotCallAnyTool => {}
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ScenarioBuildError::new(errors))
        }
    }

    /// Execute the scenario and produce a `TestReport`.
    ///
    /// If scenario validation fails, the report contains one failed test case
    /// with the validation details.
    pub async fn run_with_agent<A: ScenarioAgent>(&self, agent: &mut A) -> TestReport {
        match self.run_with_agent_checked(agent).await {
            Ok(report) => report,
            Err(err) => TestReportBuilder::new(format!("scenario:{}", self.agent_id))
                .add_result(TestCaseResult {
                    name: "scenario_validation".to_string(),
                    status: TestStatus::Failed,
                    duration: std::time::Duration::from_secs(0),
                    error: Some(err.to_string()),
                    metadata: vec![],
                })
                .build(),
        }
    }

    /// Execute the scenario and return a validated report.
    pub async fn run_with_agent_checked<A: ScenarioAgent>(
        &self,
        agent: &mut A,
    ) -> Result<TestReport, ScenarioBuildError> {
        self.validate()?;

        let mut builder = TestReportBuilder::new(format!("scenario:{}", self.agent_id));

        for (idx, turn) in self.turns.iter().enumerate() {
            let test_name = format!("turn_{}_{}", idx + 1, sanitize_name(&turn.user_input));
            let start = Instant::now();

            let result = agent
                .execute_turn(self.system_prompt.as_deref(), &turn.user_input)
                .await;

            let (status, error, metadata) = match result {
                Ok(output) => {
                    let failures = evaluate_expectations(&turn.expectations, &output);
                    let mut metadata = vec![
                        ("user_input".to_string(), turn.user_input.clone()),
                        ("response".to_string(), output.response.clone()),
                        (
                            "tool_call_count".to_string(),
                            output.tool_calls.len().to_string(),
                        ),
                    ];
                    if !output.tool_calls.is_empty() {
                        let names = output
                            .tool_calls
                            .iter()
                            .map(|call| call.name.clone())
                            .collect::<Vec<_>>()
                            .join(",");
                        metadata.push(("tool_calls".to_string(), names));
                    }

                    if failures.is_empty() {
                        (TestStatus::Passed, None, metadata)
                    } else {
                        (TestStatus::Failed, Some(failures.join("; ")), metadata)
                    }
                }
                Err(err) => {
                    let metadata = vec![("user_input".to_string(), turn.user_input.clone())];
                    (TestStatus::Failed, Some(err), metadata)
                }
            };

            builder = builder.add_result(TestCaseResult {
                name: test_name,
                status,
                duration: start.elapsed(),
                error,
                metadata,
            });
        }

        Ok(builder.build())
    }
}

fn evaluate_expectations(
    expectations: &[TurnExpectation],
    output: &ScenarioTurnOutput,
) -> Vec<String> {
    let mut failures = Vec::new();

    for expectation in expectations {
        match expectation {
            TurnExpectation::CallTool { name } => {
                if !output.tool_calls.iter().any(|call| call.name == *name) {
                    failures.push(format!("expected tool '{}' to be called", name));
                }
            }
            TurnExpectation::CallToolWith { name, arguments } => {
                if !output
                    .tool_calls
                    .iter()
                    .any(|call| call.name == *name && call.arguments == *arguments)
                {
                    failures.push(format!(
                        "expected tool '{}' to be called with arguments {}",
                        name, arguments
                    ));
                }
            }
            TurnExpectation::NotCallAnyTool => {
                if !output.tool_calls.is_empty() {
                    let called = output
                        .tool_calls
                        .iter()
                        .map(|call| call.name.clone())
                        .collect::<Vec<_>>()
                        .join(",");
                    failures.push(format!(
                        "expected no tool calls, but got {} call(s): {}",
                        output.tool_calls.len(),
                        called
                    ));
                }
            }
            TurnExpectation::RespondContaining { text } => {
                if !output.response.contains(text) {
                    failures.push(format!(
                        "expected response to contain '{}', but got '{}'",
                        text, output.response
                    ));
                }
            }
            TurnExpectation::RespondExact { text } => {
                if output.response != *text {
                    failures.push(format!(
                        "expected exact response '{}', but got '{}'",
                        text, output.response
                    ));
                }
            }
            TurnExpectation::RespondMatchingRegex { pattern } => match Regex::new(pattern) {
                Ok(regex) => {
                    if !regex.is_match(&output.response) {
                        failures.push(format!(
                            "expected response to match regex '{}', but got '{}'",
                            pattern, output.response
                        ));
                    }
                }
                Err(err) => failures.push(format!("invalid regex '{}': {}", pattern, err)),
            },
        }
    }

    failures
}

fn sanitize_name(input: &str) -> String {
    let mut name = input
        .chars()
        .take(24)
        .map(|ch| if ch.is_alphanumeric() { ch } else { '_' })
        .collect::<String>();

    while name.ends_with('_') {
        name.pop();
    }

    if name.is_empty() {
        "turn".to_string()
    } else {
        name
    }
}

/// Fluent builder for `AgentTestScenario`.
#[derive(Debug, Clone)]
pub struct AgentTest {
    scenario: AgentTestScenario,
    pending_user_input: Option<String>,
    pending_expectations: Vec<TurnExpectation>,
    build_errors: Vec<String>,
}

impl AgentTest {
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            scenario: AgentTestScenario {
                agent_id: agent_id.into(),
                system_prompt: None,
                tools: Vec::new(),
                turns: Vec::new(),
            },
            pending_user_input: None,
            pending_expectations: Vec::new(),
            build_errors: Vec::new(),
        }
    }

    pub fn given_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.scenario.system_prompt = Some(prompt.into());
        self
    }

    pub fn given_tool(mut self, tool_name: impl Into<String>) -> Self {
        self.scenario.tools.push(tool_name.into());
        self
    }

    /// Convenience method that registers a `MockTool` name in the scenario.
    pub fn given_mock_tool(mut self, tool: &MockTool) -> Self {
        self.scenario.tools.push(tool.name().to_string());
        self
    }

    pub fn when_user_says(mut self, user_input: impl Into<String>) -> Self {
        self.flush_pending_turn();
        self.pending_user_input = Some(user_input.into());
        self
    }

    pub fn then_agent_should(self) -> Self {
        self
    }

    pub fn call_tool(mut self, tool_name: impl Into<String>) -> Self {
        if self.ensure_active_turn("call_tool") {
            self.pending_expectations.push(TurnExpectation::CallTool {
                name: tool_name.into(),
            });
        }
        self
    }

    pub fn call_tool_with(mut self, tool_name: impl Into<String>, arguments: Value) -> Self {
        if self.ensure_active_turn("call_tool_with") {
            self.pending_expectations
                .push(TurnExpectation::CallToolWith {
                    name: tool_name.into(),
                    arguments,
                });
        }
        self
    }

    pub fn not_call_any_tool(mut self) -> Self {
        if self.ensure_active_turn("not_call_any_tool") {
            self.pending_expectations
                .push(TurnExpectation::NotCallAnyTool);
        }
        self
    }

    pub fn respond_containing(mut self, text: impl Into<String>) -> Self {
        if self.ensure_active_turn("respond_containing") {
            self.pending_expectations
                .push(TurnExpectation::RespondContaining { text: text.into() });
        }
        self
    }

    pub fn respond_exact(mut self, text: impl Into<String>) -> Self {
        if self.ensure_active_turn("respond_exact") {
            self.pending_expectations
                .push(TurnExpectation::RespondExact { text: text.into() });
        }
        self
    }

    pub fn respond_matching_regex(mut self, pattern: impl Into<String>) -> Self {
        if self.ensure_active_turn("respond_matching_regex") {
            self.pending_expectations
                .push(TurnExpectation::RespondMatchingRegex {
                    pattern: pattern.into(),
                });
        }
        self
    }

    pub fn build(mut self) -> Result<AgentTestScenario, ScenarioBuildError> {
        self.flush_pending_turn();

        let mut errors = self.build_errors;
        if self.scenario.turns.is_empty() {
            errors.push("scenario must include at least one turn".to_string());
        }

        if let Err(validation) = self.scenario.validate() {
            errors.extend(validation.errors);
        }

        if errors.is_empty() {
            Ok(self.scenario)
        } else {
            Err(ScenarioBuildError::new(errors))
        }
    }

    fn ensure_active_turn(&mut self, method_name: &str) -> bool {
        if self.pending_user_input.is_none() {
            self.build_errors.push(format!(
                "{}() must be called after when_user_says()",
                method_name
            ));
            false
        } else {
            true
        }
    }

    fn flush_pending_turn(&mut self) {
        if let Some(user_input) = self.pending_user_input.take() {
            let expectations = std::mem::take(&mut self.pending_expectations);
            self.scenario.turns.push(ScenarioTurn {
                user_input,
                expectations,
            });
        }
    }
}

/// Rule defining when a `MockTool` should be invoked by `MockScenarioAgent`.
#[derive(Debug, Clone)]
pub struct ToolInvocationRule {
    pub prompt_substring: String,
    pub tool_name: String,
    pub arguments: Value,
}

/// A practical scenario agent harness that combines `MockLLMBackend` and `MockTool`.
///
/// Each turn:
/// 1. Calls `backend.infer(model_id, user_input)` to generate response text.
/// 2. Executes tools for all matching `ToolInvocationRule` prompt patterns.
/// 3. Emits `ToolCallRecord` entries for expectation validation.
pub struct MockScenarioAgent {
    model_id: String,
    backend: MockLLMBackend,
    tools: HashMap<String, MockTool>,
    tool_rules: Vec<ToolInvocationRule>,
}

impl MockScenarioAgent {
    pub fn new(model_id: impl Into<String>, backend: MockLLMBackend) -> Self {
        Self {
            model_id: model_id.into(),
            backend,
            tools: HashMap::new(),
            tool_rules: Vec::new(),
        }
    }

    pub fn with_mock_tool(mut self, tool: MockTool) -> Self {
        self.tools.insert(tool.name().to_string(), tool);
        self
    }

    pub fn add_tool_rule(
        mut self,
        prompt_substring: impl Into<String>,
        tool_name: impl Into<String>,
        arguments: Value,
    ) -> Self {
        self.tool_rules.push(ToolInvocationRule {
            prompt_substring: prompt_substring.into(),
            tool_name: tool_name.into(),
            arguments,
        });
        self
    }
}

#[async_trait]
impl ScenarioAgent for MockScenarioAgent {
    async fn execute_turn(
        &mut self,
        _system_prompt: Option<&str>,
        user_input: &str,
    ) -> Result<ScenarioTurnOutput, String> {
        let response = self
            .backend
            .infer(&self.model_id, user_input)
            .await
            .map_err(|err| err.to_string())?;

        let mut tool_calls = Vec::new();

        for rule in &self.tool_rules {
            if !user_input.contains(&rule.prompt_substring) {
                continue;
            }

            let tool = self.tools.get(&rule.tool_name).ok_or_else(|| {
                format!(
                    "tool '{}' not registered in MockScenarioAgent",
                    rule.tool_name
                )
            })?;

            let input = ToolInput::from_json(rule.arguments.clone());
            let result = tool.execute(input).await;

            if !result.success {
                return Err(format!(
                    "tool '{}' failed: {}",
                    rule.tool_name,
                    result
                        .error
                        .unwrap_or_else(|| "unknown tool error".to_string())
                ));
            }

            tool_calls.push(ToolCallRecord {
                name: rule.tool_name.clone(),
                arguments: rule.arguments.clone(),
            });
        }

        Ok(ScenarioTurnOutput {
            response,
            tool_calls,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ScenarioFile {
    agent_id: String,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    tools: Vec<String>,
    turns: Vec<ScenarioFileTurn>,
}

#[derive(Debug, Clone, Deserialize)]
struct ScenarioFileTurn {
    user: String,
    #[serde(default, alias = "expectations")]
    expect: Vec<ScenarioFileExpectation>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ScenarioFileExpectation {
    CallTool { name: String },
    CallToolWith { name: String, arguments: Value },
    NotCallAnyTool,
    RespondContaining { text: String },
    RespondExact { text: String },
    RespondMatchingRegex { pattern: String },
}

impl From<ScenarioFileExpectation> for TurnExpectation {
    fn from(value: ScenarioFileExpectation) -> Self {
        match value {
            ScenarioFileExpectation::CallTool { name } => TurnExpectation::CallTool { name },
            ScenarioFileExpectation::CallToolWith { name, arguments } => {
                TurnExpectation::CallToolWith { name, arguments }
            }
            ScenarioFileExpectation::NotCallAnyTool => TurnExpectation::NotCallAnyTool,
            ScenarioFileExpectation::RespondContaining { text } => {
                TurnExpectation::RespondContaining { text }
            }
            ScenarioFileExpectation::RespondExact { text } => {
                TurnExpectation::RespondExact { text }
            }
            ScenarioFileExpectation::RespondMatchingRegex { pattern } => {
                TurnExpectation::RespondMatchingRegex { pattern }
            }
        }
    }
}

impl From<ScenarioFile> for AgentTestScenario {
    fn from(value: ScenarioFile) -> Self {
        Self {
            agent_id: value.agent_id,
            system_prompt: value.system_prompt,
            tools: value.tools,
            turns: value
                .turns
                .into_iter()
                .map(|turn| ScenarioTurn {
                    user_input: turn.user,
                    expectations: turn.expect.into_iter().map(Into::into).collect(),
                })
                .collect(),
        }
    }
}
