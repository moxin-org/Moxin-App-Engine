//! LLM 模块
//! LLM Module
//!
//! 提供 LLM (Large Language Model) 集成支持
//! Provides Large Language Model integration support
//!
//! # 架构
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                           LLM 模块架构                               │
//! │                       LLM Module Architecture                       │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                                                                     │
//! │  ┌─────────────┐    ┌─────────────┐    ┌─────────────────────────┐  │
//! │  │  LLMClient  │───▶│  Provider   │───▶│        具体实现         │  │
//! │  │  (高级API)  │    │   (trait)   │    │  Concrete Implementation│  │
//! │  │ (High-level)│    │             │    │  - OpenAI               │  │
//! │  └─────────────┘    └─────────────┘    │  - Anthropic            │  │
//! │         │                              │  - Ollama               │  │
//! │         ▼                              │  - 自定义...             │  │
//! │  ┌─────────────┐                       │  - Custom...            │  │
//! │  │ ChatSession │                       └─────────────────────────┘  │
//! │  │ (会话管理)  │                                                    │
//! │  │ (Session Mgmt)                                                   │
//! │  └─────────────┘                                                    │
//! │         │                                                           │
//! │         ▼                                                           │
//! │  ┌─────────────┐    ┌─────────────┐                                 │
//! │  │  LLMPlugin  │───▶│ AgentPlugin │  ← 集成到 MoFA Agent            │
//! │  │  (插件封装)  │    │   (trait)   │  ← Integrate to MoFA Agent      │
//! │  │ (Plugin Wrap)                                                    │
//! │  └─────────────┘                                                    │
//! │         │                                                           │
//! │         ▼                                                           │
//! │  ┌─────────────────────────────────────────────────────────────┐    │
//! │  │                         高级 API                            │    │
//! │  │                       Advanced API                          │    │
//! │  ├─────────────────────────────────────────────────────────────┤    │
//! │  │  AgentWorkflow  │  多 Agent 工作流编排                       │    │
//! │  │                 │  Multi-Agent Workflow Orchestration       │    │
//! │  │  AgentTeam      │  团队协作模式 (链式/并行/辩论/监督)          │    │
//! │  │                 │  Team Collaboration (Chain/Para/Debate)   │    │
//! │  │  Pipeline       │  函数式流水线 API                          │    │
//! │  │                 │  Functional Pipeline API                  │    │
//! │  └─────────────────────────────────────────────────────────────┘    │
//! │                                                                     │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # 快速开始
//! # Quick Start
//!
//! ## 1. 实现自定义 LLM Provider
//! ## 1. Implement Custom LLM Provider
//!
//! ```rust,ignore
//! use mofa_foundation::llm::{LLMProvider, ChatCompletionRequest, ChatCompletionResponse, LLMResult};
//!
//! struct MyLLMProvider {
//!     api_key: String,
//! }
//!
//! #[async_trait::async_trait]
//! impl LLMProvider for MyLLMProvider {
//!     fn name(&self) -> &str {
//!         "my-llm"
//!     }
//!
//!     fn default_model(&self) -> &str {
//!         "my-model-v1"
//!     }
//!
//!     async fn chat(&self, request: ChatCompletionRequest) -> LLMResult<ChatCompletionResponse> {
//!         // 实现具体的 API 调用逻辑
//!         // Implement specific API calling logic
//!         todo!()
//!     }
//! }
//! ```
//!
//! ## 2. 使用 LLMClient 进行对话
//! ## 2. Using LLMClient for Dialogue
//!
//! ```rust,ignore
//! use mofa_foundation::llm::{LLMClient, ChatMessage};
//! use std::sync::Arc;
//!
//! let provider = Arc::new(MyLLMProvider::new("api-key"));
//! let client = LLMClient::new(provider);
//!
//! // 简单问答
//! // Simple Q&A
//! let answer = client.ask("What is Rust?").await?;
//!
//! // 带系统提示的对话
//! // Dialogue with system prompt
//! let response = client
//!     .chat()
//!     .system("You are a helpful coding assistant.")
//!     .user("How do I read a file in Rust?")
//!     .temperature(0.7)
//!     .max_tokens(1000)
//!     .send()
//!     .await?;
//!
//! info!("{}", response.content().unwrap());
//! ```
//!
//! ## 3. 使用工具调用
//! ## 3. Using Tool Calling
//!
//! ```rust,ignore
//! use mofa_foundation::llm::{LLMClient, Tool, ToolExecutor};
//! use serde_json::json;
//!
//! // 定义工具
//! // Define tool
//! let weather_tool = Tool::function(
//!     "get_weather",
//!     "Get weather for a location",
//!     json!({
//!         "type": "object",
//!         "properties": {
//!             "location": { "type": "string" }
//!         },
//!         "required": ["location"]
//!     })
//! );
//!
//! // 实现工具执行器
//! // Implement tool executor
//! struct MyToolExecutor;
//!
//! #[async_trait::async_trait]
//! impl ToolExecutor for MyToolExecutor {
//!     async fn execute(&self, name: &str, arguments: &str) -> LLMResult<String> {
//!         match name {
//!             "get_weather" => Ok(r#"{"temp": 22, "condition": "sunny"}"#.to_string()),
//!             _ => Err(LLMError::Other("Unknown tool".to_string()))
//!         }
//!     }
//!
//!     async fn available_tools(&self) -> LLMResult<Vec<Tool>> {
//!         Ok(vec![weather_tool.clone()])
//!     }
//! }
//!
//! // 使用自动工具调用
//! // Use automatic tool calling
//! let response = client
//!     .chat()
//!     .system("You can use tools to help answer questions.")
//!     .user("What's the weather in Tokyo?")
//!     .tool(weather_tool)
//!     .with_tool_executor(Arc::new(MyToolExecutor))
//!     .send_with_tools()
//!     .await?;
//! ```
//!
//! ## 4. 作为插件集成到 Agent
//! ## 4. Integrating as a Plugin into Agent
//!
//! ```rust,ignore
//! use mofa_foundation::llm::{LLMPlugin, LLMConfig};
//! use mofa_sdk::kernel::MoFAAgent;
//! use mofa_sdk::runtime::AgentBuilder;
//!
//! // 创建 LLM 插件
//! // Create LLM plugin
//! let llm_plugin = LLMPlugin::new("openai-llm", provider);
//!
//! // 添加到 Agent
//! // Add to Agent
//! let runtime = AgentBuilder::new("my-agent", "My Agent")
//!     .with_plugin(Box::new(llm_plugin))
//!     .with_agent(agent)
//!     .await?;
//! ```
//!
//! ## 5. 使用会话管理
//! ## 5. Using Session Management
//!
//! ```rust,ignore
//! use mofa_foundation::llm::{LLMClient, ChatSession};
//!
//! let client = LLMClient::new(provider);
//! let mut session = ChatSession::new(client)
//!     .with_system("You are a helpful assistant.");
//!
//! // 多轮对话
//! // Multi-turn conversation
//! let r1 = session.send("Hello!").await?;
//! let r2 = session.send("What did I just say?").await?;  // 会记住上下文
//!                                                       // Context will be remembered
//!
//! // 清空历史
//! // Clear history
//! session.clear();
//! ```
//!
//! # 高级 API
//! # Advanced API
//!
//! ## 6. Agent 工作流编排 (AgentWorkflow)
//! ## 6. Agent Workflow Orchestration (AgentWorkflow)
//!
//! 创建复杂的多 Agent 工作流，支持条件分支、并行执行、聚合等。
//! Create complex multi-agent workflows, supporting branches, parallel execution, etc.
//!
//! ```rust,ignore
//! use mofa_foundation::llm::{AgentWorkflow, LLMAgent};
//! use std::sync::Arc;
//!
//! // 创建简单的 Agent 链
//! // Create simple agent chain
//! let workflow = agent_chain("content-pipeline", vec![
//!     ("researcher", researcher_agent.clone()),
//!     ("writer", writer_agent.clone()),
//!     ("editor", editor_agent.clone()),
//! ]);
//!
//! let result = workflow.run("Write an article about Rust").await?;
//!
//! // 使用构建器创建更复杂的工作流
//! // Create more complex workflow using builder
//! let workflow = AgentWorkflow::new("complex-pipeline")
//!     .add_agent("analyzer", analyzer_agent)
//!     .add_agent("writer", writer_agent)
//!     .add_llm_router("router", router_agent, vec!["technical", "creative"])
//!     .connect("start", "analyzer")
//!     .connect("analyzer", "router")
//!     .connect_on("router", "technical", "technical")
//!     .connect_on("router", "creative", "creative")
//!     .build();
//! ```
//!
//! ## 7. Agent 团队协作 (AgentTeam)
//! ## 7. Agent Team Collaboration (AgentTeam)
//!
//! 支持多种协作模式：链式、并行、辩论、监督、MapReduce。
//! Supports collaboration patterns: Chain, Parallel, Debate, Supervisor, MapReduce.
//!
//! ```rust,ignore
//! use mofa_foundation::llm::{AgentTeam, TeamPattern, AgentRole};
//!
//! // 使用预定义的团队模式
//! // Use predefined team patterns
//! let team = content_creation_team(researcher, writer, editor);
//! let article = team.run("Write about AI safety").await?;
//!
//! // 自定义团队
//! // Custom team
//! let team = AgentTeam::new("analysis-team")
//!     .add_member("expert1", expert1_agent)
//!     .add_member("expert2", expert2_agent)
//!     .add_member("synthesizer", synthesizer_agent)
//!     .with_pattern(TeamPattern::MapReduce)
//!     .with_aggregate_prompt("Synthesize: {results}")
//!     .build();
//!
//! // 辩论模式
//! // Debate mode
//! let debate = debate_team(agent1, agent2, 3);  // 3 轮辩论
//!                                              // 3 rounds of debate
//! let conclusion = debate.run("Is Rust better than Go?").await?;
//! ```
//!
//! ## 8. 函数式流水线 (Pipeline)
//! ## 8. Functional Pipeline (Pipeline)
//!
//! 提供简洁的函数式 API 构建 Agent 处理流程。
//! Provides clean functional API to build Agent processing flows.
//!
//! ```rust,ignore
//! use mofa_foundation::llm::Pipeline;
//!
//! // 简单流水线
//! // Simple pipeline
//! let result = Pipeline::new()
//!     .with_agent(translator)
//!     .map(|s| s.to_uppercase())
//!     .with_agent(summarizer)
//!     .run("Translate and summarize this text")
//!     .await?;
//!
//! // 带模板的流水线
//! // Pipeline with template
//! let result = Pipeline::new()
//!     .with_agent_template(agent, "Please analyze: {input}")
//!     .map(|s| format!("Analysis: {}", s))
//!     .run("Some data to analyze")
//!     .await?;
//!
//! // 流式流水线
//! // Streaming pipeline
//! let stream = StreamPipeline::new(agent)
//!     .with_template("Tell me about {input}")
//!     .run_stream("Rust programming")
//!     .await?;
//! ```

pub mod agent;
pub mod client;
pub mod fallback;
pub mod plugin;
pub mod provider;
pub mod retry;
pub mod tool_executor;
pub mod tool_schema;
pub mod types;

// 高级 API
// Advanced API
pub mod agent_workflow;
pub mod anthropic;
pub mod google;
pub mod multi_agent;
pub mod ollama;
pub mod openai;
pub mod pipeline;

// Framework components
pub mod agent_loop;
pub mod context;
pub mod stream_adapter;
pub mod stream_bridge;
pub mod task_orchestrator;
pub mod token_budget;
pub mod vision;
// Audio processing
pub mod transcription;
//CANDLE FEATURE
#[cfg(feature = "linux-candle")]
pub mod candle;

// Re-export 核心类型
// Re-export core types
pub use client::{ChatRequestBuilder, ChatSession, LLMClient, function_tool};
pub use fallback::{
    CircuitBreakerConfig, FallbackChain, FallbackChainBuilder, FallbackChainConfig,
    FallbackCondition, FallbackConditionConfig, FallbackProviderConfig, FallbackSnapshot,
    FallbackTrigger, FallbackTriggerConfig, ProviderSnapshot,
};
pub use plugin::{LLMCapability, LLMPlugin, MockLLMProvider};
pub use provider::{
    ChatStream, LLMConfig, LLMProvider, LLMRegistry, ModelCapabilities, ModelInfo, global_registry,
};
pub use retry::RetryExecutor;
pub use stream_adapter::{GenericStreamAdapter, StreamAdapter, adapter_for_provider};
pub use stream_bridge::{stream_error_to_llm_error, token_stream_to_events, token_stream_to_text};
pub use tool_executor::ToolExecutor;
pub use tool_schema::{normalize_schema, parse_schema, validate_schema};
pub use types::*;

// Re-export 标准 LLM Agent
// Re-export standard LLM Agent
pub use agent::{
    LLMAgent, LLMAgentBuilder, LLMAgentConfig, LLMAgentEventHandler, StreamEvent, TextStream,
    simple_llm_agent,
};

// Re-export agent_from_config (when openai feature is enabled)
pub use agent::agent_from_config;

// Re-export OpenAI Provider (when enabled)
pub use openai::{OpenAIConfig, OpenAIProvider};
// Re-export Anthropic Provider
pub use anthropic::{AnthropicConfig, AnthropicProvider};
// Re-export Google Gemini Provider
pub use google::{GeminiConfig, GeminiProvider};
// Re-export Ollama Provider
pub use ollama::{OllamaConfig, OllamaProvider};
// Re-export Candle Provider (when enabled)
#[cfg(feature = "linux-candle")]
// pub use candle::{
//     CandleConfig, CandleProvider, CandleProviderEmbeddingPooling as EmbeddingPooling,
//     ToolCallFormat, candle_from_path, candle_from_path_gpu,
// };
// Add after `pub mod transcription;`:
pub use candle::{CandleConfig, CandleProvider, candle_from_path, candle_from_path_gpu};

// Re-export 高级 API
// Re-export Advanced API
pub use agent_workflow::{
    AgentEdge, AgentNode, AgentNodeType, AgentValue, AgentWorkflow, AgentWorkflowBuilder,
    AgentWorkflowContext, agent_chain, agent_parallel, agent_router,
};
pub use multi_agent::{
    AgentMember, AgentRole, AgentTeam, AgentTeamBuilder, TeamPattern, analysis_team,
    code_review_team, content_creation_team, debate_team,
};
pub use pipeline::{
    Pipeline, StreamPipeline, agent_pipe, agent_pipe_with_templates, ask_with_template, batch_ask,
    quick_ask,
};

// Re-export framework components
pub use agent_loop::{AgentLoop, AgentLoopConfig, AgentLoopRunner, SimpleToolExecutor};
pub use context::{AgentContextBuilder, AgentIdentity, NoOpSkillsManager, SkillsManager};
pub use task_orchestrator::{
    BackgroundTask, TaskOrchestrator, TaskOrchestratorConfig, TaskOrigin, TaskResult, TaskStatus,
};
pub use token_budget::{
    CharBasedEstimator, ContextWindowManager, ContextWindowPolicy, TokenBudgetConfig,
    TokenEstimator, TrimResult,
};
pub use vision::{
    ImageDetailExt, build_vision_chat_message, build_vision_chat_message_single,
    build_vision_message, encode_image_data_url, encode_image_url, get_mime_type,
    image_url_from_string, image_url_with_detail, is_image_file,
};
// ImageDetail is already re-exported via types::*;

// Compatibility re-export for older AgentLoopToolExecutor name
#[deprecated(
    note = "Use llm::ToolExecutor instead. AgentLoop now uses the unified ToolExecutor."
)]
pub use tool_executor::ToolExecutor as AgentLoopToolExecutor;

// Re-export transcription module
pub use transcription::{
    GroqTranscriptionProvider, OpenAITranscriptionProvider, TranscriptionProvider,
};

#[cfg(test)]
mod tests {
    use super::{
        AnthropicConfig, AnthropicProvider, GeminiConfig, GeminiProvider, LLMProvider,
        OpenAIConfig, OpenAIProvider,
    };

    #[test]
    fn openai_provider_uses_configured_model() {
        let cfg = OpenAIConfig::new("k")
            .with_base_url("https://example.test/v1")
            .with_model("gpt-4o-mini");
        let provider = OpenAIProvider::with_config(cfg);
        assert_eq!(provider.name(), "openai");
        assert_eq!(provider.default_model(), "gpt-4o-mini");
    }

    #[test]
    fn anthropic_provider_uses_configured_model() {
        let cfg = AnthropicConfig::new("k")
            .with_base_url("https://example.test")
            .with_model("claude-3-5-sonnet-latest");
        let provider = AnthropicProvider::with_config(cfg);
        assert_eq!(provider.name(), "anthropic");
        assert_eq!(provider.default_model(), "claude-3-5-sonnet-latest");
    }

    #[test]
    fn gemini_provider_uses_configured_model() {
        let cfg = GeminiConfig::new("k")
            .with_base_url("https://example.test")
            .with_model("gemini-1.5-flash");
        let provider = GeminiProvider::with_config(cfg);
        assert_eq!(provider.name(), "gemini");
        assert_eq!(provider.default_model(), "gemini-1.5-flash");
    }
}

