//! 自适应协作协议模块
//! Adaptive Collaboration Protocol Module
//!
//! 实现多Agent自适应协作的核心抽象，**所有协作决策由 LLM 驱动**。
//! Core abstractions for multi-agent adaptive collaboration, **all decisions driven by LLM**.
//!
//! # 核心理念
//! # Core Philosophy
//!
//! 在 Agent 框架中，所有操作都应该面向 LLM：
//! In the Agent framework, all operations should be LLM-oriented:
//! - **协作模式选择**：由 LLM 分析任务内容并决定使用哪种协作模式
//! - **Mode Selection**: LLM analyzes task content and decides which collaboration mode to use
//! - **消息处理**：协议通过 LLM 来理解和处理协作消息
//! - **Message Processing**: Protocols use LLM to understand and process collaboration messages
//! - **动态适应**：基于任务上下文和历史反馈，LLM 动态调整协作策略
//! - **Dynamic Adaptation**: LLM dynamically adjusts collaboration strategy based on context and feedback
//!
//! # 架构设计
//! # Architecture Design
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    LLM 驱动的协作决策                        │
//! │                    LLM-Driven Collaboration Decisions       │
//! │  ┌──────────────┐     ┌──────────────┐     ┌──────────────┐ │
//! │  │   任务分析    │───▶│  模式选择     │───▶│  协议执行     │ │
//! │  │ Task Analysis│───▶│ Mode Selection│───▶│Protocol Exec │ │
//! │  │   (LLM)      │     │   (LLM)      │     │  (LLM辅助)    │ │
//! │  │   (LLM)      │     │   (LLM)      │     │ (LLM-Assisted)│ │
//! │  └──────────────┘     └──────────────┘     └──────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # 快速开始
//! # Quick Start
//!
//! ```rust,ignore
//! use mofa_foundation::llm::{LLMClient, OpenAIProvider};
//! use mofa_kernel::collaboration::{
//!     CollaborationProtocol, CollaborationMode,
//!     LLMDrivenCollaborationManager, CollaborationMessage,
//! };
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> GlobalResult<()> {
//!     // 创建 LLM 客户端
//!     // Create LLM client
//!     let provider = Arc::new(OpenAIProvider::with_config(openai_config));
//!     let llm_client = Arc::new(LLMClient::new(provider));
//!
//!     // 创建 LLM 驱动的协作管理器
//!     // Create LLM-driven collaboration manager
//!     let manager = LLMDrivenCollaborationManager::new(
//!         "agent_001",
//!         llm_client.clone()
//!     );
//!
//!     // 注册协议
//!     // Register protocol
//!     manager.register_protocol(Arc::new(
//!         RequestResponseProtocol::with_llm("agent_001", llm_client.clone())
//!     )).await?;
//!
//!     // 执行任务（LLM 自动选择最合适的协议）
//!     // Execute task (LLM automatically selects the most suitable protocol)
//!     let result = manager.execute_task(
//!         "分析这个数据集并提供洞察",
//!         serde_json::json!({"dataset": "sales_2024.csv"})
//!     ).await?;
//!
//!     // LLM 的决策过程会被记录
//!     // LLM's decision process will be recorded
//!     println!("选择的模式: {:?}", result.mode);
//!     println!("决策理由: {:?}", result.decision_context);
//! }
//! ```

use async_trait::async_trait;
use mofa_kernel::agent::types::error::{GlobalError, GlobalResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// ============================================================================
// 核心类型定义
// Core Type Definitions
// ============================================================================

/// 协作模式
/// Collaboration Mode
///
/// 定义 Agent 之间的协作通信模式，供 LLM 选择使用
/// Defines collaboration communication modes between Agents, for LLM selection
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum CollaborationMode {
    /// 请求-响应模式：适合一对一的确定性任务
    /// Request-Response mode: suitable for one-to-one deterministic tasks
    /// 特点：同步等待、明确返回结果
    /// Features: synchronous waiting, explicit return results
    #[default]
    RequestResponse,
    /// 发布-订阅模式：适合一对多的发散性任务
    /// Publish-Subscribe mode: suitable for one-to-many divergent tasks
    /// 特点：异步广播、多个接收者
    /// Features: asynchronous broadcast, multiple receivers
    PublishSubscribe,
    /// 共识机制模式：适合需要达成一致的决策任务
    /// Consensus mode: suitable for decision tasks requiring agreement
    /// 特点：多轮协商、投票决策
    /// Features: multi-round negotiation, voting decisions
    Consensus,
    /// 辩论模式：适合需要迭代优化的审查任务
    /// Debate mode: suitable for review tasks requiring iterative refinement
    /// 特点：轮流发表、多轮改进
    /// Features: turn-based expression, multi-round improvement
    Debate,
    /// 并行模式：适合可分解的独立任务
    /// Parallel mode: suitable for decomposable independent tasks
    /// 特点：同时执行、结果聚合
    /// Features: simultaneous execution, result aggregation
    Parallel,
    /// 顺序模式：适合有依赖关系的任务链
    /// Sequential mode: suitable for task chains with dependencies
    /// 特点：串行执行、流水线处理
    /// Features: serial execution, pipeline processing
    Sequential,
    /// 自定义模式（由 LLM 解释）
    /// Custom mode (interpreted by LLM)
    Custom(String),
}

impl std::fmt::Display for CollaborationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CollaborationMode::RequestResponse => write!(f, "Request-Response Mode"),
            CollaborationMode::PublishSubscribe => write!(f, "Publish-Subscribe Mode"),
            CollaborationMode::Consensus => write!(f, "Consensus Mode"),
            CollaborationMode::Debate => write!(f, "Debate Mode"),
            CollaborationMode::Parallel => write!(f, "Parallel Processing Mode"),
            CollaborationMode::Sequential => write!(f, "Sequential Execution Mode"),
            CollaborationMode::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// 协作消息
/// Collaboration Message
///
/// Agent 之间协作时传递的消息格式，内容应该是 LLM 可理解的
/// Message format passed between Agents during collaboration, content should be LLM-understandable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationMessage {
    /// 消息唯一标识
    /// Unique message identifier
    pub id: String,
    /// 发送者 Agent ID
    /// Sender Agent ID
    pub sender: String,
    /// 接收者 Agent ID (None 表示广播)
    /// Receiver Agent ID (None means broadcast)
    pub receiver: Option<String>,
    /// 主题 (用于发布-订阅模式)
    /// Topic (used for publish-subscribe mode)
    pub topic: Option<String>,
    /// 消息内容 (LLM 可理解的自然语言或结构化数据)
    /// Message content (natural language or structured data understandable by LLM)
    pub content: CollaborationContent,
    /// 协作模式
    /// Collaboration mode
    pub mode: CollaborationMode,
    /// 时间戳
    /// Timestamp
    pub timestamp: u64,
    /// 元数据
    /// Metadata
    pub metadata: HashMap<String, String>,
}

/// 协作消息内容
/// Collaboration Message Content
///
/// 支持多种内容格式，方便 LLM 理解和处理
/// Supports multiple content formats for easy LLM understanding and processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CollaborationContent {
    /// 纯文本内容（自然语言）
    /// Plain text content (natural language)
    Text(String),
    /// 结构化数据（JSON）
    /// Structured data (JSON)
    Data(serde_json::Value),
    /// 混合内容（文本 + 数据）
    /// Mixed content (text + data)
    Mixed {
        text: String,
        data: serde_json::Value,
    },
    /// LLM 生成的响应
    /// LLM-generated response
    LLMResponse {
        reasoning: String, // LLM 的推理过程
        // LLM's reasoning process
        conclusion: String, // LLM 的结论
        // LLM's conclusion
        data: serde_json::Value, // 相关数据
                                 // Related data
    },
}

impl CollaborationContent {
    /// 获取文本表示（供 LLM 理解）
    /// Get text representation (for LLM understanding)
    pub fn to_text(&self) -> String {
        match self {
            CollaborationContent::Text(s) => s.clone(),
            CollaborationContent::Data(v) => v.to_string(),
            CollaborationContent::Mixed { text, data } => {
                format!("{text}\n\nData: {data}")
            }
            CollaborationContent::LLMResponse {
                reasoning,
                conclusion,
                ..
            } => {
                format!("Reasoning: {reasoning}\n\nConclusion: {conclusion}")
            }
        }
    }
}

impl From<String> for CollaborationContent {
    fn from(s: String) -> Self {
        CollaborationContent::Text(s)
    }
}

impl From<&str> for CollaborationContent {
    fn from(s: &str) -> Self {
        CollaborationContent::Text(s.to_string())
    }
}

impl From<serde_json::Value> for CollaborationContent {
    fn from(v: serde_json::Value) -> Self {
        CollaborationContent::Data(v)
    }
}

impl CollaborationMessage {
    /// 创建新的协作消息
    /// Create a new collaboration message
    pub fn new(
        sender: String,
        content: impl Into<CollaborationContent>,
        mode: CollaborationMode,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            id: Uuid::now_v7().to_string(),
            sender,
            receiver: None,
            topic: None,
            content: content.into(),
            mode,
            timestamp: now,
            metadata: HashMap::new(),
        }
    }

    /// 设置接收者
    /// Set receiver
    pub fn with_receiver(mut self, receiver: String) -> Self {
        self.receiver = Some(receiver);
        self
    }

    /// 设置主题
    /// Set topic
    pub fn with_topic(mut self, topic: String) -> Self {
        self.topic = Some(topic);
        self
    }

    /// 添加元数据
    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// 协作协议执行结果
/// Collaboration Protocol Execution Result
///
/// 包含执行结果和 LLM 的决策上下文
/// Contains execution result and LLM's decision context
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationResult {
    /// 是否成功
    /// Whether successful
    pub success: bool,
    /// 结果数据
    /// Result data
    pub data: Option<CollaborationContent>,
    /// 错误信息
    /// Error message
    pub error: Option<String>,
    /// 执行时间（毫秒）
    /// Execution time (milliseconds)
    pub duration_ms: u64,
    /// 参与的 Agent 列表
    /// List of participating Agents
    pub participants: Vec<String>,
    /// 使用的协作模式
    /// Collaboration mode used
    pub mode: CollaborationMode,
    /// LLM 决策上下文（如果适用）
    /// LLM decision context (if applicable)
    pub decision_context: Option<DecisionContext>,
}

/// LLM 决策上下文
/// LLM Decision Context
///
/// 记录 LLM 在协作过程中的决策信息
/// Records LLM's decision information during collaboration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionContext {
    /// LLM 选择该模式的原因
    /// Reason why LLM chose this mode
    pub reasoning: String,
    /// 任务分析
    /// Task analysis
    pub task_analysis: String,
    /// 备选方案（LLM 考虑过的其他模式）
    /// Alternatives (other modes LLM considered)
    pub alternatives: Vec<CollaborationMode>,
    /// 置信度 (0.0 - 1.0)
    /// Confidence level (0.0 - 1.0)
    pub confidence: f32,
}

impl CollaborationResult {
    /// 创建成功结果
    /// Create success result
    pub fn success(
        data: impl Into<CollaborationContent>,
        duration_ms: u64,
        mode: CollaborationMode,
    ) -> Self {
        Self {
            success: true,
            data: Some(data.into()),
            error: None,
            duration_ms,
            participants: Vec::new(),
            mode,
            decision_context: None,
        }
    }

    /// 创建带 LLM 决策的成功结果
    /// Create success result with LLM decision
    pub fn success_with_llm_decision(
        data: impl Into<CollaborationContent>,
        duration_ms: u64,
        mode: CollaborationMode,
        decision: DecisionContext,
    ) -> Self {
        Self {
            success: true,
            data: Some(data.into()),
            error: None,
            duration_ms,
            participants: Vec::new(),
            mode,
            decision_context: Some(decision),
        }
    }

    /// 创建失败结果
    /// Create failure result
    pub fn failure(error: String, mode: CollaborationMode) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error),
            duration_ms: 0,
            participants: Vec::new(),
            mode,
            decision_context: None,
        }
    }

    /// 添加参与者
    /// Add participant
    pub fn with_participant(mut self, agent_id: String) -> Self {
        self.participants.push(agent_id);
        self
    }
}

// ============================================================================
// 协作协议 Trait (核心抽象)
// Collaboration Protocol Trait (Core Abstraction)
// ============================================================================

/// 协作协议 Trait
/// Collaboration Protocol Trait
///
/// 定义了协作协议必须实现的核心接口，协议可以**选择性地使用 LLM**。
/// Defines core interfaces that collaboration protocols must implement, protocols can **optionally use LLM**.
#[async_trait]
pub trait CollaborationProtocol: Send + Sync {
    /// 获取协议名称
    /// Get protocol name
    fn name(&self) -> &str;

    /// 获取协议版本
    /// Get protocol version
    fn version(&self) -> &str {
        "1.0.0"
    }

    /// 获取协作模式
    /// Get collaboration mode
    fn mode(&self) -> CollaborationMode;

    /// 协议描述（供 LLM 理解）
    /// Protocol description (for LLM understanding)
    fn description(&self) -> &str {
        "Collaboration protocol"
    }

    /// 协议适用场景（供 LLM 参考选择）
    /// Applicable scenarios (for LLM reference when selecting)
    fn applicable_scenarios(&self) -> Vec<String> {
        vec![]
    }

    /// 发送消息
    /// Send message
    async fn send_message(&self, msg: CollaborationMessage) -> GlobalResult<()>;

    /// 接收消息
    /// Receive message
    async fn receive_message(&self) -> GlobalResult<Option<CollaborationMessage>>;

    /// 处理消息并返回结果
    /// Process message and return result
    ///
    /// 协议可以选择：
    /// Protocol can choose:
    /// 1. 直接处理（快速路径）
    /// 1. Direct processing (fast path)
    /// 2. 调用 LLM 辅助处理（智能路径）
    /// 2. LLM-assisted processing (intelligent path)
    async fn process_message(&self, msg: CollaborationMessage)
    -> GlobalResult<CollaborationResult>;

    /// 检查协议是否可用
    /// Check if protocol is available
    fn is_available(&self) -> bool {
        true
    }

    /// 获取协议统计信息
    /// Get protocol statistics
    fn stats(&self) -> HashMap<String, serde_json::Value> {
        HashMap::new()
    }
}

// ============================================================================
// 任务到协议的映射策略（作为 LLM 的参考基准）
// Task-to-Protocol Mapping Strategy (as reference baseline for LLM)
// ============================================================================

/// 默认场景映射（供 LLM 参考）
/// Default scenario mappings (for LLM reference)
///
/// 这不是硬编码的映射，而是提供给 LLM 的参考信息
/// This is not hardcoded mapping, but reference information provided to LLM
pub fn scenario_to_mode_suggestions() -> HashMap<String, Vec<CollaborationMode>> {
    HashMap::from([
        (
            "Data Processing".to_string(),
            vec![
                CollaborationMode::RequestResponse,
                CollaborationMode::Parallel,
            ],
        ),
        (
            "Creative Generation".to_string(),
            vec![
                CollaborationMode::PublishSubscribe,
                CollaborationMode::Debate,
            ],
        ),
        (
            "Decision Making".to_string(),
            vec![CollaborationMode::Consensus, CollaborationMode::Debate],
        ),
        (
            "Analysis Tasks".to_string(),
            vec![CollaborationMode::Parallel, CollaborationMode::Sequential],
        ),
        (
            "Review Tasks".to_string(),
            vec![CollaborationMode::Debate, CollaborationMode::Consensus],
        ),
        (
            "Search Tasks".to_string(),
            vec![
                CollaborationMode::Parallel,
                CollaborationMode::RequestResponse,
            ],
        ),
    ])
}

// ============================================================================
// 协作协议注册表
// Collaboration Protocol Registry
// ============================================================================

/// 协作协议注册表
/// Collaboration Protocol Registry
///
/// 管理所有已注册的协作协议，供 LLM 选择
/// Manages all registered collaboration protocols, for LLM selection
#[derive(Clone)]
pub struct ProtocolRegistry {
    protocols: Arc<RwLock<HashMap<String, Arc<dyn CollaborationProtocol>>>>,
}

impl Default for ProtocolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtocolRegistry {
    /// 创建新的协议注册表
    /// Create a new protocol registry
    pub fn new() -> Self {
        Self {
            protocols: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册协作协议
    /// Register collaboration protocol
    pub async fn register(&self, protocol: Arc<dyn CollaborationProtocol>) -> GlobalResult<()> {
        let name = protocol.name().to_string();
        let mut protocols = self.protocols.write().await;
        protocols.insert(name.clone(), protocol);
        tracing::debug!("Registered collaboration protocol: {}", name);
        Ok(())
    }

    /// 获取指定名称的协议
    /// Get protocol by name
    pub async fn get(&self, name: &str) -> Option<Arc<dyn CollaborationProtocol>> {
        let protocols = self.protocols.read().await;
        protocols.get(name).cloned()
    }

    /// 获取所有协议（供 LLM 选择）
    /// Get all protocols (for LLM selection)
    pub async fn list_all(&self) -> Vec<Arc<dyn CollaborationProtocol>> {
        let protocols = self.protocols.read().await;
        protocols.values().cloned().collect()
    }

    /// 列出所有协议名称
    /// List all protocol names
    pub async fn list_names(&self) -> Vec<String> {
        let protocols = self.protocols.read().await;
        protocols.keys().cloned().collect()
    }

    /// 获取协议数量
    /// Get protocol count
    pub async fn count(&self) -> usize {
        let protocols = self.protocols.read().await;
        protocols.len()
    }

    /// 获取协议描述（供 LLM 理解）
    /// Get protocol descriptions (for LLM understanding)
    pub async fn get_descriptions(&self) -> HashMap<String, ProtocolDescription> {
        let protocols = self.protocols.read().await;
        protocols
            .iter()
            .map(|(name, protocol)| {
                (
                    name.clone(),
                    ProtocolDescription {
                        name: name.clone(),
                        mode: protocol.mode(),
                        description: protocol.description().to_string(),
                        scenarios: protocol.applicable_scenarios(),
                    },
                )
            })
            .collect()
    }
}

/// 协议描述（供 LLM 理解）
/// Protocol Description (for LLM understanding)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolDescription {
    pub name: String,
    pub mode: CollaborationMode,
    pub description: String,
    pub scenarios: Vec<String>,
}

// ============================================================================
// 统计信息
// Statistics
// ============================================================================

/// 协作统计信息
/// Collaboration Statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CollaborationStats {
    /// 总任务数
    /// Total task count
    pub total_tasks: u64,
    /// 成功任务数
    /// Successful task count
    pub successful_tasks: u64,
    /// 失败任务数
    /// Failed task count
    pub failed_tasks: u64,
    /// 各模式使用次数
    /// Usage count per mode
    pub mode_usage: HashMap<String, u64>,
    /// 平均执行时间（毫秒）
    /// Average execution time (milliseconds)
    pub avg_duration_ms: f64,
    /// LLM 决策统计
    /// LLM decision statistics
    pub llm_decisions: LLMDecisionStats,
}

/// LLM 决策统计
/// LLM Decision Statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LLMDecisionStats {
    /// LLM 决策次数
    /// LLM decision count
    pub total_decisions: u64,
    /// 各模式被选择的次数
    /// Selection count per mode
    pub mode_selections: HashMap<String, u64>,
    /// 平均置信度
    /// Average confidence
    pub avg_confidence: f32,
}

// ============================================================================
// LLM 驱动的协作管理器
// LLM-Driven Collaboration Manager
// ============================================================================

/// LLM 驱动的自适应协作管理器
/// LLM-Driven Adaptive Collaboration Manager
///
/// **核心特性**：所有协作决策由 LLM 驱动
/// **Core Feature**: All collaboration decisions are driven by LLM
/// - LLM 分析任务内容
/// - LLM analyzes task content
/// - LLM 选择最合适的协作模式
/// - LLM selects the most suitable collaboration mode
/// - LLM 可以动态调整协作策略
/// - LLM can dynamically adjust collaboration strategy
pub struct LLMDrivenCollaborationManager {
    /// Agent ID
    /// Agent ID
    agent_id: String,
    /// 协议注册表
    /// Protocol registry
    registry: ProtocolRegistry,
    /// 当前使用的协议
    /// Currently used protocol
    current_protocol: Arc<RwLock<Option<Arc<dyn CollaborationProtocol>>>>,
    /// 消息队列
    /// Message queue
    message_queue: Arc<RwLock<Vec<CollaborationMessage>>>,
    /// 统计信息
    /// Statistics
    stats: Arc<RwLock<CollaborationStats>>,
}

impl LLMDrivenCollaborationManager {
    /// 创建新的 LLM 驱动协作管理器
    /// Create a new LLM-driven collaboration manager
    ///
    /// 注意：实际的 LLM 客户端由各个协议自行管理
    /// Note: Actual LLM client is managed by each protocol individually
    pub fn new(agent_id: impl Into<String>) -> Self {
        let agent_id = agent_id.into();
        Self {
            agent_id,
            registry: ProtocolRegistry::new(),
            current_protocol: Arc::new(RwLock::new(None)),
            message_queue: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(CollaborationStats::default())),
        }
    }

    /// 获取 Agent ID
    /// Get Agent ID
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// 获取协议注册表
    /// Get protocol registry
    pub fn registry(&self) -> &ProtocolRegistry {
        &self.registry
    }

    /// 注册协作协议
    /// Register collaboration protocol
    pub async fn register_protocol(
        &self,
        protocol: Arc<dyn CollaborationProtocol>,
    ) -> GlobalResult<()> {
        self.registry.register(protocol).await
    }

    /// 执行任务（使用指定的协议，不进行 LLM 决策）
    /// Execute task (using specified protocol, without LLM decision)
    ///
    /// 当你想明确指定使用某个协议时使用此方法
    /// Use this method when you want to explicitly specify which protocol to use
    pub async fn execute_task_with_protocol(
        &self,
        protocol_name: &str,
        content: impl Into<CollaborationContent>,
    ) -> GlobalResult<CollaborationResult> {
        let start = std::time::Instant::now();

        // 更新统计
        // Update statistics
        {
            let mut stats = self.stats.write().await;
            stats.total_tasks += 1;
        }

        // 获取协议
        // Get protocol
        let protocol =
            self.registry.get(protocol_name).await.ok_or_else(|| {
                GlobalError::Other(format!("Protocol not found: {}", protocol_name))
            })?;

        // 更新当前协议
        // Update current protocol
        {
            let mut current = self.current_protocol.write().await;
            *current = Some(protocol.clone());
        }

        // 创建协作消息
        // Create collaboration message
        let msg = CollaborationMessage::new(self.agent_id.clone(), content, protocol.mode());

        // 处理消息
        // Process message
        let result = protocol.process_message(msg).await;

        let duration = start.elapsed().as_millis() as u64;

        match result {
            Ok(mut result) => {
                result.duration_ms = duration;

                // 更新成功统计
                // Update success statistics
                {
                    let mut stats = self.stats.write().await;
                    stats.successful_tasks += 1;
                    let mode_key = protocol.mode().to_string();
                    *stats.mode_usage.entry(mode_key).or_insert(0) += 1;

                    // 更新平均执行时间
                    // Update average execution time
                    let total = stats.successful_tasks + stats.failed_tasks;
                    if total > 0 {
                        stats.avg_duration_ms = (stats.avg_duration_ms * (total - 1) as f64
                            + duration as f64)
                            / total as f64;
                    }
                }

                Ok(result)
            }
            Err(e) => {
                // 更新失败统计
                // Update failure statistics
                {
                    let mut stats = self.stats.write().await;
                    stats.failed_tasks += 1;
                }

                Ok(CollaborationResult::failure(e.to_string(), protocol.mode()))
            }
        }
    }

    /// 发送协作消息
    /// Send collaboration message
    pub async fn send_message(&self, msg: CollaborationMessage) -> GlobalResult<()> {
        let mut queue = self.message_queue.write().await;
        queue.push(msg);
        Ok(())
    }

    /// 接收协作消息
    /// Receive collaboration message
    pub async fn receive_message(&self) -> GlobalResult<Option<CollaborationMessage>> {
        let mut queue = self.message_queue.write().await;
        Ok(queue.pop())
    }

    /// 获取当前使用的协议
    /// Get currently used protocol
    pub async fn current_protocol(&self) -> Option<Arc<dyn CollaborationProtocol>> {
        let current = self.current_protocol.read().await;
        current.clone()
    }

    /// 获取统计信息
    /// Get statistics
    pub async fn stats(&self) -> CollaborationStats {
        self.stats.read().await.clone()
    }

    /// 重置统计信息
    /// Reset statistics
    pub async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        *stats = CollaborationStats::default();
    }

    /// 获取所有协议的描述（供外部 LLM 使用）
    /// Get descriptions of all protocols (for external LLM use)
    pub async fn get_protocol_descriptions(&self) -> HashMap<String, ProtocolDescription> {
        self.registry.get_descriptions().await
    }
}

// ============================================================================
// 测试
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collaboration_mode_display() {
        assert_eq!(
            CollaborationMode::RequestResponse.to_string(),
            "Request-Response Mode"
        );
        assert_eq!(
            CollaborationMode::PublishSubscribe.to_string(),
            "Publish-Subscribe Mode"
        );
        assert_eq!(CollaborationMode::Consensus.to_string(), "Consensus Mode");
    }

    #[test]
    fn test_collaboration_message_creation() {
        let msg = CollaborationMessage::new(
            "agent_001".to_string(),
            "Test content",
            CollaborationMode::RequestResponse,
        )
        .with_receiver("agent_002".to_string())
        .with_topic("test_topic".to_string());

        assert_eq!(msg.sender, "agent_001");
        assert_eq!(msg.receiver, Some("agent_002".to_string()));
        assert_eq!(msg.topic, Some("test_topic".to_string()));
    }

    #[test]
    fn test_collaboration_content() {
        let text_content: CollaborationContent = "Hello".to_string().into();
        assert_eq!(text_content.to_text(), "Hello");

        let json_content: CollaborationContent = serde_json::json!({"key": "value"}).into();
        assert!(json_content.to_text().contains("key"));
    }

    #[tokio::test]
    async fn test_protocol_registry() {
        let registry = ProtocolRegistry::new();
        assert_eq!(registry.count().await, 0);
        assert!(registry.list_names().await.is_empty());
    }

    #[tokio::test]
    async fn test_llm_driven_manager() {
        let manager = LLMDrivenCollaborationManager::new("test_agent");

        assert_eq!(manager.agent_id(), "test_agent");
        assert_eq!(manager.registry().count().await, 0);
    }

    #[tokio::test]
    async fn test_message_queue() {
        let manager = LLMDrivenCollaborationManager::new("test_agent");

        let msg = CollaborationMessage::new(
            "agent_001".to_string(),
            "Test content",
            CollaborationMode::RequestResponse,
        );

        manager.send_message(msg).await.unwrap();

        let received = manager.receive_message().await.unwrap();
        assert!(received.is_some());
        assert_eq!(received.unwrap().sender, "agent_001");
    }
}
