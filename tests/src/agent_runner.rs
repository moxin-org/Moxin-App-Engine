//! Real agent runner harness for integration-style tests.
//!
//! Provides a lightweight wrapper around the MoFA runtime `AgentRunner`
//! with an isolated workspace and deterministic mock LLM.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mofa_foundation::agent::context::prompt::AgentIdentity;
use mofa_foundation::agent::executor::{AgentExecutor, AgentExecutorConfig};
use mofa_kernel::agent::context::AgentContext;
use mofa_kernel::agent::core::MoFAAgent;
use mofa_kernel::agent::error::{AgentError, AgentResult};
use mofa_foundation::agent::components::tool::as_tool;
use mofa_foundation::agent::components::tool::SimpleTool;
use mofa_foundation::agent::session::{JsonlSessionStorage, Session, SessionStorage};
use crate::tools::MockTool;
use mofa_kernel::agent::types::{AgentInput, AgentOutput, ChatCompletionRequest};
use mofa_kernel::agent::types::{ChatCompletionResponse, ToolCall};
use mofa_kernel::agent::AgentCapabilities;
use mofa_kernel::agent::AgentState;
use mofa_runtime::runner::{AgentRunner, RunnerState, RunnerStats};
use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Errors returned by the agent runner harness itself.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AgentRunnerError {
    #[error("failed to create test workspace: {0}")]
    WorkspaceIo(#[from] std::io::Error),

    #[error("agent runner failure: {0}")]
    Agent(#[from] AgentError),
}

/// Metadata captured for each run.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AgentRunMetadata {
    pub agent_id: String,
    pub agent_name: String,
    pub execution_id: String,
    pub session_id: Option<String>,
    pub workspace_root: PathBuf,
    pub runner_state_before: RunnerState,
    pub runner_state_after: RunnerState,
    pub runner_stats_before: RunnerStats,
    pub runner_stats_after: RunnerStats,
    pub agent_state_before: AgentState,
    pub agent_state_after: AgentState,
    pub started_at: DateTime<Utc>,
    pub session_snapshot: Option<Session>,
    pub tool_calls: Vec<ToolCallRecord>,
    pub llm_last_request: Option<ChatCompletionRequest>,
    pub llm_last_response: Option<ChatCompletionResponse>,
    pub workspace_snapshot_before: WorkspaceSnapshot,
    pub workspace_snapshot_after: WorkspaceSnapshot,
}

/// Result of a single agent run.
#[derive(Debug)]
#[non_exhaustive]
pub struct AgentRunResult {
    pub output: Option<AgentOutput>,
    pub error: Option<AgentError>,
    pub duration: Duration,
    pub metadata: AgentRunMetadata,
}

/// Captures a tool call with its input and output.
#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub success: bool,
    pub duration_ms: Option<u64>,
    pub timed_out: bool,
}

/// Snapshot of files in the test workspace.
#[derive(Debug, Clone)]
pub struct WorkspaceSnapshot {
    pub files: Vec<WorkspaceFileSnapshot>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceFileSnapshot {
    pub relative_path: String,
    pub size_bytes: u64,
    pub modified_ms: Option<u64>,
    pub checksum: u64,
}

impl AgentRunResult {
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }

    pub fn output_text(&self) -> Option<String> {
        self.output.as_ref().map(AgentOutput::to_text)
    }
}

/// Simple deterministic LLM provider for tests.
#[derive(Debug)]
pub struct MockAgentLLMProvider {
    name: String,
    responses: RwLock<VecDeque<MockLlmResponse>>,
    default_response: RwLock<String>,
    last_request: RwLock<Option<ChatCompletionRequest>>,
    last_response: RwLock<Option<ChatCompletionResponse>>,
}

#[derive(Debug, Clone)]
enum MockLlmResponse {
    Text(String),
    ToolCall {
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
    },
    Error(String),
}

impl MockAgentLLMProvider {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            responses: RwLock::new(VecDeque::new()),
            default_response: RwLock::new("This is a mock response.".to_string()),
            last_request: RwLock::new(None),
            last_response: RwLock::new(None),
        }
    }

    pub async fn add_response(&self, response: impl Into<String>) {
        self.responses
            .write()
            .await
            .push_back(MockLlmResponse::Text(response.into()));
    }

    pub async fn add_tool_call_response(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
        content: Option<String>,
    ) {
        let tool_call = ToolCall {
            id: Uuid::now_v7().to_string(),
            name: tool_name.to_string(),
            arguments,
        };
        self.responses.write().await.push_back(MockLlmResponse::ToolCall {
            content,
            tool_calls: vec![tool_call],
        });
    }

    pub async fn add_error_response(&self, message: impl Into<String>) {
        self.responses
            .write()
            .await
            .push_back(MockLlmResponse::Error(message.into()));
    }

    pub async fn set_default_response(&self, response: impl Into<String>) {
        *self.default_response.write().await = response.into();
    }

    pub async fn pending_responses(&self) -> usize {
        self.responses.read().await.len()
    }

    pub async fn last_request(&self) -> Option<ChatCompletionRequest> {
        self.last_request.read().await.clone()
    }

    pub async fn last_response(&self) -> Option<ChatCompletionResponse> {
        self.last_response.read().await.clone()
    }
}

#[async_trait]
impl mofa_kernel::agent::types::LLMProvider for MockAgentLLMProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat(
        &self,
        request: ChatCompletionRequest,
    ) -> AgentResult<ChatCompletionResponse> {
        *self.last_request.write().await = Some(request);
        let response = {
            let mut responses = self.responses.write().await;
            if let Some(next) = responses.pop_front() {
                next
            } else {
                MockLlmResponse::Text(self.default_response.read().await.clone())
            }
        };

        let response = match response {
            MockLlmResponse::Text(content) => Ok(ChatCompletionResponse {
                content: Some(content),
                tool_calls: Some(Vec::<ToolCall>::new()),
                usage: None,
            }),
            MockLlmResponse::ToolCall { content, tool_calls } => Ok(ChatCompletionResponse {
                content,
                tool_calls: Some(tool_calls),
                usage: None,
            }),
            MockLlmResponse::Error(message) => Err(AgentError::ExecutionFailed(message)),
        }?;

        *self.last_response.write().await = Some(response.clone());
        Ok(response)
    }
}

struct SessionAwareExecutor {
    executor: AgentExecutor,
}

impl SessionAwareExecutor {
    fn new(executor: AgentExecutor) -> Self {
        Self { executor }
    }

    async fn register_tool(
        &self,
        tool: Arc<dyn mofa_kernel::agent::components::tool::DynTool>,
    ) -> AgentResult<()> {
        self.executor.register_tool(tool).await
    }

    async fn update_prompt_context<F>(&self, updater: F)
    where
        F: FnOnce(&mut mofa_foundation::agent::context::prompt::PromptContext),
    {
        self.executor.update_prompt_context(updater).await;
    }
}

#[async_trait]
impl MoFAAgent for SessionAwareExecutor {
    fn id(&self) -> &str {
        self.executor.id()
    }

    fn name(&self) -> &str {
        self.executor.name()
    }

    fn capabilities(&self) -> &AgentCapabilities {
        self.executor.capabilities()
    }

    fn state(&self) -> mofa_kernel::agent::AgentState {
        self.executor.state()
    }

    async fn initialize(&mut self, ctx: &AgentContext) -> AgentResult<()> {
        self.executor.initialize(ctx).await
    }

    async fn execute(
        &mut self,
        input: AgentInput,
        ctx: &AgentContext,
    ) -> AgentResult<AgentOutput> {
        let message = input.as_text().unwrap_or("");
        let session_key = ctx.session_id.as_deref().unwrap_or("default");
        let response = self.executor.process_message(session_key, message).await?;
        Ok(AgentOutput::text(response))
    }

    async fn shutdown(&mut self) -> AgentResult<()> {
        self.executor.shutdown().await
    }
}

struct TempWorkspace {
    root: PathBuf,
}

impl TempWorkspace {
    fn new(prefix: &str) -> Result<Self, AgentRunnerError> {
        let root = std::env::temp_dir().join(format!("{}-{}", prefix, Uuid::now_v7()));
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write_file(&self, relative_path: &Path, content: &str) -> Result<PathBuf, AgentRunnerError> {
        let path = self.root.join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        Ok(path)
    }

    fn snapshot(&self) -> WorkspaceSnapshot {
        let mut files = Vec::new();
        collect_workspace_files(&self.root, &self.root, &mut files);
        files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        WorkspaceSnapshot { files }
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn collect_workspace_files(root: &Path, current: &Path, files: &mut Vec<WorkspaceFileSnapshot>) {
    let entries = match std::fs::read_dir(current) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_workspace_files(root, &path, files);
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };

        let size_bytes = metadata.len();
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64);

        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => Vec::new(),
        };
        let checksum = hash_bytes(&bytes);
        let relative_path = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        files.push(WorkspaceFileSnapshot {
            relative_path,
            size_bytes,
            modified_ms,
            checksum,
        });
    }
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// Test harness for running real agent execution paths.
pub struct AgentTestRunner {
    workspace: TempWorkspace,
    session_id: String,
    execution_id: String,
    llm: Arc<MockAgentLLMProvider>,
    runner: AgentRunner<SessionAwareExecutor>,
    mock_tools: Vec<MockTool>,
}

impl AgentTestRunner {
    pub async fn new() -> Result<Self, AgentRunnerError> {
        Self::with_config(AgentExecutorConfig::default()).await
    }

    pub async fn with_config(config: AgentExecutorConfig) -> Result<Self, AgentRunnerError> {
        let workspace = TempWorkspace::new("mofa-agent-test")?;
        let llm = Arc::new(MockAgentLLMProvider::new("mock-llm"));
        let executor = AgentExecutor::with_config(llm.clone(), workspace.path(), config).await?;
        let agent = SessionAwareExecutor::new(executor);

        let execution_id = Uuid::now_v7().to_string();
        let session_id = Uuid::now_v7().to_string();
        let context = AgentContext::with_session(&execution_id, &session_id);

        let runner = AgentRunner::with_context(agent, context).await?;

        Ok(Self {
            workspace,
            session_id,
            execution_id,
            llm,
            runner,
            mock_tools: Vec::new(),
        })
    }

    pub fn workspace(&self) -> &Path {
        self.workspace.path()
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }

    pub fn mock_llm(&self) -> Arc<MockAgentLLMProvider> {
        Arc::clone(&self.llm)
    }

    pub fn write_bootstrap_file(
        &self,
        filename: &str,
        content: &str,
    ) -> Result<PathBuf, AgentRunnerError> {
        self.workspace.write_file(Path::new(filename), content)
    }

    pub fn write_workspace_file(
        &self,
        relative_path: impl AsRef<Path>,
        content: &str,
    ) -> Result<PathBuf, AgentRunnerError> {
        self.workspace.write_file(relative_path.as_ref(), content)
    }

    pub async fn register_simple_tool<T>(&self, tool: T) -> Result<(), AgentRunnerError>
    where
        T: mofa_foundation::agent::components::tool::SimpleTool + Send + Sync + 'static,
    {
        let tool_ref = as_tool(tool);
        self.runner
            .agent()
            .register_tool(tool_ref)
            .await
            .map_err(AgentRunnerError::from)
    }

    pub async fn register_mock_tool(&mut self, tool: MockTool) -> Result<(), AgentRunnerError> {
        self.register_simple_tool(tool.clone()).await?;
        self.mock_tools.push(tool);
        Ok(())
    }

    pub async fn configure_prompt(
        &self,
        identity: Option<AgentIdentity>,
        bootstrap_files: Option<Vec<String>>,
    ) {
        self.runner
            .agent()
            .update_prompt_context(|ctx| {
                if let Some(identity) = identity {
                    ctx.set_identity(identity);
                }
                if let Some(files) = bootstrap_files {
                    ctx.set_bootstrap_files(files);
                }
            })
            .await;
    }

    pub async fn run_text(&mut self, input: &str) -> Result<AgentRunResult, AgentRunnerError> {
        self.run_input(AgentInput::text(input)).await
    }

    pub async fn run_text_with_session(
        &mut self,
        session_id: &str,
        input: &str,
    ) -> Result<AgentRunResult, AgentRunnerError> {
        let original_session = self.runner.context().session_id.clone();
        self.runner
            .set_session_id(Some(session_id.to_string()));
        let result = self.run_text(input).await;
        self.runner.set_session_id(original_session);
        result
    }

    pub async fn run_texts(
        &mut self,
        inputs: &[&str],
    ) -> Result<Vec<AgentRunResult>, AgentRunnerError> {
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            results.push(self.run_text(input).await?);
        }
        Ok(results)
    }

    pub async fn run_input(
        &mut self,
        input: AgentInput,
    ) -> Result<AgentRunResult, AgentRunnerError> {
        let started_at = Utc::now();
        let runner_state_before = self.runner.state().await;
        let runner_stats_before = self.runner.stats().await;
        let agent_state_before = self.runner.agent_state();
        let workspace_snapshot_before = self.workspace.snapshot();
        let timer = Instant::now();
        let result = self.runner.execute(input).await;
        let duration = timer.elapsed();
        let runner_state_after = self.runner.state().await;
        let runner_stats_after = self.runner.stats().await;
        let agent_state_after = self.runner.agent_state();
        let session_snapshot = self.load_session_snapshot().await;
        let workspace_snapshot_after = self.workspace.snapshot();
        let tool_calls = self.collect_tool_calls().await;
        let llm_last_request = self.llm.last_request().await;
        let llm_last_response = self.llm.last_response().await;

        let (output, error) = match result {
            Ok(output) => (Some(output), None),
            Err(err) => (None, Some(err)),
        };

        let metadata = AgentRunMetadata {
            agent_id: self.runner.agent().id().to_string(),
            agent_name: self.runner.agent().name().to_string(),
            execution_id: self.runner.context().execution_id.clone(),
            session_id: self.runner.context().session_id.clone(),
            workspace_root: self.workspace.path().to_path_buf(),
            runner_state_before,
            runner_state_after,
            runner_stats_before,
            runner_stats_after,
            agent_state_before,
            agent_state_after,
            started_at,
            session_snapshot,
            tool_calls,
            llm_last_request,
            llm_last_response,
            workspace_snapshot_before,
            workspace_snapshot_after,
        };

        Ok(AgentRunResult {
            output,
            error,
            duration,
            metadata,
        })
    }

    pub async fn shutdown(self) -> Result<(), AgentRunnerError> {
        self.runner.shutdown().await?;
        Ok(())
    }

    async fn load_session_snapshot(&self) -> Option<Session> {
        let session_id = self.runner.context().session_id.as_deref()?;
        let storage = JsonlSessionStorage::new(self.workspace.path()).await.ok()?;
        storage.load(session_id).await.ok()?
    }

    async fn collect_tool_calls(&self) -> Vec<ToolCallRecord> {
        let mut records = Vec::new();
        for tool in &self.mock_tools {
            let calls = tool.history().await;
            let results = tool.results().await;
            for (idx, call) in calls.into_iter().enumerate() {
                let result = results.get(idx).cloned();
                let (output, success, duration_ms, timed_out) = match result {
                    Some(result) => {
                        let duration_ms = result
                            .metadata
                            .get("duration_ms")
                            .and_then(|value| value.parse::<u64>().ok());
                        let timed_out = result
                            .error
                            .as_ref()
                            .map(|err| err.contains("timed out"))
                            .unwrap_or(false);
                        (
                            Some(result.output.clone()),
                            result.success,
                            duration_ms,
                            timed_out,
                        )
                    }
                    None => (None, false, None, false),
                };
                records.push(ToolCallRecord {
                    tool_name: tool.name().to_string(),
                    input: call.arguments,
                    output,
                    success,
                    duration_ms,
                    timed_out,
                });
            }
        }
        records
    }
}
