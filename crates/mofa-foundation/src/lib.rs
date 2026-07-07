#![allow(
    dead_code,
    unused_imports,
    non_camel_case_types,
    ambiguous_glob_reexports
)]
// orchestrator module - Model Lifecycle & Allocation
pub mod orchestrator;

// hardware discovery module
pub mod hardware;

// adapter registry module - Runtime model adapter discovery
pub mod adapter;

// inference orchestration module - Unified Inference Routing & Lifecycle
pub mod inference;

// prompt module
pub mod prompt;

// react module
pub mod react;

// messaging module
pub mod messaging;

// persistence module
pub mod persistence;

// llm module
pub mod llm;

// workflow module
pub mod workflow;

// coordination module
pub mod coordination;

// scheduler module - periodic agent execution & memory-budgeted scheduling
pub mod scheduler;

// config module
pub mod config;

// secretary module - 秘书Agent模式
pub mod secretary;

// collaboration module - 自适应协作协议
pub mod agent;
pub mod collaboration;

// RAG module - vector store and document chunking
pub mod rag;

// Security module - security governance (RBAC, PII, moderation, prompt guard)
// HITL module - Human-in-the-Loop system
pub mod hitl;
// cost module - concrete pricing registry and budget enforcer implementations
pub mod cost;

// swarm module - Multi-agent swarm orchestration
pub mod swarm;
// Structured output: JSON schema validator and agent executor
pub mod agent_executor;
pub mod schema_validator;
pub use agent_executor::{AgentExecutor, ExecutorError};
pub use schema_validator::{SchemaError, SchemaValidator};

// middleware module
pub mod middleware;

// Security governance - PII redaction, content moderation, prompt guard
pub mod security;

// Agent capability manifest and discovery registry
pub mod capability_registry;
pub use capability_registry::CapabilityRegistry;
// Error recovery strategies (Backoff, RetryPolicy, CircuitBreaker, retry, fallback_chain)
pub mod recovery;

// Metrics and telemetry module
pub mod metrics;

// Re-export metrics types
pub use metrics::{
    AgentMetrics, BusinessMetrics, CircuitBreakerEvent, CircuitBreakerMetrics, CircuitBreakerState,
    LatencyPercentiles, MetricBuilder, MetricsBackend, MetricsCollector, ModelPoolEvent,
    ModelPoolMetrics, RetryMetrics, RoutingMetrics, SchedulerMetrics, StepStatus, StepTiming,
    TokenUsage, ToolMetrics, WorkflowMetrics,
};

// Gateway implementations (rate limiter, routing strategies)
pub mod gateway;
pub use gateway::TokenBucketRateLimiter;

// Re-export config types
pub use config::{AgentInfo, AgentYamlConfig, LLMYamlConfig, RuntimeConfig, ToolConfig};

// Re-export messaging types
pub use messaging::{
    InboundMessage, MessageBus, OutboundMessage, SimpleInboundMessage, SimpleOutboundMessage,
};

// Re-export prompt types
pub use prompt::{
    ConversationBuilder, GlobalPromptRegistry, PromptBuilder, PromptComposition, PromptError,
    PromptRegistry, PromptResult, PromptTemplate, PromptVariable, VariableType,
};

// Re-export orchestrator types (GSoC 2026 Edge Model Orchestrator)
pub use orchestrator::{
    DegradationLevel, ModelOrchestrator, ModelProvider, ModelProviderConfig, ModelType,
    OrchestratorError, OrchestratorResult, PoolStatistics,
};

pub mod speech_registry;
pub mod voice_pipeline;

pub use speech_registry::SpeechAdapterRegistry;
pub use voice_pipeline::{VoicePipeline, VoicePipelineConfig, VoicePipelineResult};

// Re-export Linux implementation and pipeline when available
#[cfg(all(target_os = "linux", feature = "linux-candle"))]
pub use orchestrator::{
    InferencePipeline, LinuxCandleProvider, ModelPool, PipelineBuilder, PipelineOutput,
    PipelineStage,
};

// Re-export secretary types for convenience
pub use secretary::{
    Artifact,
    ChannelConnection,
    ChatMessage,
    // Connection
    CriticalDecision,
    DecisionOption,
    // LLM
    DecisionType,
    DefaultInput,
    DefaultOutput,
    DefaultSecretaryBehavior,
    // Default implementation
    DefaultSecretaryBuilder,
    ExecutionResult,
    HumanResponse,
    // LLM integration
    LLMProvider,
    ProjectRequirement,
    QueryType,
    // Command types
    Report,
    ReportType,
    Resource,
    // Task types
    SecretaryBehavior,
    SecretaryCommand,
    SecretaryContext,
    SecretaryCore,
    SecretaryEvent,
    SecretaryHandle,
    SecretaryMessage,
    Subtask,
    TaskExecutionStatus,
    TodoItem,
    TodoPriority,
    TodoStatus,
    UserConnection,
    WorkPhase,
    // Core types
    extract_json_block,
    parse_llm_json,
};

// Re-export scheduler types for convenience
pub use scheduler::CronScheduler;
