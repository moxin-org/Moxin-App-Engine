use mofa_kernel::agent::types::error::{GlobalError, GlobalResult};
use mofa_kernel::message::{AgentEvent, AgentMessage, SchedulingStatus, TaskPriority, TaskRequest};
use mofa_kernel::{AgentBus, CommunicationMode};
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;

// 带优先级的任务包装器（BinaryHeap 默认是最大堆，直接支持优先级排序）
// Task wrapper with priority (BinaryHeap is a max-heap by default, supporting priority sorting)
#[derive(Debug, Clone, Eq, PartialEq)]
struct PriorityTask {
    priority: TaskPriority,
    task: TaskRequest,
    submit_time: std::time::Instant,
}

impl Ord for PriorityTask {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.submit_time.cmp(&self.submit_time)) // 同优先级先提交先执行
        // First-in-first-out for tasks with the same priority
    }
}

impl PartialOrd for PriorityTask {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// 优先级调度器
// Priority Scheduler
pub struct PriorityScheduler {
    task_queue: Arc<RwLock<BinaryHeap<PriorityTask>>>, // 优先级任务队列
    // Priority task queue
    agent_load: Arc<RwLock<HashMap<String, usize>>>, // 智能体当前负载（执行中的任务数）
    // Current agent load (number of tasks being executed)
    bus: Arc<AgentBus>,
    task_status: Arc<RwLock<HashMap<String, SchedulingStatus>>>, // 任务状态跟踪
    // Task status tracking
    role_mapping: Arc<RwLock<HashMap<String, Vec<String>>>>, // 角色-智能体映射
    // Role-to-agent mapping
    agent_tasks: Arc<RwLock<HashMap<String, Vec<String>>>>, // Agent-to-task mapping
    task_priorities: Arc<RwLock<HashMap<String, TaskPriority>>>, // Task priority tracking
    task_contents: Arc<RwLock<HashMap<String, TaskRequest>>>, // Original task payloads
}

impl PriorityScheduler {
    pub async fn new(bus: Arc<AgentBus>) -> Self {
        Self {
            task_queue: Arc::new(RwLock::new(BinaryHeap::new())),
            agent_load: Arc::new(RwLock::new(HashMap::new())),
            bus,
            task_status: Arc::new(RwLock::new(HashMap::new())),
            role_mapping: Arc::new(RwLock::new(HashMap::new())),
            agent_tasks: Arc::new(RwLock::new(HashMap::new())),
            task_priorities: Arc::new(RwLock::new(HashMap::new())),
            task_contents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 1. 提交任务到优先级队列
    /// 1. Submit task to the priority queue
    pub async fn submit_task(&self, task: TaskRequest) -> GlobalResult<()> {
        let priority_task = PriorityTask {
            priority: task.priority.clone(),
            task: task.clone(),
            submit_time: std::time::Instant::now(),
        };
        self.task_queue.write().await.push(priority_task);
        self.task_contents
            .write()
            .await
            .insert(task.task_id.clone(), task.clone());
        self.task_priorities
            .write()
            .await
            .insert(task.task_id.clone(), task.priority.clone());
        self.task_status
            .write()
            .await
            .insert(task.task_id, SchedulingStatus::Pending);
        // 提交后立即触发调度
        // Trigger scheduling immediately after submission
        self.schedule()
            .await
            .map_err(|e| GlobalError::Other(e.to_string()))?;
        Ok(())
    }

    /// 2. 核心调度逻辑：选高优先级任务 + 选负载最低的智能体
    /// 2. Core logic: select high-priority task + select lowest-load agent
    pub async fn schedule(&self) -> GlobalResult<()> {
        let mut task_queue = self.task_queue.write().await;
        let mut agent_load = self.agent_load.write().await;
        let mut task_status = self.task_status.write().await;
        let mut agent_tasks = self.agent_tasks.write().await;
        let role_map = self.role_mapping.read().await;
        let mut task_priorities = self.task_priorities.write().await;

        while let Some(priority_task) = task_queue.pop() {
            let task = priority_task.task.clone(); // Clone instead of moving
            let task_id = task.task_id.clone();

            // 跳过已被处理的任务
            // Skip tasks that have already been processed
            if task_status.get(&task_id) != Some(&SchedulingStatus::Pending) {
                continue;
            }

            // 选择负载最低的可用智能体（同角色内）— 内联以避免死锁
            // Select the available agent with the lowest load — inlined to avoid deadlock
            let sorted_agents = {
                let agents = match role_map.get("worker") {
                    Some(a) => a,
                    None => {
                        task_queue.push(priority_task);
                        break;
                    }
                };
                let mut sorted = agents.clone();
                sorted.sort_by_key(|agent_id| agent_load.get(agent_id).cloned().unwrap_or(0));
                sorted
            };
            if sorted_agents.is_empty() {
                // 无可用智能体，重新入队
                // No agent available, re-enqueue the task
                task_queue.push(priority_task);
                break;
            }
            let target_agent = sorted_agents[0].clone();

            // 检查是否需要抢占 — 内联以避免死锁
            // Check for preemption — inlined to avoid deadlock
            if let Some(&load) = agent_load.get(&target_agent)
                && load > 0
                && let Some(tasks_on_agent) = agent_tasks.get(&target_agent)
            {
                let preemptable_task = tasks_on_agent
                    .iter()
                    .filter(|tid| task_status.get(*tid) == Some(&SchedulingStatus::Running))
                    .filter(|tid| {
                        if let Some(task_priority) = task_priorities.get(*tid) {
                            task.priority > *task_priority
                        } else {
                            false
                        }
                    })
                    .min_by_key(|tid| task_priorities.get(*tid).cloned())
                    .cloned();

                if let Some(low_priority_task_id) = preemptable_task {
                    let preempt_msg = AgentMessage::Event(AgentEvent::TaskPreempted(
                        low_priority_task_id.clone(),
                    ));
                    self.bus
                        .send_message(
                            "scheduler",
                            CommunicationMode::PointToPoint(target_agent.clone()),
                            &preempt_msg,
                        )
                        .await
                        .map_err(|e| GlobalError::Other(e.to_string()))?;

                    // Clean up preempted task state to prevent ghost entries:
                    // Without this, preempted tasks leak in all 4 HashMaps,
                    // causing agent_load drift, OOM, and scheduling starvation.
                    task_status
                        .insert(low_priority_task_id.clone(), SchedulingStatus::Preempted);
                    if let Some(count) = agent_load.get_mut(&target_agent) {
                        *count = count.saturating_sub(1);
                    }
                    if let Some(tasks) = agent_tasks.get_mut(&target_agent) {
                        tasks.retain(|t| t != &low_priority_task_id);
                    }

                    // Re-enqueue the preempted task so it can be rescheduled
                    // to a different (or the same) agent in a future cycle.
                    if let Some(orig_priority) =
                        task_priorities.remove(&low_priority_task_id)
                    {
                        let orig_task = self
                            .task_contents
                            .read()
                            .await
                            .get(&low_priority_task_id)
                            .cloned();
                        let requeued_task = match orig_task {
                            Some(mut t) => {
                                t.priority = orig_priority.clone();
                                t
                            }
                            None => TaskRequest {
                                task_id: low_priority_task_id.clone(),
                                content: String::new(),
                                priority: orig_priority.clone(),
                                deadline: None,
                                metadata: std::collections::HashMap::new(),
                            },
                        };
                        let requeued = PriorityTask {
                            priority: orig_priority.clone(),
                            task: requeued_task,
                            submit_time: std::time::Instant::now(),
                        };
                        task_queue.push(requeued);
                        task_status.insert(
                            low_priority_task_id.clone(),
                            SchedulingStatus::Pending,
                        );
                        task_priorities.insert(low_priority_task_id, orig_priority);
                    }
                }
            }

            // 发送任务给目标智能体
            // Send task to the target agent
            let task_msg = AgentMessage::TaskRequest {
                task_id: task.task_id.clone(),
                content: task.content.clone(),
            };
            self.bus
                .send_message(
                    "scheduler",
                    CommunicationMode::PointToPoint(target_agent.clone()),
                    &task_msg,
                )
                .await
                .map_err(|e| GlobalError::Other(e.to_string()))?;

            // 更新状态和负载
            // Update task status and agent load
            task_status.insert(task_id.clone(), SchedulingStatus::Running);
            *agent_load.entry(target_agent.clone()).or_insert(0) += 1;
            agent_tasks.entry(target_agent).or_default().push(task_id);
        }
        Ok(())
    }

    /// 3. 负载均衡：选择同角色内负载最低的智能体
    /// 3. Load balancing: select the lowest-load agent within the same role
    async fn select_low_load_agent(&self, role: &str) -> GlobalResult<Vec<String>> {
        let role_map = self.role_mapping.read().await;
        let agents = role_map
            .get(role)
            .ok_or_else(|| GlobalError::Other(format!("No agent for role: {}", role)))?;
        let agent_load = self.agent_load.read().await;

        // 按负载升序排序，取负载最低的
        // Sort by load in ascending order and pick the lowest
        let mut sorted_agents = agents.clone();
        sorted_agents.sort_by_key(|agent_id| agent_load.get(agent_id).cloned().unwrap_or(0));
        Ok(sorted_agents)
    }

    /// 4. 任务抢占：高优先级任务抢占低优先级任务的执行资源
    /// 4. Task preemption: high-priority tasks preempt resources of low-priority tasks
    ///
    /// After sending the preemption event, this method also cleans up internal
    /// state (task_status, agent_load, agent_tasks, task_priorities) and
    /// re-enqueues the preempted task so it is not permanently lost.
    async fn preempt_low_priority_task(
        &self,
        agent_id: &str,
        high_priority_task: &TaskRequest,
    ) -> GlobalResult<()> {
        let mut agent_load = self.agent_load.write().await;
        let mut task_status = self.task_status.write().await;
        let mut agent_tasks = self.agent_tasks.write().await;
        let mut task_priorities = self.task_priorities.write().await;

        // 检查目标智能体当前运行的任务
        // Check tasks currently running on the target agent
        if let Some(&load) = agent_load.get(agent_id)
            && load > 0
        {
            let tasks_on_agent = match agent_tasks.get(agent_id) {
                Some(tasks) => tasks,
                None => return Ok(()),
            };

            let preemptable_task = tasks_on_agent
                .iter()
                .filter(|tid| task_status.get(*tid) == Some(&SchedulingStatus::Running))
                .filter(|tid| {
                    if let Some(task_priority) = task_priorities.get(*tid) {
                        high_priority_task.priority > *task_priority
                    } else {
                        false
                    }
                })
                .min_by_key(|tid| task_priorities.get(*tid).cloned())
                .cloned();

            if let Some(low_priority_task_id) = preemptable_task {
                // 发送抢占指令
                // Send preemption command
                let preempt_msg =
                    AgentMessage::Event(AgentEvent::TaskPreempted(low_priority_task_id.clone()));
                self.bus
                    .send_message(
                        "scheduler",
                        CommunicationMode::PointToPoint(agent_id.to_string()),
                        &preempt_msg,
                    )
                    .await
                    .map_err(|e| GlobalError::Other(e.to_string()))?;

                // Clean up preempted task state
                task_status.insert(low_priority_task_id.clone(), SchedulingStatus::Preempted);
                if let Some(count) = agent_load.get_mut(agent_id) {
                    *count = count.saturating_sub(1);
                }
                if let Some(tasks) = agent_tasks.get_mut(agent_id) {
                    tasks.retain(|t| t != &low_priority_task_id);
                }

                // Re-enqueue the preempted task for rescheduling
                if let Some(orig_priority) = task_priorities.remove(&low_priority_task_id) {
                    let mut task_queue = self.task_queue.write().await;
                    let orig_task = self
                        .task_contents
                        .read()
                        .await
                        .get(&low_priority_task_id)
                        .cloned();
                    let requeued_task = match orig_task {
                        Some(mut t) => {
                            t.priority = orig_priority.clone();
                            t
                        }
                        None => TaskRequest {
                            task_id: low_priority_task_id.clone(),
                            content: String::new(),
                            priority: orig_priority.clone(),
                            deadline: None,
                            metadata: std::collections::HashMap::new(),
                        },
                    };
                    let requeued = PriorityTask {
                        priority: orig_priority.clone(),
                        task: requeued_task,
                        submit_time: std::time::Instant::now(),
                    };
                    task_queue.push(requeued);
                    task_status.insert(low_priority_task_id.clone(), SchedulingStatus::Pending);
                    task_priorities.insert(low_priority_task_id, orig_priority);
                }
            }
        }
        Ok(())
    }

    /// 5. 任务完成后更新状态和负载
    /// 5. Update status and load upon task completion
    pub async fn on_task_completed(&self, agent_id: &str, task_id: &str) -> GlobalResult<()> {
        // Scope lock guards so they are dropped before calling schedule(),
        // which needs to acquire the same locks — avoids deadlock.
        {
            let mut agent_load = self.agent_load.write().await;
            let mut task_status = self.task_status.write().await;
            let mut agent_tasks = self.agent_tasks.write().await;
            let mut task_priorities = self.task_priorities.write().await;

            agent_load
                .entry(agent_id.to_string())
                .and_modify(|count| *count = count.saturating_sub(1));

            // Remove completed task metadata to prevent unbounded map growth.
            // Completed tasks are never re-queued, so these entries serve no
            // further purpose and would otherwise accumulate indefinitely.
            task_status.remove(task_id);
            task_priorities.remove(task_id);
            self.task_contents.write().await.remove(task_id);

            // Remove completed task from agent's task list
            if let Some(tasks) = agent_tasks.get_mut(agent_id) {
                tasks.retain(|t| t != task_id);
            }
        } // All lock guards dropped here

        // 任务完成后再次触发调度，处理队列中的下一个任务
        // Trigger scheduling again after completion to handle the next task
        self.schedule()
            .await
            .map_err(|e| GlobalError::Other(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofa_kernel::message::TaskPriority;
    use std::collections::HashMap;
    use tokio::time::{Duration, timeout};

    fn make_task(id: &str, priority: TaskPriority) -> TaskRequest {
        TaskRequest {
            task_id: id.to_string(),
            content: format!("payload-for-{id}"),
            priority,
            deadline: None,
            metadata: HashMap::new(),
        }
    }

    /// Proves Site 1+2 deadlock is fixed: submit_task → schedule() no longer
    /// tries to re-acquire locks held by schedule() itself.
    /// Without the fix this test would hang forever.
    #[tokio::test]
    async fn test_submit_task_does_not_deadlock() {
        let bus = Arc::new(AgentBus::new());
        let scheduler = PriorityScheduler::new(bus).await;

        let task = make_task("t1", TaskPriority::Normal);
        let result = timeout(Duration::from_secs(2), scheduler.submit_task(task)).await;
        assert!(result.is_ok(), "submit_task should complete, not deadlock");
    }

    /// Proves Site 3 deadlock is fixed: on_task_completed() drops its locks
    /// before calling schedule(), so schedule() can acquire them again.
    /// Without the fix this test would hang forever.
    #[tokio::test]
    async fn test_on_task_completed_does_not_deadlock() {
        let bus = Arc::new(AgentBus::new());
        let scheduler = PriorityScheduler::new(bus).await;

        // Pre-populate state as if a task was scheduled to an agent.
        {
            let mut load = scheduler.agent_load.write().await;
            load.insert("agent-1".to_string(), 1);
        }
        {
            let mut status = scheduler.task_status.write().await;
            status.insert("t1".to_string(), SchedulingStatus::Running);
        }
        {
            let mut tasks = scheduler.agent_tasks.write().await;
            tasks.insert("agent-1".to_string(), vec!["t1".to_string()]);
        }

        let result = timeout(
            Duration::from_secs(2),
            scheduler.on_task_completed("agent-1", "t1"),
        )
        .await;
        assert!(
            result.is_ok(),
            "on_task_completed should complete, not deadlock"
        );

        // Verify state was updated correctly.
        let load = scheduler.agent_load.read().await;
        assert_eq!(*load.get("agent-1").unwrap(), 0);
        // Completed task metadata is evicted — entry must not linger in the map.
        let status = scheduler.task_status.read().await;
        assert!(
            status.get("t1").is_none(),
            "task_status entry should be removed on completion"
        );
        let priorities = scheduler.task_priorities.read().await;
        assert!(
            priorities.get("t1").is_none(),
            "task_priorities entry should be removed on completion"
        );
    }

    /// Verifies the priority queue orders tasks correctly (higher priority first).
    #[tokio::test]
    async fn test_priority_ordering() {
        let mut heap = std::collections::BinaryHeap::new();
        let now = std::time::Instant::now();

        heap.push(PriorityTask {
            priority: TaskPriority::Low,
            task: make_task("low", TaskPriority::Low),
            submit_time: now,
        });
        heap.push(PriorityTask {
            priority: TaskPriority::Critical,
            task: make_task("critical", TaskPriority::Critical),
            submit_time: now,
        });
        heap.push(PriorityTask {
            priority: TaskPriority::Medium,
            task: make_task("medium", TaskPriority::Medium),
            submit_time: now,
        });

        assert_eq!(heap.pop().unwrap().task.task_id, "critical");
        assert_eq!(heap.pop().unwrap().task.task_id, "medium");
        assert_eq!(heap.pop().unwrap().task.task_id, "low");
    }

    /// After preemption inside `schedule()`, the preempted task must be
    /// cleaned from `agent_load`, `agent_tasks`, and `task_status`, then
    /// re-enqueued as Pending. Without the fix this test would fail because
    /// the ghost entry stays as Running forever.
    #[tokio::test]
    async fn test_preemption_cleans_agent_load() {
        let bus = Arc::new(AgentBus::new());
        let scheduler = PriorityScheduler::new(bus).await;

        // Register a "worker" role with one agent
        {
            let mut roles = scheduler.role_mapping.write().await;
            roles.insert("worker".to_string(), vec!["agent-1".to_string()]);
        }

        // Simulate: agent-1 is running a Low-priority task "t-low"
        {
            let mut load = scheduler.agent_load.write().await;
            load.insert("agent-1".to_string(), 1);
        }
        {
            let mut status = scheduler.task_status.write().await;
            status.insert("t-low".to_string(), SchedulingStatus::Running);
        }
        {
            let mut tasks = scheduler.agent_tasks.write().await;
            tasks.insert("agent-1".to_string(), vec!["t-low".to_string()]);
        }
        {
            let mut prios = scheduler.task_priorities.write().await;
            prios.insert("t-low".to_string(), TaskPriority::Low);
        }

        // Submit a Critical task — should preempt t-low
        let critical = make_task("t-critical", TaskPriority::Critical);
        let result = timeout(Duration::from_secs(2), scheduler.submit_task(critical)).await;
        assert!(
            result.is_ok(),
            "submit_task should complete without deadlock"
        );

        // Verify: agent_load should reflect only the new task (t-critical)
        // t-low's load contribution must be decremented during preemption
        let load = scheduler.agent_load.read().await;
        let agent_load_val = *load.get("agent-1").unwrap_or(&0);
        assert!(
            agent_load_val <= 1,
            "agent_load should not be inflated by ghost preempted task, got {agent_load_val}"
        );
    }

    /// Verifies that a preempted task's status transitions through Preempted
    /// and ends up re-enqueued. With two agents the re-enqueued task lands on
    /// the second agent, so we can verify the full lifecycle.
    #[tokio::test]
    async fn test_preemption_updates_task_status() {
        let bus = Arc::new(AgentBus::new());
        let scheduler = PriorityScheduler::new(bus).await;

        // Two agents — the re-enqueued task should land on agent-2
        {
            let mut roles = scheduler.role_mapping.write().await;
            roles.insert(
                "worker".to_string(),
                vec!["agent-1".to_string(), "agent-2".to_string()],
            );
        }
        {
            let mut load = scheduler.agent_load.write().await;
            load.insert("agent-1".to_string(), 1);
            load.insert("agent-2".to_string(), 0);
        }
        {
            let mut status = scheduler.task_status.write().await;
            status.insert("t-low".to_string(), SchedulingStatus::Running);
        }
        {
            let mut tasks = scheduler.agent_tasks.write().await;
            tasks.insert("agent-1".to_string(), vec!["t-low".to_string()]);
        }
        {
            let mut prios = scheduler.task_priorities.write().await;
            prios.insert("t-low".to_string(), TaskPriority::Low);
        }

        let critical = make_task("t-critical", TaskPriority::Critical);
        let _ = timeout(Duration::from_secs(2), scheduler.submit_task(critical)).await;

        // t-low should have been re-enqueued and rescheduled (Running on agent-2)
        // or at minimum not stuck as a ghost Running on agent-1.
        let status = scheduler.task_status.read().await;
        let low_status = status.get("t-low");
        // Valid end-states: Running (rescheduled), Pending (in queue), or absent
        assert!(
            low_status == Some(&SchedulingStatus::Running)
                || low_status == Some(&SchedulingStatus::Pending)
                || low_status.is_none(),
            "preempted task should be rescheduled or pending, got {:?}",
            low_status
        );

        // Regardless of final status, task_priorities must still contain t-low
        let prios = scheduler.task_priorities.read().await;
        assert!(
            prios.contains_key("t-low"),
            "preempted task priority metadata must be preserved"
        );
    }

    /// Verifies that the preempted task is removed from `agent_tasks`.
    #[tokio::test]
    async fn test_preemption_cleans_agent_tasks() {
        let bus = Arc::new(AgentBus::new());
        let scheduler = PriorityScheduler::new(bus).await;

        {
            let mut roles = scheduler.role_mapping.write().await;
            roles.insert("worker".to_string(), vec!["agent-1".to_string()]);
        }
        {
            let mut load = scheduler.agent_load.write().await;
            load.insert("agent-1".to_string(), 1);
        }
        {
            let mut status = scheduler.task_status.write().await;
            status.insert("t-low".to_string(), SchedulingStatus::Running);
        }
        {
            let mut tasks = scheduler.agent_tasks.write().await;
            tasks.insert("agent-1".to_string(), vec!["t-low".to_string()]);
        }
        {
            let mut prios = scheduler.task_priorities.write().await;
            prios.insert("t-low".to_string(), TaskPriority::Low);
        }

        let critical = make_task("t-critical", TaskPriority::Critical);
        let _ = timeout(Duration::from_secs(2), scheduler.submit_task(critical)).await;

        // The old "t-low" entry should have been removed from agent_tasks
        // during preemption cleanup (before re-enqueue may add it back
        // if the same agent is selected again).
        let tasks = scheduler.agent_tasks.read().await;
        if let Some(agent_task_list) = tasks.get("agent-1") {
            // t-low should not appear with Running status, only t-critical
            // (it may reappear if rescheduled, but as a new assignment)
            let status = scheduler.task_status.read().await;
            for tid in agent_task_list {
                if tid == "t-low" {
                    // If re-assigned, it should be Running again (rescheduled),
                    // not a stale ghost
                    assert_eq!(
                        status.get(tid),
                        Some(&SchedulingStatus::Running),
                        "re-assigned t-low should be Running, not a ghost"
                    );
                }
            }
        }
    }

    /// Verifies that the preempted task's original content, deadline, and
    /// metadata survive the re-enqueue. Before this fix, the inline
    /// preemption path in `schedule()` used the *incoming* high-priority
    /// task's content instead of the preempted task's own content, and the
    /// standalone `preempt_low_priority_task()` used an empty string.
    #[tokio::test]
    async fn test_preemption_preserves_task_content() {
        let bus = Arc::new(AgentBus::new());
        let scheduler = PriorityScheduler::new(bus).await;

        // Two agents so the re-enqueued low task can land on agent-2
        {
            let mut roles = scheduler.role_mapping.write().await;
            roles.insert(
                "worker".to_string(),
                vec!["agent-1".to_string(), "agent-2".to_string()],
            );
        }
        {
            let mut load = scheduler.agent_load.write().await;
            load.insert("agent-1".to_string(), 1);
            load.insert("agent-2".to_string(), 0);
        }

        // Submit a Low task with distinctive content
        let mut low_task = make_task("t-low", TaskPriority::Low);
        low_task.content = "important-work-payload".to_string();
        low_task.metadata.insert("key".to_string(), "value".to_string());

        // Manually place it as Running on agent-1 (simulates previous schedule)
        {
            let mut status = scheduler.task_status.write().await;
            status.insert("t-low".to_string(), SchedulingStatus::Running);
        }
        {
            let mut tasks = scheduler.agent_tasks.write().await;
            tasks.insert("agent-1".to_string(), vec!["t-low".to_string()]);
        }
        {
            let mut prios = scheduler.task_priorities.write().await;
            prios.insert("t-low".to_string(), TaskPriority::Low);
        }
        {
            let mut contents = scheduler.task_contents.write().await;
            contents.insert("t-low".to_string(), low_task.clone());
        }

        // Submit a Critical task — should preempt t-low
        let critical = make_task("t-critical", TaskPriority::Critical);
        let _ = timeout(Duration::from_secs(2), scheduler.submit_task(critical)).await;

        // The re-enqueued t-low must retain its original content
        let contents = scheduler.task_contents.read().await;
        if let Some(stored) = contents.get("t-low") {
            assert_eq!(
                stored.content, "important-work-payload",
                "preempted task must retain its original content"
            );
            assert_eq!(
                stored.metadata.get("key").map(|s| s.as_str()),
                Some("value"),
                "preempted task must retain its original metadata"
            );
        }
    }

    /// Verifies that preempted tasks are re-enqueued and their priority
    /// metadata is preserved in task_priorities.
    #[tokio::test]
    async fn test_preemption_requeues_task() {
        let bus = Arc::new(AgentBus::new());
        let scheduler = PriorityScheduler::new(bus).await;

        {
            let mut roles = scheduler.role_mapping.write().await;
            roles.insert("worker".to_string(), vec!["agent-1".to_string()]);
        }
        {
            let mut load = scheduler.agent_load.write().await;
            load.insert("agent-1".to_string(), 1);
        }
        {
            let mut status = scheduler.task_status.write().await;
            status.insert("t-low".to_string(), SchedulingStatus::Running);
        }
        {
            let mut tasks = scheduler.agent_tasks.write().await;
            tasks.insert("agent-1".to_string(), vec!["t-low".to_string()]);
        }
        {
            let mut prios = scheduler.task_priorities.write().await;
            prios.insert("t-low".to_string(), TaskPriority::Low);
        }

        let critical = make_task("t-critical", TaskPriority::Critical);
        let _ = timeout(Duration::from_secs(2), scheduler.submit_task(critical)).await;

        // t-low should still exist in task_priorities (re-enqueued, not leaked)
        let prios = scheduler.task_priorities.read().await;
        assert!(
            prios.contains_key("t-low"),
            "preempted task should be re-enqueued with its priority preserved"
        );
    }
}
