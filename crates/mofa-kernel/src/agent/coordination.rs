//! Multi-Agent Coordination Traits — AgentHub Integration
//! 多智能体协调 Trait — AgentHub 集成
//!
//! This module defines the kernel-level coordination contracts for multi-agent
//! handoffs, shared memory, conflict detection, and governance.
//! 本模块定义了多智能体交接、共享内存、冲突检测和治理的内核级协调契约。
//!
//! # Design Principles
//! # 设计原则
//!
//! 1. **Traits only** — no concrete implementations in the kernel layer.
//! 1. **仅定义 Trait** — 内核层不包含具体实现代码。
//! 2. **`Send + Sync`** — all traits are safe for concurrent access.
//! 2. **`Send + Sync`** — 所有 Trait 均可安全并发访问。
//! 3. **`u64` timestamps** — milliseconds since epoch, consistent with `crate::utils::now_ms()`.
//! 3. **`u64` 时间戳** — 自 epoch 起的毫秒数，与 `crate::utils::now_ms()` 一致。
//! 4. **Error integration** — maps into [`AgentError`] via `From<CoordinationError>`.
//! 4. **错误集成** — 通过 `From<CoordinationError>` 映射到 [`AgentError`]。
//!
//! Implementations of these traits live in higher layers such as `mofa-foundation`.
//! 这些 Trait 的具体实现位于更高层，例如 `mofa-foundation`。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

// ============================================================================
// Core Types — 协调核心类型
// ============================================================================

/// Reference to a memory entry in the shared store.
/// 共享存储中内存条目的引用。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryRef {
    /// Unique identifier for the memory entry.
    /// 内存条目的唯一标识符。
    pub id: Uuid,
}

impl MemoryRef {
    /// Create a new memory reference.
    /// 创建新的内存引用。
    pub fn new(id: Uuid) -> Self {
        Self { id }
    }
}

/// A memory object stored in the shared memory store.
/// 存储在共享内存中的内存对象。
///
/// Contains the actual content along with ownership and workflow metadata.
/// 包含实际内容以及归属和工作流元数据。
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryObject {
    /// Unique identifier for this memory entry.
    /// 此内存条目的唯一标识符。
    pub memory_id: Uuid,

    /// ID of the agent that wrote this memory.
    /// 写入此内存的 Agent ID。
    pub owner_agent: String,

    /// The stored content.
    /// 存储的内容。
    pub content: String,

    /// Workflow this memory belongs to.
    /// 此内存所属的工作流。
    pub workflow_id: String,

    /// Timestamp in milliseconds since epoch.
    /// 自 epoch 起的毫秒时间戳。
    pub timestamp: u64,
}

impl MemoryObject {
    /// Create a new memory object.
    /// 创建新的内存对象。
    pub fn new(
        memory_id: Uuid,
        owner_agent: String,
        content: String,
        workflow_id: String,
        timestamp: u64,
    ) -> Self {
        Self {
            memory_id,
            owner_agent,
            content,
            workflow_id,
            timestamp,
        }
    }
}

/// Context passed during an agent-to-agent handoff.
/// Agent 之间交接传递的上下文。
///
/// Captures the completed task, decisions made, confidence level,
/// and references to shared memory entries.
/// 捕获已完成的任务、做出的决策、置信度以及共享内存条目的引用。
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffContext {
    /// Description of the completed task.
    /// 已完成任务的描述。
    pub task_completed: String,

    /// Decisions made during execution.
    /// 执行期间做出的决策。
    pub decisions: Vec<String>,

    /// Confidence score (0.0 to 1.0).
    /// 置信度分数（0.0 到 1.0）。
    pub confidence: f32,

    /// Description of the next task for the receiving agent.
    /// 接收 Agent 的下一个任务描述。
    pub next_task: String,

    /// References to shared memory entries relevant to this handoff.
    /// 与此交接相关的共享内存条目引用。
    pub memory_refs: Vec<MemoryRef>,
}

impl HandoffContext {
    /// Create a new handoff context.
    /// 创建新的交接上下文。
    pub fn new(
        task_completed: String,
        decisions: Vec<String>,
        confidence: f32,
        next_task: String,
        memory_refs: Vec<MemoryRef>,
    ) -> Self {
        Self {
            task_completed,
            decisions,
            confidence,
            next_task,
            memory_refs,
        }
    }
}

/// A typed, schema-validated packet for agent-to-agent handoffs.
/// 用于 Agent 之间交接的类型化、模式验证数据包。
///
/// Nothing is lost at handoff boundaries — every decision, confidence score,
/// and memory reference is preserved and auditable.
/// 交接边界不会丢失任何内容 — 每个决策、置信度分数和内存引用都被保留并可审计。
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffPacket {
    /// Unique identifier for this handoff.
    /// 此交接的唯一标识符。
    pub handoff_id: Uuid,

    /// Source agent ID.
    /// 源 Agent ID。
    pub from_agent: String,

    /// Destination agent ID.
    /// 目标 Agent ID。
    pub to_agent: String,

    /// Description of the completed task.
    /// 已完成任务的描述。
    pub task_completed: String,

    /// Decisions made during execution.
    /// 执行期间做出的决策。
    pub decisions: Vec<String>,

    /// Confidence score (0.0 to 1.0).
    /// 置信度分数（0.0 到 1.0）。
    pub confidence: f32,

    /// References to shared memory entries.
    /// 共享内存条目引用。
    pub memory_refs: Vec<MemoryRef>,

    /// Description of the next task for the receiving agent.
    /// 接收 Agent 的下一个任务描述。
    pub next_task: String,

    /// Workflow this handoff belongs to.
    /// 此交接所属的工作流。
    pub workflow_id: String,

    /// Timestamp in milliseconds since epoch.
    /// 自 epoch 起的毫秒时间戳。
    pub timestamp: u64,
}

impl HandoffPacket {
    /// Create a new handoff packet.
    /// 创建新的交接数据包。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        handoff_id: Uuid,
        from_agent: String,
        to_agent: String,
        task_completed: String,
        decisions: Vec<String>,
        confidence: f32,
        memory_refs: Vec<MemoryRef>,
        next_task: String,
        workflow_id: String,
        timestamp: u64,
    ) -> Self {
        Self {
            handoff_id,
            from_agent,
            to_agent,
            task_completed,
            decisions,
            confidence,
            memory_refs,
            next_task,
            workflow_id,
            timestamp,
        }
    }
}
/// 内存条目之间检测到的冲突信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictInfo {
    /// Unique identifier for this conflict.
    /// 此冲突的唯一标识符。
    pub conflict_id: Uuid,

    /// Reference to the memory entry involved.
    /// 涉及的内存条目引用。
    pub memory_ref: MemoryRef,

    /// The existing value in the store.
    /// 存储中的现有值。
    pub existing_value: String,

    /// The incoming (conflicting) value.
    /// 传入的（冲突的）值。
    pub incoming_value: String,

    /// When the conflict was detected (ms since epoch).
    /// 检测到冲突的时间（自 epoch 起的毫秒数）。
    pub detected_at: u64,

    /// Workflow where the conflict occurred.
    /// 发生冲突的工作流。
    pub workflow_id: String,
}

impl ConflictInfo {
    /// Create a new conflict info.
    /// 创建新的冲突信息。
    pub fn new(
        conflict_id: Uuid,
        memory_ref: MemoryRef,
        existing_value: String,
        incoming_value: String,
        detected_at: u64,
        workflow_id: String,
    ) -> Self {
        Self {
            conflict_id,
            memory_ref,
            existing_value,
            incoming_value,
            detected_at,
            workflow_id,
        }
    }
}

/// Strategy for resolving a detected conflict.
/// 解决检测到的冲突的策略。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionStrategy {
    /// Keep the existing value, discard incoming.
    /// 保留现有值，丢弃传入值。
    KeepExisting,

    /// Keep the incoming value, overwrite existing.
    /// 保留传入值，覆盖现有值。
    KeepIncoming,

    /// Merge both values (implementation-defined).
    /// 合并两个值（由实现定义）。
    Merge,

    /// Escalate to a human or higher-level agent.
    /// 上报给人类或更高级别的 Agent。
    Escalate,
}

/// Configuration for governance policies.
/// 治理策略的配置。
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceConfig {
    /// Maximum depth of agent spawning chains.
    /// Agent 生成链的最大深度。
    pub max_spawn_depth: u32,

    /// Maximum number of agents allowed in a single workflow.
    /// 单个工作流中允许的最大 Agent 数量。
    pub max_agents_per_workflow: u32,

    /// Whether to record all handoffs in the audit trail.
    /// 是否在审计跟踪中记录所有交接。
    pub enable_audit: bool,
}

impl GovernanceConfig {
    /// Create a new governance configuration.
    /// 创建新的治理配置。
    pub fn new(max_spawn_depth: u32, max_agents_per_workflow: u32, enable_audit: bool) -> Self {
        Self {
            max_spawn_depth,
            max_agents_per_workflow,
            enable_audit,
        }
    }
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            max_spawn_depth: 5,
            max_agents_per_workflow: 20,
            enable_audit: true,
        }
    }
}

// ============================================================================
// CoordinationError — 协调错误类型
// ============================================================================

/// Coordination-specific errors.
/// 协调专用错误类型。
///
/// These are mapped into [`AgentError::CoordinationError`] via `From` impl.
/// 这些错误通过 `From` 实现映射到 [`AgentError::CoordinationError`]。
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum CoordinationError {
    /// Shared memory operation failed.
    /// 共享内存操作失败。
    #[error("Memory error: {0}")]
    Memory(String),

    /// Handoff operation failed.
    /// 交接操作失败。
    #[error("Handoff error: {0}")]
    Handoff(String),

    /// Conflict resolution failed.
    /// 冲突解决失败。
    #[error("Conflict error: {0}")]
    Conflict(String),

    /// Governance rule violation.
    /// 治理规则违反。
    #[error("Governance error: {0}")]
    Governance(String),
}

/// Convenience result type for coordination operations.
/// 协调操作的便捷 Result 类型。
pub type CoordinationResult<T> = Result<T, CoordinationError>;

/// Convert coordination errors into the existing `AgentError` hierarchy.
/// 将协调错误转换为现有的 `AgentError` 层次结构。
impl From<CoordinationError> for super::error::AgentError {
    fn from(err: CoordinationError) -> Self {
        super::error::AgentError::CoordinationError(err.to_string())
    }
}

// ============================================================================
// Trait 1: AgentMemory — 共享内存接口
// ============================================================================

/// Shared memory interface for multi-agent workflows.
/// 多智能体工作流的共享内存接口。
///
/// All agents in a workflow read from and write to a single shared store,
/// enabling grounded decision-making and eliminating information silos.
/// 工作流中的所有 Agent 读写同一个共享存储，实现有据可依的决策，消除信息孤岛。
///
/// Implementations live in higher layers such as `mofa-foundation`.
/// 实现在更高层（如 `mofa-foundation`）中提供。
#[async_trait]
pub trait AgentMemory: Send + Sync {
    /// Write content to the shared memory store.
    /// 将内容写入共享内存存储。
    ///
    /// # Parameters / 参数
    ///
    /// - `content`: The content to store.
    /// - `agent_id`: ID of the agent writing.
    /// - `workflow_id`: Workflow this memory belongs to.
    ///
    /// # Returns / 返回
    ///
    /// A reference to the newly created memory entry.
    /// 新创建的内存条目的引用。
    async fn write(
        &self,
        content: &str,
        agent_id: &str,
        workflow_id: &str,
    ) -> CoordinationResult<MemoryRef>;

    /// Search the shared memory store.
    /// 搜索共享内存存储。
    ///
    /// The matching strategy (substring, semantic, etc.) is implementation-defined.
    /// 匹配策略（子串、语义等）由具体实现决定。
    ///
    /// # Parameters / 参数
    ///
    /// - `query`: Search query string.
    /// - `limit`: Maximum number of results to return.
    ///
    /// # Returns / 返回
    ///
    /// Matching memory objects. Ordering is implementation-defined.
    /// 匹配的内存对象。排序由具体实现决定。
    async fn read(&self, query: &str, limit: usize) -> CoordinationResult<Vec<MemoryObject>>;

    /// Delete a memory entry from the store.
    /// 从存储中删除内存条目。
    async fn delete(&self, memory_ref: &MemoryRef) -> CoordinationResult<()>;

    /// List all memory entries for a given workflow.
    /// 列出给定工作流的所有内存条目。
    async fn list_by_workflow(&self, workflow_id: &str) -> CoordinationResult<Vec<MemoryObject>>;
}

// ============================================================================
// Trait 2: HandoffProtocol — 交接协议
// ============================================================================

/// Typed context passing between agents.
/// Agent 之间的类型化上下文传递。
///
/// Ensures that every handoff is schema-validated — no agent starts fresh
/// without the decisions, confidence scores, and memory refs from its predecessor.
/// 确保每次交接都经过模式验证 — 没有 Agent 在缺少前任的决策、置信度分数和内存引用的情况下从零开始。
#[async_trait]
pub trait HandoffProtocol: Send + Sync {
    /// Create a handoff packet from one agent to another.
    /// 创建从一个 Agent 到另一个 Agent 的交接数据包。
    async fn create_handoff(
        &self,
        from: &str,
        to: &str,
        context: HandoffContext,
    ) -> CoordinationResult<HandoffPacket>;

    /// Receive a pending handoff for the given agent.
    /// 接收给定 Agent 的待处理交接。
    ///
    /// Returns `None` if no handoff is pending.
    /// 如果没有待处理的交接，则返回 `None`。
    async fn receive_handoff(
        &self,
        agent_id: &str,
    ) -> CoordinationResult<Option<HandoffPacket>>;

    /// Acknowledge receipt of a handoff.
    /// 确认收到交接。
    async fn acknowledge_handoff(&self, handoff_id: Uuid) -> CoordinationResult<()>;

    /// List all handoffs within a workflow.
    /// 列出工作流中的所有交接。
    async fn list_handoffs(&self, workflow_id: &str) -> CoordinationResult<Vec<HandoffPacket>>;
}

// ============================================================================
// Trait 3: ConflictDetector — 冲突检测
// ============================================================================

/// Detects and resolves conflicts when multiple agents write to shared memory.
/// 当多个 Agent 写入共享内存时检测和解决冲突。
///
/// Without conflict detection, agents silently overwrite each other's findings.
/// 有了冲突检测，冲突会被检测、记录，并根据策略解决。
#[async_trait]
pub trait ConflictDetector: Send + Sync {
    /// Detect a conflict between an existing and an incoming memory object.
    /// 检测现有内存对象和传入内存对象之间的冲突。
    ///
    /// Returns `None` if no conflict is detected.
    /// 如果未检测到冲突，则返回 `None`。
    fn detect(&self, existing: &MemoryObject, incoming: &MemoryObject) -> Option<ConflictInfo>;

    /// Resolve a conflict using the given strategy.
    /// 使用给定策略解决冲突。
    async fn resolve(
        &self,
        conflict: &ConflictInfo,
        strategy: ResolutionStrategy,
    ) -> CoordinationResult<MemoryObject>;

    /// List all unresolved conflicts in a workflow.
    /// 列出工作流中所有未解决的冲突。
    async fn list_conflicts(&self, workflow_id: &str) -> CoordinationResult<Vec<ConflictInfo>>;
}

// ============================================================================
// Trait 4: CoordinationGovernor — 治理控制
// ============================================================================

/// Governance layer — span-of-control limits, audit trail, dead-letter queue.
/// 治理层 — 控制范围限制、审计跟踪、死信队列。
///
/// Prevents runaway agent spawning, records every handoff for auditing,
/// and captures failed handoffs for later diagnosis.
/// 防止 Agent 无限制生成，记录每次交接以供审计，并捕获失败的交接以供后续诊断。
#[async_trait]
pub trait CoordinationGovernor: Send + Sync {
    /// Check whether an agent is allowed to spawn at the given depth.
    /// 检查 Agent 是否允许在给定深度生成。
    ///
    /// # Parameters / 参数
    ///
    /// - `agent_id`: The agent requesting to spawn.
    /// - `depth`: Current spawn depth.
    /// - `config`: Governance rules to apply.
    fn check_spawn_allowed(&self, agent_id: &str, depth: u32, config: &GovernanceConfig) -> bool;

    /// Record a handoff in the audit trail.
    /// 在审计跟踪中记录交接。
    async fn record_handoff(&self, packet: &HandoffPacket) -> CoordinationResult<()>;

    /// Get the full audit trail for a workflow.
    /// 获取工作流的完整审计跟踪。
    async fn get_audit_trail(
        &self,
        workflow_id: &str,
    ) -> CoordinationResult<Vec<HandoffPacket>>;

    /// Add a failed handoff to the dead-letter queue for later inspection.
    /// 将失败的交接添加到死信队列以供后续检查。
    async fn add_to_dead_letter(
        &self,
        failed: HandoffPacket,
        reason: &str,
    ) -> CoordinationResult<()>;
}

// ============================================================================
// Tests — 编译和 mock 实现测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tokio::sync::Mutex;

    // ---- Mock in-memory shared store ----

    struct MockMemoryStore {
        entries: Mutex<HashMap<Uuid, MemoryObject>>,
    }

    impl MockMemoryStore {
        fn new() -> Self {
            Self {
                entries: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl AgentMemory for MockMemoryStore {
        async fn write(
            &self,
            content: &str,
            agent_id: &str,
            workflow_id: &str,
        ) -> CoordinationResult<MemoryRef> {
            let id = Uuid::new_v4();
            let obj = MemoryObject {
                memory_id: id,
                owner_agent: agent_id.to_string(),
                content: content.to_string(),
                workflow_id: workflow_id.to_string(),
                timestamp: crate::utils::now_ms(),
            };
            self.entries.lock().await.insert(id, obj);
            Ok(MemoryRef::new(id))
        }

        async fn read(&self, query: &str, limit: usize) -> CoordinationResult<Vec<MemoryObject>> {
            let entries = self.entries.lock().await;
            let results: Vec<MemoryObject> = entries
                .values()
                .filter(|obj| obj.content.contains(query))
                .take(limit)
                .cloned()
                .collect();
            Ok(results)
        }

        async fn delete(&self, memory_ref: &MemoryRef) -> CoordinationResult<()> {
            self.entries.lock().await.remove(&memory_ref.id);
            Ok(())
        }

        async fn list_by_workflow(
            &self,
            workflow_id: &str,
        ) -> CoordinationResult<Vec<MemoryObject>> {
            let entries = self.entries.lock().await;
            let results: Vec<MemoryObject> = entries
                .values()
                .filter(|obj| obj.workflow_id == workflow_id)
                .cloned()
                .collect();
            Ok(results)
        }
    }

    // ---- Compile-time trait object tests ----

    #[test]
    fn types_are_serializable() {
        let mem_ref = MemoryRef::new(Uuid::new_v4());
        let json = serde_json::to_string(&mem_ref).unwrap();
        let _: MemoryRef = serde_json::from_str(&json).unwrap();

        let strategy = ResolutionStrategy::KeepExisting;
        let json = serde_json::to_string(&strategy).unwrap();
        let _: ResolutionStrategy = serde_json::from_str(&json).unwrap();

        let config = GovernanceConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let _: GovernanceConfig = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn coordination_error_converts_to_agent_error() {
        let coord_err = CoordinationError::Memory("test failure".to_string());
        let agent_err: super::super::error::AgentError = coord_err.into();
        assert!(agent_err.to_string().contains("test failure"));
    }

    #[test]
    fn governance_config_defaults_are_sane() {
        let config = GovernanceConfig::default();
        assert!(config.max_spawn_depth > 0);
        assert!(config.max_agents_per_workflow > 0);
        assert!(config.enable_audit);
    }

    #[tokio::test]
    async fn mock_memory_store_roundtrip() {
        let store = MockMemoryStore::new();

        // Write
        let mem_ref = store
            .write("finding: X is true", "agent-1", "wf-1")
            .await
            .unwrap();

        // Read
        let results = store.read("X is true", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].owner_agent, "agent-1");

        // List by workflow
        let wf_results = store.list_by_workflow("wf-1").await.unwrap();
        assert_eq!(wf_results.len(), 1);

        // Delete
        store.delete(&mem_ref).await.unwrap();
        let results = store.read("X is true", 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn handoff_packet_roundtrip() {
        let packet = HandoffPacket {
            handoff_id: Uuid::new_v4(),
            from_agent: "agent-a".to_string(),
            to_agent: "agent-b".to_string(),
            task_completed: "Analyzed dataset".to_string(),
            decisions: vec!["Use method A".to_string()],
            confidence: 0.92,
            memory_refs: vec![MemoryRef::new(Uuid::new_v4())],
            next_task: "Generate report".to_string(),
            workflow_id: "wf-1".to_string(),
            timestamp: crate::utils::now_ms(),
        };

        let json = serde_json::to_string(&packet).unwrap();
        let deserialized: HandoffPacket = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.from_agent, "agent-a");
        assert_eq!(deserialized.confidence, 0.92);
        assert_eq!(deserialized.workflow_id, "wf-1");
    }

    #[test]
    fn conflict_info_roundtrip() {
        let info = ConflictInfo {
            conflict_id: Uuid::new_v4(),
            memory_ref: MemoryRef::new(Uuid::new_v4()),
            existing_value: "value A".to_string(),
            incoming_value: "value B".to_string(),
            detected_at: crate::utils::now_ms(),
            workflow_id: "wf-1".to_string(),
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: ConflictInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.existing_value, "value A");
    }

    // Ensure trait objects compile (object safety check).
    fn _assert_object_safe(
        _memory: &dyn AgentMemory,
        _handoff: &dyn HandoffProtocol,
        _conflict: &dyn ConflictDetector,
        _governor: &dyn CoordinationGovernor,
    ) {
    }
}

