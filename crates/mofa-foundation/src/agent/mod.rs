//! Agent 基础构建块
//! Agent foundational building blocks
//!
//! 包含 Agent 能力描述和组件 trait 定义
//! Contains Agent capability descriptions and component trait definitions

pub mod base;
pub mod builder;
pub mod components;
pub mod context;
pub mod event_sourcing;
pub mod executor;
pub mod session;
pub mod tools;

// ========================================================================
// 从 Kernel 层重导出核心类型
// Re-export core types from the Kernel layer
// ========================================================================

pub use mofa_kernel::agent::{AgentCapabilities, AgentRequirements, ReasoningStrategy};

// Re-export additional types needed by components
pub use mofa_kernel::agent::context::AgentContext;
pub use mofa_kernel::agent::error::{AgentError, AgentResult};
pub use mofa_kernel::agent::types::AgentInput;

// 重新导出组件 (从 components 模块统一导入)
// Re-export components (unified import from components module)
pub use components::{
    // Context compressor trait and implementations
    CompressionMetrics,
    CompressionResult,
    CompressionStrategy,
    ContextCompressor,
    CoordinationPattern,
    // Kernel traits 和类型 (通过 components 重导出)
    // Kernel traits and types (re-exported via components)
    CompletedStep,
    Coordinator,
    Decision,
    // Foundation 具体实现
    // Foundation concrete implementations
    DirectReasoner,
    DispatchResult,
    EchoTool,
    // Long-term memory implementations
    Embedder,
    Episode,
    EpisodicMemory,
    ExecutionPlan,
    FileBasedStorage,
    HashEmbedder,
    HierarchicalCompressor,
    HybridCompressor,
    InMemoryStorage,
    LLMTool,
    Memory,
    MemoryItem,
    MemoryItemSnapshot,
    MemoryStats,
    MemoryValue,
    Message,
    MessageRole,
    ParallelCoordinator,
    // Planning trait + types (kernel) and implementations (foundation)
    PlannedStep,
    Planner,
    PlanningExecutionResult,
    PlanningExecutor,
    PlanningPhase,
    PlanningRequest,
    PlanningStepRecord,
    PromptPlanner,
    Reasoner,
    ReasoningResult,
    SemanticCompressor,
    SemanticMemory,
    SequentialCoordinator,
    // SimpleTool 便捷接口
    // SimpleTool convenient interfaces
    SimpleTool,
    SimpleToolAdapter,
    SimpleToolRegistry,
    SlidingWindowCompressor,
    StepEvaluation,
    StepExecutionRecord,
    StepExecutionStatus,
    SummarizingCompressor,
    Task,
    ThoughtStep,
    TokenCounter,
    Tool,
    // Foundation 扩展类型
    // Foundation extension types
    ToolCategory,
    ToolDescriptor,
    ToolExt,
    ToolInput,
    ToolMetadata,
    ToolRegistry,
    ToolResult,
    as_tool,
};

// Tool adapters and registries (Foundation implementations)
pub use tools::{
    BuiltinTools, ClosureTool, DateTimeTool, FileReadTool, FileWriteTool, FunctionTool, HttpTool,
    JsonParseTool, ShellTool, ToolSearcher,
};

// Re-export context module
pub use context::{
    AgentIdentity, ContextExt, PromptContext, PromptContextBuilder, RichAgentContext,
};

// Re-export business types from rich context
pub use context::rich::{ComponentOutput, ExecutionMetrics};

// Re-export session module
pub use session::{
    JsonlSessionStorage, MemorySessionStorage, Session, SessionManager, SessionMessage,
    SessionStorage,
};

// Re-export executor module
pub use executor::{AgentExecutor, AgentExecutorConfig};

// Re-export builder module
pub use builder::{AgentBuilder, AgentProfile, AgentRegistry};

// Re-export event sourcing module
pub use event_sourcing::*;

// Re-export LLM types from kernel
pub use mofa_kernel::agent::types::{
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, LLMProvider, TokenUsage, ToolCall,
    ToolDefinition,
};

// Re-export BaseAgent from base module
pub use base::BaseAgent;

// Note: Secretary abstract traits are in mofa_kernel::agent::secretary
// Foundation layer provides concrete implementations
// Use mofa_kernel::agent::secretary for traits, or mofa_foundation::secretary for implementations

/// Prelude 模块
/// Prelude module
pub mod prelude {
    pub use super::{AgentCapabilities, AgentRequirements, ReasoningStrategy};
}
