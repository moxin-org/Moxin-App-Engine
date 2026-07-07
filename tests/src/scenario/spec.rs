use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScenarioSpec {
    #[serde(default)]
    pub case_name: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    pub suite_name: String,
    #[serde(default)]
    pub clock: ClockScenarioSpec,
    #[serde(default)]
    pub llm: LlmScenarioSpec,
    #[serde(default)]
    pub bus: BusScenarioSpec,
    #[serde(default)]
    pub tools: Vec<ToolScenarioSpec>,
    #[serde(default)]
    pub expectations: ScenarioExpectations,
}

impl ScenarioSpec {
    pub fn from_yaml_str(input: &str) -> Result<Self> {
        Ok(serde_yaml::from_str(input)?)
    }

    pub fn from_json_str(input: &str) -> Result<Self> {
        Ok(serde_json::from_str(input)?)
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read scenario fixture '{}'", path.display()))?;

        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("yaml") | Some("yml") => Self::from_yaml_str(&raw),
            Some("json") => Self::from_json_str(&raw),
            Some(other) => bail!(
                "unsupported scenario fixture extension '{}' for '{}'",
                other,
                path.display()
            ),
            None => bail!("scenario fixture '{}' has no extension", path.display()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClockScenarioSpec {
    pub start_ms: Option<u64>,
    pub auto_advance_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponseRule {
    pub prompt_substring: String,
    pub response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponseSequence {
    pub prompt_substring: String,
    pub responses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmFailureSpec {
    pub prompt_substring: String,
    pub error: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LlmScenarioSpec {
    pub model_name: Option<String>,
    pub fallback: Option<String>,
    #[serde(default)]
    pub responses: Vec<LlmResponseRule>,
    #[serde(default)]
    pub response_sequences: Vec<LlmResponseSequence>,
    #[serde(default)]
    pub fail_next: Vec<ToolFailureSpec>,
    #[serde(default)]
    pub fail_on: Vec<LlmFailureSpec>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BusScenarioSpec {
    #[serde(default)]
    pub fail_next_send: Vec<ToolFailureSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultSpec {
    pub text: Option<String>,
    pub json: Option<Value>,
    pub error: Option<String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFailureSpec {
    pub count: usize,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallExpectation {
    pub tool_name: String,
    pub expected: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptCountExpectation {
    pub substring: String,
    pub expected: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusExpectation {
    pub sender_id: String,
    pub expected: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolScenarioSpec {
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub stubbed_result: Option<ToolResultSpec>,
    #[serde(default)]
    pub fail_next: Vec<ToolFailureSpec>,
    #[serde(default)]
    pub fail_on_input: Vec<ToolInputFailureSpec>,
    #[serde(default)]
    pub result_sequence: Vec<ToolResultSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInputFailureSpec {
    pub input: Value,
    pub error: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScenarioExpectations {
    pub infer_total: Option<usize>,
    #[serde(default)]
    pub prompt_counts: Vec<PromptCountExpectation>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallExpectation>,
    #[serde(default)]
    pub bus_messages_from: Vec<BusExpectation>,
}
