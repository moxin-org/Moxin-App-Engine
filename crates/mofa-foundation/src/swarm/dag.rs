//! SubtaskDAG: Directed Acyclic Graph for task decomposition

use chrono::{DateTime, Utc};
use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use mofa_kernel::agent::types::error::{GlobalError, GlobalResult};

// ── Risk Classification ───────────────────────────────────────────────────────

/// Risk level classification for a subtask.
///
/// Drives automatic HITL routing: [`High`] and [`Critical`] tasks
/// are intercepted by `SwarmHITLGate` before execution.
///
/// The variants are ordered from lowest to highest risk so that
/// `PartialOrd`/`Ord` comparisons work naturally
/// (`Critical > High > Medium > Low`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RiskLevel {
    /// Read-only, fully reversible, no external side-effects
    /// (e.g. web search, text summarisation).
    #[default]
    Low,
    /// Writes to internal state; reversible with effort
    /// (e.g. draft a document, send an internal notification).
    Medium,
    /// Writes to external systems or has significant impact
    /// (e.g. an API call that modifies third-party data).
    High,
    /// Irreversible, financial, security-sensitive, or production deployment
    /// (e.g. execute a payment, delete a database, deploy to production).
    Critical,
}

impl RiskLevel {
    /// Returns `true` if this risk level requires human review before execution.
    ///
    /// [`High`] and [`Critical`] tasks require HITL by default.
    pub fn requires_hitl(&self) -> bool {
        matches!(self, Self::High | Self::Critical)
    }

    /// Maps the risk level to a numeric priority (1–10) suitable for
    /// `ReviewMetadata::priority` in the HITL system.
    pub fn to_priority(&self) -> u8 {
        match self {
            Self::Low => 2,
            Self::Medium => 4,
            Self::High => 7,
            Self::Critical => 10,
        }
    }
}

// SwarmSubtask Types
/// Status of an individual subtask in the DAG
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubtaskStatus {
    #[default]
    Pending,
    Ready,
    Running,
    Completed,
    Failed(String),
    Skipped,
}

/// A single subtask node in the DAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmSubtask {
    pub id: String,
    pub description: String,
    pub required_capabilities: Vec<String>,
    pub status: SubtaskStatus,
    pub assigned_agent: Option<String>,
    pub output: Option<String>,
    pub complexity: f64,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    /// Risk classification for this subtask (defaults to [`RiskLevel::Low`]).
    #[serde(default)]
    pub risk_level: RiskLevel,
    /// Whether this subtask requires human approval before execution.
    ///
    /// Automatically set to `true` when `risk_level` is [`RiskLevel::High`]
    /// or [`RiskLevel::Critical`], but can also be overridden explicitly.
    #[serde(default)]
    pub hitl_required: bool,
    /// LLM-estimated wall-clock duration for this subtask in seconds.
    ///
    /// Used by [`SubtaskDAG::critical_path`] for scheduling hints.
    /// `None` means the duration is unknown.
    #[serde(default)]
    pub estimated_duration_secs: Option<u64>,
}

impl SwarmSubtask {
    /// Create a new subtask with the given id and description
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            required_capabilities: Vec::new(),
            status: SubtaskStatus::Pending,
            assigned_agent: None,
            output: None,
            complexity: 0.5,
            started_at: None,
            completed_at: None,
            risk_level: RiskLevel::Low,
            hitl_required: false,
            estimated_duration_secs: None,
        }
    }

    /// Set required capabilities for this subtask
    pub fn with_capabilities(mut self, caps: Vec<String>) -> Self {
        self.required_capabilities = caps;
        self
    }

    /// Set the estimated complexity
    pub fn with_complexity(mut self, complexity: f64) -> Self {
        self.complexity = complexity.clamp(0.0, 1.0);
        self
    }

    /// Set the risk level and automatically derive [`hitl_required`].
    ///
    /// Tasks rated [`RiskLevel::High`] or [`RiskLevel::Critical`] will have
    /// `hitl_required` set to `true` automatically.
    pub fn with_risk_level(mut self, risk: RiskLevel) -> Self {
        self.hitl_required = risk.requires_hitl();
        self.risk_level = risk;
        self
    }

    /// Set the LLM-estimated execution duration in seconds.
    pub fn with_estimated_duration(mut self, secs: u64) -> Self {
        self.estimated_duration_secs = Some(secs);
        self
    }

    /// Override the HITL requirement flag directly, regardless of risk level.
    pub fn with_hitl_required(mut self, required: bool) -> Self {
        self.hitl_required = required;
        self
    }
}

/// Edge metadata representing a dependency between subtasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyEdge {
    /// What kind of dependency this is
    pub kind: DependencyKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    Sequential,
    DataFlow,
    Soft,
}

impl Default for DependencyEdge {
    fn default() -> Self {
        Self {
            kind: DependencyKind::Sequential,
        }
    }
}

// SubtaskDAG
/// Directed Acyclic Graph representing a decomposed task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtaskDAG {
    pub id: String,
    pub name: String,
    graph: DiGraph<SwarmSubtask, DependencyEdge>,
    #[serde(skip)]
    id_to_index: HashMap<String, NodeIndex>,
}

impl SubtaskDAG {
    /// Create a new empty DAG
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7().to_string(),
            name: name.into(),
            graph: DiGraph::new(),
            id_to_index: HashMap::new(),
        }
    }

    /// Add a subtask to the DAG, returns its node index
    pub fn add_task(&mut self, task: SwarmSubtask) -> NodeIndex {
        let id = task.id.clone();
        let idx = self.graph.add_node(task);
        self.id_to_index.insert(id, idx);
        idx
    }

    /// Add a dependency edge: `from` must complete before `to` can start
    pub fn add_dependency(&mut self, from: NodeIndex, to: NodeIndex) -> GlobalResult<()> {
        self.add_dependency_with_kind(from, to, DependencyKind::Sequential)
    }

    /// Add a dependency edge with a specific kind
    pub fn add_dependency_with_kind(
        &mut self,
        from: NodeIndex,
        to: NodeIndex,
        kind: DependencyKind,
    ) -> GlobalResult<()> {
        self.graph.add_edge(from, to, DependencyEdge { kind });
        if petgraph::algo::is_cyclic_directed(&self.graph) {
            if let Some(edge) = self.graph.find_edge(from, to) {
                self.graph.remove_edge(edge);
            }
            return Err(GlobalError::Other(format!(
                "Adding dependency from {:?} to {:?} would create a cycle",
                from, to
            )));
        }

        Ok(())
    }

    /// Return tasks that are pending and have all hard dependencies satisfied
    pub fn ready_tasks(&self) -> Vec<NodeIndex> {
        self.graph
            .node_indices()
            .filter(|&idx| {
                let task = &self.graph[idx];
                if task.status != SubtaskStatus::Pending {
                    return false;
                }
                self.graph
                    .edges_directed(idx, Direction::Incoming)
                    .all(|edge| {
                        let dep = &self.graph[edge.source()];
                        let dep_edge = edge.weight();
                        match dep_edge.kind {
                            // Sequential is a temporal ordering — the downstream task can
                            // run once the upstream has reached any terminal state. We
                            // intentionally treat Failed as satisfying the dep here so a
                            // single failure does not deadlock the rest of the DAG;
                            // callers opt into hard cascading via `cascade_skip` /
                            // `FailurePolicy::FailFastCascade`.
                            DependencyKind::Sequential => matches!(
                                dep.status,
                                SubtaskStatus::Completed
                                    | SubtaskStatus::Skipped
                                    | SubtaskStatus::Failed(_)
                            ),
                            // DataFlow means the downstream consumes the upstream's
                            // output. A Failed or Skipped upstream produced no output,
                            // so running the downstream would feed it missing data.
                            // Leave it Pending and let the scheduler's stall handling
                            // mark it Skipped at the end of the run.
                            DependencyKind::DataFlow => {
                                dep.status == SubtaskStatus::Completed
                            }
                            DependencyKind::Soft => true,
                        }
                    })
            })
            .collect()
    }

    /// Mark a task as running and record its start time
    pub fn mark_running(&mut self, idx: NodeIndex) {
        if let Some(task) = self.graph.node_weight_mut(idx) {
            task.status = SubtaskStatus::Running;
            task.started_at = Some(Utc::now());
        }
    }

    /// Mark a task as completed
    pub fn mark_complete(&mut self, idx: NodeIndex) {
        self.mark_complete_with_output(idx, None);
    }

    /// Mark a task as completed and attach its output
    pub fn mark_complete_with_output(&mut self, idx: NodeIndex, output: Option<String>) {
        if let Some(task) = self.graph.node_weight_mut(idx) {
            task.status = SubtaskStatus::Completed;
            task.completed_at = Some(Utc::now());
            task.output = output;
        }
    }

    /// Mark a task as failed with a reason string
    pub fn mark_failed(&mut self, idx: NodeIndex, reason: impl Into<String>) {
        if let Some(task) = self.graph.node_weight_mut(idx) {
            task.status = SubtaskStatus::Failed(reason.into());
            task.completed_at = Some(Utc::now());
        }
    }

    /// Mark a task as skipped
    pub fn mark_skipped(&mut self, idx: NodeIndex) {
        if let Some(task) = self.graph.node_weight_mut(idx) {
            task.status = SubtaskStatus::Skipped;
            task.completed_at = Some(Utc::now());
        }
    }

    /// Check if all tasks are completed (or skipped/failed)
    pub fn is_complete(&self) -> bool {
        self.graph.node_weights().all(|task| {
            matches!(
                task.status,
                SubtaskStatus::Completed | SubtaskStatus::Skipped | SubtaskStatus::Failed(_)
            )
        })
    }

    /// Get the topological execution order
    pub fn topological_order(&self) -> GlobalResult<Vec<NodeIndex>> {
        petgraph::algo::toposort(&self.graph, None).map_err(|cycle| {
            GlobalError::Other(format!(
                "DAG contains a cycle at node {:?}",
                cycle.node_id()
            ))
        })
    }

    /// Get a subtask by its node index
    pub fn get_task(&self, idx: NodeIndex) -> Option<&SwarmSubtask> {
        self.graph.node_weight(idx)
    }

    /// Get a mutable reference to a subtask
    pub fn get_task_mut(&mut self, idx: NodeIndex) -> Option<&mut SwarmSubtask> {
        self.graph.node_weight_mut(idx)
    }

    /// Look up a node index by subtask id
    pub fn find_by_id(&self, id: &str) -> Option<NodeIndex> {
        self.id_to_index.get(id).copied()
    }

    /// Total number of tasks in the DAG
    pub fn task_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Number of tasks in the Completed state
    pub fn completed_count(&self) -> usize {
        self.graph
            .node_weights()
            .filter(|t| t.status == SubtaskStatus::Completed)
            .count()
    }

    /// Number of tasks in any terminal state (Completed, Skipped, or Failed)
    pub fn terminal_count(&self) -> usize {
        self.graph
            .node_weights()
            .filter(|t| {
                matches!(
                    t.status,
                    SubtaskStatus::Completed | SubtaskStatus::Skipped | SubtaskStatus::Failed(_)
                )
            })
            .count()
    }

    /// Fraction of tasks that have reached a terminal state.
    ///
    /// Uses the same terminal-state definition as `is_complete`: a task
    /// counts toward progress when it is Completed, Skipped, or Failed.
    pub fn progress(&self) -> f64 {
        let total = self.task_count();
        if total == 0 {
            return 1.0;
        }
        self.terminal_count() as f64 / total as f64
    }

    /// Iterate over all tasks with their node indices
    pub fn all_tasks(&self) -> Vec<(NodeIndex, &SwarmSubtask)> {
        self.graph
            .node_indices()
            .map(|idx| (idx, &self.graph[idx]))
            .collect()
    }

    /// Get the dependencies of a specific task (incoming edges)
    pub fn dependencies_of(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .map(|e| e.source())
            .collect()
    }

    /// Get the dependents of a specific task (outgoing edges)
    pub fn dependents_of(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        self.graph
            .edges_directed(idx, Direction::Outgoing)
            .map(|e| e.target())
            .collect()
    }

    /// Assign an agent to a subtask
    pub fn assign_agent(&mut self, idx: NodeIndex, agent_id: impl Into<String>) {
        if let Some(task) = self.graph.node_weight_mut(idx) {
            task.assigned_agent = Some(agent_id.into());
        }
    }

    /// Number of tasks in the Failed state
    pub fn failed_count(&self) -> usize {
        self.graph
            .node_weights()
            .filter(|t| matches!(t.status, SubtaskStatus::Failed(_)))
            .count()
    }

    // ── Risk & HITL helpers ───────────────────────────────────────────────

    /// Return the IDs of all subtasks whose `hitl_required` flag is `true`.
    pub fn hitl_required_tasks(&self) -> Vec<String> {
        self.graph
            .node_weights()
            .filter(|t| t.hitl_required)
            .map(|t| t.id.clone())
            .collect()
    }

    /// Return all tasks whose `risk_level` is at or above `min_risk`.
    pub fn tasks_at_risk(&self, min_risk: &RiskLevel) -> Vec<(NodeIndex, &SwarmSubtask)> {
        self.graph
            .node_indices()
            .filter_map(|idx| {
                let task = &self.graph[idx];
                if &task.risk_level >= min_risk {
                    Some((idx, task))
                } else {
                    None
                }
            })
            .collect()
    }

    // ── Critical path ─────────────────────────────────────────────────────

    /// Compute the critical path through the DAG.
    ///
    /// Returns an ordered list of task **IDs** forming the longest-duration
    /// path from any source node to any sink node, using
    /// `estimated_duration_secs` as the edge weight (defaulting to `0` when
    /// `None`).
    ///
    /// Returns an empty `Vec` when the DAG is empty.
    pub fn critical_path(&self) -> GlobalResult<Vec<String>> {
        let order = self.topological_order()?;
        if order.is_empty() {
            return Ok(Vec::new());
        }

        // Forward pass: longest[node] = duration(node) + max(longest[predecessors])
        let mut longest: HashMap<NodeIndex, u64> = HashMap::new();
        let mut predecessor: HashMap<NodeIndex, Option<NodeIndex>> = HashMap::new();

        for &idx in &order {
            let duration = self.graph[idx].estimated_duration_secs.unwrap_or(0);
            let best_pred = self
                .graph
                .edges_directed(idx, Direction::Incoming)
                .map(|e| (e.source(), *longest.get(&e.source()).unwrap_or(&0)))
                .max_by_key(|&(_, v)| v);

            let (pred, pred_val) = best_pred.map(|(n, v)| (Some(n), v)).unwrap_or((None, 0));

            longest.insert(idx, pred_val + duration);
            predecessor.insert(idx, pred);
        }

        // Find the sink with the maximum finish time
        let &sink = order
            .iter()
            .max_by_key(|&&idx| longest.get(&idx).unwrap_or(&0))
            .unwrap(); // safe: order is non-empty

        // Backtrack to reconstruct the path
        let mut path = Vec::new();
        let mut current = Some(sink);
        while let Some(idx) = current {
            path.push(self.graph[idx].id.clone());
            current = *predecessor.get(&idx).unwrap_or(&None);
        }
        path.reverse();
        Ok(path)
    }

    /// Total estimated seconds along the critical path.
    ///
    /// Returns `0` when the DAG is empty or all durations are unknown.
    pub fn critical_path_duration_secs(&self) -> GlobalResult<u64> {
        let path = self.critical_path()?;
        let total = path
            .iter()
            .filter_map(|id| self.find_by_id(id))
            .filter_map(|idx| self.graph[idx].estimated_duration_secs)
            .sum();
        Ok(total)
    }

    /// Skip all Pending/Ready tasks that transitively depend on `failed_idx`
    /// through hard (Sequential/DataFlow) edges. Returns the number of tasks skipped.
    pub fn cascade_skip(&mut self, failed_idx: NodeIndex) -> usize {
        let mut to_skip = Vec::new();
        let mut stack = vec![failed_idx];

        while let Some(idx) = stack.pop() {
            for edge in self.graph.edges_directed(idx, Direction::Outgoing) {
                if matches!(
                    edge.weight().kind,
                    DependencyKind::Sequential | DependencyKind::DataFlow
                ) {
                    let target = edge.target();
                    if matches!(
                        self.graph[target].status,
                        SubtaskStatus::Pending | SubtaskStatus::Ready
                    ) && !to_skip.contains(&target)
                    {
                        to_skip.push(target);
                        stack.push(target);
                    }
                }
            }
        }

        for &idx in &to_skip {
            self.mark_skipped(idx);
        }
        to_skip.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_dag() {
        let dag = SubtaskDAG::new("empty");
        assert_eq!(dag.task_count(), 0);
        assert!(dag.is_complete());
        assert_eq!(dag.progress(), 1.0);
        assert!(dag.ready_tasks().is_empty());
    }

    #[test]
    fn test_single_task() {
        let mut dag = SubtaskDAG::new("single");
        let t1 = dag.add_task(SwarmSubtask::new("t1", "Task 1"));

        assert_eq!(dag.task_count(), 1);
        assert!(!dag.is_complete());

        // Single task with no deps should be ready
        let ready = dag.ready_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0], t1);

        dag.mark_complete(t1);
        assert!(dag.is_complete());
        assert_eq!(dag.progress(), 1.0);
    }

    #[test]
    fn test_linear_chain() {
        let mut dag = SubtaskDAG::new("chain");
        let a = dag.add_task(SwarmSubtask::new("a", "Search"));
        let b = dag.add_task(SwarmSubtask::new("b", "Analyze"));
        let c = dag.add_task(SwarmSubtask::new("c", "Report"));

        dag.add_dependency(a, b).unwrap();
        dag.add_dependency(b, c).unwrap();

        // Only "a" is ready
        assert_eq!(dag.ready_tasks(), vec![a]);

        dag.mark_complete(a);
        assert_eq!(dag.ready_tasks(), vec![b]);

        dag.mark_complete(b);
        assert_eq!(dag.ready_tasks(), vec![c]);

        dag.mark_complete(c);
        assert!(dag.is_complete());
    }

    #[test]
    fn test_diamond_dag() {
        let mut dag = SubtaskDAG::new("diamond");
        let a = dag.add_task(SwarmSubtask::new("a", "Start"));
        let b = dag.add_task(SwarmSubtask::new("b", "Path 1"));
        let c = dag.add_task(SwarmSubtask::new("c", "Path 2"));
        let d = dag.add_task(SwarmSubtask::new("d", "Merge"));

        dag.add_dependency(a, b).unwrap();
        dag.add_dependency(a, c).unwrap();
        dag.add_dependency(b, d).unwrap();
        dag.add_dependency(c, d).unwrap();

        // only a has no dependencies
        let ready = dag.ready_tasks();
        assert_eq!(ready, vec![a]);

        dag.mark_complete(a);
        let mut ready = dag.ready_tasks();
        ready.sort();
        let mut expected = vec![b, c];
        expected.sort();
        assert_eq!(ready, expected); // b and c ready in parallel

        dag.mark_complete(b);
        // c is still pending; d must NOT be in ready list
        let ready_after_b = dag.ready_tasks();
        assert!(
            !ready_after_b.contains(&d),
            "d should not be ready while c is pending"
        );

        dag.mark_complete(c);
        assert_eq!(dag.ready_tasks(), vec![d]); // now d is ready

        dag.mark_complete(d);
        assert!(dag.is_complete());
    }

    #[test]
    fn test_cycle_detection() {
        let mut dag = SubtaskDAG::new("cycle");
        let a = dag.add_task(SwarmSubtask::new("a", "A"));
        let b = dag.add_task(SwarmSubtask::new("b", "B"));

        dag.add_dependency(a, b).unwrap();
        let result = dag.add_dependency(b, a);

        assert!(result.is_err());
        // Edge should have been removed
        assert_eq!(dag.graph.edge_count(), 1);
    }

    #[test]
    fn test_topological_order() {
        let mut dag = SubtaskDAG::new("topo");
        let a = dag.add_task(SwarmSubtask::new("a", "A"));
        let b = dag.add_task(SwarmSubtask::new("b", "B"));
        let c = dag.add_task(SwarmSubtask::new("c", "C"));

        dag.add_dependency(a, b).unwrap();
        dag.add_dependency(b, c).unwrap();

        let order = dag.topological_order().unwrap();
        assert_eq!(order, vec![a, b, c]);
    }

    #[test]
    fn test_find_by_id() {
        let mut dag = SubtaskDAG::new("lookup");
        let a = dag.add_task(SwarmSubtask::new("search", "Search"));
        let _b = dag.add_task(SwarmSubtask::new("analyze", "Analyze"));

        assert_eq!(dag.find_by_id("search"), Some(a));
        assert_eq!(dag.find_by_id("nonexistent"), None);
    }

    #[test]
    fn test_soft_dependency() {
        let mut dag = SubtaskDAG::new("soft");
        let a = dag.add_task(SwarmSubtask::new("a", "Optional input"));
        let b = dag.add_task(SwarmSubtask::new("b", "Main task"));

        dag.add_dependency_with_kind(a, b, DependencyKind::Soft)
            .unwrap();

        // b should be ready even though a hasn't completed (soft dep)
        let ready = dag.ready_tasks();
        assert_eq!(ready.len(), 2); // both a and b ready
    }

    #[test]
    fn test_failed_task() {
        let mut dag = SubtaskDAG::new("failure");
        let a = dag.add_task(SwarmSubtask::new("a", "Will fail"));

        dag.mark_failed(a, "timeout");

        assert!(dag.is_complete()); // failed counts as terminal
        let task = dag.get_task(a).unwrap();
        assert!(matches!(task.status, SubtaskStatus::Failed(_)));
    }

    #[test]
    fn test_failed_count() {
        let mut dag = SubtaskDAG::new("fail-count");
        let a = dag.add_task(SwarmSubtask::new("a", "A"));
        let b = dag.add_task(SwarmSubtask::new("b", "B"));
        let c = dag.add_task(SwarmSubtask::new("c", "C"));

        assert_eq!(dag.failed_count(), 0);
        assert_eq!(dag.terminal_count(), 0);

        dag.mark_failed(a, "error");
        assert_eq!(dag.failed_count(), 1);
        assert_eq!(dag.terminal_count(), 1);

        dag.mark_complete(b);
        dag.mark_skipped(c);
        assert_eq!(dag.failed_count(), 1);
        assert_eq!(dag.terminal_count(), 3);
    }

    #[test]
    fn test_cascade_skip_linear_chain() {
        let mut dag = SubtaskDAG::new("cascade-chain");
        let a = dag.add_task(SwarmSubtask::new("a", "Fetch"));
        let b = dag.add_task(SwarmSubtask::new("b", "Process"));
        let c = dag.add_task(SwarmSubtask::new("c", "Report"));

        dag.add_dependency(a, b).unwrap();
        dag.add_dependency(b, c).unwrap();

        dag.mark_failed(a, "timeout");
        let skipped = dag.cascade_skip(a);

        assert_eq!(skipped, 2);
        assert_eq!(dag.get_task(b).unwrap().status, SubtaskStatus::Skipped);
        assert_eq!(dag.get_task(c).unwrap().status, SubtaskStatus::Skipped);
        assert!(dag.is_complete());
    }

    #[test]
    fn test_cascade_skip_diamond_only_skips_hard_deps() {
        let mut dag = SubtaskDAG::new("cascade-diamond");
        let a = dag.add_task(SwarmSubtask::new("a", "Fails"));
        let b = dag.add_task(SwarmSubtask::new("b", "Hard dep on a"));
        let c = dag.add_task(SwarmSubtask::new("c", "Soft dep on a"));
        let d = dag.add_task(SwarmSubtask::new("d", "Independent"));

        dag.add_dependency(a, b).unwrap(); // Sequential (hard)
        dag.add_dependency_with_kind(a, c, DependencyKind::Soft)
            .unwrap();

        dag.mark_failed(a, "error");
        let skipped = dag.cascade_skip(a);

        // Only b should be skipped (hard dep), not c (soft dep) or d (no dep)
        assert_eq!(skipped, 1);
        assert_eq!(dag.get_task(b).unwrap().status, SubtaskStatus::Skipped);
        assert_eq!(dag.get_task(c).unwrap().status, SubtaskStatus::Pending);
        assert_eq!(dag.get_task(d).unwrap().status, SubtaskStatus::Pending);
    }

    #[test]
    fn test_cascade_skip_does_not_skip_running_tasks() {
        let mut dag = SubtaskDAG::new("cascade-running");
        let a = dag.add_task(SwarmSubtask::new("a", "Fails"));
        let b = dag.add_task(SwarmSubtask::new("b", "Already running"));
        let c = dag.add_task(SwarmSubtask::new("c", "Pending after b"));

        dag.add_dependency(a, b).unwrap();
        dag.add_dependency(b, c).unwrap();

        dag.mark_running(b); // b started before a failed
        dag.mark_failed(a, "late failure");
        let skipped = dag.cascade_skip(a);

        // b is Running (not Pending/Ready), so it should NOT be skipped
        // c depends on b which is Running, so cascade should not reach c through b
        assert_eq!(skipped, 0);
        assert_eq!(dag.get_task(b).unwrap().status, SubtaskStatus::Running);
    }

    #[test]
    fn test_failed_dependency_unblocks_downstream() {
        let mut dag = SubtaskDAG::new("fail-chain");
        let a = dag.add_task(SwarmSubtask::new("a", "Fetch data"));
        let b = dag.add_task(SwarmSubtask::new("b", "Process data"));
        let c = dag.add_task(SwarmSubtask::new("c", "Generate report"));

        dag.add_dependency(a, b).unwrap();
        dag.add_dependency(b, c).unwrap();

        // Only a is ready initially
        assert_eq!(dag.ready_tasks(), vec![a]);

        // a fails — b should become ready (not stuck forever)
        dag.mark_failed(a, "connection timeout");
        let ready = dag.ready_tasks();
        assert_eq!(
            ready,
            vec![b],
            "b must become ready when its dependency fails"
        );

        // b also fails — c should become ready
        dag.mark_failed(b, "no input data");
        let ready = dag.ready_tasks();
        assert_eq!(
            ready,
            vec![c],
            "c must become ready when its dependency fails"
        );

        dag.mark_skipped(c);
        assert!(dag.is_complete());
    }

    #[test]
    fn test_failed_dependency_diamond_dag() {
        let mut dag = SubtaskDAG::new("fail-diamond");
        let a = dag.add_task(SwarmSubtask::new("a", "Start"));
        let b = dag.add_task(SwarmSubtask::new("b", "Path 1"));
        let c = dag.add_task(SwarmSubtask::new("c", "Path 2"));
        let d = dag.add_task(SwarmSubtask::new("d", "Merge"));

        dag.add_dependency(a, b).unwrap();
        dag.add_dependency(a, c).unwrap();
        dag.add_dependency(b, d).unwrap();
        dag.add_dependency(c, d).unwrap();

        dag.mark_complete(a);
        dag.mark_complete(b);
        dag.mark_failed(c, "path 2 error");

        // d depends on both b (Completed) and c (Failed) — should be ready
        let ready = dag.ready_tasks();
        assert_eq!(
            ready,
            vec![d],
            "d must become ready when all deps are terminal"
        );
    }

    #[test]
    fn test_dataflow_dependency_blocks_on_failed_upstream() {
        // DataFlow edges represent "downstream consumes upstream's output". A
        // Failed upstream produced no output, so the downstream must NOT be
        // considered ready — otherwise we'd run a task whose input doesn't
        // exist. (Sequential edges are temporal-only and are still satisfied
        // by Failed; that's covered by test_failed_dependency_unblocks_downstream.)
        let mut dag = SubtaskDAG::new("dataflow-fail");
        let a = dag.add_task(SwarmSubtask::new("a", "Produces data"));
        let b = dag.add_task(SwarmSubtask::new("b", "Consumes a's output"));

        dag.add_dependency_with_kind(a, b, DependencyKind::DataFlow)
            .unwrap();

        dag.mark_failed(a, "upstream error");

        assert!(
            dag.ready_tasks().is_empty(),
            "b must not become ready when its DataFlow upstream failed"
        );
    }

    #[test]
    fn test_dataflow_dependency_blocks_on_skipped_upstream() {
        // Skipped upstream produced no output either.
        let mut dag = SubtaskDAG::new("dataflow-skip");
        let a = dag.add_task(SwarmSubtask::new("a", "Skipped producer"));
        let b = dag.add_task(SwarmSubtask::new("b", "Consumer"));

        dag.add_dependency_with_kind(a, b, DependencyKind::DataFlow)
            .unwrap();

        dag.mark_skipped(a);

        assert!(
            dag.ready_tasks().is_empty(),
            "b must not become ready when its DataFlow upstream was skipped"
        );
    }

    #[test]
    fn test_dataflow_dependency_ready_when_upstream_completes() {
        let mut dag = SubtaskDAG::new("dataflow-ok");
        let a = dag.add_task(SwarmSubtask::new("a", "Producer"));
        let b = dag.add_task(SwarmSubtask::new("b", "Consumer"));

        dag.add_dependency_with_kind(a, b, DependencyKind::DataFlow)
            .unwrap();

        dag.mark_complete_with_output(a, Some("payload".into()));

        assert_eq!(
            dag.ready_tasks(),
            vec![b],
            "b should be ready once its DataFlow upstream completes"
        );
    }

    #[test]
    fn test_mixed_dataflow_and_sequential_deps_on_failed_upstream() {
        // b has a DataFlow dep on `producer` and a Sequential dep on `gate`.
        // If `producer` fails, b cannot run even if `gate` is terminal —
        // the missing data is the binding constraint.
        let mut dag = SubtaskDAG::new("mixed-deps");
        let producer = dag.add_task(SwarmSubtask::new("producer", "Produces data"));
        let gate = dag.add_task(SwarmSubtask::new("gate", "Ordering only"));
        let b = dag.add_task(SwarmSubtask::new("b", "Needs data + ordering"));

        dag.add_dependency_with_kind(producer, b, DependencyKind::DataFlow)
            .unwrap();
        dag.add_dependency_with_kind(gate, b, DependencyKind::Sequential)
            .unwrap();

        dag.mark_failed(producer, "boom");
        dag.mark_complete(gate);

        assert!(
            dag.ready_tasks().is_empty(),
            "b must stay pending while its DataFlow upstream is in a non-success state"
        );
    }

    #[test]
    fn test_progress_tracking() {
        let mut dag = SubtaskDAG::new("progress");
        let a = dag.add_task(SwarmSubtask::new("a", "A"));
        let b = dag.add_task(SwarmSubtask::new("b", "B"));
        let c = dag.add_task(SwarmSubtask::new("c", "C"));
        let d = dag.add_task(SwarmSubtask::new("d", "D"));

        assert_eq!(dag.progress(), 0.0);

        dag.mark_complete(a);
        assert!((dag.progress() - 0.25).abs() < f64::EPSILON);

        dag.mark_complete(b);
        dag.mark_complete(c);
        dag.mark_complete(d);
        assert_eq!(dag.progress(), 1.0);

        let _ = (a, b, c, d);
    }

    #[test]
    fn test_progress_counts_failed_and_skipped_as_terminal() {
        let mut dag = SubtaskDAG::new("mixed");
        let a = dag.add_task(SwarmSubtask::new("a", "A"));
        let b = dag.add_task(SwarmSubtask::new("b", "B"));
        let c = dag.add_task(SwarmSubtask::new("c", "C"));
        let d = dag.add_task(SwarmSubtask::new("d", "D"));

        dag.mark_complete(a);
        dag.mark_failed(b, "error");
        dag.mark_skipped(c);
        // d stays pending

        // 3 of 4 tasks are terminal
        assert!((dag.progress() - 0.75).abs() < f64::EPSILON);
        assert_eq!(dag.terminal_count(), 3);
        assert_eq!(dag.completed_count(), 1);
        assert!(!dag.is_complete()); // d is still pending

        dag.mark_complete(d);
        assert_eq!(dag.progress(), 1.0);
        assert!(dag.is_complete());
    }

    #[test]
    fn test_agent_assignment() {
        let mut dag = SubtaskDAG::new("assign");
        let a = dag.add_task(SwarmSubtask::new("a", "A"));

        assert!(dag.get_task(a).unwrap().assigned_agent.is_none());

        dag.assign_agent(a, "agent-1");
        assert_eq!(
            dag.get_task(a).unwrap().assigned_agent.as_deref(),
            Some("agent-1")
        );
    }

    // ── RiskLevel tests ───────────────────────────────────────────────────

    #[test]
    fn test_risk_level_ordering() {
        assert!(RiskLevel::Critical > RiskLevel::High);
        assert!(RiskLevel::High > RiskLevel::Medium);
        assert!(RiskLevel::Medium > RiskLevel::Low);
        assert!(RiskLevel::Low < RiskLevel::Critical);
    }

    #[test]
    fn test_risk_level_requires_hitl() {
        assert!(!RiskLevel::Low.requires_hitl());
        assert!(!RiskLevel::Medium.requires_hitl());
        assert!(RiskLevel::High.requires_hitl());
        assert!(RiskLevel::Critical.requires_hitl());
    }

    #[test]
    fn test_risk_level_serde_roundtrip() {
        for level in [
            RiskLevel::Low,
            RiskLevel::Medium,
            RiskLevel::High,
            RiskLevel::Critical,
        ] {
            let json = serde_json::to_string(&level).unwrap();
            let decoded: RiskLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, level);
        }
    }

    #[test]
    fn test_swarmsubtask_default_risk_is_low() {
        let t = SwarmSubtask::new("t1", "Do something");
        assert_eq!(t.risk_level, RiskLevel::Low);
        assert!(!t.hitl_required);
        assert!(t.estimated_duration_secs.is_none());
    }

    #[test]
    fn test_with_risk_level_sets_hitl_required() {
        let low = SwarmSubtask::new("a", "A").with_risk_level(RiskLevel::Low);
        let med = SwarmSubtask::new("b", "B").with_risk_level(RiskLevel::Medium);
        let high = SwarmSubtask::new("c", "C").with_risk_level(RiskLevel::High);
        let crit = SwarmSubtask::new("d", "D").with_risk_level(RiskLevel::Critical);

        assert!(!low.hitl_required);
        assert!(!med.hitl_required);
        assert!(high.hitl_required);
        assert!(crit.hitl_required);
    }

    #[test]
    fn test_hitl_required_tasks_filters_correctly() {
        let mut dag = SubtaskDAG::new("hitl-filter");
        dag.add_task(SwarmSubtask::new("low", "low-risk").with_risk_level(RiskLevel::Low));
        dag.add_task(SwarmSubtask::new("med", "medium-risk").with_risk_level(RiskLevel::Medium));
        dag.add_task(SwarmSubtask::new("high", "high-risk").with_risk_level(RiskLevel::High));
        dag.add_task(
            SwarmSubtask::new("crit", "critical-risk").with_risk_level(RiskLevel::Critical),
        );

        let mut hitl = dag.hitl_required_tasks();
        hitl.sort();
        assert_eq!(hitl, vec!["crit", "high"]);
    }

    #[test]
    fn test_critical_path_linear_chain() {
        let mut dag = SubtaskDAG::new("cp-chain");
        let a = dag.add_task(SwarmSubtask::new("a", "Fetch").with_estimated_duration(10));
        let b = dag.add_task(SwarmSubtask::new("b", "Process").with_estimated_duration(20));
        let c = dag.add_task(SwarmSubtask::new("c", "Report").with_estimated_duration(30));
        dag.add_dependency(a, b).unwrap();
        dag.add_dependency(b, c).unwrap();

        let path = dag.critical_path().unwrap();
        assert_eq!(path, vec!["a", "b", "c"]);
        assert_eq!(dag.critical_path_duration_secs().unwrap(), 60);
    }

    #[test]
    fn test_critical_path_diamond_takes_longer_branch() {
        //       start(5)
        //      /        \
        //  short(10)  long(50)
        //      \        /
        //       merge(5)
        let mut dag = SubtaskDAG::new("cp-diamond");
        let start = dag.add_task(SwarmSubtask::new("start", "Start").with_estimated_duration(5));
        let short = dag.add_task(SwarmSubtask::new("short", "Short").with_estimated_duration(10));
        let long = dag.add_task(SwarmSubtask::new("long", "Long").with_estimated_duration(50));
        let merge = dag.add_task(SwarmSubtask::new("merge", "Merge").with_estimated_duration(5));

        dag.add_dependency(start, short).unwrap();
        dag.add_dependency(start, long).unwrap();
        dag.add_dependency(short, merge).unwrap();
        dag.add_dependency(long, merge).unwrap();

        let path = dag.critical_path().unwrap();
        // Critical path: start → long → merge (total = 5 + 50 + 5 = 60)
        assert!(
            path.contains(&"long".to_string()),
            "critical path must go through 'long': {path:?}"
        );
        assert!(
            !path.contains(&"short".to_string()),
            "critical path must NOT go through 'short': {path:?}"
        );
        assert_eq!(dag.critical_path_duration_secs().unwrap(), 60);
    }

    #[test]
    fn test_critical_path_empty_dag_returns_empty() {
        let dag = SubtaskDAG::new("empty-cp");
        let path = dag.critical_path().unwrap();
        assert!(path.is_empty());
        assert_eq!(dag.critical_path_duration_secs().unwrap(), 0);
    }
}
