//! Task orchestration framework for background task spawning
//!
//! This module provides:
//! - Background task spawning via tokio::spawn
//! - Origin-based result routing
//! - Task lifecycle management
//! - Result streaming via channels

use crate::llm::LLMProvider;
use chrono::{DateTime, Utc};
use mofa_kernel::agent::types::error::{GlobalError, GlobalResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::Instrument;
use uuid::Uuid;

/// Where to route task results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOrigin {
    /// Routing key (e.g., "channel:chat_id")
    pub routing_key: String,
    /// Optional metadata
    #[serde(flatten)]
    pub metadata: HashMap<String, Value>,
}

impl TaskOrigin {
    /// Create a new task origin
    pub fn new(routing_key: impl Into<String>) -> Self {
        Self {
            routing_key: routing_key.into(),
            metadata: HashMap::new(),
        }
    }

    /// Create from channel and chat_id
    pub fn from_channel(channel: &str, chat_id: &str) -> Self {
        Self::new(format!("{}:{}", channel, chat_id))
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

/// Background task status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task is pending
    Pending,
    /// Task is running
    Running,
    /// Task completed successfully
    Completed(String),
    /// Task failed
    Failed(String),
}

/// Background task with origin tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTask {
    /// Unique task ID
    pub id: String,
    /// Task prompt/description
    pub prompt: String,
    /// Where to route results
    pub origin: TaskOrigin,
    /// Current status
    pub status: TaskStatus,
    /// When the task started
    pub started_at: DateTime<Utc>,
    /// When the task completed (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

impl BackgroundTask {
    /// Create a new background task
    pub fn new(prompt: impl Into<String>, origin: TaskOrigin) -> Self {
        Self {
            id: Uuid::new_v4().to_string()[..8].to_string(),
            prompt: prompt.into(),
            origin,
            status: TaskStatus::Pending,
            started_at: Utc::now(),
            completed_at: None,
        }
    }

    /// Mark as running
    pub fn mark_running(&mut self) {
        self.status = TaskStatus::Running;
    }

    /// Mark as completed
    pub fn mark_completed(&mut self, result: impl Into<String>) {
        self.status = TaskStatus::Completed(result.into());
        self.completed_at = Some(Utc::now());
    }

    /// Mark as failed
    pub fn mark_failed(&mut self, error: impl Into<String>) {
        self.status = TaskStatus::Failed(error.into());
        self.completed_at = Some(Utc::now());
    }

    /// Check if task is finished
    pub fn is_finished(&self) -> bool {
        matches!(
            self.status,
            TaskStatus::Completed(_) | TaskStatus::Failed(_)
        )
    }
}

/// Result from a completed task
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    /// Task ID
    pub task_id: String,
    /// Where to route the result
    pub origin: TaskOrigin,
    /// Result content
    pub content: String,
    /// Whether the task succeeded
    pub success: bool,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

impl TaskResult {
    /// Create a successful result
    pub fn success(
        task_id: impl Into<String>,
        origin: TaskOrigin,
        content: impl Into<String>,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            origin,
            content: content.into(),
            success: true,
            timestamp: Utc::now(),
        }
    }

    /// Create a failed result
    pub fn failure(
        task_id: impl Into<String>,
        origin: TaskOrigin,
        error: impl Into<String>,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            origin,
            content: error.into(),
            success: false,
            timestamp: Utc::now(),
        }
    }
}

/// Configuration for task orchestrator
#[derive(Debug, Clone)]
pub struct TaskOrchestratorConfig {
    /// Maximum concurrent tasks
    pub max_concurrent_tasks: usize,
    /// Default model for tasks
    pub default_model: String,
}

impl Default for TaskOrchestratorConfig {
    fn default() -> Self {
        Self {
            max_concurrent_tasks: 10,
            default_model: "gpt-4o-mini".to_string(),
        }
    }
}

/// Orchestrator for background tasks
pub struct TaskOrchestrator {
    /// LLM provider for tasks
    provider: Arc<dyn LLMProvider>,
    /// Active tasks
    active_tasks: Arc<RwLock<HashMap<String, BackgroundTask>>>,
    /// Result sender
    result_sender: broadcast::Sender<TaskResult>,
    /// Configuration
    config: TaskOrchestratorConfig,
}

impl TaskOrchestrator {
    /// Create a new task orchestrator
    pub fn new(provider: Arc<dyn LLMProvider>, config: TaskOrchestratorConfig) -> Self {
        let (result_sender, _) = broadcast::channel(100);

        Self {
            provider,
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            result_sender,
            config,
        }
    }

    /// Create with default configuration
    pub fn with_defaults(provider: Arc<dyn LLMProvider>) -> Self {
        Self::new(provider, TaskOrchestratorConfig::default())
    }

    /// Spawn a background task
    pub async fn spawn(&self, prompt: &str, origin: TaskOrigin) -> GlobalResult<String> {
        // Check concurrent limit
        let active_count = self.active_tasks.read().await.len();
        if active_count >= self.config.max_concurrent_tasks {
            return Err(GlobalError::Other(format!(
                "Maximum concurrent tasks ({}) reached",
                self.config.max_concurrent_tasks
            )));
        }

        // Create task
        let mut task = BackgroundTask::new(prompt, origin.clone());
        let task_id = task.id.clone();
        task.mark_running();

        // Store task
        self.active_tasks
            .write()
            .await
            .insert(task_id.clone(), task.clone());

        // Spawn background task
        let provider = Arc::clone(&self.provider);
        let active_tasks = Arc::clone(&self.active_tasks);
        let result_sender = self.result_sender.clone();
        let model = self.config.default_model.clone();
        let prompt = prompt.to_string();
        let task_id_clone = task_id.clone();

        let span = tracing::info_span!("task_orchestrator.background", task_id = %task_id_clone);
        tokio::spawn(
            async move {
                let result = Self::run_task(&provider, &model, &prompt).await;

                // Update task status
                {
                    let mut tasks = active_tasks.write().await;
                    if let Some(task) = tasks.get_mut(&task_id_clone) {
                        match &result {
                            Ok(content) => task.mark_completed(content),
                            Err(e) => task.mark_failed(e.to_string()),
                        }
                    }
                }

                // Send result
                let task_result = match &result {
                    Ok(content) => TaskResult::success(&task_id_clone, origin, content),
                    Err(e) => TaskResult::failure(&task_id_clone, origin, e.to_string()),
                };

                let _ = result_sender.send(task_result);

                // Cleanup completed tasks after a delay
                tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;
                let mut tasks = active_tasks.write().await;
                tasks.remove(&task_id_clone);
            }
            .instrument(span),
        );

        Ok(task_id)
    }

    /// Run a single task to completion
    async fn run_task(
        provider: &Arc<dyn LLMProvider>,
        model: &str,
        prompt: &str,
    ) -> GlobalResult<String> {
        use crate::llm::types::ChatCompletionRequest;

        let request = ChatCompletionRequest::new(model)
            .system(
                "You are a helpful assistant. Complete the given task thoroughly and concisely.",
            )
            .user(prompt);

        let response = provider
            .chat(request)
            .await
            .map_err(|e| GlobalError::Other(e.to_string()))?;

        response
            .content()
            .map(|s| s.to_string())
            .ok_or_else(|| GlobalError::Other("No response content".to_string()))
    }

    /// Subscribe to task results
    pub fn subscribe_results(&self) -> broadcast::Receiver<TaskResult> {
        self.result_sender.subscribe()
    }

    /// Get all active tasks
    pub async fn get_active_tasks(&self) -> Vec<BackgroundTask> {
        self.active_tasks.read().await.values().cloned().collect()
    }

    /// Get a specific task
    pub async fn get_task(&self, task_id: &str) -> Option<BackgroundTask> {
        self.active_tasks.read().await.get(task_id).cloned()
    }

    /// Get the configuration
    pub fn config(&self) -> &TaskOrchestratorConfig {
        &self.config
    }
}
