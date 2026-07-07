//! 工作流执行器
//! Workflow Executor
//!
//! 负责工作流的执行调度
//! Responsible for workflow execution scheduling
//!
//! Supports optional telemetry emission for the time-travel debugger.
//! When a `TelemetryEmitter` is attached via `with_telemetry()`, the
//! executor emits `DebugEvent`s at key execution points.

use super::execution_event::ExecutionEvent;
use super::graph::WorkflowGraph;
use super::node::{NodeType, WorkflowNode};
use super::profiler::{ExecutionTimeline, ProfilerMode};
use super::state::{
    ExecutionCheckpoint, ExecutionRecord, NodeExecutionRecord, NodeResult, NodeStatus,
    WorkflowContext, WorkflowStatus, WorkflowValue,
};
use mofa_kernel::workflow::telemetry::{DebugEvent, TelemetryEmitter};
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{RwLock, Semaphore, mpsc, oneshot};
use tracing::{error, info, warn};

// Optional HITL integration
use crate::hitl::handlers::WorkflowReviewHandler;

/// 执行器配置
/// Executor Configuration
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// 最大并行度
    /// Maximum parallelism
    pub max_parallelism: usize,
    /// 是否在失败时停止
    /// Whether to stop on failure
    pub stop_on_failure: bool,
    /// 是否启用检查点
    /// Whether to enable checkpoints
    pub enable_checkpoints: bool,
    /// 检查点间隔（节点数）
    /// Checkpoint interval (number of nodes)
    pub checkpoint_interval: usize,
    /// 执行超时（毫秒）
    /// Execution timeout (milliseconds)
    pub execution_timeout_ms: Option<u64>,
    /// Per-node execution timeout (milliseconds). If a single node takes
    /// longer than this, it is cancelled and marked as failed. Default: 120s.
    pub node_timeout_ms: u64,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            max_parallelism: 10,
            stop_on_failure: true,
            enable_checkpoints: true,
            checkpoint_interval: 5,
            execution_timeout_ms: None,
            node_timeout_ms: 120_000,
        }
    }
}

/// 工作流执行器
/// Workflow Executor
pub struct WorkflowExecutor {
    /// 执行器配置
    /// Executor configuration
    config: ExecutorConfig,
    /// 事件发送器
    /// Event transmitter
    event_tx: Option<mpsc::Sender<ExecutionEvent>>,
    /// Telemetry emitter for the time-travel debugger (optional)
    telemetry: Option<Arc<dyn TelemetryEmitter>>,
    /// 子工作流注册表
    /// Sub-workflow registry
    sub_workflows: Arc<RwLock<HashMap<String, Arc<WorkflowGraph>>>>,
    /// 外部事件等待器
    /// External event waiters
    event_waiters: Arc<RwLock<HashMap<String, Vec<oneshot::Sender<WorkflowValue>>>>>,
    /// 并行执行信号量
    /// Parallel execution semaphore
    semaphore: Arc<Semaphore>,
    /// Profiler for execution timing (optional)
    profiler: ProfilerMode,
    /// HITL review handler (optional)
    review_handler: Option<Arc<WorkflowReviewHandler>>,
}

impl WorkflowExecutor {
    pub fn new(config: ExecutorConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_parallelism));
        Self {
            config,
            event_tx: None,
            telemetry: None,
            sub_workflows: Arc::new(RwLock::new(HashMap::new())),
            event_waiters: Arc::new(RwLock::new(HashMap::new())),
            semaphore,
            profiler: ProfilerMode::Disabled,
            review_handler: None,
        }
    }

    /// 设置事件发送器
    /// Set event transmitter
    pub fn with_event_sender(mut self, tx: mpsc::Sender<ExecutionEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Attach a telemetry emitter for time-travel debugger support.
    ///
    /// When set, the executor will emit `DebugEvent`s at key execution points
    /// (workflow start/end, node start/end).
    pub fn with_telemetry(mut self, emitter: Arc<dyn TelemetryEmitter>) -> Self {
        self.telemetry = Some(emitter);
        self
    }

    /// Attach a profiler for execution timing capture.
    ///
    /// When set, the executor will record execution timing spans.
    pub fn with_profiler(mut self, mode: ProfilerMode) -> Self {
        self.profiler = mode;
        self
    }

    /// Attach a review manager for Human-in-the-Loop (HITL) support.
    ///
    /// When set, the executor will pause at review nodes and wait for human approval
    /// before continuing execution. This replaces the legacy `Wait` node approach.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use mofa_foundation::hitl::*;
    /// use mofa_foundation::workflow::*;
    ///
    /// let store = Arc::new(InMemoryReviewStore::new());
    /// let manager = Arc::new(ReviewManager::new(...));
    /// let handler = Arc::new(WorkflowReviewHandler::new(manager));
    ///
    /// let executor = WorkflowExecutor::new(ExecutorConfig::default())
    ///     .with_review_manager(handler);
    /// ```
    pub fn with_review_manager(mut self, handler: Arc<WorkflowReviewHandler>) -> Self {
        self.review_handler = Some(handler);
        self
    }

    /// Get profiler timeline if profiling is enabled.
    pub fn profiler_timeline(&self) -> Option<&ExecutionTimeline> {
        match &self.profiler {
            ProfilerMode::Record(timeline) => Some(timeline.get_timeline()),
            ProfilerMode::Disabled => None,
        }
    }

    /// Create review context from workflow state
    async fn create_review_context(
        &self,
        ctx: &WorkflowContext,
        node_id: &str,
        input: &WorkflowValue,
    ) -> mofa_kernel::hitl::ReviewContext {
        use mofa_kernel::hitl::{ExecutionStep, ExecutionTrace, ReviewContext};
        use std::collections::HashMap;

        // Create execution trace from workflow context
        let mut steps = Vec::new();

        // Get node outputs and statuses to build execution history
        let node_outputs = ctx.get_all_outputs().await;
        let node_statuses = ctx.get_all_node_statuses().await;

        // Create steps from completed nodes
        for (nid, output) in node_outputs {
            if let Some(status) = node_statuses.get(&nid)
                && matches!(status, super::state::NodeStatus::Completed)
            {
                steps.push(ExecutionStep {
                    step_id: nid.clone(),
                    step_type: "workflow_node".to_string(),
                    timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
                    input: None,
                    output: serde_json::to_value(&output).ok(),
                    metadata: HashMap::new(),
                });
            }
        }

        // Add current node step
        steps.push(ExecutionStep {
            step_id: node_id.to_string(),
            step_type: "review_node".to_string(),
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            input: serde_json::to_value(input).ok(),
            output: None,
            metadata: HashMap::new(),
        });

        // Calculate duration from paused_at if available
        let duration_ms = if let Some(paused_at) = *ctx.paused_at.read().await {
            let now = chrono::Utc::now();
            now.signed_duration_since(paused_at).num_milliseconds() as u64
        } else {
            0
        };

        let trace = ExecutionTrace { steps, duration_ms };

        ReviewContext::new(
            trace,
            serde_json::to_value(input).unwrap_or(serde_json::json!({})),
        )
    }

    /// Emit a debug telemetry event (no-op if no emitter is set).
    async fn emit_debug(&self, event: DebugEvent) {
        if let Some(ref emitter) = self.telemetry
            && emitter.is_enabled()
        {
            emitter.emit(event).await;
        }
    }

    /// 注册子工作流
    /// Register sub-workflow
    pub async fn register_sub_workflow(&self, id: &str, graph: WorkflowGraph) {
        let mut workflows = self.sub_workflows.write().await;
        workflows.insert(id.to_string(), Arc::new(graph));
    }

    /// 发送执行事件
    /// Emit execution event
    async fn emit_event(&self, event: ExecutionEvent) {
        if let Some(ref tx) = self.event_tx {
            let _ = tx.send(event).await;
        }
    }

    /// 发送外部事件
    /// Send external event
    pub async fn send_external_event(&self, event_type: &str, data: WorkflowValue) {
        let mut waiters = self.event_waiters.write().await;
        if let Some(senders) = waiters.remove(event_type) {
            for sender in senders {
                let _ = sender.send(data.clone());
            }
        }
    }

    /// 执行工作流
    /// Execute workflow
    pub async fn execute(
        &self,
        graph: &WorkflowGraph,
        input: WorkflowValue,
    ) -> Result<ExecutionRecord, String> {
        let start_time = Instant::now();
        let ctx = WorkflowContext::new(&graph.id);
        ctx.set_input(input.clone()).await;

        // 发送开始事件
        // Emit start event
        self.emit_event(ExecutionEvent::WorkflowStarted {
            workflow_id: graph.id.clone(),
            workflow_name: graph.name.clone(),
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        })
        .await;

        // Emit debug telemetry: WorkflowStart
        self.emit_debug(DebugEvent::WorkflowStart {
            workflow_id: graph.id.clone(),
            execution_id: ctx.execution_id.clone(),
            timestamp_ms: DebugEvent::now_ms(),
        })
        .await;

        info!(
            "Starting workflow execution: {} ({})",
            graph.name, ctx.execution_id
        );

        // 验证图
        // Validate graph
        if let Err(errors) = graph.validate() {
            let error_msg = errors.join("; ");
            error!("Workflow validation failed: {}", error_msg);
            return Err(error_msg);
        }

        // 获取开始节点
        // Get start node
        let start_node_id = graph
            .start_node()
            .ok_or_else(|| "No start node".to_string())?;

        // 执行工作流
        // Execute workflow
        let mut execution_record = ExecutionRecord {
            execution_id: ctx.execution_id.clone(),
            workflow_id: graph.id.clone(),
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            ended_at: None,
            status: WorkflowStatus::Running,
            node_records: Vec::new(),
            outputs: HashMap::new(),
            total_wait_time_ms: 0,
            context: None,
        };

        // 使用基于依赖的执行
        // Use dependency-based execution
        let result = if let Some(timeout_ms) = self.config.execution_timeout_ms {
            match tokio::time::timeout(
                std::time::Duration::from_millis(timeout_ms),
                self.execute_from_node(graph, &ctx, start_node_id, input, &mut execution_record),
            )
            .await
            {
                Ok(inner) => inner,
                Err(_) => Err(format!(
                    "Workflow execution timed out after {}ms",
                    timeout_ms
                )),
            }
        } else {
            self.execute_from_node(graph, &ctx, start_node_id, input, &mut execution_record)
                .await
        };

        let duration = start_time.elapsed();
        execution_record.ended_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );

        let final_status = match result {
            Ok(_) => {
                if execution_record.status != WorkflowStatus::Paused {
                    execution_record.status = WorkflowStatus::Completed;
                    info!("Workflow {} completed in {:?}", graph.name, duration);
                    "completed".to_string()
                } else {
                    info!("Workflow {} paused after {:?}", graph.name, duration);
                    execution_record.context = Some(ctx.clone());
                    "paused".to_string()
                }
            }
            Err(ref e) => {
                execution_record.status = WorkflowStatus::Failed(e.clone());
                error!("Workflow {} failed: {}", graph.name, e);
                format!("failed: {}", e)
            }
        };
        execution_record.outputs = ctx.get_all_outputs().await;

        // 发送完成事件
        // Emit completion event
        match &execution_record.status {
            WorkflowStatus::Failed(e) => {
                self.emit_event(ExecutionEvent::WorkflowFailed {
                    workflow_id: graph.id.clone(),
                    error: e.clone(),
                    total_duration_ms: duration.as_millis() as u64,
                })
                .await;
            }
            _ => {
                self.emit_event(ExecutionEvent::WorkflowCompleted {
                    workflow_id: graph.id.clone(),
                    final_output: None,
                    total_duration_ms: duration.as_millis() as u64,
                })
                .await;
            }
        }

        // Emit debug telemetry: WorkflowEnd
        self.emit_debug(DebugEvent::WorkflowEnd {
            workflow_id: graph.id.clone(),
            execution_id: ctx.execution_id.clone(),
            timestamp_ms: DebugEvent::now_ms(),
            status: final_status,
        })
        .await;

        Ok(execution_record)
    }

    pub async fn resume_with_human_input(
        &self,
        graph: &WorkflowGraph,
        ctx: WorkflowContext,
        waiting_node_id: &str,
        human_input: WorkflowValue,
    ) -> Result<ExecutionRecord, String> {
        info!(
            "Resuming workflow {} from node {} with human input",
            graph.id, waiting_node_id
        );

        // Check if this was a unified HITL review (check for review_id in variables)
        if let Some(review_id_value) = ctx.get_variable("review_id").await
            && let WorkflowValue::String(ref review_id_str) = review_id_value
            && let Some(ref review_handler) = self.review_handler
        {
            use mofa_kernel::hitl::ReviewRequestId;
            let review_id = ReviewRequestId::new(review_id_str.clone());

            // Check if review is approved
            match review_handler.is_approved(&review_id).await {
                Ok(true) => {
                    info!(
                        "Review {} approved, proceeding with workflow",
                        review_id_str
                    );
                }
                Ok(false) => {
                    // Check if rejected
                    if let Ok(Some(response)) = review_handler.get_review_response(&review_id).await
                    {
                        match response {
                            mofa_kernel::hitl::ReviewResponse::Rejected { reason, .. } => {
                                return Err(format!("Review rejected: {}", reason));
                            }
                            _ => {
                                return Err(format!("Review {} not approved", review_id_str));
                            }
                        }
                    } else {
                        return Err(format!("Review {} not yet resolved", review_id_str));
                    }
                }
                Err(e) => {
                    warn!("Failed to check review status: {}, proceeding anyway", e);
                }
            }
        }

        // Calculate wait time
        if let Some(paused_at) = *ctx.paused_at.read().await {
            let duration = chrono::Utc::now().signed_duration_since(paused_at);
            let wait_duration_ms = duration.num_milliseconds().max(0) as u64;
            *ctx.total_wait_time_ms.write().await += wait_duration_ms; // ← accumulate
        }

        ctx.set_node_output(waiting_node_id, human_input).await;
        ctx.set_node_status(waiting_node_id, NodeStatus::Completed)
            .await;
        *ctx.paused_at.write().await = None;
        *ctx.last_waiting_node.write().await = None;

        let start_time = Instant::now();

        let mut execution_record = ExecutionRecord {
            execution_id: ctx.execution_id.clone(),
            workflow_id: graph.id.clone(),
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            ended_at: None,
            status: WorkflowStatus::Running,
            node_records: Vec::new(),
            outputs: HashMap::new(),
            // total_wait_time_ms: wait_duration_ms,
            total_wait_time_ms: *ctx.total_wait_time_ms.read().await,
            context: None,
        };

        let start_node_id = graph
            .start_node()
            .ok_or_else(|| "No start node".to_string())?;

        let result = self
            .execute_from_node(
                graph,
                &ctx,
                start_node_id,
                WorkflowValue::Null,
                &mut execution_record,
            )
            .await;

        let duration = start_time.elapsed();
        execution_record.ended_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );

        let final_status = match result {
            Ok(_) => {
                if execution_record.status != WorkflowStatus::Paused {
                    execution_record.status = WorkflowStatus::Completed;
                    info!("Workflow {} completed in {:?}", graph.name, duration);
                    "completed".to_string()
                } else {
                    "paused".to_string()
                }
            }
            Err(ref e) => {
                execution_record.status = WorkflowStatus::Failed(e.clone());
                error!("Workflow {} failed: {}", graph.name, e);
                format!("failed: {}", e)
            }
        };
        execution_record.outputs = ctx.get_all_outputs().await;

        match &execution_record.status {
            WorkflowStatus::Failed(e) => {
                self.emit_event(ExecutionEvent::WorkflowFailed {
                    workflow_id: graph.id.clone(),
                    error: e.clone(),
                    total_duration_ms: duration.as_millis() as u64,
                })
                .await;
            }
            _ => {
                self.emit_event(ExecutionEvent::WorkflowCompleted {
                    workflow_id: graph.id.clone(),
                    final_output: None,
                    total_duration_ms: duration.as_millis() as u64,
                })
                .await;
            }
        }

        self.emit_debug(DebugEvent::WorkflowEnd {
            workflow_id: graph.id.clone(),
            execution_id: ctx.execution_id.clone(),
            timestamp_ms: DebugEvent::now_ms(),
            status: final_status,
        })
        .await;

        Ok(execution_record)
    }

    pub async fn resume_from_checkpoint(
        &self,
        graph: &WorkflowGraph,
        checkpoint: ExecutionCheckpoint,
    ) -> Result<ExecutionRecord, String> {
        let start_time = Instant::now();
        let mut ctx = WorkflowContext::new_with_id(&graph.id, checkpoint.execution_id.clone());

        self.emit_event(ExecutionEvent::WorkflowStarted {
            workflow_id: graph.id.clone(),
            workflow_name: graph.name.clone(),
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        })
        .await;

        info!(
            "Resuming workflow execution: {} ({} from checkpoint {})",
            graph.name, ctx.execution_id, checkpoint.execution_id
        );

        if let Err(errors) = graph.validate() {
            let error_msg = errors.join("; ");
            error!("Workflow validation failed: {}", error_msg);
            return Err(error_msg);
        }

        // Restore checkpoint data
        for (node_id, output) in checkpoint.node_outputs {
            ctx.set_node_output(&node_id, output).await;
        }
        for node_id in checkpoint.completed_nodes {
            ctx.set_node_status(&node_id, NodeStatus::Completed).await;
        }
        for (var_name, value) in checkpoint.variables {
            ctx.set_variable(&var_name, value).await;
        }

        let start_node_id = graph
            .start_node()
            .ok_or_else(|| "No start node".to_string())?;

        let mut execution_record = ExecutionRecord {
            execution_id: ctx.execution_id.clone(),
            workflow_id: graph.id.clone(),
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            ended_at: None,
            status: WorkflowStatus::Running,
            node_records: Vec::new(),
            outputs: HashMap::new(),
            total_wait_time_ms: 0,
            context: None,
        };

        let result = self
            .execute_from_node(
                graph,
                &ctx,
                start_node_id,
                WorkflowValue::Null,
                &mut execution_record,
            )
            .await;

        let duration = start_time.elapsed();
        execution_record.ended_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );

        let final_status = match result {
            Ok(_) => {
                if execution_record.status != WorkflowStatus::Paused {
                    execution_record.status = WorkflowStatus::Completed;
                    info!(
                        "Workflow {} resumed and completed in {:?}",
                        graph.name, duration
                    );
                    "completed".to_string()
                } else {
                    info!(
                        "Workflow {} resumed and paused again after {:?}",
                        graph.name, duration
                    );
                    // Preserve context so the caller can resume again later
                    execution_record.context = Some(ctx.clone());
                    "paused".to_string()
                }
            }
            Err(ref e) => {
                execution_record.status = WorkflowStatus::Failed(e.clone());
                error!("Workflow {} resumed and failed: {}", graph.name, e);
                format!("failed: {}", e)
            }
        };

        execution_record.outputs = ctx.get_all_outputs().await;

        match &execution_record.status {
            WorkflowStatus::Failed(e) => {
                self.emit_event(ExecutionEvent::WorkflowFailed {
                    workflow_id: graph.id.clone(),
                    error: e.clone(),
                    total_duration_ms: duration.as_millis() as u64,
                })
                .await;
            }
            _ => {
                self.emit_event(ExecutionEvent::WorkflowCompleted {
                    workflow_id: graph.id.clone(),
                    final_output: None,
                    total_duration_ms: duration.as_millis() as u64,
                })
                .await;
            }
        }

        // Emit debug telemetry: WorkflowEnd (was missing in resume path)
        self.emit_debug(DebugEvent::WorkflowEnd {
            workflow_id: graph.id.clone(),
            execution_id: ctx.execution_id.clone(),
            timestamp_ms: DebugEvent::now_ms(),
            status: final_status,
        })
        .await;

        Ok(execution_record)
    }

    /// 尝试跳过已完成节点
    /// Try to skip completed node
    async fn try_skip_completed_node(
        &self,
        graph: &WorkflowGraph,
        ctx: &WorkflowContext,
        node_id: &str,
    ) -> Option<(Option<String>, WorkflowValue)> {
        if ctx.get_node_status(node_id).await != Some(NodeStatus::Completed) {
            return None;
        }

        info!("Node {} already completed, skipping...", node_id);
        let output = ctx
            .get_node_output(node_id)
            .await
            .unwrap_or(WorkflowValue::Null);

        let node = graph.get_node(node_id)?;
        let next_node = self.determine_next_node(graph, node, &output).await;

        Some((next_node, output))
    }

    /// 从指定节点开始执行（迭代版本，避免递归异步问题）
    /// Execute from specified node (iterative version to avoid async recursion issues)
    async fn execute_from_node(
        &self,
        graph: &WorkflowGraph,
        ctx: &WorkflowContext,
        start_node_id: &str,
        initial_input: WorkflowValue,
        record: &mut ExecutionRecord,
    ) -> Result<WorkflowValue, String> {
        let mut current_node_id = start_node_id.to_string();
        let mut current_input = initial_input;

        loop {
            let node = graph
                .get_node(&current_node_id)
                .ok_or_else(|| format!("Node {} not found", current_node_id))?;

            // 1. Try to skip completed node
            if let Some((next_opt, output)) = self
                .try_skip_completed_node(graph, ctx, &current_node_id)
                .await
            {
                if let Some(next_id) = next_opt {
                    current_node_id = next_id;
                    current_input = output;
                    continue;
                } else {
                    info!("Workflow completed at node {}", current_node_id);
                    return Ok(output);
                }
            }

            // 2. Check for HITL review node (unified system or legacy Wait)
            if node.config.node_type == NodeType::Wait {
                // Use unified HITL system if review handler is available
                if let Some(ref review_handler) = self.review_handler {
                    info!(
                        "Requesting review at node: {} (unified HITL)",
                        current_node_id
                    );

                    // Create review context from workflow state
                    let review_context = self
                        .create_review_context(ctx, &current_node_id, &current_input)
                        .await;

                    // Request review
                    match review_handler
                        .request_node_review(&record.execution_id, &current_node_id, review_context)
                        .await
                    {
                        Ok(review_id) => {
                            info!("Review requested: {} - workflow paused", review_id.as_str());
                            *ctx.paused_at.write().await = Some(chrono::Utc::now());
                            *ctx.last_waiting_node.write().await = Some(current_node_id.clone());
                            ctx.set_node_status(&current_node_id, NodeStatus::Waiting)
                                .await;
                            record.status = WorkflowStatus::Paused;

                            // Store review ID in context variables for later retrieval
                            ctx.set_variable(
                                "review_id",
                                WorkflowValue::String(review_id.as_str().to_string()),
                            )
                            .await;

                            return Ok(WorkflowValue::Null);
                        }
                        Err(e) => {
                            error!(
                                "Failed to request review: {} - falling back to legacy Wait",
                                e
                            );
                            // Fall through to legacy Wait handling
                        }
                    }
                }

                // Legacy Wait node handling (fallback)
                info!(
                    "Pausing workflow at node: {} (legacy Wait)",
                    current_node_id
                );
                *ctx.paused_at.write().await = Some(chrono::Utc::now());
                *ctx.last_waiting_node.write().await = Some(current_node_id.clone());

                ctx.set_node_status(&current_node_id, NodeStatus::Waiting)
                    .await;
                record.status = WorkflowStatus::Paused;
                return Ok(WorkflowValue::Null);
            }

            // 3. Execute new node
            // Emit debug telemetry: NodeStart
            self.emit_debug(DebugEvent::NodeStart {
                node_id: current_node_id.clone(),
                timestamp_ms: DebugEvent::now_ms(),
                state_snapshot: serde_json::to_value(&current_input).unwrap_or_default(),
            })
            .await;

            let start_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            ctx.set_node_status(&current_node_id, NodeStatus::Running)
                .await;
            self.emit_event(ExecutionEvent::NodeStarted {
                node_id: current_node_id.clone(),
                node_name: node.config.name.clone(),
                parent_span_id: None,
            })
            .await;

            let result = match node.node_type() {
                NodeType::Parallel => {
                    self.execute_parallel(graph, ctx, node, current_input.clone(), record)
                        .await
                }
                NodeType::Join => self.execute_join(graph, ctx, node, record).await,
                NodeType::SubWorkflow => {
                    self.execute_sub_workflow(graph, ctx, node, current_input.clone(), record)
                        .await
                }
                NodeType::Wait => self.execute_wait(ctx, node, current_input.clone()).await,
                _ => {
                    let node_timeout =
                        std::time::Duration::from_millis(self.config.node_timeout_ms);
                    let result = match tokio::time::timeout(
                        node_timeout,
                        node.execute(ctx, current_input.clone()),
                    )
                    .await
                    {
                        Ok(r) => r,
                        Err(_) => {
                            warn!(
                                "Node {} timed out after {:?}",
                                current_node_id, node_timeout
                            );
                            NodeResult::failed(
                                &current_node_id,
                                &format!("Node timed out after {:?}", node_timeout),
                                node_timeout.as_millis() as u64,
                            )
                        }
                    };
                    ctx.set_node_output(&current_node_id, result.output.clone())
                        .await;
                    ctx.set_node_status(&current_node_id, result.status.clone())
                        .await;
                    let node_end_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    self.emit_event(ExecutionEvent::NodeCompleted {
                        node_id: current_node_id.clone(),
                        output: serde_json::to_value(&result.output).ok(),
                        duration_ms: node_end_ms.saturating_sub(start_time),
                    })
                    .await;
                    if result.status.is_success() {
                        Ok(result.output)
                    } else {
                        Err(result.error.unwrap_or_else(|| "Unknown error".to_string()))
                    }
                }
            };

            let end_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            // Emit debug telemetry: NodeEnd
            self.emit_debug(DebugEvent::NodeEnd {
                node_id: current_node_id.clone(),
                timestamp_ms: end_time,
                state_snapshot: match &result {
                    Ok(output) => serde_json::to_value(output).unwrap_or_default(),
                    Err(e) => serde_json::json!({"error": e}),
                },
                duration_ms: end_time.saturating_sub(start_time),
            })
            .await;

            // Record node execution
            record.node_records.push(NodeExecutionRecord {
                node_id: current_node_id.clone(),
                started_at: start_time,
                ended_at: end_time,
                status: ctx
                    .get_node_status(&current_node_id)
                    .await
                    .unwrap_or(NodeStatus::Pending),
                retry_count: 0,
            });

            // 检查点
            // Checkpoints
            if self.config.enable_checkpoints
                && self.config.checkpoint_interval > 0
                && !record.node_records.is_empty()
                && record
                    .node_records
                    .len()
                    .is_multiple_of(self.config.checkpoint_interval)
            {
                let label = format!("auto_checkpoint_{}", record.node_records.len());
                ctx.create_checkpoint(&label).await;
                self.emit_event(ExecutionEvent::CheckpointCreated { label })
                    .await;
            }

            // 处理结果
            // Handle result
            match result {
                Ok(output) => {
                    // 确定下一个节点
                    // Determine next node
                    let next = self.determine_next_node(graph, node, &output).await;

                    match next {
                        Some(next_node_id) => {
                            // 继续执行下一个节点
                            // Continue executing next node
                            current_node_id = next_node_id;
                            current_input = output;
                            // 继续循环
                            // Continue loop
                        }
                        None => {
                            // 没有下一个节点，返回当前输出
                            // No next node, return current output
                            return Ok(output);
                        }
                    }
                }
                Err(e) => {
                    // 尝试错误处理
                    // Attempt error handling
                    if let Some(error_handler) = graph.get_error_handler(&current_node_id) {
                        warn!(
                            "Node {} failed, executing error handler: {}",
                            current_node_id, error_handler
                        );
                        let error_input = WorkflowValue::Map({
                            let mut m = HashMap::new();
                            m.insert("error".to_string(), WorkflowValue::String(e.clone()));
                            m.insert(
                                "node_id".to_string(),
                                WorkflowValue::String(current_node_id.clone()),
                            );
                            m
                        });
                        current_node_id = error_handler.to_string();
                        current_input = error_input;
                        // 继续循环执行错误处理器
                        // Continue loop to execute error handler
                    } else if self.config.stop_on_failure {
                        return Err(e);
                    } else {
                        warn!("Node {} failed but continuing: {}", current_node_id, e);
                        // 尝试继续执行下一个节点
                        // Attempt to continue to next node
                        if let Some(next_node_id) = graph.get_next_node(&current_node_id, None) {
                            current_node_id = next_node_id.to_string();
                            current_input = WorkflowValue::Null;
                            // 继续循环
                            // Continue loop
                        } else {
                            return Err(e);
                        }
                    }
                }
            }
        }
    }

    /// 确定下一个节点
    /// Determine the next node
    async fn determine_next_node(
        &self,
        graph: &WorkflowGraph,
        node: &WorkflowNode,
        output: &WorkflowValue,
    ) -> Option<String> {
        let node_id = node.id();

        match node.node_type() {
            NodeType::Condition => {
                // 条件节点根据输出确定分支
                // Condition nodes determine branches based on output
                let condition = output.as_str().unwrap_or("false");
                graph
                    .get_next_node(node_id, Some(condition))
                    .map(|s| s.to_string())
            }
            NodeType::End => {
                // 结束节点没有后续
                // End nodes have no subsequent nodes
                None
            }
            _ => {
                // 其他节点获取默认下一个
                // Other nodes get the default next node
                graph.get_next_node(node_id, None).map(|s| s.to_string())
            }
        }
    }

    /// 执行并行节点
    /// Execute parallel nodes
    async fn execute_parallel(
        &self,
        graph: &WorkflowGraph,
        ctx: &WorkflowContext,
        node: &WorkflowNode,
        input: WorkflowValue,
        record: &mut ExecutionRecord,
    ) -> Result<WorkflowValue, String> {
        let branches = node.parallel_branches();

        if branches.is_empty() {
            // 如果没有指定分支，使用出边作为分支
            // If no branches specified, use outgoing edges as branches
            let edges = graph.get_outgoing_edges(node.id());
            let branch_ids: Vec<String> = edges.iter().map(|e| e.to.clone()).collect();

            if branch_ids.is_empty() {
                ctx.set_node_output(node.id(), input.clone()).await;
                ctx.set_node_status(node.id(), NodeStatus::Completed).await;
                return Ok(input);
            }

            let result = self
                .execute_branches_parallel(graph, ctx, &branch_ids, input, record)
                .await?;
            ctx.set_node_output(node.id(), result.clone()).await;
            ctx.set_node_status(node.id(), NodeStatus::Completed).await;
            return Ok(result);
        }

        let result = self
            .execute_branches_parallel(graph, ctx, branches, input, record)
            .await?;
        ctx.set_node_output(node.id(), result.clone()).await;
        ctx.set_node_status(node.id(), NodeStatus::Completed).await;
        Ok(result)
    }

    /// 并行执行多个分支
    /// Execute multiple branches in parallel
    async fn execute_branches_parallel(
        &self,
        graph: &WorkflowGraph,
        ctx: &WorkflowContext,
        branches: &[String],
        input: WorkflowValue,
        _record: &mut ExecutionRecord,
    ) -> Result<WorkflowValue, String> {
        let mut results = HashMap::new();
        let mut errors = Vec::new();
        let semaphore = Arc::clone(&self.semaphore);

        // 执行各分支（使用 tokio::task::JoinSet concurrency）
        // Execute branches concurrently using tokio::task::JoinSet
        tracing::debug!("Spawning {} parallel branches", branches.len());

        let mut join_set: tokio::task::JoinSet<Result<(String, WorkflowValue), String>> =
            tokio::task::JoinSet::new();
        for branch_id in branches {
            let input_clone = input.clone();
            let b_id = branch_id.clone();
            let ctx_clone = ctx.clone();
            let node_clone = graph.get_node(&b_id).cloned();
            let semaphore = Arc::clone(&semaphore);
            let node_timeout_ms = self.config.node_timeout_ms;

            join_set.spawn(async move {
                let start_time = std::time::Instant::now();
                let _permit = semaphore
                    .acquire_owned()
                    .await
                    .map_err(|_| "parallel execution semaphore closed".to_string())?;
                if let Some(node) = node_clone {
                    if ctx_clone.get_node_status(&b_id).await == Some(NodeStatus::Completed) {
                        if let Some(output) = ctx_clone.get_node_output(&b_id).await {
                            return Ok((b_id, output));
                        }
                        return Ok((b_id, WorkflowValue::Null));
                    }

                    let node_timeout = std::time::Duration::from_millis(node_timeout_ms);
                    let result = match tokio::time::timeout(
                        node_timeout,
                        node.execute(&ctx_clone, input_clone),
                    )
                    .await
                    {
                        Ok(r) => r,
                        Err(_) => {
                            tracing::warn!(
                                "Branch {} timed out after {:?}",
                                b_id, node_timeout
                            );
                            NodeResult::failed(
                                &b_id,
                                &format!("Node timed out after {:?}", node_timeout),
                                node_timeout_ms,
                            )
                        }
                    };
                    ctx_clone
                        .set_node_output(&b_id, result.output.clone())
                        .await;
                    ctx_clone
                        .set_node_status(&b_id, result.status.clone())
                        .await;

                    if result.status.is_success() {
                        let duration = start_time.elapsed();
                        tracing::debug!("Branch {} completed successfully in {:?}", b_id, duration);
                        Ok((b_id, result.output))
                    } else {
                        tracing::debug!("Branch {} failed", b_id);
                        Err(format!(
                            "{}: {}",
                            b_id,
                            result.error.unwrap_or_else(|| "Unknown error".to_string())
                        ))
                    }
                } else {
                    tracing::debug!("Branch {} not found", b_id);
                    Err(format!("Node {} not found", b_id))
                }
            });
        }

        // The result of parallel branches are merged into a single WorkflowValue::Map.
        // State merging: later branches overwrite earlier ones on key collision (last-write-wins)
        // TODO: Consider configurable reducers for custom merge logic
        // If `stop_on_failure` is enabled, the first error cancels all remaining branches.
        while let Some(res_join) = join_set.join_next().await {
            let res = res_join.map_err(|e| format!("Join error or panic: {}", e))?;
            match res {
                Ok((id, output)) => {
                    results.insert(id, output);
                }
                Err(e) => {
                    errors.push(e);
                    if self.config.stop_on_failure {
                        join_set.abort_all();
                        break;
                    }
                }
            }
        }

        if !errors.is_empty() && self.config.stop_on_failure {
            return Err(errors.join("; "));
        }

        Ok(WorkflowValue::Map(results))
    }

    /// 执行聚合节点
    /// Execute join nodes
    async fn execute_join(
        &self,
        _graph: &WorkflowGraph,
        ctx: &WorkflowContext,
        node: &WorkflowNode,
        _record: &mut ExecutionRecord,
    ) -> Result<WorkflowValue, String> {
        let wait_for = node.join_nodes();

        // 等待所有前置节点完成
        // Wait for all predecessor nodes to complete
        let mut all_completed = false;
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 1000;

        while !all_completed && attempts < MAX_ATTEMPTS {
            all_completed = true;
            for node_id in wait_for {
                match ctx.get_node_status(node_id).await {
                    Some(status) if status.is_terminal() => {}
                    _ => {
                        all_completed = false;
                        break;
                    }
                }
            }
            if !all_completed {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                attempts += 1;
            }
        }

        if !all_completed {
            return Err("Join timeout waiting for nodes".to_string());
        }

        // 收集所有前置节点的输出
        // Collect outputs from all predecessor nodes
        let outputs = ctx
            .get_node_outputs(&wait_for.iter().map(|s| s.as_str()).collect::<Vec<_>>())
            .await;

        // 执行节点（可能有转换函数）
        // Execute node (may contain transformation functions)
        let node_timeout = std::time::Duration::from_millis(self.config.node_timeout_ms);
        let result = match tokio::time::timeout(
            node_timeout,
            node.execute(ctx, WorkflowValue::Map(outputs)),
        )
        .await
        {
            Ok(r) => r,
            Err(_) => {
                warn!(
                    "Join node {} timed out after {:?}",
                    node.id(), node_timeout
                );
                NodeResult::failed(
                    node.id(),
                    &format!("Join node timed out after {:?}", node_timeout),
                    self.config.node_timeout_ms,
                )
            }
        };

        ctx.set_node_output(node.id(), result.output.clone()).await;
        ctx.set_node_status(node.id(), result.status.clone()).await;

        if result.status.is_success() {
            Ok(result.output)
        } else {
            Err(result.error.unwrap_or_else(|| "Join failed".to_string()))
        }
    }

    /// 执行子工作流
    /// Execute sub-workflow
    /// 注意：子工作流执行使用独立的执行上下文
    /// Note: Sub-workflow execution uses an independent execution context
    async fn execute_sub_workflow(
        &self,
        _graph: &WorkflowGraph,
        ctx: &WorkflowContext,
        node: &WorkflowNode,
        input: WorkflowValue,
        _record: &mut ExecutionRecord,
    ) -> Result<WorkflowValue, String> {
        let sub_workflow_id = node
            .sub_workflow_id()
            .ok_or_else(|| "No sub-workflow specified".to_string())?;

        let workflows = self.sub_workflows.read().await;
        let sub_graph = workflows
            .get(sub_workflow_id)
            .ok_or_else(|| format!("Sub-workflow {} not found", sub_workflow_id))?
            .clone();
        drop(workflows);

        info!("Executing sub-workflow: {}", sub_workflow_id);

        // 使用 execute_parallel_workflow 而不是 execute 来避免递归
        // Use execute_parallel_workflow instead of execute to avoid recursion
        // 这样可以避免无限递归的 Future 大小问题
        // This avoids Future size issues caused by infinite recursion
        let sub_record = self.execute_parallel_workflow(&sub_graph, input).await?;

        // 获取子工作流的最终输出
        // Get the final output of the sub-workflow
        let output = if let Some(end_node) = sub_graph.end_nodes().first() {
            sub_record
                .outputs
                .get(end_node)
                .cloned()
                .unwrap_or(WorkflowValue::Null)
        } else {
            WorkflowValue::Null
        };
        ctx.set_node_output(node.id(), output.clone()).await;
        ctx.set_node_status(node.id(), NodeStatus::Completed).await;

        Ok(output)
    }

    /// 执行等待节点
    /// Execute wait node
    async fn execute_wait(
        &self,
        ctx: &WorkflowContext,
        node: &WorkflowNode,
        _input: WorkflowValue,
    ) -> Result<WorkflowValue, String> {
        let event_type = node
            .wait_event_type()
            .ok_or_else(|| "No event type specified".to_string())?;

        info!("Waiting for event: {}", event_type);

        // 创建等待通道
        // Create waiting channel
        let (tx, rx) = oneshot::channel();

        {
            let mut waiters = self.event_waiters.write().await;
            waiters.entry(event_type.to_string()).or_default().push(tx);
        }

        // 等待事件或超时
        // Wait for event or timeout
        let timeout = node.config.timeout.execution_timeout_ms;
        let result = if timeout > 0 {
            tokio::time::timeout(std::time::Duration::from_millis(timeout), rx)
                .await
                .map_err(|_| "Wait timeout".to_string())?
                .map_err(|_| "Wait cancelled".to_string())?
        } else {
            rx.await.map_err(|_| "Wait cancelled".to_string())?
        };

        ctx.set_node_output(node.id(), result.clone()).await;
        ctx.set_node_status(node.id(), NodeStatus::Completed).await;

        Ok(result)
    }

    /// 基于拓扑层次执行工作流
    /// Execute workflow based on topological layers
    /// 同一层的节点并行执行
    /// Nodes in the same layer are executed in parallel
    pub async fn execute_parallel_workflow(
        &self,
        graph: &WorkflowGraph,
        input: WorkflowValue,
    ) -> Result<ExecutionRecord, String> {
        let ctx = WorkflowContext::new(&graph.id);
        ctx.set_input(input.clone()).await;

        let start_time = Instant::now();

        info!(
            "Starting layered workflow execution: {} ({})",
            graph.name, ctx.execution_id
        );

        // 获取并行组（按拓扑层次分组）
        // Get parallel groups (grouped by topological layers)
        let groups = graph.get_parallel_groups();

        let mut execution_record = ExecutionRecord {
            execution_id: ctx.execution_id.clone(),
            workflow_id: graph.id.clone(),
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            ended_at: None,
            status: WorkflowStatus::Running,
            node_records: Vec::new(),
            outputs: HashMap::new(),
            total_wait_time_ms: 0,
            context: None,
        };

        let ctx_ref = &ctx;
        let semaphore = Arc::clone(&self.semaphore);

        // 按层次执行（同一层次的节点可以并发执行）
        // Execute by layer (nodes in same layer execute concurrently)
        for group in groups {
            tracing::debug!("Spawning {} parallel branches in layer", group.len());
            let mut join_set: tokio::task::JoinSet<(NodeResult, NodeExecutionRecord)> =
                tokio::task::JoinSet::new();
            for node_id in &group {
                let n_id = node_id.clone();
                let ctx_clone = ctx_ref.clone();
                let node_clone = graph.get_node(&n_id).cloned();
                let semaphore = Arc::clone(&semaphore);
                let node_timeout_ms = self.config.node_timeout_ms;
                let predecessors: Vec<String> = graph
                    .get_predecessors(&n_id)
                    .into_iter()
                    .map(String::from)
                    .collect();

                join_set.spawn(async move {
                    let node_start_time = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let _permit = match semaphore.acquire_owned().await {
                        Ok(permit) => permit,
                        Err(e) => {
                            let result = NodeResult::failed(
                                &n_id,
                                &format!("Parallel semaphore closed: {}", e),
                                0,
                            );
                            let record_entry = NodeExecutionRecord {
                                node_id: n_id,
                                started_at: node_start_time,
                                ended_at: node_start_time,
                                status: result.status.clone(),
                                retry_count: result.retry_count,
                            };
                            return (result, record_entry);
                        }
                    };

                    let result = if let Some(node) = node_clone {
                        if ctx_clone.get_node_status(&n_id).await == Some(NodeStatus::Completed) {
                            info!("Skipping already completed node: {}", n_id);
                            NodeResult::success(
                                &n_id,
                                ctx_clone
                                    .get_node_output(&n_id)
                                    .await
                                    .unwrap_or(WorkflowValue::Null),
                                0,
                            )
                        } else {
                            let node_input = if predecessors.is_empty() {
                                ctx_clone.get_input().await
                            } else if predecessors.len() == 1 {
                                ctx_clone
                                    .get_node_output(&predecessors[0])
                                    .await
                                    .unwrap_or(WorkflowValue::Null)
                            } else {
                                let pred_refs: Vec<&str> =
                                    predecessors.iter().map(|s| s.as_str()).collect();
                                let outputs = ctx_clone.get_node_outputs(&pred_refs).await;
                                WorkflowValue::Map(outputs)
                            };
                            let node_timeout = std::time::Duration::from_millis(node_timeout_ms);
                            let result = match tokio::time::timeout(
                                node_timeout,
                                node.execute(&ctx_clone, node_input),
                            )
                            .await
                            {
                                Ok(r) => r,
                                Err(_) => {
                                    tracing::warn!(
                                        "Node {} timed out after {:?}",
                                        n_id, node_timeout
                                    );
                                    NodeResult::failed(
                                        &n_id,
                                        &format!("Node timed out after {:?}", node_timeout),
                                        node_timeout_ms,
                                    )
                                }
                            };
                            ctx_clone
                                .set_node_output(&n_id, result.output.clone())
                                .await;
                            ctx_clone
                                .set_node_status(&n_id, result.status.clone())
                                .await;
                            result
                        }
                    } else {
                        NodeResult::failed(&n_id, "Node not found", 0)
                    };

                    let node_end_time = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;

                    tracing::debug!(
                        "Branch {} completed in {}ms",
                        n_id,
                        node_end_time.saturating_sub(node_start_time)
                    );

                    let record_entry = NodeExecutionRecord {
                        node_id: n_id,
                        started_at: node_start_time,
                        ended_at: node_end_time,
                        status: result.status.clone(),
                        retry_count: result.retry_count,
                    };

                    (result, record_entry)
                });
            }

            // Wait for all nodes in this layer to finish.
            // Node updates are written synchronously to the WorkflowContext as each task finishes.
            // If `stop_on_failure` is enabled, any failure will abort remaining tasks in the layer.
            while let Some(res_join) = join_set.join_next().await {
                let (result, record_entry) = res_join.unwrap_or_else(|e| {
                    (
                        NodeResult::failed("unknown", &format!("Join error or panic: {}", e), 0),
                        NodeExecutionRecord {
                            node_id: "unknown".to_string(),
                            started_at: 0,
                            ended_at: 0,
                            status: NodeStatus::Failed(format!("Join panic: {}", e)),
                            retry_count: 0,
                        },
                    )
                });
                execution_record.node_records.push(record_entry);

                if !result.status.is_success() && self.config.stop_on_failure {
                    join_set.abort_all();
                    execution_record.status = WorkflowStatus::Failed(
                        result.error.unwrap_or_else(|| "Unknown error".to_string()),
                    );
                    execution_record.ended_at = Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                    );
                    execution_record.outputs = ctx_ref.get_all_outputs().await;
                    return Ok(execution_record);
                }
            }
        }

        let duration = start_time.elapsed();
        execution_record.status = WorkflowStatus::Completed;
        execution_record.ended_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );

        execution_record.outputs = ctx.get_all_outputs().await;

        info!(
            "Layered workflow {} completed in {:?}",
            graph.name, duration
        );

        Ok(execution_record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{Duration, Instant, sleep};

    #[tokio::test]
    async fn test_simple_workflow_execution() {
        let mut graph = WorkflowGraph::new("test", "Simple Workflow");

        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::task(
            "double",
            "Double",
            |_ctx, input| async move {
                let value = input.as_i64().unwrap_or(0);
                Ok(WorkflowValue::Int(value * 2))
            },
        ));
        graph.add_node(WorkflowNode::task(
            "add_ten",
            "Add Ten",
            |_ctx, input| async move {
                let value = input.as_i64().unwrap_or(0);
                Ok(WorkflowValue::Int(value + 10))
            },
        ));
        graph.add_node(WorkflowNode::end("end"));

        graph.connect("start", "double");
        graph.connect("double", "add_ten");
        graph.connect("add_ten", "end");

        let executor = WorkflowExecutor::new(ExecutorConfig::default());
        let result = executor
            .execute(&graph, WorkflowValue::Int(5))
            .await
            .unwrap();

        assert!(matches!(result.status, WorkflowStatus::Completed));
    }

    #[tokio::test]
    async fn test_conditional_workflow() {
        let mut graph = WorkflowGraph::new("test", "Conditional Workflow");

        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::condition(
            "check",
            "Check Value",
            |_ctx, input| async move { input.as_i64().unwrap_or(0) > 10 },
        ));
        graph.add_node(WorkflowNode::task(
            "high",
            "High Path",
            |_ctx, _input| async move { Ok(WorkflowValue::String("high".to_string())) },
        ));
        graph.add_node(WorkflowNode::task(
            "low",
            "Low Path",
            |_ctx, _input| async move { Ok(WorkflowValue::String("low".to_string())) },
        ));
        graph.add_node(WorkflowNode::end("end"));

        graph.connect("start", "check");
        graph.connect_conditional("check", "high", "true");
        graph.connect_conditional("check", "low", "false");
        graph.connect("high", "end");
        graph.connect("low", "end");

        let executor = WorkflowExecutor::new(ExecutorConfig::default());

        // 测试高路径
        // Test high path
        let result = executor
            .execute(&graph, WorkflowValue::Int(20))
            .await
            .unwrap();
        assert!(matches!(result.status, WorkflowStatus::Completed));
    }

    #[tokio::test]
    async fn test_checkpoint_resume() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let mut graph = WorkflowGraph::new("test", "Checkpoint Workflow");

        let step1_count = Arc::new(AtomicUsize::new(0));
        let step2_count = Arc::new(AtomicUsize::new(0));

        let step1_count_clone = Arc::clone(&step1_count);
        let step2_count_clone = Arc::clone(&step2_count);

        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::task(
            "step1",
            "Step 1",
            move |_ctx, _input| {
                let count = Arc::clone(&step1_count_clone);
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(WorkflowValue::String("step1_done".to_string()))
                }
            },
        ));
        graph.add_node(WorkflowNode::task(
            "step2",
            "Step 2",
            move |_ctx, _input| {
                let count = Arc::clone(&step2_count_clone);
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(WorkflowValue::String("step2_done".to_string()))
                }
            },
        ));
        graph.add_node(WorkflowNode::end("end"));

        graph.connect("start", "step1");
        graph.connect("step1", "step2");
        graph.connect("step2", "end");

        let executor = WorkflowExecutor::new(ExecutorConfig::default());

        //simulate crashing after step1
        let mut node_outputs = HashMap::new();
        node_outputs.insert("start".to_string(), WorkflowValue::Null);
        node_outputs.insert(
            "step1".to_string(),
            WorkflowValue::String("step1_done".to_string()),
        );

        let checkpoint = ExecutionCheckpoint {
            execution_id: "test-exec-id".to_string(),
            workflow_id: "test".to_string(),
            completed_nodes: vec!["start".to_string(), "step1".to_string()],
            node_outputs,
            variables: HashMap::new(),
            timestamp: 0,
        };

        let result2 = executor
            .resume_from_checkpoint(&graph, checkpoint)
            .await
            .unwrap();
        assert!(matches!(result2.status, WorkflowStatus::Completed));

        assert_eq!(
            step1_count.load(Ordering::SeqCst),
            0,
            "Step1 should be skipped"
        );
        assert_eq!(
            step2_count.load(Ordering::SeqCst),
            1,
            "Step2 should be executed"
        );
    }

    #[tokio::test]
    async fn test_sub_workflow_output() {
        let executor = WorkflowExecutor::new(ExecutorConfig::default());

        let mut sub_graph = WorkflowGraph::new("sub_wf", "Sub Workflow");
        sub_graph.add_node(WorkflowNode::start("sub_start"));
        sub_graph.add_node(WorkflowNode::task(
            "sub_task",
            "Sub Task",
            |_ctx, _input| async move { Ok(WorkflowValue::String("hello from sub".to_string())) },
        ));
        sub_graph.add_node(WorkflowNode::end("sub_end"));
        sub_graph.connect("sub_start", "sub_task");
        sub_graph.connect("sub_task", "sub_end");

        executor.register_sub_workflow("sub_wf", sub_graph).await;

        let mut parent_graph = WorkflowGraph::new("parent_wf", "Parent Workflow");
        parent_graph.add_node(WorkflowNode::start("parent_start"));
        parent_graph.add_node(WorkflowNode::sub_workflow(
            "call_sub",
            "Call Sub Workflow",
            "sub_wf",
        ));
        parent_graph.add_node(WorkflowNode::end("parent_end"));
        parent_graph.connect("parent_start", "call_sub");
        parent_graph.connect("call_sub", "parent_end");

        let result = executor
            .execute(&parent_graph, WorkflowValue::Null)
            .await
            .expect("Workflow execution failed");

        assert!(matches!(result.status, WorkflowStatus::Completed));

        let sub_output = result
            .outputs
            .get("call_sub")
            .cloned()
            .unwrap_or(WorkflowValue::Null);

        assert_eq!(
            sub_output.as_str().unwrap_or("Null"),
            "hello from sub",
            "Sub-workflow output was discarded!"
        );
    }

    #[tokio::test]
    async fn test_parallel_output() {
        let executor = WorkflowExecutor::new(ExecutorConfig::default());
        let mut graph = WorkflowGraph::new("parallel_wf", "Parallel Output Workflow");

        graph.add_node(WorkflowNode::start("start"));

        // Add parallel node
        graph.add_node(WorkflowNode::parallel(
            "parallel_split",
            "Split execution",
            vec!["branch_a", "branch_b"],
        ));

        // Add branches
        graph.add_node(WorkflowNode::task(
            "branch_a",
            "Branch A",
            |_ctx, _input| async move { Ok(WorkflowValue::String("result_from_a".to_string())) },
        ));
        graph.add_node(WorkflowNode::task(
            "branch_b",
            "Branch B",
            |_ctx, _input| async move { Ok(WorkflowValue::String("result_from_b".to_string())) },
        ));

        graph.add_node(WorkflowNode::end("end"));

        graph.connect("start", "parallel_split");
        graph.connect("parallel_split", "branch_a");
        graph.connect("parallel_split", "branch_b");
        graph.connect("branch_a", "end");
        graph.connect("branch_b", "end");

        let result = executor
            .execute(&graph, WorkflowValue::Null)
            .await
            .expect("Workflow execution failed");

        assert!(matches!(result.status, WorkflowStatus::Completed));

        let parallel_output = result
            .outputs
            .get("parallel_split")
            .cloned()
            .unwrap_or(WorkflowValue::Null);
        let map = parallel_output.as_map().cloned().unwrap_or_default();

        assert_eq!(
            map.get("branch_a").and_then(|v| v.as_str()),
            Some("result_from_a"),
            "Parallel node output missing branch A"
        );
        assert_eq!(
            map.get("branch_b").and_then(|v| v.as_str()),
            Some("result_from_b"),
            "Parallel node output missing branch B"
        );
    }

    #[tokio::test]
    async fn test_parallel_branches_execute_concurrently() {
        let executor = WorkflowExecutor::new(ExecutorConfig::default());
        let mut graph = WorkflowGraph::new("parallel_timing_wf", "Parallel Timing Workflow");

        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::parallel(
            "parallel_split",
            "Split execution",
            vec!["branch_a", "branch_b"],
        ));
        graph.add_node(WorkflowNode::task(
            "branch_a",
            "Branch A",
            |_ctx, _input| async move {
                sleep(Duration::from_millis(300)).await;
                Ok(WorkflowValue::String("a_done".to_string()))
            },
        ));
        graph.add_node(WorkflowNode::task(
            "branch_b",
            "Branch B",
            |_ctx, _input| async move {
                sleep(Duration::from_millis(300)).await;
                Ok(WorkflowValue::String("b_done".to_string()))
            },
        ));
        graph.add_node(WorkflowNode::end("end"));

        graph.connect("start", "parallel_split");
        graph.connect("parallel_split", "branch_a");
        graph.connect("parallel_split", "branch_b");
        graph.connect("branch_a", "end");
        graph.connect("branch_b", "end");

        let started = Instant::now();
        let result = executor
            .execute(&graph, WorkflowValue::Null)
            .await
            .expect("Workflow execution failed");
        let elapsed = started.elapsed();

        assert!(matches!(result.status, WorkflowStatus::Completed));
        assert!(
            elapsed < Duration::from_millis(500),
            "Expected parallel execution under 500ms, got {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_parallel_branch_input_isolation() {
        let executor = WorkflowExecutor::new(ExecutorConfig::default());
        let mut graph =
            WorkflowGraph::new("parallel_input_wf", "Parallel Input Isolation Workflow");

        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::parallel(
            "parallel_split",
            "Split execution",
            vec!["branch_a", "branch_b"],
        ));

        graph.add_node(WorkflowNode::task(
            "branch_a",
            "Branch A",
            |_ctx, input| async move {
                let mut map = input.as_map().cloned().unwrap_or_default();
                map.insert("branch".to_string(), WorkflowValue::String("a".to_string()));
                Ok(WorkflowValue::Map(map))
            },
        ));
        graph.add_node(WorkflowNode::task(
            "branch_b",
            "Branch B",
            |_ctx, input| async move { Ok(input) },
        ));
        graph.add_node(WorkflowNode::end("end"));

        graph.connect("start", "parallel_split");
        graph.connect("parallel_split", "branch_a");
        graph.connect("parallel_split", "branch_b");
        graph.connect("branch_a", "end");
        graph.connect("branch_b", "end");

        let mut input = HashMap::new();
        input.insert("seed".to_string(), WorkflowValue::Int(7));

        let result = executor
            .execute(&graph, WorkflowValue::Map(input))
            .await
            .expect("Workflow execution failed");

        let split_map = result
            .outputs
            .get("parallel_split")
            .and_then(|v| v.as_map())
            .cloned()
            .expect("parallel_split output must be map");

        let branch_a = split_map
            .get("branch_a")
            .and_then(|v| v.as_map())
            .cloned()
            .expect("branch_a output must be map");
        let branch_b = split_map
            .get("branch_b")
            .and_then(|v| v.as_map())
            .cloned()
            .expect("branch_b output must be map");

        assert_eq!(branch_a.get("branch").and_then(|v| v.as_str()), Some("a"));
        assert!(
            !branch_b.contains_key("branch"),
            "branch_b should not observe branch_a input mutation"
        );
        assert_eq!(branch_b.get("seed").and_then(|v| v.as_i64()), Some(7));
    }

    #[tokio::test]
    async fn test_execution_timeout_enforcement() {
        let config = ExecutorConfig {
            execution_timeout_ms: Some(100),
            ..ExecutorConfig::default()
        };
        let executor = WorkflowExecutor::new(config);

        let mut graph = WorkflowGraph::new("timeout_wf", "Timeout Workflow");
        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::task(
            "slow_task",
            "Slow Task",
            |_ctx, _input| async move {
                sleep(Duration::from_millis(500)).await;
                Ok(WorkflowValue::String("done".to_string()))
            },
        ));
        graph.add_node(WorkflowNode::end("end"));
        graph.connect("start", "slow_task");
        graph.connect("slow_task", "end");

        let record = executor
            .execute(&graph, WorkflowValue::Null)
            .await
            .expect("execute() should return Ok(record) even on timeout");

        match &record.status {
            WorkflowStatus::Failed(msg) => {
                assert!(
                    msg.contains("timed out"),
                    "Expected timeout message, got: {}",
                    msg
                );
            }
            other => panic!("Expected Failed status with timeout, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_no_timeout_when_none() {
        let config = ExecutorConfig {
            execution_timeout_ms: None,
            ..ExecutorConfig::default()
        };
        let executor = WorkflowExecutor::new(config);

        let mut graph = WorkflowGraph::new("no_timeout_wf", "No Timeout Workflow");
        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::task(
            "fast_task",
            "Fast Task",
            |_ctx, _input| async move { Ok(WorkflowValue::String("fast".to_string())) },
        ));
        graph.add_node(WorkflowNode::end("end"));
        graph.connect("start", "fast_task");
        graph.connect("fast_task", "end");

        let result = executor
            .execute(&graph, WorkflowValue::Null)
            .await
            .expect("Should complete without timeout");
        assert!(matches!(result.status, WorkflowStatus::Completed));
    }

    // -------------------------------------------------------------------------
    // Tests for fix #994: resume_from_checkpoint must preserve Paused status
    // and emit WorkflowEnd telemetry.
    // -------------------------------------------------------------------------

    /// Build a simple graph that goes: start → task → wait → end.
    /// The wait node simulates the point at which a human-in-the-loop review
    /// would pause the workflow.
    fn make_wait_graph(id: &str) -> WorkflowGraph {
        let mut graph = WorkflowGraph::new(id, "Wait-node workflow");
        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::task(
            "task1",
            "First task",
            |_ctx, _input| async move { Ok(WorkflowValue::String("step1_done".into())) },
        ));
        graph.add_node(WorkflowNode::wait(
            "wait_review",
            "Await review",
            "human_review",
        ));
        graph.add_node(WorkflowNode::end("end"));

        graph.connect("start", "task1");
        graph.connect("task1", "wait_review");
        graph.connect("wait_review", "end");
        graph
    }

    /// Bug regression guard: the old code unconditionally set status to
    /// `Completed` inside `resume_from_checkpoint`, even when the execution hit
    /// another Wait node and set the status to `Paused`.
    ///
    /// After the fix, when `execute_from_node` marks the record as `Paused`,
    /// `resume_from_checkpoint` must NOT overwrite it with `Completed`.
    #[tokio::test]
    async fn test_resume_from_checkpoint_preserves_paused_status() {
        let executor = WorkflowExecutor::new(ExecutorConfig::default());
        let graph = make_wait_graph("resume_paused_wf");

        // --- First run: execute until the Wait node pauses the workflow ---
        let first_run = executor
            .execute(&graph, WorkflowValue::Null)
            .await
            .expect("First execute should succeed");

        assert!(
            matches!(first_run.status, WorkflowStatus::Paused),
            "Expected Paused after reaching Wait node, got: {:?}",
            first_run.status
        );
        assert!(
            first_run.context.is_some(),
            "ExecutionRecord.context must be populated when workflow is Paused"
        );

        // --- Build a checkpoint from the node outputs collected so far ---
        //     In a real application this would come from a persisted store; we
        //     synthesise it from the first run's outputs to verify the path.
        let checkpoint = ExecutionCheckpoint {
            execution_id: first_run.execution_id.clone(),
            workflow_id: "resume_paused_wf".to_string(),
            // The task node completed; the wait node did NOT complete.
            completed_nodes: vec!["start".to_string(), "task1".to_string()],
            node_outputs: first_run.outputs.clone(),
            variables: std::collections::HashMap::new(),
            timestamp: 0,
        };

        // --- Resume from checkpoint: the workflow will hit the Wait node again ---
        let resumed = executor
            .resume_from_checkpoint(&graph, checkpoint)
            .await
            .expect("resume_from_checkpoint should return Ok");

        // Core invariant: status must still be Paused, NOT Completed.
        assert!(
            matches!(resumed.status, WorkflowStatus::Paused),
            "BUG #994: resume_from_checkpoint must keep Paused when a Wait node \
             is encountered during resume. Got: {:?}",
            resumed.status
        );

        // The context must be preserved for a subsequent resume cycle.
        assert!(
            resumed.context.is_some(),
            "ExecutionRecord.context must be set when resume ends with Paused"
        );
    }

    /// A workflow with NO wait nodes should reach `Completed` when resumed from
    /// a checkpoint, confirming the fix does not regress the happy path.
    #[tokio::test]
    async fn test_resume_from_checkpoint_completes_when_no_wait_node() {
        let executor = WorkflowExecutor::new(ExecutorConfig::default());

        let mut graph = WorkflowGraph::new("resume_complete_wf", "No-wait workflow");
        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::task(
            "task1",
            "Task 1",
            |_ctx, _input| async move { Ok(WorkflowValue::String("result".into())) },
        ));
        graph.add_node(WorkflowNode::end("end"));
        graph.connect("start", "task1");
        graph.connect("task1", "end");

        // Build a checkpoint that simulates having already completed `start`.
        let checkpoint = ExecutionCheckpoint {
            execution_id: uuid::Uuid::now_v7().to_string(),
            workflow_id: "resume_complete_wf".to_string(),
            completed_nodes: vec!["start".to_string()],
            node_outputs: std::collections::HashMap::new(),
            variables: std::collections::HashMap::new(),
            timestamp: 0,
        };

        let record = executor
            .resume_from_checkpoint(&graph, checkpoint)
            .await
            .expect("resume should succeed");

        assert!(
            matches!(record.status, WorkflowStatus::Completed),
            "Expected Completed for no-wait resume, got: {:?}",
            record.status
        );
    }

    /// `resume_from_checkpoint` must surface `Failed` status when a task node
    /// returns an error – the fix must not break error propagation.
    #[tokio::test]
    async fn test_resume_from_checkpoint_propagates_failure() {
        let executor = WorkflowExecutor::new(ExecutorConfig::default());

        let mut graph = WorkflowGraph::new("resume_fail_wf", "Failing resume workflow");
        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::task(
            "bad_task",
            "Bad task",
            |_ctx, _input| async move { Err("intentional failure".to_string()) },
        ));
        graph.add_node(WorkflowNode::end("end"));
        graph.connect("start", "bad_task");
        graph.connect("bad_task", "end");

        let checkpoint = ExecutionCheckpoint {
            execution_id: uuid::Uuid::now_v7().to_string(),
            workflow_id: "resume_fail_wf".to_string(),
            completed_nodes: vec!["start".to_string()],
            node_outputs: std::collections::HashMap::new(),
            variables: std::collections::HashMap::new(),
            timestamp: 0,
        };

        let record = executor
            .resume_from_checkpoint(&graph, checkpoint)
            .await
            .expect("resume_from_checkpoint itself should return Ok");

        match &record.status {
            WorkflowStatus::Failed(msg) => {
                assert!(
                    msg.contains("intentional failure"),
                    "Unexpected failure message: {}",
                    msg
                );
            }
            other => panic!("Expected Failed status from resume, got: {:?}", other),
        }
    }

    /// Verify that `WorkflowEnd` telemetry is emitted during
    /// `resume_from_checkpoint`.  We attach a simple in-memory collector and
    /// confirm that at least one `WorkflowEnd` event is recorded after resume.
    #[tokio::test]
    async fn test_resume_from_checkpoint_emits_workflow_end_telemetry() {
        use mofa_kernel::workflow::telemetry::{DebugEvent, TelemetryEmitter};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        // Minimal in-process collector
        #[derive(Clone)]
        struct TestEmitter {
            events: Arc<RwLock<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl TelemetryEmitter for TestEmitter {
            async fn emit(&self, event: DebugEvent) {
                if matches!(event, DebugEvent::WorkflowEnd { .. }) {
                    self.events.write().await.push("WorkflowEnd".to_string());
                }
            }
            fn is_enabled(&self) -> bool {
                true
            }
        }

        let collected: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new(Vec::new()));
        let emitter = Arc::new(TestEmitter {
            events: collected.clone(),
        });

        let executor = WorkflowExecutor::new(ExecutorConfig::default()).with_telemetry(emitter);

        let mut graph = WorkflowGraph::new("telemetry_resume_wf", "Telemetry resume test");
        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::task(
            "task1",
            "Task 1",
            |_ctx, _input| async move { Ok(WorkflowValue::String("ok".into())) },
        ));
        graph.add_node(WorkflowNode::end("end"));
        graph.connect("start", "task1");
        graph.connect("task1", "end");

        let checkpoint = ExecutionCheckpoint {
            execution_id: uuid::Uuid::now_v7().to_string(),
            workflow_id: "telemetry_resume_wf".to_string(),
            completed_nodes: vec!["start".to_string()],
            node_outputs: std::collections::HashMap::new(),
            variables: std::collections::HashMap::new(),
            timestamp: 0,
        };

        executor
            .resume_from_checkpoint(&graph, checkpoint)
            .await
            .expect("resume should succeed");

        let events = collected.read().await;
        assert!(
            events.iter().any(|e| e == "WorkflowEnd"),
            "Expected WorkflowEnd telemetry event from resume_from_checkpoint, \
             got events: {:?}",
            *events
        );
    }

    // -------------------------------------------------------------------------
    // Tests for fix #1428: node_timeout_ms must be enforced in parallel paths
    // -------------------------------------------------------------------------

    /// Verify that `node_timeout_ms` is enforced within `execute_branches_parallel`.
    /// A parallel branch that exceeds the configured timeout must be terminated
    /// and produce a `Failed` status containing "timed out".
    #[tokio::test]
    async fn test_parallel_branch_timeout_enforcement() {
        let config = ExecutorConfig {
            node_timeout_ms: 100, // 100ms timeout
            ..ExecutorConfig::default()
        };
        let executor = WorkflowExecutor::new(config);

        let mut graph = WorkflowGraph::new("par_timeout_wf", "Parallel Branch Timeout");
        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::parallel(
            "par_split",
            "Split",
            vec!["slow_branch", "fast_branch"],
        ));
        graph.add_node(WorkflowNode::task(
            "slow_branch",
            "Slow Branch",
            |_ctx, _input| async move {
                sleep(Duration::from_secs(5)).await;
                Ok(WorkflowValue::String("should_not_reach".to_string()))
            },
        ));
        graph.add_node(WorkflowNode::task(
            "fast_branch",
            "Fast Branch",
            |_ctx, _input| async move {
                Ok(WorkflowValue::String("fast_done".to_string()))
            },
        ));
        graph.add_node(WorkflowNode::end("end"));

        graph.connect("start", "par_split");
        graph.connect("par_split", "slow_branch");
        graph.connect("par_split", "fast_branch");
        graph.connect("slow_branch", "end");
        graph.connect("fast_branch", "end");

        let started = Instant::now();
        let record = executor
            .execute(&graph, WorkflowValue::Null)
            .await
            .expect("execute() should return Ok");
        let elapsed = started.elapsed();

        // Should complete well under 5s (the slow_branch sleep duration)
        assert!(
            elapsed < Duration::from_secs(2),
            "Parallel branch timeout was not enforced — took {:?}",
            elapsed
        );

        // With stop_on_failure=true (default), the workflow should fail
        match &record.status {
            WorkflowStatus::Failed(msg) => {
                assert!(
                    msg.contains("timed out"),
                    "Expected timeout error, got: {}",
                    msg
                );
            }
            other => panic!(
                "Expected Failed status from timed-out parallel branch, got: {:?}",
                other
            ),
        }
    }

    /// Verify that `node_timeout_ms` is enforced in `execute_parallel_workflow`.
    /// A node in the layered execution path that exceeds the timeout must be
    /// terminated and marked as failed.
    #[tokio::test]
    async fn test_parallel_workflow_timeout_enforcement() {
        let config = ExecutorConfig {
            node_timeout_ms: 100,
            stop_on_failure: true,
            ..ExecutorConfig::default()
        };
        let executor = WorkflowExecutor::new(config);

        let mut graph = WorkflowGraph::new("layered_timeout_wf", "Layered Timeout");
        graph.add_node(WorkflowNode::start("start"));
        graph.add_node(WorkflowNode::task(
            "hanging_task",
            "Hanging Task",
            |_ctx, _input| async move {
                sleep(Duration::from_secs(5)).await;
                Ok(WorkflowValue::String("should_not_reach".to_string()))
            },
        ));
        graph.add_node(WorkflowNode::end("end"));
        graph.connect("start", "hanging_task");
        graph.connect("hanging_task", "end");

        let started = Instant::now();
        let record = executor
            .execute_parallel_workflow(&graph, WorkflowValue::Null)
            .await
            .expect("execute_parallel_workflow should return Ok");
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(2),
            "Layered parallel timeout was not enforced — took {:?}",
            elapsed
        );

        match &record.status {
            WorkflowStatus::Failed(msg) => {
                assert!(
                    msg.contains("timed out"),
                    "Expected timeout error, got: {}",
                    msg
                );
            }
            other => panic!(
                "Expected Failed status from timed-out layered node, got: {:?}",
                other
            ),
        }
    }
}
