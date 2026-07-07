//! Swarm DAG schedulers (sequential + parallel).

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::future::{BoxFuture, join_all};
use petgraph::graph::NodeIndex;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tracing::{error, info, instrument, warn};

use mofa_kernel::agent::types::error::{GlobalError, GlobalResult};

use crate::swarm::hitl_gate::HITLGateMetrics;
use crate::swarm::{CoordinationPattern, SubtaskDAG, SwarmSubtask};

/// task executor used by schedulers
pub type SubtaskExecutorFn =
    Arc<dyn Fn(NodeIndex, SwarmSubtask) -> BoxFuture<'static, GlobalResult<String>> + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskOutcome {
    Success(String),
    Failure(String),
    Skipped(String),
}

impl TaskOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success(_))
    }

    pub fn output(&self) -> Option<&str> {
        if let Self::Success(s) = self {
            Some(s)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionResult {
    pub task_id: String,
    pub node_index: usize,
    pub outcome: TaskOutcome,
    pub wall_time: Duration,
    pub attempt: u32,
}

impl TaskExecutionResult {
    fn success(task: &SwarmSubtask, idx: NodeIndex, output: String, elapsed: Duration) -> Self {
        Self {
            task_id: task.id.clone(),
            node_index: idx.index(),
            outcome: TaskOutcome::Success(output),
            wall_time: elapsed,
            attempt: 1,
        }
    }

    fn failure(task: &SwarmSubtask, idx: NodeIndex, error: String, elapsed: Duration) -> Self {
        Self {
            task_id: task.id.clone(),
            node_index: idx.index(),
            outcome: TaskOutcome::Failure(error),
            wall_time: elapsed,
            attempt: 1,
        }
    }

    fn skipped(task: &SwarmSubtask, idx: NodeIndex, reason: String) -> Self {
        Self {
            task_id: task.id.clone(),
            node_index: idx.index(),
            outcome: TaskOutcome::Skipped(reason),
            wall_time: Duration::ZERO,
            attempt: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerSummary {
    pub pattern: CoordinationPattern,
    pub total_tasks: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub total_wall_time: Duration,
    pub results: Vec<TaskExecutionResult>,
    /// HITL gate metrics for this execution.
    ///
    /// `None` when no gate was used.  Populated by calling
    /// [`SwarmHITLGate::enrich_summary`] after the scheduler returns.
    #[serde(default)]
    pub hitl_stats: Option<HITLGateMetrics>,
}

impl SchedulerSummary {
    pub fn success_rate(&self) -> f64 {
        if self.total_tasks == 0 {
            return 0.0;
        }
        self.succeeded as f64 / self.total_tasks as f64
    }

    pub fn is_fully_successful(&self) -> bool {
        self.failed == 0 && self.skipped == 0
    }

    pub fn successful_outputs(&self) -> Vec<&str> {
        self.results
            .iter()
            .filter_map(|r| r.outcome.output())
            .collect()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FailurePolicy {
    #[default]
    Continue,

    FailFastCascade,
}

#[derive(Debug, Clone)]
pub struct SwarmSchedulerConfig {
    pub task_timeout: Duration,
    pub failure_policy: FailurePolicy,
    pub concurrency_limit: Option<usize>,
}

impl Default for SwarmSchedulerConfig {
    fn default() -> Self {
        Self {
            task_timeout: Duration::from_secs(120),
            failure_policy: FailurePolicy::default(),
            concurrency_limit: None,
        }
    }
}

#[async_trait]
pub trait SwarmScheduler: Send + Sync {
    fn pattern(&self) -> CoordinationPattern;

    async fn execute(
        &self,
        dag: &mut SubtaskDAG,
        executor: SubtaskExecutorFn,
    ) -> GlobalResult<SchedulerSummary>;
}

pub struct SequentialScheduler {
    pub config: SwarmSchedulerConfig,
}

impl SequentialScheduler {
    pub fn new() -> Self {
        Self {
            config: SwarmSchedulerConfig::default(),
        }
    }

    pub fn with_config(config: SwarmSchedulerConfig) -> Self {
        Self { config }
    }
}

impl Default for SequentialScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SwarmScheduler for SequentialScheduler {
    fn pattern(&self) -> CoordinationPattern {
        CoordinationPattern::Sequential
    }

    #[instrument(
        skip(self, dag, executor),
        fields(pattern = "sequential", task_count = dag.task_count())
    )]
    async fn execute(
        &self,
        dag: &mut SubtaskDAG,
        executor: SubtaskExecutorFn,
    ) -> GlobalResult<SchedulerSummary> {
        let wall_start = Instant::now();
        let total = dag.task_count();

        let ordered_indices = dag.topological_order().map_err(|e| {
            GlobalError::runtime(format!("Sequential scheduling failed, DAG error: {e}"))
        })?;

        let mut results = Vec::with_capacity(total);
        let mut succeeded = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;

        info!(task_count = total, "Sequential scheduler starting");

        for idx in ordered_indices {
            let task = &dag.get_task(idx).expect("Task missing by index");

            if !matches!(
                task.status,
                crate::swarm::SubtaskStatus::Pending | crate::swarm::SubtaskStatus::Ready
            ) {
                if matches!(task.status, crate::swarm::SubtaskStatus::Skipped) {
                    skipped += 1;
                    results.push(TaskExecutionResult::skipped(
                        task,
                        idx,
                        "skipped by cascade".into(),
                    ));
                    continue;
                }
                continue;
            }

            dag.mark_running(idx);
            let task_snapshot = dag.get_task(idx).unwrap().clone();
            let task_id = task_snapshot.id.clone();

            let start = Instant::now();
            info!(task_id = %task_id, task_desc = %task_snapshot.description, "Executing node");

            let outcome = timeout(
                self.config.task_timeout,
                executor(idx, task_snapshot.clone()),
            )
            .await;
            let elapsed = start.elapsed();

            match outcome {
                Ok(Ok(output)) => {
                    info!(task_id = %task_id, elapsed_ms = elapsed.as_millis(), "Succeeded");
                    dag.mark_complete_with_output(idx, Some(output.clone()));
                    results.push(TaskExecutionResult::success(
                        &task_snapshot,
                        idx,
                        output,
                        elapsed,
                    ));
                    succeeded += 1;
                }
                Ok(Err(e)) => {
                    error!(task_id = %task_id, error = %e, "Failed");
                    let err_str = e.to_string();
                    dag.mark_failed(idx, err_str.clone());
                    results.push(TaskExecutionResult::failure(
                        &task_snapshot,
                        idx,
                        err_str,
                        elapsed,
                    ));
                    failed += 1;

                    if self.config.failure_policy == FailurePolicy::FailFastCascade {
                        let newly_skipped = dag.cascade_skip(idx);
                        info!(cascaded = newly_skipped, "cascaded skip");
                    }
                }
                Err(_) => {
                    let msg = format!("timed out after {:?}", self.config.task_timeout);
                    error!(task_id = %task_id, "{msg}");
                    dag.mark_failed(idx, msg.clone());
                    results.push(TaskExecutionResult::failure(
                        &task_snapshot,
                        idx,
                        msg,
                        elapsed,
                    ));
                    failed += 1;

                    if self.config.failure_policy == FailurePolicy::FailFastCascade {
                        dag.cascade_skip(idx);
                    }
                }
            }
        }

        Ok(SchedulerSummary {
            pattern: CoordinationPattern::Sequential,
            total_tasks: total,
            succeeded,
            failed,
            skipped,
            total_wall_time: wall_start.elapsed(),
            results,
            hitl_stats: None,
        })
    }
}

pub struct ParallelScheduler {
    pub config: SwarmSchedulerConfig,
}

impl ParallelScheduler {
    pub fn new() -> Self {
        Self {
            config: SwarmSchedulerConfig::default(),
        }
    }

    pub fn with_config(config: SwarmSchedulerConfig) -> Self {
        Self { config }
    }
}

impl Default for ParallelScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SwarmScheduler for ParallelScheduler {
    fn pattern(&self) -> CoordinationPattern {
        CoordinationPattern::Parallel
    }

    #[instrument(
        skip(self, dag, executor),
        fields(pattern = "parallel", task_count = dag.task_count())
    )]
    async fn execute(
        &self,
        dag: &mut SubtaskDAG,
        executor: SubtaskExecutorFn,
    ) -> GlobalResult<SchedulerSummary> {
        let wall_start = Instant::now();
        let total = dag.task_count();

        let semaphore = self
            .config
            .concurrency_limit
            .map(|n| Arc::new(Semaphore::new(n)));

        let mut results: Vec<TaskExecutionResult> = Vec::with_capacity(total);
        let mut succeeded = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;

        info!(
            task_count = total,
            concurrency_limit = ?self.config.concurrency_limit,
            "Parallel scheduler starting"
        );

        while !dag.is_complete() {
            let ready_indices = dag.ready_tasks();

            if ready_indices.is_empty() {
                warn!("DAG stalled: incomplete, but 0 ready tasks");
                break;
            }

            let wave_size = ready_indices.len();
            info!(wave_size, "dispatching wave");

            let mut wave_futures = Vec::with_capacity(wave_size);

            for &idx in &ready_indices {
                dag.mark_running(idx);
                let task_snapshot = dag.get_task(idx).expect("missing idx").clone();

                let exec = executor.clone();
                let sem = semaphore.clone();
                let timeout_dur = self.config.task_timeout;

                let fut = async move {
                    let _permit = if let Some(s) = sem {
                        Some(s.acquire_owned().await.expect("semaphore closed"))
                    } else {
                        None
                    };

                    let start = Instant::now();
                    match timeout(timeout_dur, exec(idx, task_snapshot.clone())).await {
                        Ok(Ok(output)) => TaskExecutionResult::success(
                            &task_snapshot,
                            idx,
                            output,
                            start.elapsed(),
                        ),
                        Ok(Err(e)) => TaskExecutionResult::failure(
                            &task_snapshot,
                            idx,
                            e.to_string(),
                            start.elapsed(),
                        ),
                        Err(_) => TaskExecutionResult::failure(
                            &task_snapshot,
                            idx,
                            format!("timed out after {:?}", timeout_dur),
                            start.elapsed(),
                        ),
                    }
                };

                wave_futures.push(fut);
            }

            let wave_results = join_all(wave_futures).await;

            for result in wave_results {
                let idx = NodeIndex::new(result.node_index);

                if result.outcome.is_success() {
                    let text = result
                        .outcome
                        .output()
                        .expect("Success has output")
                        .to_string();
                    dag.mark_complete_with_output(idx, Some(text));
                    succeeded += 1;
                } else {
                    let err_msg = match &result.outcome {
                        TaskOutcome::Failure(e) => e.clone(),
                        TaskOutcome::Skipped(_) => {
                            unreachable!("Skipped state not emitted dynamically")
                        }
                        _ => unreachable!(),
                    };

                    dag.mark_failed(idx, err_msg);
                    failed += 1;

                    if self.config.failure_policy == FailurePolicy::FailFastCascade {
                        let cascaded = dag.cascade_skip(idx);
                        skipped += cascaded;
                        info!(cascaded, "cascaded skip");
                    }
                }

                results.push(result);
            }
        }

        for (idx, task) in dag.all_tasks() {
            if task.status == crate::swarm::SubtaskStatus::Pending {
                skipped += 1;
                results.push(TaskExecutionResult::skipped(
                    task,
                    idx,
                    "dag stalled".into(),
                ));
            } else if task.status == crate::swarm::SubtaskStatus::Skipped
                && !results.iter().any(|r| r.node_index == idx.index())
            {
                results.push(TaskExecutionResult::skipped(task, idx, "skipped".into()));
            }
        }

        Ok(SchedulerSummary {
            pattern: CoordinationPattern::Parallel,
            total_tasks: total,
            succeeded,
            failed,
            skipped,
            total_wall_time: wall_start.elapsed(),
            results,
            hitl_stats: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::{DependencyKind, SwarmSubtask};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_sequential_topological_order() {
        let mut dag = SubtaskDAG::new("test");
        let idx_a = dag.add_task(SwarmSubtask::new("A", "Task A"));
        let idx_b = dag.add_task(SwarmSubtask::new("B", "Task B"));
        let idx_c = dag.add_task(SwarmSubtask::new("C", "Task C"));

        dag.add_dependency(idx_a, idx_b).unwrap();
        dag.add_dependency(idx_b, idx_c).unwrap();

        let exec_order = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let exec_order_clone = exec_order.clone();

        let executor: SubtaskExecutorFn = Arc::new(move |_idx, task| {
            let order = exec_order_clone.clone();
            let task_id = task.id.clone();
            Box::pin(async move {
                order.lock().await.push(task_id.clone());
                Ok(format!("{} done", task_id))
            })
        });

        let scheduler = SequentialScheduler::new();
        let summary = scheduler.execute(&mut dag, executor).await.unwrap();

        assert!(summary.is_fully_successful());
        assert_eq!(summary.succeeded, 3);

        let order = exec_order.lock().await;
        assert_eq!(*order, vec!["A", "B", "C"]);
    }

    #[tokio::test]
    async fn test_parallel_fail_fast_cascades() {
        let mut dag = SubtaskDAG::new("test");
        let idx_a = dag.add_task(SwarmSubtask::new("A", "Task A"));
        let idx_b = dag.add_task(SwarmSubtask::new("B", "Task B"));
        let idx_c = dag.add_task(SwarmSubtask::new("C", "Task C"));

        dag.add_dependency(idx_a, idx_b).unwrap();
        dag.add_dependency(idx_a, idx_c).unwrap();

        let executor: SubtaskExecutorFn = Arc::new(move |_idx, task| {
            Box::pin(async move {
                if task.id == "A" {
                    Err(GlobalError::runtime("A failed"))
                } else {
                    Ok("done".into())
                }
            })
        });

        let mut config = SwarmSchedulerConfig::default();
        config.failure_policy = FailurePolicy::FailFastCascade;
        let scheduler = ParallelScheduler::with_config(config);

        let summary = scheduler.execute(&mut dag, executor).await.unwrap();

        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 2);
        assert_eq!(summary.succeeded, 0);

        assert_eq!(
            dag.get_task(idx_a).unwrap().status,
            crate::swarm::SubtaskStatus::Failed("Runtime error: A failed".into())
        );
        assert_eq!(
            dag.get_task(idx_b).unwrap().status,
            crate::swarm::SubtaskStatus::Skipped
        );
        assert_eq!(
            dag.get_task(idx_c).unwrap().status,
            crate::swarm::SubtaskStatus::Skipped
        );
    }

    #[tokio::test]
    async fn test_continue_lets_downstream_run() {
        let mut dag = SubtaskDAG::new("test");
        let idx_a = dag.add_task(SwarmSubtask::new("A", "Task A"));
        let idx_b = dag.add_task(SwarmSubtask::new("B", "Task B"));

        dag.add_dependency(idx_a, idx_b).unwrap();

        let executor: SubtaskExecutorFn = Arc::new(move |_idx, task| {
            Box::pin(async move {
                if task.id == "A" {
                    Err(GlobalError::runtime("A failed"))
                } else {
                    Ok("B ran anyway".into())
                }
            })
        });

        let scheduler = SequentialScheduler::new();
        let summary = scheduler.execute(&mut dag, executor).await.unwrap();

        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 0);
        assert_eq!(summary.succeeded, 1);

        assert_eq!(
            dag.get_task(idx_b).unwrap().status,
            crate::swarm::SubtaskStatus::Completed
        );
    }

    #[tokio::test]
    async fn test_timeout_marks_failed() {
        let mut dag = SubtaskDAG::new("test");
        let _idx_a = dag.add_task(SwarmSubtask::new("A", "Task A"));

        let executor: SubtaskExecutorFn = Arc::new(move |_idx, _task| {
            Box::pin(async move {
                sleep(Duration::from_millis(100)).await;
                Ok("done".into())
            })
        });

        let mut config = SwarmSchedulerConfig::default();
        config.task_timeout = Duration::from_millis(1);
        let scheduler = SequentialScheduler::with_config(config);

        let summary = scheduler.execute(&mut dag, executor).await.unwrap();

        assert_eq!(summary.failed, 1);
        let results = summary.results;
        assert!(
            matches!(results[0].outcome, TaskOutcome::Failure(ref s) if s.contains("timed out"))
        );
    }

    #[tokio::test]
    async fn test_parallel_diamond_waves() {
        let mut dag = SubtaskDAG::new("test");
        let idx_a = dag.add_task(SwarmSubtask::new("A", "Task A"));
        let idx_b = dag.add_task(SwarmSubtask::new("B", "Task B"));
        let idx_c = dag.add_task(SwarmSubtask::new("C", "Task C"));
        let idx_d = dag.add_task(SwarmSubtask::new("D", "Task D"));

        dag.add_dependency(idx_a, idx_b).unwrap();
        dag.add_dependency(idx_a, idx_c).unwrap();
        dag.add_dependency(idx_b, idx_d).unwrap();
        dag.add_dependency(idx_c, idx_d).unwrap();

        let execution_log = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let execution_log_clone = execution_log.clone();

        let executor: SubtaskExecutorFn = Arc::new(move |_idx, task| {
            let log = execution_log_clone.clone();
            let task_id = task.id.clone();
            Box::pin(async move {
                sleep(Duration::from_millis(50)).await;
                log.lock().await.push(task_id.clone());
                Ok(format!("{} done", task_id))
            })
        });

        let scheduler = ParallelScheduler::new();
        let summary = scheduler.execute(&mut dag, executor).await.unwrap();

        assert!(summary.is_fully_successful());
        assert_eq!(summary.succeeded, 4);

        let log = execution_log.lock().await;
        assert_eq!(log[0], "A");
        assert!(log[1] == "B" || log[1] == "C");
        assert!(log[2] == "B" || log[2] == "C");
        assert_ne!(log[1], log[2]);
        assert_eq!(log[3], "D");
    }

    #[tokio::test]
    async fn test_parallel_concurrency_limit() {
        let mut dag = SubtaskDAG::new("test");
        for i in 0..10 {
            dag.add_task(SwarmSubtask::new(format!("T{}", i), format!("Task {}", i)));
        }

        let active_tasks = Arc::new(AtomicUsize::new(0));
        let peak_tasks = Arc::new(AtomicUsize::new(0));

        let active_clone = active_tasks.clone();
        let peak_clone = peak_tasks.clone();

        let executor: SubtaskExecutorFn = Arc::new(move |_idx, _task| {
            let active = active_clone.clone();
            let peak = peak_clone.clone();

            Box::pin(async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                let mut current_peak = peak.load(Ordering::SeqCst);
                while current > current_peak {
                    match peak.compare_exchange_weak(
                        current_peak,
                        current,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    ) {
                        Ok(_) => break,
                        Err(x) => current_peak = x,
                    }
                }

                sleep(Duration::from_millis(50)).await;

                active.fetch_sub(1, Ordering::SeqCst);
                Ok("done".into())
            })
        });

        let mut config = SwarmSchedulerConfig::default();
        config.concurrency_limit = Some(3);
        let scheduler = ParallelScheduler::with_config(config);

        let summary = scheduler.execute(&mut dag, executor).await.unwrap();

        assert!(summary.is_fully_successful());
        assert_eq!(summary.succeeded, 10);

        let actual_peak = peak_tasks.load(Ordering::SeqCst);
        assert_eq!(
            actual_peak, 3,
            "Peak concurrency should strictly match the limit of 3"
        );
    }

    #[test]
    fn test_into_scheduler() {
        let seq = CoordinationPattern::Sequential.into_scheduler();
        assert_eq!(seq.pattern(), CoordinationPattern::Sequential);

        let par = CoordinationPattern::Parallel.into_scheduler();
        assert_eq!(par.pattern(), CoordinationPattern::Parallel);
    }

    #[tokio::test]
    async fn test_sequential_fail_fast_cascades_skip_to_hard_dependents() {
        let mut dag = SubtaskDAG::new("test");
        let idx_a = dag.add_task(SwarmSubtask::new("A", "Task A"));
        let idx_b = dag.add_task(SwarmSubtask::new("B", "Task B"));
        let idx_c = dag.add_task(SwarmSubtask::new("C", "Task C"));

        // A -> B -> C
        dag.add_dependency(idx_a, idx_b).unwrap();
        dag.add_dependency(idx_b, idx_c).unwrap();

        let executor: SubtaskExecutorFn = Arc::new(move |_idx, task| {
            Box::pin(async move {
                if task.id == "A" {
                    Err(GlobalError::runtime("boom"))
                } else {
                    Ok("should not run".into())
                }
            })
        });

        let mut config = SwarmSchedulerConfig::default();
        config.failure_policy = FailurePolicy::FailFastCascade;
        let scheduler = SequentialScheduler::with_config(config);

        let summary = scheduler.execute(&mut dag, executor).await.unwrap();
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 2);
        assert_eq!(summary.succeeded, 0);
        assert_eq!(
            dag.get_task(idx_b).unwrap().status,
            crate::swarm::SubtaskStatus::Skipped
        );
        assert_eq!(
            dag.get_task(idx_c).unwrap().status,
            crate::swarm::SubtaskStatus::Skipped
        );
    }

    #[tokio::test]
    async fn test_soft_dependency_does_not_block_execution() {
        let mut dag = SubtaskDAG::new("test");
        let idx_a = dag.add_task(SwarmSubtask::new("A", "Optional"));
        let idx_b = dag.add_task(SwarmSubtask::new("B", "Main"));

        dag.add_dependency_with_kind(idx_a, idx_b, DependencyKind::Soft)
            .unwrap();

        // Executor fails A, but B must still run because it's only a soft dependency.
        let executor: SubtaskExecutorFn = Arc::new(move |_idx, task| {
            Box::pin(async move {
                if task.id == "A" {
                    Err(GlobalError::runtime("A failed"))
                } else {
                    Ok("B ok".into())
                }
            })
        });

        let scheduler = ParallelScheduler::new();
        let summary = scheduler.execute(&mut dag, executor).await.unwrap();

        assert_eq!(summary.failed, 1);
        assert_eq!(summary.succeeded, 1);
        assert_eq!(summary.skipped, 0);
        assert_eq!(
            dag.get_task(idx_b).unwrap().status,
            crate::swarm::SubtaskStatus::Completed
        );
    }

    #[tokio::test]
    async fn test_outputs_are_persisted_on_dag_nodes() {
        let mut dag = SubtaskDAG::new("test");
        let idx_a = dag.add_task(SwarmSubtask::new("A", "Task A"));

        let executor: SubtaskExecutorFn =
            Arc::new(move |_idx, task| Box::pin(async move { Ok(format!("out:{}", task.id)) }));

        let scheduler = SequentialScheduler::new();
        let summary = scheduler.execute(&mut dag, executor).await.unwrap();
        assert!(summary.is_fully_successful());

        let task = dag.get_task(idx_a).unwrap();
        assert_eq!(task.status, crate::swarm::SubtaskStatus::Completed);
        assert_eq!(task.output.as_deref(), Some("out:A"));
    }

    #[tokio::test]
    async fn test_empty_dag_is_noop_and_does_not_panic() {
        let mut dag = SubtaskDAG::new("empty");

        let executor: SubtaskExecutorFn =
            Arc::new(move |_idx, _task| Box::pin(async move { Ok("unused".into()) }));

        let scheduler = SequentialScheduler::new();
        let summary = scheduler.execute(&mut dag, executor).await.unwrap();
        assert_eq!(summary.total_tasks, 0);
        assert_eq!(summary.succeeded, 0);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 0);
    }

    #[tokio::test]
    async fn test_parallel_continue_after_failure_matches_ready_tasks_semantics() {
        let mut dag = SubtaskDAG::new("test");
        let idx_a = dag.add_task(SwarmSubtask::new("A", "A"));
        let idx_b = dag.add_task(SwarmSubtask::new("B", "B"));
        dag.add_dependency(idx_a, idx_b).unwrap();

        let executor: SubtaskExecutorFn = Arc::new(move |_idx, task| {
            Box::pin(async move {
                if task.id == "A" {
                    Err(GlobalError::runtime("A failed"))
                } else {
                    Ok("B ran".into())
                }
            })
        });

        let scheduler = ParallelScheduler::new(); // default FailurePolicy::Continue
        let summary = scheduler.execute(&mut dag, executor).await.unwrap();

        assert_eq!(summary.failed, 1);
        assert_eq!(summary.succeeded, 1);
        assert_eq!(summary.skipped, 0);
        assert_eq!(
            dag.get_task(idx_b).unwrap().status,
            crate::swarm::SubtaskStatus::Completed
        );
    }

    #[tokio::test]
    async fn test_concurrency_limit_one_never_exceeds_one_active_task() {
        let mut dag = SubtaskDAG::new("test");
        for i in 0..5 {
            dag.add_task(SwarmSubtask::new(format!("T{}", i), "t"));
        }

        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let a = active.clone();
        let p = peak.clone();

        let executor: SubtaskExecutorFn = Arc::new(move |_idx, _task| {
            let a = a.clone();
            let p = p.clone();
            Box::pin(async move {
                let current = a.fetch_add(1, Ordering::SeqCst) + 1;
                p.fetch_max(current, Ordering::SeqCst);
                sleep(Duration::from_millis(10)).await;
                a.fetch_sub(1, Ordering::SeqCst);
                Ok("ok".into())
            })
        });

        let mut config = SwarmSchedulerConfig::default();
        config.concurrency_limit = Some(1);
        let scheduler = ParallelScheduler::with_config(config);

        let summary = scheduler.execute(&mut dag, executor).await.unwrap();
        assert!(summary.is_fully_successful());
        assert_eq!(peak.load(Ordering::SeqCst), 1);
    }
}
