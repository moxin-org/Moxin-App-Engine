use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use mofa_foundation::llm::provider::LLMProvider as FoundationLLMProvider;
use mofa_foundation::llm::types::{
    ChatCompletionRequest as FoundationChatReq, ChatCompletionResponse as FoundationChatRes,
    ChatMessage as FoundationChatMessage, Choice as FoundationChoice,
    FinishReason as FoundationFinishReason,
};
use mofa_foundation::llm::{LLMAgent, LLMAgentConfig, LLMError};
use mofa_foundation::react::{ReActAgent, ReActTool};

use mofa_kernel::llm::capability::dispatch_chat;
use mofa_kernel::llm::provider::LLMProvider as KernelLLMProvider;
use mofa_kernel::llm::types::{
    ChatCompletionRequest as KernelChatReq, ChatCompletionResponse as KernelChatRes,
    ChatMessage as KernelChatMessage, Choice as KernelChoice, FinishReason as KernelFinishReason,
};
use mofa_kernel::plugin::manager::PluginManager;
use mofa_kernel::session::registry::SessionRegistry;

use std::fs::OpenOptions;
use std::io::Write;

fn log_debug(msg: &str) {
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug.log")
    {
        writeln!(file, "{}", msg).unwrap_or(());
    }
}

// ===================================================================
// Mock Providers (Kernel types)
// ===================================================================

/// Provider V1 answers with an Action: ask the tool to do something.
struct MockProviderV1;

#[async_trait]
impl KernelLLMProvider for MockProviderV1 {
    fn name(&self) -> &str {
        "mock-openai"
    }

    async fn chat(&self, request: KernelChatReq) -> mofa_kernel::agent::AgentResult<KernelChatRes> {
        let text = request
            .messages
            .last()
            .and_then(|m| m.text_content())
            .unwrap_or("");
        log_debug(&format!("V1 called with: {}", text));

        let msg = KernelChatMessage::assistant("Action: reload_tool[hello]");

        let res = KernelChatRes {
            choices: vec![KernelChoice {
                index: 0,
                message: msg,
                finish_reason: Some(KernelFinishReason::Stop),
                logprobs: None,
            }],
        };
        Ok(res)
    }
}

/// Provider V2 is what gets loaded after the reload. It checks the conversation
/// history to ensure it retains context, and then answers.
struct MockProviderV2;

#[async_trait]
impl KernelLLMProvider for MockProviderV2 {
    fn name(&self) -> &str {
        "mock-openai"
    }

    async fn chat(&self, request: KernelChatReq) -> mofa_kernel::agent::AgentResult<KernelChatRes> {
        let text = request
            .messages
            .last()
            .and_then(|m| m.text_content())
            .unwrap_or("");
        log_debug(&format!("V2 called with: {}", text));

        let has_context = request.messages.iter().any(|m| match m.text_content() {
            Some(content) => content.contains("Observation: Reloaded."),
            None => false,
        });

        let answer = if has_context {
            "Final Answer: Success with context"
        } else {
            "Final Answer: Failed no context"
        };
        log_debug(&format!("V2 responding with: {}", answer));

        let msg = KernelChatMessage::assistant(answer);
        let res = KernelChatRes {
            choices: vec![KernelChoice {
                index: 0,
                message: msg,
                finish_reason: Some(KernelFinishReason::Stop),
                logprobs: None,
            }],
        };
        Ok(res)
    }
}

// ===================================================================
// The Router Provider connecting ReActAgent --> PluginManager
// ===================================================================

struct PluginRouterLLM {
    manager: Arc<PluginManager>,
    registry: Arc<SessionRegistry>,
    session_id: String,
}

#[async_trait]
impl FoundationLLMProvider for PluginRouterLLM {
    fn name(&self) -> &str {
        "plugin-router"
    }

    async fn chat(&self, request: FoundationChatReq) -> Result<FoundationChatRes, LLMError> {
        let json_req = serde_json::to_string(&request).unwrap();
        let kernel_req: KernelChatReq = match serde_json::from_str(&json_req) {
            Ok(req) => req,
            Err(e) => panic!("Failed to parse request: {} (json: {})", e, json_req),
        };

        let kernel_res = dispatch_chat(&self.manager, &self.registry, &self.session_id, kernel_req)
            .await
            .map_err(|e| LLMError::ConfigError(e.to_string()))?;

        // FoundationChatRes requires extra fields
        let msg = FoundationChatMessage::assistant(
            kernel_res.choices[0].message.text_content().unwrap_or(""),
        );

        let foundation_res = FoundationChatRes {
            id: "fake-id".into(),
            object: "chat.completion".into(),
            created: 1234,
            model: "mock".into(),
            system_fingerprint: None,
            choices: vec![FoundationChoice {
                index: 0,
                message: msg,
                finish_reason: Some(FoundationFinishReason::Stop),
                logprobs: None,
            }],
            usage: None,
        };

        Ok(foundation_res)
    }
}

// ===================================================================
// The Tool that triggers a Hot Reload midway
// ===================================================================

struct HotReloadTool {
    manager: Arc<PluginManager>,
    registry: Arc<SessionRegistry>,
}

#[async_trait]
impl ReActTool for HotReloadTool {
    fn name(&self) -> &str {
        "reload_tool"
    }

    fn description(&self) -> &str {
        "Triggers a hot reload of the mock-openai plugin mid-task."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "input": {"type": "string"}
            }
        }))
    }

    async fn execute(&self, _input: &str) -> Result<String, String> {
        let provider_v2: Arc<dyn KernelLLMProvider> = Arc::new(MockProviderV2);

        self.manager
            .reload_plugin("mock-openai", provider_v2, &self.registry)
            .await
            .expect("Mid-task plugin reload failed!");

        Ok("Reloaded.".to_string())
    }
}

// ===================================================================
// Integration Test
// ===================================================================

#[tokio::test]
async fn test_react_agent_survives_runtime_plugin_reload() {
    let manager = Arc::new(PluginManager::new());
    let registry = Arc::new(SessionRegistry::new());

    let provider_v1: Arc<dyn KernelLLMProvider> = Arc::new(MockProviderV1);
    manager
        .register_plugin("mock-openai", Arc::clone(&provider_v1))
        .await;

    let session_id = "session-123".to_string();
    registry
        .create_session(
            session_id.clone(),
            "mock-openai".to_string(),
            Arc::clone(&provider_v1),
        )
        .await;

    let router_provider: Arc<dyn FoundationLLMProvider> = Arc::new(PluginRouterLLM {
        manager: Arc::clone(&manager),
        registry: Arc::clone(&registry),
        session_id: session_id.clone(),
    });

    let llm_config = LLMAgentConfig {
        agent_id: "test-agent".to_string(),
        name: "Test Agent".to_string(),
        system_prompt: None,
        ..Default::default()
    };
    let llm_agent = Arc::new(LLMAgent::new(llm_config, router_provider));

    let react_agent = ReActAgent::builder()
        .with_llm(llm_agent)
        .with_tool(Arc::new(HotReloadTool {
            manager: Arc::clone(&manager),
            registry: Arc::clone(&registry),
        }))
        .with_max_iterations(5)
        .with_verbose(true)
        .build_async()
        .await
        .expect("Failed to build ReActAgent");

    let result = react_agent
        .run("Start task")
        .await
        .expect("ReAct Agent run failed");

    assert!(
        result.success,
        "ReAct task should succeed. Error: {:?}, Iterations: {}, Answer: {}",
        result.error, result.iterations, result.answer
    );
    assert_eq!(
        result.answer, "Success with context",
        "Agent history was lost! V2 provider did not see the observation history across the reload."
    );
}
