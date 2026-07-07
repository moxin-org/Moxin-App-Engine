#![allow(ambiguous_glob_reexports)]

// context module
pub mod context;

// plugin module
pub mod plugin;
pub use plugin::*;

// bus module
pub mod bus;
pub use bus::{
    MessageBus, MessageBusError, MessageBusResult,
    MessageEnvelope,
    DeliveryGuarantee, DeliveryReceipt, NackAction, ReceiveOptions, ReceivedMessage, SubscribeOptions,
    MessageBusCounters, MessageBusMetrics, MessageBusObserver, SharedCounters, new_shared_counters,
    AgentBus, CommunicationMode,
};

// utils module
pub mod utils;

// logging module
pub mod logging;

// error module
pub mod error;
pub use error::{IntoKernelReport, KernelError, KernelResult};

// core module
pub mod core;
pub use core::*;

// message module
pub mod message;

// MessageGraph module
pub mod message_graph;
pub use message_graph::*;

// Agent Framework (统一 Agent 框架)
pub mod agent;
pub use agent::{AgentManifest, AgentManifestBuilder};

// Global Configuration System (全局配置系统)
#[cfg(feature = "config")]
pub mod config;
#[cfg(feature = "config")]
pub use config::*;

// Storage traits (存储接口)
pub mod storage;
pub use storage::{ObjectMetadata, ObjectStore, Storage};

// RAG traits (向量存储接口)
pub mod rag;
pub use rag::{
    Document, DocumentChunk, GenerateInput, Generator, RagPipeline, RagPipelineOutput, Reranker,
    Retriever, ScoredDocument, SearchResult, SimilarityMetric, VectorStore,
};

// Workflow traits (工作流接口)
pub mod workflow;
// Explicit re-exports instead of `pub use workflow::*` to avoid ambiguous
// `policy` module collision with `hitl::policy`. Fixes #1217.
pub use workflow::{
    CircuitBreakerState, CircuitState, Command, CompiledGraph, ControlFlow, DebugEvent,
    DebugSession, END, EdgeTarget, GraphConfig, GraphState, JsonState, NodeFunc, NodePolicy,
    Reducer, ReducerType, RemainingSteps, RetryCondition, RuntimeContext, START, SendCommand,
    SessionRecorder, StateGraph, StateSchema, StateUpdate, StepResult, StreamEvent,
    TelemetryEmitter,
};
pub mod llm;
// Metrics traits for monitoring integration
pub mod metrics;
pub use metrics::*;

// Human-in-the-Loop (HITL) module
pub mod hitl;
// Explicit re-exports instead of `pub use hitl::*` to avoid ambiguous
// `policy` module collision with `workflow::policy`. Fixes #1217.
pub use hitl::{
    AlwaysReviewPolicy, AuditLogQuery, Change, Diff, ExecutionStep, ExecutionTrace, HitlError,
    HitlResult, NeverReviewPolicy, PerformanceData, ReviewAuditEvent, ReviewAuditEventType,
    ReviewContext, ReviewMetadata, ReviewPolicy, ReviewRequest, ReviewRequestId, ReviewResponse,
    ReviewStatus, ReviewType, StoreError, TelemetrySnapshot,
};
// Provider pricing registry (LLM cost calculation)
pub mod pricing;

// Budget configuration & enforcement
pub mod budget;

// Structured output parsing with JSON schema validation
pub mod structured_output;
pub use structured_output::StructuredOutput;
// Security governance (PII redaction, content moderation, prompt guard)
pub mod security;

// Gateway routing abstractions (kernel-level traits for agent request dispatch)
pub mod gateway;
pub use gateway::{
    AgentResponse, ApiKeyStore, AuthClaims, AuthError, AuthProvider, GatewayConfigError,
    GatewayContext, GatewayRateLimiter, GatewayRequest, GatewayResponse, GatewayRoute, HttpMethod,
    KeyStrategy, RateLimitDecision, RateLimiterConfig, RegistryError, RequestEnvelope, RouteMatch,
    RouteRegistry, RoutingContext,
};

// Scheduler kernel contract (traits, types, errors for periodic agent execution)
pub mod scheduler;
pub use scheduler::{
    AgentScheduler, Clock, MissedTickPolicy, ScheduleDefinition, ScheduleHandle, ScheduleInfo,
    ScheduledAgentRunner, SchedulerError,
};

// Speech kernel contracts (traits and types for TTS/ASR)
pub mod speech;
pub use speech::{AsrAdapter, TtsAdapter};
