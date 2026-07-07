//! Canonical run artifacts for DSL-backed agent test execution.
//!
//! These types provide the stable, serializable output model for DSL runs,
//! built from the existing runner result.

use crate::agent_runner::{AgentRunResult, ToolCallRecord, WorkspaceFileSnapshot, WorkspaceSnapshot};
use crate::dsl::{AssertionOutcome, TestCaseDsl};
use mofa_foundation::agent::session::Session;
use serde::{Deserialize, Serialize};
use serde_json::json;

// Top-level artifact emitted for a single DSL-backed case execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunArtifact {
    pub case_name: String,
    pub status: String,
    pub output_text: Option<String>,
    pub runner_error: Option<String>,
    pub duration_ms: u64,
    pub started_at_ms: u64,
    pub execution_id: String,
    pub session_id: Option<String>,
    pub workspace_root: String,
    pub agent: AgentArtifact,
    pub assertions: Vec<AssertionOutcome>,
    pub tool_calls: Vec<ToolCallArtifact>,
    pub llm_request: Option<LlmRequestArtifact>,
    pub llm_response: Option<LlmResponseArtifact>,
    pub session_snapshot: Option<SessionArtifact>,
    pub workspace_before: WorkspaceSnapshotArtifact,
    pub workspace_after: WorkspaceSnapshotArtifact,
}

// Compact identity data for the agent used by the run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentArtifact {
    pub id: String,
    pub name: String,
}

// Tool execution records are flattened into the artifact for downstream checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallArtifact {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub success: bool,
    pub duration_ms: Option<u64>,
    pub timed_out: bool,
}

// LLM request/response types keep only the fields needed for stable inspection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequestArtifact {
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub messages: Vec<LlmMessageArtifact>,
    pub tool_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponseArtifact {
    pub content: Option<String>,
    pub tool_calls: Vec<LlmToolCallArtifact>,
    pub usage: Option<TokenUsageArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessageArtifact {
    pub role: String,
    pub content: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_calls: Vec<LlmToolCallArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmToolCallArtifact {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageArtifact {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionArtifact {
    pub messages: Vec<SessionMessageArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessageArtifact {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshotArtifact {
    pub files: Vec<WorkspaceFileArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceFileArtifact {
    pub relative_path: String,
    pub size_bytes: u64,
    pub modified_ms: Option<u64>,
    pub checksum: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunArtifactDiff {
    pub matches: bool,
    pub differences: Vec<ArtifactDifference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactDifference {
    pub field: String,
    pub expected: serde_json::Value,
    pub actual: serde_json::Value,
}

impl AgentRunArtifact {
    // Build the canonical artifact from the current runner result plus DSL assertion outcomes.
    pub fn from_run_result(
        case: &TestCaseDsl,
        result: &AgentRunResult,
        assertions: Vec<AssertionOutcome>,
    ) -> Self {
        Self {
            case_name: case.name.clone(),
            status: if result.is_success() && assertions.iter().all(|item| item.passed) {
                "passed".to_string()
            } else {
                "failed".to_string()
            },
            output_text: result.output_text(),
            runner_error: result.error.as_ref().map(ToString::to_string),
            duration_ms: result.duration.as_millis() as u64,
            started_at_ms: result.metadata.started_at.timestamp_millis() as u64,
            execution_id: result.metadata.execution_id.clone(),
            session_id: result.metadata.session_id.clone(),
            workspace_root: result.metadata.workspace_root.display().to_string(),
            agent: AgentArtifact {
                id: result.metadata.agent_id.clone(),
                name: result.metadata.agent_name.clone(),
            },
            assertions,
            tool_calls: result
                .metadata
                .tool_calls
                .iter()
                .map(tool_call_artifact)
                .collect(),
            llm_request: result
                .metadata
                .llm_last_request
                .as_ref()
                .map(|request| LlmRequestArtifact {
                    model: request.model.clone(),
                    temperature: request.temperature,
                    max_tokens: request.max_tokens,
                    messages: request
                        .messages
                        .iter()
                        .map(|message| LlmMessageArtifact {
                            role: message.role.clone(),
                            content: message.content.clone(),
                            tool_call_id: message.tool_call_id.clone(),
                            tool_calls: message
                                .tool_calls
                                .clone()
                                .unwrap_or_default()
                                .into_iter()
                                .map(llm_tool_call_artifact)
                                .collect(),
                        })
                        .collect(),
                    tool_names: request
                        .tools
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|tool| tool.name)
                        .collect(),
                }),
            llm_response: result
                .metadata
                .llm_last_response
                .as_ref()
                .map(|response| LlmResponseArtifact {
                    content: response.content.clone(),
                    tool_calls: response
                        .tool_calls
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .map(llm_tool_call_artifact)
                        .collect(),
                    usage: response.usage.as_ref().map(|usage| TokenUsageArtifact {
                        prompt_tokens: usage.prompt_tokens,
                        completion_tokens: usage.completion_tokens,
                        total_tokens: usage.total_tokens,
                    }),
                }),
            session_snapshot: result
                .metadata
                .session_snapshot
                .as_ref()
                .map(session_artifact),
            workspace_before: workspace_snapshot_artifact(&result.metadata.workspace_snapshot_before),
            workspace_after: workspace_snapshot_artifact(&result.metadata.workspace_snapshot_after),
        }
    }

    // Compare the MVP baseline fields while keeping deeper metadata out of scope for now.
    pub fn compare_to(&self, baseline: &Self) -> AgentRunArtifactDiff {
        let mut differences = Vec::new();

        if self.status != baseline.status {
            differences.push(ArtifactDifference {
                field: "status".to_string(),
                expected: json!(baseline.status),
                actual: json!(self.status),
            });
        }

        if self.output_text != baseline.output_text {
            differences.push(ArtifactDifference {
                field: "output_text".to_string(),
                expected: json!(baseline.output_text),
                actual: json!(self.output_text),
            });
        }

        let baseline_assertions = baseline
            .assertions
            .iter()
            .map(assertion_signature)
            .collect::<Vec<_>>();
        let actual_assertions = self
            .assertions
            .iter()
            .map(assertion_signature)
            .collect::<Vec<_>>();
        if actual_assertions != baseline_assertions {
            differences.push(ArtifactDifference {
                field: "assertions".to_string(),
                expected: json!(baseline_assertions),
                actual: json!(actual_assertions),
            });
        }

        let baseline_tools = baseline
            .tool_calls
            .iter()
            .map(|call| call.tool_name.clone())
            .collect::<Vec<_>>();
        let actual_tools = self
            .tool_calls
            .iter()
            .map(|call| call.tool_name.clone())
            .collect::<Vec<_>>();
        if actual_tools != baseline_tools {
            differences.push(ArtifactDifference {
                field: "tool_calls".to_string(),
                expected: json!(baseline_tools),
                actual: json!(actual_tools),
            });
        }

        AgentRunArtifactDiff {
            matches: differences.is_empty(),
            differences,
        }
    }
}

fn assertion_signature(outcome: &AssertionOutcome) -> serde_json::Value {
    json!({
        "kind": outcome.kind,
        "passed": outcome.passed,
    })
}

fn tool_call_artifact(record: &ToolCallRecord) -> ToolCallArtifact {
    ToolCallArtifact {
        tool_name: record.tool_name.clone(),
        input: record.input.clone(),
        output: record.output.clone(),
        success: record.success,
        duration_ms: record.duration_ms,
        timed_out: record.timed_out,
    }
}

fn llm_tool_call_artifact(tool_call: mofa_kernel::agent::types::ToolCall) -> LlmToolCallArtifact {
    LlmToolCallArtifact {
        id: tool_call.id,
        name: tool_call.name,
        arguments: tool_call.arguments,
    }
}

// Session snapshots are reduced to ordered role/content pairs for stable comparisons.
fn session_artifact(session: &Session) -> SessionArtifact {
    SessionArtifact {
        messages: session
            .messages
            .iter()
            .map(|message| SessionMessageArtifact {
                role: message.role.clone(),
                content: message.content.clone(),
            })
            .collect(),
    }
}

// Workspace snapshots preserve a compact file-level view before and after execution.
fn workspace_snapshot_artifact(snapshot: &WorkspaceSnapshot) -> WorkspaceSnapshotArtifact {
    WorkspaceSnapshotArtifact {
        files: snapshot.files.iter().map(workspace_file_artifact).collect(),
    }
}

fn workspace_file_artifact(file: &WorkspaceFileSnapshot) -> WorkspaceFileArtifact {
    WorkspaceFileArtifact {
        relative_path: file.relative_path.clone(),
        size_bytes: file.size_bytes,
        modified_ms: file.modified_ms,
        checksum: file.checksum,
    }
}
