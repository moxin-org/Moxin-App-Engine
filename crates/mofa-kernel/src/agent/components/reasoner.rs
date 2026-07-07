//! 推理组件
//! Reasoning component
//!
//! 定义 Agent 的推理/思考能力
//! Defines the reasoning/thinking capabilities of the Agent

use crate::agent::capabilities::ReasoningStrategy;
use crate::agent::context::AgentContext;
use crate::agent::error::AgentResult;
use crate::agent::types::AgentInput;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// 推理器 Trait
/// Reasoner Trait
///
/// 负责 Agent 的推理/思考过程
/// Responsible for the Agent's reasoning/thinking process
///
/// # 示例
/// # Example
///
/// ```rust,ignore
/// use mofa_kernel::agent::components::reasoner::{Reasoner, ReasoningResult};
/// use mofa_foundation::agent::components::reasoner::DirectReasoner;
///
/// // 使用 foundation 层提供的具体实现
/// // Use concrete implementations from the foundation layer
/// let reasoner = DirectReasoner;
/// // 或者实现自定义 Reasoner
/// // Or implement a custom Reasoner
/// struct MyReasoner;
///
/// #[async_trait]
/// impl Reasoner for MyReasoner {
///     async fn reason(&self, input: &AgentInput, ctx: &CoreAgentContext) -> AgentResult<ReasoningResult> {
///         Ok(ReasoningResult {
///             thoughts: vec![],
///             decision: Decision::Respond { content: input.to_text() },
///             confidence: 1.0,
///         })
///     }
///
///     fn strategy(&self) -> ReasoningStrategy {
///         ReasoningStrategy::Direct
///     }
/// }
/// ```
#[async_trait]
pub trait Reasoner: Send + Sync {
    /// 执行推理过程
    /// Execute the reasoning process
    async fn reason(&self, input: &AgentInput, ctx: &AgentContext) -> AgentResult<ReasoningResult>;

    /// 获取推理策略
    /// Get the reasoning strategy
    fn strategy(&self) -> ReasoningStrategy;

    /// 是否支持多步推理
    /// Whether multi-step reasoning is supported
    fn supports_multi_step(&self) -> bool {
        matches!(
            self.strategy(),
            ReasoningStrategy::ReAct { .. }
                | ReasoningStrategy::ChainOfThought
                | ReasoningStrategy::TreeOfThought { .. }
        )
    }

    /// 推理器名称
    /// Reasoner name
    fn name(&self) -> &str {
        "reasoner"
    }

    /// 推理器描述
    /// Reasoner description
    fn description(&self) -> Option<&str> {
        None
    }
}

/// 推理结果
/// Reasoning result
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningResult {
    /// 思考步骤
    /// Thought steps
    pub thoughts: Vec<ThoughtStep>,
    /// 最终决策
    /// Final decision
    pub decision: Decision,
    /// 置信度 (0.0 - 1.0)
    /// Confidence (0.0 - 1.0)
    pub confidence: f32,
}

impl ReasoningResult {
    /// 创建简单的响应结果
    /// Create a simple response result
    pub fn respond(content: impl Into<String>) -> Self {
        Self {
            thoughts: vec![],
            decision: Decision::Respond {
                content: content.into(),
            },
            confidence: 1.0,
        }
    }

    /// 创建工具调用结果
    /// Create a tool call result
    pub fn call_tool(tool_name: impl Into<String>, arguments: serde_json::Value) -> Self {
        Self {
            thoughts: vec![],
            decision: Decision::CallTool {
                tool_name: tool_name.into(),
                arguments,
            },
            confidence: 1.0,
        }
    }

    /// 添加思考步骤
    /// Add a thought step
    pub fn with_thought(mut self, step: ThoughtStep) -> Self {
        self.thoughts.push(step);
        self
    }

    /// 设置置信度
    /// Set confidence
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }
}

/// 思考步骤
/// Thought step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThoughtStep {
    /// 步骤类型
    /// Step type
    pub step_type: ThoughtStepType,
    /// 步骤内容
    /// Step content
    pub content: String,
    /// 步骤序号
    /// Step number
    pub step_number: usize,
    /// 时间戳 (毫秒)
    /// Timestamp (milliseconds)
    pub timestamp_ms: u64,
}

impl ThoughtStep {
    /// 创建新的思考步骤
    /// Create a new thought step
    pub fn new(step_type: ThoughtStepType, content: impl Into<String>, step_number: usize) -> Self {
        let now = crate::utils::now_ms();

        Self {
            step_type,
            content: content.into(),
            step_number,
            timestamp_ms: now,
        }
    }

    /// 创建思考步骤
    /// Create thinking step
    pub fn thought(content: impl Into<String>, step_number: usize) -> Self {
        Self::new(ThoughtStepType::Thought, content, step_number)
    }

    /// 创建分析步骤
    /// Create analysis step
    pub fn analysis(content: impl Into<String>, step_number: usize) -> Self {
        Self::new(ThoughtStepType::Analysis, content, step_number)
    }

    /// 创建规划步骤
    /// Create planning step
    pub fn planning(content: impl Into<String>, step_number: usize) -> Self {
        Self::new(ThoughtStepType::Planning, content, step_number)
    }

    /// 创建反思步骤
    /// Create reflection step
    pub fn reflection(content: impl Into<String>, step_number: usize) -> Self {
        Self::new(ThoughtStepType::Reflection, content, step_number)
    }
}

/// 思考步骤类型
/// Thought step type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ThoughtStepType {
    /// 思考
    /// Thought
    Thought,
    /// 分析
    /// Analysis
    Analysis,
    /// 规划
    /// Planning
    Planning,
    /// 反思
    /// Reflection
    Reflection,
    /// 评估
    /// Evaluation
    Evaluation,
    /// 自定义
    /// Custom
    Custom(String),
}

/// 决策类型
/// Decision type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Decision {
    /// 直接响应
    /// Direct response
    Respond {
        /// 响应内容
        /// Response content
        content: String,
    },
    /// 调用工具
    /// Call tool
    CallTool {
        /// 工具名称
        /// Tool name
        tool_name: String,
        /// 工具参数
        /// Tool arguments
        arguments: serde_json::Value,
    },
    /// 调用多个工具 (并行)
    /// Call multiple tools (parallel)
    CallMultipleTools {
        /// 工具调用列表
        /// Tool call list
        tool_calls: Vec<ToolCall>,
    },
    /// 委托给其他 Agent
    /// Delegate to other Agent
    Delegate {
        /// 目标 Agent ID
        /// Target Agent ID
        agent_id: String,
        /// 委托任务
        /// Delegated task
        task: String,
    },
    /// 需要更多信息
    /// Need more information
    NeedMoreInfo {
        /// 需要的信息
        /// Required information
        questions: Vec<String>,
    },
    /// 无法处理
    /// Cannot handle
    CannotHandle {
        /// 原因
        /// Reason
        reason: String,
    },
}

/// 工具调用
/// Tool call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// 工具名称
    /// Tool name
    pub tool_name: String,
    /// 工具参数
    /// Tool arguments
    pub arguments: serde_json::Value,
}

impl ToolCall {
    /// 创建新的工具调用
    /// Create a new tool call
    pub fn new(tool_name: impl Into<String>, arguments: serde_json::Value) -> Self {
        Self {
            tool_name: tool_name.into(),
            arguments,
        }
    }
}

// Note: Concrete Reasoner implementations (like DirectReasoner) are provided
// in the foundation layer (mofa-foundation::agent::components::reasoner)

#[cfg(test)]
mod tests {
    use super::*;

    // ── ReasoningResult::respond ──────────────────────────────────────────

    #[test]
    fn reasoning_result_respond() {
        let r = ReasoningResult::respond("hello world");
        assert!(r.thoughts.is_empty());
        assert!((r.confidence - 1.0).abs() < f32::EPSILON);
        match &r.decision {
            Decision::Respond { content } => assert_eq!(content, "hello world"),
            _ => panic!("expected Decision::Respond"),
        }
    }

    // ── ReasoningResult::call_tool ────────────────────────────────────────

    #[test]
    fn reasoning_result_call_tool() {
        let args = serde_json::json!({"query": "weather"});
        let r = ReasoningResult::call_tool("search", args.clone());
        assert!(r.thoughts.is_empty());
        assert!((r.confidence - 1.0).abs() < f32::EPSILON);
        match &r.decision {
            Decision::CallTool {
                tool_name,
                arguments,
            } => {
                assert_eq!(tool_name, "search");
                assert_eq!(arguments, &args);
            }
            _ => panic!("expected Decision::CallTool"),
        }
    }

    // ── ReasoningResult::with_confidence clamping ─────────────────────────

    #[test]
    fn confidence_clamps_above_one() {
        let r = ReasoningResult::respond("x").with_confidence(5.0);
        assert!((r.confidence - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn confidence_clamps_below_zero() {
        let r = ReasoningResult::respond("x").with_confidence(-2.0);
        assert!((r.confidence - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn confidence_normal_value() {
        let r = ReasoningResult::respond("x").with_confidence(0.42);
        assert!((r.confidence - 0.42).abs() < f32::EPSILON);
    }

    // ── ReasoningResult::with_thought ─────────────────────────────────────

    #[test]
    fn with_thought_appends() {
        let r = ReasoningResult::respond("answer")
            .with_thought(ThoughtStep::thought("first idea", 1))
            .with_thought(ThoughtStep::analysis("deeper look", 2));
        assert_eq!(r.thoughts.len(), 2);
        assert_eq!(r.thoughts[0].step_number, 1);
        assert_eq!(r.thoughts[0].step_type, ThoughtStepType::Thought);
        assert_eq!(r.thoughts[1].step_type, ThoughtStepType::Analysis);
    }

    // ── ThoughtStep factory methods ───────────────────────────────────────

    #[test]
    fn thought_step_thought() {
        let s = ThoughtStep::thought("thinking...", 1);
        assert_eq!(s.step_type, ThoughtStepType::Thought);
        assert_eq!(s.content, "thinking...");
        assert_eq!(s.step_number, 1);
        assert!(s.timestamp_ms > 0);
    }

    #[test]
    fn thought_step_analysis() {
        let s = ThoughtStep::analysis("analyzing...", 2);
        assert_eq!(s.step_type, ThoughtStepType::Analysis);
        assert_eq!(s.step_number, 2);
    }

    #[test]
    fn thought_step_planning() {
        let s = ThoughtStep::planning("planning...", 3);
        assert_eq!(s.step_type, ThoughtStepType::Planning);
        assert_eq!(s.step_number, 3);
    }

    #[test]
    fn thought_step_reflection() {
        let s = ThoughtStep::reflection("reflecting...", 4);
        assert_eq!(s.step_type, ThoughtStepType::Reflection);
        assert_eq!(s.step_number, 4);
    }

    #[test]
    fn thought_step_custom_type() {
        let s = ThoughtStep::new(ThoughtStepType::Custom("brainstorm".into()), "ideas", 5);
        assert_eq!(s.step_type, ThoughtStepType::Custom("brainstorm".into()));
        assert_eq!(s.content, "ideas");
    }

    // ── ToolCall ──────────────────────────────────────────────────────────

    #[test]
    fn tool_call_new() {
        let tc = ToolCall::new("calculator", serde_json::json!({"expr": "2+2"}));
        assert_eq!(tc.tool_name, "calculator");
        assert_eq!(tc.arguments["expr"], "2+2");
    }

    // ── Decision variants ─────────────────────────────────────────────────

    #[test]
    fn decision_delegate() {
        let d = Decision::Delegate {
            agent_id: "agent-2".into(),
            task: "summarize".into(),
        };
        match d {
            Decision::Delegate { agent_id, task } => {
                assert_eq!(agent_id, "agent-2");
                assert_eq!(task, "summarize");
            }
            _ => panic!("expected Delegate"),
        }
    }

    #[test]
    fn decision_need_more_info() {
        let d = Decision::NeedMoreInfo {
            questions: vec!["what format?".into(), "how long?".into()],
        };
        match d {
            Decision::NeedMoreInfo { questions } => assert_eq!(questions.len(), 2),
            _ => panic!("expected NeedMoreInfo"),
        }
    }

    #[test]
    fn decision_cannot_handle() {
        let d = Decision::CannotHandle {
            reason: "out of scope".into(),
        };
        match d {
            Decision::CannotHandle { reason } => assert_eq!(reason, "out of scope"),
            _ => panic!("expected CannotHandle"),
        }
    }

    #[test]
    fn decision_call_multiple_tools() {
        let d = Decision::CallMultipleTools {
            tool_calls: vec![
                ToolCall::new("tool_a", serde_json::json!({})),
                ToolCall::new("tool_b", serde_json::json!({"k": "v"})),
            ],
        };
        match d {
            Decision::CallMultipleTools { tool_calls } => {
                assert_eq!(tool_calls.len(), 2);
                assert_eq!(tool_calls[0].tool_name, "tool_a");
                assert_eq!(tool_calls[1].tool_name, "tool_b");
            }
            _ => panic!("expected CallMultipleTools"),
        }
    }

    // ── Serialization ─────────────────────────────────────────────────────

    #[test]
    fn reasoning_result_serde_roundtrip() {
        let r = ReasoningResult::respond("test answer")
            .with_confidence(0.9)
            .with_thought(ThoughtStep::thought("step 1", 1));
        let json = serde_json::to_string(&r).unwrap();
        let recovered: ReasoningResult = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.thoughts.len(), 1);
        assert!((recovered.confidence - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn thought_step_type_serde_roundtrip() {
        for step_type in [
            ThoughtStepType::Thought,
            ThoughtStepType::Analysis,
            ThoughtStepType::Planning,
            ThoughtStepType::Reflection,
            ThoughtStepType::Evaluation,
            ThoughtStepType::Custom("test".into()),
        ] {
            let json = serde_json::to_string(&step_type).unwrap();
            let recovered: ThoughtStepType = serde_json::from_str(&json).unwrap();
            assert_eq!(recovered, step_type);
        }
    }
}
