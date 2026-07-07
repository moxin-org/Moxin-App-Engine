//! Mock implementation of [`LLMProvider`] for deterministic agent testing.

use async_trait::async_trait;
use mofa_kernel::agent::{AgentError, AgentResult};
use mofa_kernel::llm::provider::LLMProvider;
use mofa_kernel::llm::types::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

pub struct MockLLMProvider {
    rules: Arc<RwLock<Vec<(String, String)>>>,
    tool_call_rules: Arc<RwLock<Vec<(String, Vec<ToolCall>)>>>,
    sequences: Arc<RwLock<Vec<(String, VecDeque<String>)>>>,
    failure_queue: Arc<RwLock<VecDeque<AgentError>>>,
    history: Arc<RwLock<Vec<ChatCompletionRequest>>>,
    call_count: Arc<AtomicUsize>,
    fallback: String,
    usage: Usage,
}

impl Default for MockLLMProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MockLLMProvider {
    pub fn new() -> Self {
        Self {
            rules: Arc::new(RwLock::new(Vec::new())),
            tool_call_rules: Arc::new(RwLock::new(Vec::new())),
            sequences: Arc::new(RwLock::new(Vec::new())),
            failure_queue: Arc::new(RwLock::new(VecDeque::new())),
            history: Arc::new(RwLock::new(Vec::new())),
            call_count: Arc::new(AtomicUsize::new(0)),
            fallback: "Mock response.".into(),
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
            },
        }
    }

    pub fn add_response(&self, prompt_contains: &str, response: &str) {
        self.rules
            .write()
            .expect("lock poisoned")
            .push((prompt_contains.to_string(), response.to_string()));
    }

    pub fn add_tool_call_response(&self, prompt_contains: &str, tool_calls: Vec<ToolCall>) {
        self.tool_call_rules
            .write()
            .expect("lock poisoned")
            .push((prompt_contains.to_string(), tool_calls));
    }

    pub fn add_response_sequence(&self, prompt_contains: &str, responses: Vec<&str>) {
        let deque: VecDeque<String> = responses.into_iter().map(String::from).collect();
        self.sequences
            .write()
            .expect("lock poisoned")
            .push((prompt_contains.to_string(), deque));
    }

    pub fn fail_next(&self, error: AgentError) {
        self.failure_queue
            .write()
            .expect("lock poisoned")
            .push_back(error);
    }

    pub fn set_fallback(&mut self, response: &str) {
        self.fallback = response.to_string();
    }

    pub fn set_usage(&mut self, prompt_tokens: u32, completion_tokens: u32) {
        self.usage = Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        };
    }

    pub fn request_history(&self) -> Vec<ChatCompletionRequest> {
        self.history.read().expect("lock poisoned").clone()
    }

    pub fn last_request(&self) -> Option<ChatCompletionRequest> {
        self.history.read().expect("lock poisoned").last().cloned()
    }

    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::Relaxed)
    }

    pub fn reset(&self) {
        self.history.write().expect("lock poisoned").clear();
        self.call_count.store(0, Ordering::Relaxed);
        self.failure_queue.write().expect("lock poisoned").clear();
    }

    fn extract_prompt(request: &ChatCompletionRequest) -> String {
        request
            .messages
            .iter()
            .filter_map(|m| m.text_content())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn resolve(&self, prompt: &str) -> ChatCompletionResponse {
        {
            let tool_rules = self.tool_call_rules.read().expect("lock poisoned");
            for (key, calls) in tool_rules.iter() {
                if prompt.contains(key.as_str()) {
                    return ChatCompletionResponse {
                        choices: vec![Choice {
                            index: 0,
                            message: ChatMessage::assistant_with_tool_calls(calls.clone()),
                            finish_reason: Some(FinishReason::ToolCalls),
                            logprobs: None,
                        }],
                    };
                }
            }
        }

        {
            let mut seqs = self.sequences.write().expect("lock poisoned");
            for (key, deque) in seqs.iter_mut() {
                if prompt.contains(key.as_str()) {
                    let text = if deque.len() > 1 {
                        deque.pop_front().expect("deque non-empty")
                    } else {
                        deque.front().cloned().unwrap_or_default()
                    };
                    return self.text_response(&text);
                }
            }
        }

        {
            let rules = self.rules.read().expect("lock poisoned");
            for (key, value) in rules.iter() {
                if prompt.contains(key.as_str()) {
                    return self.text_response(value);
                }
            }
        }

        self.text_response(&self.fallback)
    }

    fn text_response(&self, content: &str) -> ChatCompletionResponse {
        ChatCompletionResponse {
            choices: vec![Choice {
                index: 0,
                message: ChatMessage::assistant(content),
                finish_reason: Some(FinishReason::Stop),
                logprobs: None,
            }],
        }
    }
}

#[async_trait]
impl LLMProvider for MockLLMProvider {
    fn name(&self) -> &str {
        "MockLLMProvider"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    fn supported_models(&self) -> Vec<&str> {
        vec!["mock-model"]
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn supports_tools(&self) -> bool {
        true
    }

    async fn chat(&self, request: ChatCompletionRequest) -> AgentResult<ChatCompletionResponse> {
        self.call_count.fetch_add(1, Ordering::Relaxed);

        self.history
            .write()
            .expect("lock poisoned")
            .push(request.clone());

        {
            let mut queue = self.failure_queue.write().expect("lock poisoned");
            if let Some(err) = queue.pop_front() {
                return Err(err);
            }
        }

        let prompt = Self::extract_prompt(&request);
        Ok(self.resolve(&prompt))
    }

    async fn embedding(&self, _request: EmbeddingRequest) -> AgentResult<EmbeddingResponse> {
        Ok(EmbeddingResponse {
            data: vec![EmbeddingData {
                object: "embedding".to_string(),
                index: 0,
                embedding: vec![0.0, 0.0, 0.0],
            }],
            usage: Some(EmbeddingUsage {
                prompt_tokens: 5,
                total_tokens: 5,
            }),
        })
    }

    async fn health_check(&self) -> AgentResult<bool> {
        Ok(true)
    }
}
