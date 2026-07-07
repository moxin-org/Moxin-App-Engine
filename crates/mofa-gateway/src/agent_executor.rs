//! Agent execution bridge for the Gateway.
//!
//! [`AgentExecutor`] translates an OpenAI-compatible [`ChatCompletionRequest`]
//! into a kernel [`AgentInput`], invokes the registered [`MoFAAgent`] via
//! [`MoFAAgent::execute()`], and maps the result back to a
//! [`ChatCompletionResponse`].
//!
//! # Why a separate module?
//!
//! The [`InvocationRouter`][crate::handlers::InvocationRouter] (introduced in
//! the previous PR) resolved *where* to dispatch a request but could only
//! return agent **metadata** for the `Agent` variant.  `AgentExecutor` closes
//! that gap by implementing the actual execution protocol, making the Gateway
//! a first-class agent runtime rather than a simple proxy.
//!
//! # Execution flow
//!
//! ```text
//! ChatCompletionRequest
//!        │
//!        ▼  AgentExecutor::run()
//!   concat messages → AgentInput::Text
//!        │
//!        ▼  AgentRegistry::get(agent_id)
//!   Arc<RwLock<dyn MoFAAgent>>
//!        │
//!        ▼  MoFAAgent::execute(input, ctx)
//!   AgentOutput  ──── error? ──→ GatewayError::AgentOperationFailed
//!        │
//!        ▼  map_output()
//!   ChatCompletionResponse (OpenAI-compatible)
//! ```

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use mofa_kernel::agent::{
    context::AgentContext,
    types::{AgentInput, OutputContent},
};
use mofa_runtime::agent::registry::AgentRegistry;

use crate::{
    error::GatewayError,
    inference_bridge::{
        ChatCompletionRequest, ChatCompletionResponse, Choice, Message, Usage,
    },
};

/// Executes a registered [`MoFAAgent`] and converts its output to an
/// OpenAI-compatible [`ChatCompletionResponse`].
///
/// # Thread safety
///
/// [`AgentExecutor`] holds only an `Arc<AgentRegistry>` and is therefore
/// `Send + Sync`.  The actual agent lock (`Arc<RwLock<dyn MoFAAgent>>`) is
/// acquired per-request and immediately released after execution completes.
pub struct AgentExecutor {
    registry: Arc<AgentRegistry>,
}

impl AgentExecutor {
    /// Create a new executor backed by `registry`.
    pub fn new(registry: Arc<AgentRegistry>) -> Self {
        Self { registry }
    }

    /// Execute `agent_id` with the provided [`ChatCompletionRequest`].
    ///
    /// # Conversion rules
    ///
    /// | OpenAI field | Kernel type |
    /// |:---|:---|
    /// | `messages[].content` (concatenated with newlines) | `AgentInput::Text` |
    /// | `AgentOutput::content` (text) | `choices[0].message.content` |
    /// | `token_usage` from `AgentOutput` | `usage.prompt_tokens` / `completion_tokens` |
    ///
    /// System messages are prepended verbatim so that agents that inspect the
    /// raw input string can still honour them.
    ///
    /// # Errors
    ///
    /// * [`GatewayError::AgentNotFound`] — agent absent from registry.
    /// * [`GatewayError::AgentOperationFailed`] — `MoFAAgent::execute()` returned an error.
    pub async fn run(
        &self,
        agent_id: &str,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, GatewayError> {
        let started = Instant::now();

        // ── 1. Resolve agent from registry ───────────────────────────────────
        let agent_arc = self.registry.get(agent_id).await.ok_or_else(|| {
            tracing::warn!(
                target = "mofa_gateway::executor",
                agent_id = agent_id,
                "AgentExecutor: agent not found in registry"
            );
            GatewayError::AgentNotFound(agent_id.to_string())
        })?;

        // ── 2. Convert OpenAI messages → AgentInput ───────────────────────────
        //
        // Strategy: concatenate all message contents separated by newlines.
        // System messages are labelled so agents that inspect the raw string
        // can branch on them.
        let input_text = request
            .messages
            .iter()
            .map(|m| {
                let prefix = match m.role.as_str() {
                    "system" => "[SYSTEM] ",
                    "assistant" => "[ASSISTANT] ",
                    _ => "",
                };
                format!("{}{}", prefix, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let input = AgentInput::Text(input_text.clone());

        // ── 3. Build a per-request AgentContext ───────────────────────────────
        let exec_id = format!("gw-{}", uuid::Uuid::new_v4());
        let ctx: AgentContext = AgentContext::new(exec_id);

        tracing::info!(
            target = "mofa_gateway::executor",
            agent_id = agent_id,
            input_len = input_text.len(),
            "executing agent"
        );

        // ── 4. Execute (write-lock the agent for the duration) ────────────────
        let output = {
            let mut agent = agent_arc.write().await;
            agent.execute(input, &ctx).await.map_err(|e| {
                tracing::error!(
                    target = "mofa_gateway::executor",
                    agent_id = agent_id,
                    error = %e,
                    "agent execution failed"
                );
                GatewayError::AgentOperationFailed(e.to_string())
            })?
        };

        let elapsed_ms = started.elapsed().as_millis() as u64;

        tracing::debug!(
            target = "mofa_gateway::executor",
            agent_id = agent_id,
            elapsed_ms = elapsed_ms,
            "agent execution complete"
        );

        // ── 5. Map AgentOutput → ChatCompletionResponse ───────────────────────
        let content = output.to_text();

        // Token usage: use the agent-reported values when present, otherwise
        // estimate via the standard ~4 chars/token heuristic.
        let (prompt_tokens, completion_tokens) = output
            .token_usage
            .as_ref()
            .map(|u| (u.prompt_tokens, u.completion_tokens))
            .unwrap_or_else(|| {
                let pt = (input_text.len() as f32 / 4.0) as u32;
                let ct = (content.len() as f32 / 4.0) as u32;
                (pt, ct)
            });

        // Map OutputContent variants to a meaningful finish_reason.
        let finish_reason = match &output.content {
            OutputContent::Error(_) => "content_filter",
            OutputContent::Stream => "length",
            _ => "stop",
        }
        .to_string();

        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Ok(ChatCompletionResponse {
            id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            object_type: "chat.completion".to_string(),
            created: now_secs,
            model: request.model,
            choices: vec![Choice {
                index: 0,
                message: Message {
                    role: "assistant".to_string(),
                    content,
                },
                finish_reason,
            }],
            usage: Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use mofa_kernel::agent::{
        capabilities::AgentCapabilities,
        context::AgentContext,
        core::MoFAAgent,
        error::AgentResult,
        types::{AgentInput, AgentOutput, AgentState},
    };
    use mofa_runtime::agent::registry::AgentRegistry;
    use tokio::sync::RwLock;

    use crate::inference_bridge::Message;

    // ── Test agent ────────────────────────────────────────────────────────────

    struct EchoAgent {
        id: String,
        name: String,
        capabilities: AgentCapabilities,
        state: AgentState,
    }

    impl EchoAgent {
        fn new(id: &str) -> Self {
            Self {
                id: id.to_string(),
                name: id.to_string(),
                capabilities: AgentCapabilities::default(),
                state: AgentState::Created,
            }
        }
    }

    #[async_trait]
    impl MoFAAgent for EchoAgent {
        fn id(&self) -> &str { &self.id }
        fn name(&self) -> &str { &self.name }
        fn capabilities(&self) -> &AgentCapabilities { &self.capabilities }
        fn state(&self) -> AgentState { self.state.clone() }
        async fn initialize(&mut self, _ctx: &AgentContext) -> AgentResult<()> {
            self.state = AgentState::Ready;
            Ok(())
        }
        async fn execute(
            &mut self,
            input: AgentInput,
            _ctx: &AgentContext,
        ) -> AgentResult<AgentOutput> {
            // Echo the input back with a prefix so tests can assert on content.
            let echo = format!("ECHO: {}", input.to_text());
            Ok(AgentOutput::text(echo))
        }
        async fn shutdown(&mut self) -> AgentResult<()> {
            self.state = AgentState::Shutdown;
            Ok(())
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    async fn make_executor_with_agent(agent_id: &str) -> AgentExecutor {
        let registry = Arc::new(AgentRegistry::new());
        let agent = Arc::new(RwLock::new(EchoAgent::new(agent_id)));
        registry.register(agent).await.expect("register should succeed");
        AgentExecutor::new(registry)
    }

    fn make_request(model: &str, user_msg: &str) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: model.to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: user_msg.to_string(),
            }],
            max_tokens: Some(128),
            temperature: Some(0.7),
            stream: Some(false),
        }
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn run_echo_agent_returns_valid_response() {
        let executor = make_executor_with_agent("echo-bot").await;
        let resp = executor
            .run("echo-bot", make_request("echo-bot", "Hello, agent!"))
            .await
            .unwrap();

        assert!(resp.id.starts_with("chatcmpl-"));
        assert_eq!(resp.object_type, "chat.completion");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.role, "assistant");
        assert!(resp.choices[0].message.content.contains("Hello, agent!"),
            "content should echo the input: {}", resp.choices[0].message.content);
        assert_eq!(resp.choices[0].finish_reason, "stop");
    }

    #[tokio::test]
    async fn run_with_system_message_prepends_prefix() {
        let executor = make_executor_with_agent("labeller").await;
        let req = ChatCompletionRequest {
            model: "labeller".to_string(),
            messages: vec![
                Message { role: "system".to_string(), content: "You are helpful".to_string() },
                Message { role: "user".to_string(), content: "What are you?".to_string() },
            ],
            max_tokens: Some(64),
            temperature: Some(0.0),
            stream: Some(false),
        };
        let resp = executor.run("labeller", req).await.unwrap();
        // System message should appear in the echo with [SYSTEM] prefix
        assert!(resp.choices[0].message.content.contains("[SYSTEM]"));
        assert!(resp.choices[0].message.content.contains("You are helpful"));
    }

    #[tokio::test]
    async fn run_unknown_agent_returns_not_found_error() {
        let executor = AgentExecutor::new(Arc::new(AgentRegistry::new()));
        let err = executor
            .run("nonexistent-bot", make_request("nonexistent-bot", "hi"))
            .await
            .unwrap_err();
        assert!(matches!(err, GatewayError::AgentNotFound(_)));
    }

    #[tokio::test]
    async fn token_usage_is_populated_in_response() {
        let executor = make_executor_with_agent("token-tracker").await;
        let resp = executor
            .run("token-tracker", make_request("token-tracker", "Count my tokens please"))
            .await
            .unwrap();
        assert!(resp.usage.prompt_tokens > 0);
        assert!(resp.usage.completion_tokens > 0);
        assert_eq!(resp.usage.total_tokens, resp.usage.prompt_tokens + resp.usage.completion_tokens);
    }

    #[tokio::test]
    async fn model_field_is_preserved_in_response() {
        let executor = make_executor_with_agent("my-agent").await;
        let resp = executor
            .run("my-agent", make_request("my-agent", "hello"))
            .await
            .unwrap();
        assert_eq!(resp.model, "my-agent");
    }
}
