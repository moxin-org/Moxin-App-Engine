//! 默认类型定义
//! Default type definitions
//!
//! 包含默认秘书实现使用的所有类型。
//! Contains all types used by the default secretary implementation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// Todo 任务类型
// Todo Task Types
// =============================================================================

/// Todo 任务状态
/// Todo task status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TodoStatus {
    /// 待处理
    /// Pending
    Pending,
    /// 需求澄清中
    /// Clarifying requirement
    Clarifying,
    /// 执行中
    /// In progress
    InProgress,
    /// 等待反馈
    /// Waiting for feedback
    WaitingFeedback,
    /// 已完成
    /// Completed
    Completed,
    /// 已取消
    /// Cancelled
    Cancelled,
    /// 阻塞中（需要人类决策）
    /// Blocked (requires human decision)
    Blocked(String),
}

/// Todo 任务优先级
/// Todo task priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TodoPriority {
    Low = 0,
    Medium = 1,
    High = 2,
    Urgent = 3,
}

/// Todo 任务项
/// Todo task item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// 任务ID
    /// Task ID
    pub id: String,
    /// 原始想法/需求描述
    /// Original idea/requirement description
    pub raw_idea: String,
    /// 澄清后的需求文档
    /// Clarified requirement document
    pub clarified_requirement: Option<ProjectRequirement>,
    /// 任务状态
    /// Task status
    pub status: TodoStatus,
    /// 优先级
    /// Priority
    pub priority: TodoPriority,
    /// 创建时间（Unix时间戳）
    /// Creation time (Unix timestamp)
    pub created_at: u64,
    /// 更新时间
    /// Update time
    pub updated_at: u64,
    /// 分配的执行Agent ID列表
    /// List of assigned execution Agent IDs
    pub assigned_agents: Vec<String>,
    /// 执行结果
    /// Execution result
    pub execution_result: Option<ExecutionResult>,
    /// 元数据
    /// Metadata
    pub metadata: HashMap<String, String>,
}

impl TodoItem {
    pub fn new(id: &str, raw_idea: &str, priority: TodoPriority) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            id: id.to_string(),
            raw_idea: raw_idea.to_string(),
            clarified_requirement: None,
            status: TodoStatus::Pending,
            priority,
            created_at: now,
            updated_at: now,
            assigned_agents: Vec::new(),
            execution_result: None,
            metadata: HashMap::new(),
        }
    }

    /// 更新状态
    /// Update status
    pub fn update_status(&mut self, status: TodoStatus) {
        self.status = status;
        self.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
}

// =============================================================================
// 项目需求类型
// Project Requirement Types
// =============================================================================

/// 项目需求文档（澄清后的需求）
/// Project requirement document (clarified requirement)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRequirement {
    /// 需求标题
    /// Requirement title
    pub title: String,
    /// 详细描述
    /// Detailed description
    pub description: String,
    /// 验收标准
    /// Acceptance criteria
    pub acceptance_criteria: Vec<String>,
    /// 子任务列表
    /// Subtask list
    pub subtasks: Vec<Subtask>,
    /// 依赖关系
    /// Dependencies
    pub dependencies: Vec<String>,
    /// 预估工作量（可选）
    /// Estimated effort (optional)
    pub estimated_effort: Option<String>,
    /// 相关资源
    /// Related resources
    pub resources: Vec<Resource>,
}

/// 子任务
/// Subtask
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtask {
    /// 子任务ID
    /// Subtask ID
    pub id: String,
    /// 子任务描述
    /// Subtask description
    pub description: String,
    /// 所需能力（用于匹配执行Agent）
    /// Required capabilities (for matching execution Agents)
    pub required_capabilities: Vec<String>,
    /// 执行顺序（可并行的任务可以有相同的顺序号）
    /// Execution order (parallel tasks can have the same order number)
    pub order: u32,
    /// 依赖的其他子任务ID
    /// IDs of other dependent subtasks
    pub depends_on: Vec<String>,
}

/// 相关资源
/// Related resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    /// 资源名称
    /// Resource name
    pub name: String,
    /// 资源类型（file/url/api等）
    /// Resource type (file/url/api etc.)
    pub resource_type: String,
    /// 资源路径或URL
    /// Resource path or URL
    pub path: String,
}

// =============================================================================
// 执行结果类型
// Execution Result Types
// =============================================================================

/// 执行结果
/// Execution result
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// 是否成功
    /// Whether successful
    pub success: bool,
    /// 结果摘要
    /// Result summary
    pub summary: String,
    /// 详细输出
    /// Detailed output
    pub details: HashMap<String, String>,
    /// 产出物列表
    /// List of artifacts
    pub artifacts: Vec<Artifact>,
    /// 执行时间（毫秒）
    /// Execution time (milliseconds)
    pub execution_time_ms: u64,
    /// 错误信息（如果失败）
    /// Error message (if failed)
    pub error: Option<String>,
}

/// 产出物
/// Artifact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// 产出物名称
    /// Artifact name
    pub name: String,
    /// 产出物类型
    /// Artifact type
    pub artifact_type: String,
    /// 产出物路径或内容
    /// Artifact path or content
    pub content: String,
}

// =============================================================================
// 决策类型
// Decision Types
// =============================================================================

/// 关键决策（需要推送给人类）
/// Critical decision (needs to be pushed to human)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticalDecision {
    /// 决策ID
    /// Decision ID
    pub id: String,
    /// 关联的Todo ID
    /// Associated Todo ID
    pub todo_id: String,
    /// 决策类型
    /// Decision type
    pub decision_type: DecisionType,
    /// 决策描述
    /// Decision description
    pub description: String,
    /// 可选方案
    /// Optional solutions
    pub options: Vec<DecisionOption>,
    /// 推荐方案（如果有）
    /// Recommended option (if any)
    pub recommended_option: Option<usize>,
    /// 截止时间（Unix时间戳，可选）
    /// Deadline (Unix timestamp, optional)
    pub deadline: Option<u64>,
    /// 创建时间
    /// Creation time
    pub created_at: u64,
    /// 人类响应
    /// Human response
    pub human_response: Option<HumanResponse>,
}

/// 决策类型
/// Decision type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DecisionType {
    /// 需求澄清
    /// Requirement clarification
    RequirementClarification,
    /// 技术选型
    /// Technical choice
    TechnicalChoice,
    /// 优先级调整
    /// Priority adjustment
    PriorityAdjustment,
    /// 异常处理
    /// Exception handling
    ExceptionHandling,
    /// 资源申请
    /// Resource request
    ResourceRequest,
    /// 其他
    /// Other
    Other(String),
}

/// 决策选项
/// Decision option
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionOption {
    /// 选项标签
    /// Option label
    pub label: String,
    /// 选项描述
    /// Option description
    pub description: String,
    /// 预期影响
    /// Expected impact
    pub impact: String,
}

/// 人类响应
/// Human response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanResponse {
    /// 选择的选项索引
    /// Selected option index
    pub selected_option: usize,
    /// 附加说明
    /// Additional comments
    pub comment: Option<String>,
    /// 响应时间
    /// Response time
    pub responded_at: u64,
}

// =============================================================================
// 汇报类型
// Report Types
// =============================================================================

/// 汇报消息
/// Report message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    /// 汇报ID
    /// Report ID
    pub id: String,
    /// 汇报类型
    /// Report type
    pub report_type: ReportType,
    /// 关联的Todo ID列表
    /// List of associated Todo IDs
    pub todo_ids: Vec<String>,
    /// 汇报内容
    /// Report content
    pub content: String,
    /// 附带的统计数据
    /// Attached statistical data
    pub statistics: HashMap<String, serde_json::Value>,
    /// 创建时间
    /// Creation time
    pub created_at: u64,
}

/// 汇报类型
/// Report type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReportType {
    /// 任务完成汇报
    /// Task completion report
    TaskCompletion,
    /// 进度汇报
    /// Progress report
    Progress,
    /// 异常汇报
    /// Exception report
    Exception,
    /// 每日总结
    /// Daily summary
    DailySummary,
}

// =============================================================================
// A2A 消息类型
// A2A Message Types
// =============================================================================

/// 秘书 Agent 与执行 Agent 之间的 A2A 消息
/// A2A message between Secretary Agent and Execution Agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecretaryMessage {
    /// 分配任务
    /// Assign task
    AssignTask {
        task_id: String,
        subtask: Subtask,
        context: HashMap<String, String>,
    },
    /// 任务状态查询
    /// Query task status
    QueryTaskStatus { task_id: String },
    /// 任务取消
    /// Cancel task
    CancelTask { task_id: String, reason: String },
    /// 任务状态报告（执行Agent发送）
    /// Task status report (sent by Execution Agent)
    TaskStatusReport {
        task_id: String,
        status: TaskExecutionStatus,
        progress: u32,
        message: Option<String>,
    },
    /// 任务完成报告（执行Agent发送）
    /// Task complete report (sent by Execution Agent)
    TaskCompleteReport {
        task_id: String,
        result: ExecutionResult,
    },
    /// 请求决策（执行Agent发送）
    /// Request decision (sent by Execution Agent)
    RequestDecision {
        task_id: String,
        decision: CriticalDecision,
    },
}

/// 任务执行状态
/// Task execution status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskExecutionStatus {
    /// 已接收
    /// Received
    Received,
    /// 准备中
    /// Preparing
    Preparing,
    /// 执行中
    /// Executing
    Executing,
    /// 等待外部响应
    /// Waiting for external response
    WaitingExternal,
    /// 已完成
    /// Completed
    Completed,
    /// 失败
    /// Failed
    Failed(String),
}

// =============================================================================
// 默认输入输出类型
// Default Input and Output Types
// =============================================================================

/// 默认用户输入
/// Default user input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DefaultInput {
    /// 新想法/需求
    /// New idea/requirement
    Idea {
        content: String,
        priority: Option<TodoPriority>,
        metadata: Option<HashMap<String, String>>,
    },
    /// 决策响应
    /// Decision response
    Decision {
        decision_id: String,
        selected_option: usize,
        comment: Option<String>,
    },
    /// 查询请求
    /// Query request
    Query(QueryType),
    /// 命令
    /// Command
    Command(SecretaryCommand),
}

/// 查询类型
/// Query type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryType {
    /// 获取 Todo 列表
    /// Get Todo list
    ListTodos { filter: Option<TodoStatus> },
    /// 获取 Todo 详情
    /// Get Todo details
    GetTodo { todo_id: String },
    /// 获取统计信息
    /// Get statistics
    Statistics,
    /// 获取待决策列表
    /// Get pending decisions
    PendingDecisions,
    /// 获取汇报历史
    /// Get report history
    Reports { report_type: Option<ReportType> },
}

/// 秘书命令
/// Secretary command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecretaryCommand {
    /// 开始澄清某个 Todo
    /// Start clarifying a specific Todo
    Clarify { todo_id: String },
    /// 分配任务
    /// Dispatch task
    Dispatch { todo_id: String },
    /// 取消任务
    /// Cancel task
    Cancel { todo_id: String, reason: String },
    /// 生成汇报
    /// Generate report
    GenerateReport { report_type: ReportType },
    /// 暂停秘书 Agent
    /// Pause Secretary Agent
    Pause,
    /// 恢复秘书 Agent
    /// Resume Secretary Agent
    Resume,
    /// 关闭秘书 Agent
    /// Shutdown Secretary Agent
    Shutdown,
}

/// 默认秘书输出
/// Default secretary output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DefaultOutput {
    /// 确认消息
    /// Acknowledgment message
    Acknowledgment { message: String },
    /// 需要决策
    /// Decision required
    DecisionRequired { decision: CriticalDecision },
    /// 汇报
    /// Report
    Report { report: Report },
    /// 状态更新
    /// Status update
    StatusUpdate { todo_id: String, status: TodoStatus },
    /// 任务完成通知
    /// Task completion notification
    TaskCompleted {
        todo_id: String,
        result: ExecutionResult,
    },
    /// 错误消息
    /// Error message
    Error { message: String },
    /// 通用消息（LLM生成）
    /// General message (generated by LLM)
    Message { content: String },
}

/// 秘书 Agent 工作阶段
/// Secretary Agent work phase
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkPhase {
    /// 阶段1: 接收想法
    /// Phase 1: Receiving idea
    ReceivingIdea,
    /// 阶段2: 澄清需求
    /// Phase 2: Clarifying requirement
    ClarifyingRequirement,
    /// 阶段3: 调度分配
    /// Phase 3: Dispatching task
    DispatchingTask,
    /// 阶段4: 监控反馈
    /// Phase 4: Monitoring feedback
    MonitoringExecution,
    /// 阶段5: 验收汇报
    ReportingCompletion,
}
