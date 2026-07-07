//! Minimal TOML DSL support for the testing MVP.
//!
//! This module keeps the schema intentionally small so contributors can define
//! simple agent tests without introducing a full DSL framework yet.

use crate::agent_runner::{AgentRunResult, AgentRunnerError, AgentTestRunner};
use crate::tools::MockTool;
use mofa_foundation::agent::context::prompt::AgentIdentity;
use mofa_kernel::agent::components::tool::ToolResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DslError {
    #[error("failed to read DSL file: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse TOML DSL: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("runner error: {0}")]
    Runner(#[from] AgentRunnerError),

    #[error("test case must define either `prompt` or `input`")]
    MissingPrompt,

    #[error("expected output to contain `{expected}`, got `{actual}`")]
    ExpectedContains { expected: String, actual: String },

    #[error("expected tool `{tool}` to be called, found tool calls: {actual:?}")]
    ExpectedToolCall { tool: String, actual: Vec<String> },

    #[error("run produced no text output")]
    MissingOutput,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TestCaseDsl {
    pub name: String,
    pub prompt: Option<String>,
    pub input: Option<String>,
    pub expected_text: Option<String>,
    #[serde(default)]
    pub bootstrap_files: Vec<BootstrapFileDsl>,
    pub agent: Option<AgentDsl>,
    #[serde(default)]
    pub tools: Vec<ToolDsl>,
    pub llm: Option<LlmDsl>,
    #[serde(rename = "assert")]
    pub assertions: Option<AssertDsl>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BootstrapFileDsl {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentDsl {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolDsl {
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub result: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmDsl {
    #[serde(default)]
    pub responses: Vec<String>,
    #[serde(default)]
    pub steps: Vec<LlmStepDsl>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmStepDsl {
    #[serde(rename = "type")]
    pub kind: LlmStepKind,
    pub content: Option<String>,
    pub tool: Option<String>,
    pub arguments: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmStepKind {
    Text,
    ToolCall,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssertDsl {
    pub contains: Option<String>,
    pub tool_called: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertionOutcome {
    pub kind: String,
    pub expected: Value,
    pub actual: Value,
    pub passed: bool,
}

impl TestCaseDsl {
    pub fn from_toml_str(input: &str) -> Result<Self, DslError> {
        Ok(toml::from_str(input)?)
    }

    pub fn from_toml_file(path: impl AsRef<Path>) -> Result<Self, DslError> {
        let input = std::fs::read_to_string(path)?;
        Self::from_toml_str(&input)
    }

    fn execution_input(&self) -> Result<&str, DslError> {
        self.prompt
            .as_deref()
            .or(self.input.as_deref())
            .ok_or(DslError::MissingPrompt)
    }
}

pub async fn run_test_case(case: &TestCaseDsl) -> Result<AgentRunResult, DslError> {
    let result = execute_test_case(case).await?;
    let assertions = collect_assertion_outcomes(case, &result);
    if let Some(error) = assertion_error_from_outcomes(&assertions) {
        return Err(error);
    }
    Ok(result)
}

pub async fn execute_test_case(case: &TestCaseDsl) -> Result<AgentRunResult, DslError> {
    let mut runner = AgentTestRunner::new().await?;
    configure_runner_from_test_case(case, &mut runner).await?;
    let result = runner.run_text(case.execution_input()?).await?;
    runner.shutdown().await?;
    Ok(result)
}

pub async fn configure_runner_from_test_case(
    case: &TestCaseDsl,
    runner: &mut AgentTestRunner,
) -> Result<(), DslError> {
    if !case.bootstrap_files.is_empty() {
        let mut bootstrap_paths = Vec::with_capacity(case.bootstrap_files.len());
        for file in &case.bootstrap_files {
            runner.write_bootstrap_file(&file.path, &file.content)?;
            bootstrap_paths.push(file.path.clone());
        }
        runner
            .configure_prompt(agent_identity(case.agent.as_ref()), Some(bootstrap_paths))
            .await;
    } else if case.agent.is_some() {
        runner
            .configure_prompt(agent_identity(case.agent.as_ref()), None)
            .await;
    }

    for tool in &case.tools {
        let mock_tool = MockTool::new(&tool.name, &tool.description, tool.schema.clone());
        if let Some(result) = &tool.result {
            mock_tool
                .set_result(ToolResult::success(result.clone()))
                .await;
        }
        runner.register_mock_tool(mock_tool).await?;
    }

    // Queue deterministic LLM responses before execution so the DSL stays a thin
    // adapter over the existing runner harness.
    if let Some(llm) = &case.llm {
        if !llm.steps.is_empty() {
            for step in &llm.steps {
                match step.kind {
                    LlmStepKind::Text => {
                        runner
                            .mock_llm()
                            .add_response(step.content.clone().unwrap_or_default())
                            .await;
                    }
                    LlmStepKind::ToolCall => {
                        runner
                            .mock_llm()
                            .add_tool_call_response(
                                step.tool.as_deref().unwrap_or_default(),
                                step.arguments.clone().unwrap_or(Value::Null),
                                step.content.clone(),
                            )
                            .await;
                    }
                }
            }
        } else {
            for response in &llm.responses {
                runner.mock_llm().add_response(response).await;
            }
        }
    }
    Ok(())
}

fn agent_identity(agent: Option<&AgentDsl>) -> Option<AgentIdentity> {
    let agent = agent?;
    let name = agent.name.clone()?;
    Some(AgentIdentity {
        name,
        description: agent.description.clone().unwrap_or_default(),
        icon: None,
    })
}

fn expected_contains(case: &TestCaseDsl) -> Option<&str> {
    // Prefer the explicit assertion block when present, while keeping
    // `expected_text` as a lightweight shorthand for the MVP schema.
    case.assertions
        .as_ref()
        .and_then(|assertions| assertions.contains.as_deref())
        .or(case.expected_text.as_deref())
}

fn expected_tool_call(case: &TestCaseDsl) -> Option<&str> {
    case.assertions
        .as_ref()
        .and_then(|assertions| assertions.tool_called.as_deref())
}

pub fn collect_assertion_outcomes(case: &TestCaseDsl, result: &AgentRunResult) -> Vec<AssertionOutcome> {
    let mut outcomes = Vec::new();

    if let Some(expected) = expected_contains(case) {
        let actual = result.output_text();
        outcomes.push(AssertionOutcome {
            kind: "contains".to_string(),
            expected: Value::String(expected.to_string()),
            actual: actual
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
            passed: actual
                .as_ref()
                .map(|value| value.contains(expected))
                .unwrap_or(false),
        });
    }

    if let Some(expected_tool) = expected_tool_call(case) {
        let actual = result
            .metadata
            .tool_calls
            .iter()
            .map(|record| Value::String(record.tool_name.clone()))
            .collect::<Vec<_>>();
        outcomes.push(AssertionOutcome {
            kind: "tool_called".to_string(),
            expected: Value::String(expected_tool.to_string()),
            actual: Value::Array(actual.clone()),
            passed: actual
                .iter()
                .any(|tool| tool.as_str() == Some(expected_tool)),
        });
    }

    outcomes
}

pub fn assertion_error_from_outcomes(outcomes: &[AssertionOutcome]) -> Option<DslError> {
    for outcome in outcomes {
        if outcome.passed {
            continue;
        }

        match outcome.kind.as_str() {
            "contains" => {
                return if outcome.actual.is_null() {
                    Some(DslError::MissingOutput)
                } else {
                    Some(DslError::ExpectedContains {
                        expected: outcome.expected.as_str().unwrap_or_default().to_string(),
                        actual: outcome.actual.as_str().unwrap_or_default().to_string(),
                    })
                };
            }
            "tool_called" => {
                let actual = outcome
                    .actual
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|value| value.as_str().map(ToString::to_string))
                    .collect::<Vec<_>>();
                return Some(DslError::ExpectedToolCall {
                    tool: outcome.expected.as_str().unwrap_or_default().to_string(),
                    actual,
                });
            }
            _ => continue,
        }
    }

    None
}
