//! Mock inference backend for deterministic agent testing.

use async_trait::async_trait;
use mofa_foundation::orchestrator::{
    ModelOrchestrator, ModelProviderConfig, ModelType, OrchestratorError, OrchestratorResult,
    PoolStatistics,
};
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

type ResponseSequences = Vec<(String, VecDeque<String>)>;

#[derive(Debug, Clone, PartialEq)]
pub struct InferenceUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageTotals {
    pub successful_calls: usize,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub total_cost_usd: f64,
}

#[derive(Debug, Clone)]
struct TokenCostRates {
    input_per_1k_tokens_usd: f64,
    output_per_1k_tokens_usd: f64,
}

/// Deterministic mock implementation of [`ModelOrchestrator`].
///
/// Supports first-match response rules, sequenced responses, failure injection,
/// rate limiting, and call counting.
pub struct MockLLMBackend {
    responses: Arc<RwLock<Vec<(String, String)>>>,
    fallback: String,
    registered: Arc<RwLock<HashSet<String>>>,
    loaded: Arc<RwLock<HashSet<String>>>,
    memory_threshold: Arc<RwLock<u64>>,
    idle_timeout_secs: Arc<RwLock<u64>>,
    failure_queue: Arc<RwLock<VecDeque<OrchestratorError>>>,
    failure_patterns: Arc<RwLock<Vec<(String, OrchestratorError)>>>,
    response_sequences: Arc<RwLock<ResponseSequences>>,
    call_count: Arc<AtomicUsize>,
    rate_limit: Arc<RwLock<Option<RateLimit>>>,
    token_cost_rates: Arc<RwLock<TokenCostRates>>,
    usage_history: Arc<RwLock<Vec<(String, InferenceUsage)>>>,
}

struct RateLimit {
    max_calls: usize,
    window_calls: usize,
}

impl Default for MockLLMBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MockLLMBackend {
    /// Create a new backend with an empty response table.
    pub fn new() -> Self {
        Self {
            responses: Arc::new(RwLock::new(Vec::new())),
            fallback: "Mock fallback response.".into(),
            registered: Arc::new(RwLock::new(HashSet::new())),
            loaded: Arc::new(RwLock::new(HashSet::new())),
            memory_threshold: Arc::new(RwLock::new(u64::MAX)),
            idle_timeout_secs: Arc::new(RwLock::new(300)),
            failure_queue: Arc::new(RwLock::new(VecDeque::new())),
            failure_patterns: Arc::new(RwLock::new(Vec::new())),
            response_sequences: Arc::new(RwLock::new(Vec::new())),
            call_count: Arc::new(AtomicUsize::new(0)),
            rate_limit: Arc::new(RwLock::new(None)),
            token_cost_rates: Arc::new(RwLock::new(TokenCostRates {
                input_per_1k_tokens_usd: 0.0,
                output_per_1k_tokens_usd: 0.0,
            })),
            usage_history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Append a response rule. Order determines priority (first match wins).
    pub fn add_response(&self, prompt_substring: &str, response: &str) {
        self.responses
            .write()
            .expect("lock poisoned")
            .push((prompt_substring.to_string(), response.to_string()));
    }

    /// Replace the fallback response returned when no rule matches.
    pub fn set_fallback(&mut self, response: &str) {
        self.fallback = response.to_string();
    }

    /// Queue errors to be returned by the next N `infer()` calls (FIFO).
    pub fn fail_next(&self, count: usize, error: OrchestratorError) {
        let mut queue = self.failure_queue.write().expect("lock poisoned");
        for _ in 0..count {
            queue.push_back(error.clone());
        }
    }

    /// Fail any `infer()` call whose prompt contains the given substring.
    pub fn fail_on(&self, prompt_substring: &str, error: OrchestratorError) {
        self.failure_patterns
            .write()
            .expect("lock poisoned")
            .push((prompt_substring.to_string(), error));
    }

    /// Add a sequence of responses for a prompt pattern.
    /// Each matching call consumes the next value; the last value repeats forever.
    pub fn add_response_sequence(&self, prompt_substring: &str, responses: Vec<&str>) {
        let deque: VecDeque<String> = responses.into_iter().map(String::from).collect();
        self.response_sequences
            .write()
            .expect("lock poisoned")
            .push((prompt_substring.to_string(), deque));
    }

    /// Set a rate limit: after `max_calls` invocations, subsequent calls fail.
    /// Call [`reset_rate_limit`] to clear the counter.
    pub fn set_rate_limit(&self, max_calls: usize) {
        *self.rate_limit.write().expect("lock poisoned") = Some(RateLimit {
            max_calls,
            window_calls: 0,
        });
    }

    /// Reset the rate limit call counter without removing the limit.
    pub fn reset_rate_limit(&self) {
        if let Some(rl) = self.rate_limit.write().expect("lock poisoned").as_mut() {
            rl.window_calls = 0;
        }
    }

    /// Total number of `infer()` calls made.
    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::Relaxed)
    }

    /// Reset the call counter to zero.
    pub fn reset_call_count(&self) {
        self.call_count.store(0, Ordering::Relaxed);
    }

    /// Configure deterministic token pricing used for usage accounting.
    pub fn set_token_cost_rates(
        &self,
        input_per_1k_tokens_usd: f64,
        output_per_1k_tokens_usd: f64,
    ) {
        *self.token_cost_rates.write().expect("lock poisoned") = TokenCostRates {
            input_per_1k_tokens_usd,
            output_per_1k_tokens_usd,
        };
    }

    /// Clear tracked usage history for successful inferences.
    pub fn reset_usage_accounting(&self) {
        self.usage_history.write().expect("lock poisoned").clear();
    }

    /// Return usage for the last successful inference.
    pub fn last_usage(&self) -> Option<InferenceUsage> {
        self.usage_history
            .read()
            .expect("lock poisoned")
            .last()
            .map(|(_, usage)| usage.clone())
    }

    /// Return prompt + usage pairs for successful inferences.
    pub fn usage_history(&self) -> Vec<(String, InferenceUsage)> {
        self.usage_history.read().expect("lock poisoned").clone()
    }

    /// Alias for [`usage_history`] following the crate's `get_*` accessor style.
    pub fn get_usage_history(&self) -> Vec<(String, InferenceUsage)> {
        self.usage_history()
    }

    /// Aggregate usage totals for successful inferences.
    pub fn usage_totals(&self) -> UsageTotals {
        let history = self.usage_history.read().expect("lock poisoned");
        let mut totals = UsageTotals {
            successful_calls: history.len(),
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            total_cost_usd: 0.0,
        };

        for (_, usage) in history.iter() {
            totals.prompt_tokens += usage.prompt_tokens as u64;
            totals.completion_tokens += usage.completion_tokens as u64;
            totals.total_tokens += usage.total_tokens as u64;
            totals.total_cost_usd += usage.cost_usd;
        }
        totals.total_cost_usd = round_cost(totals.total_cost_usd);
        totals
    }

    /// Alias for [`usage_totals`] following the crate's `get_*` accessor style.
    pub fn get_usage_totals(&self) -> UsageTotals {
        self.usage_totals()
    }

    /// Look up the response for a given prompt.
    /// Sequence responses take priority over static rules.
    fn resolve(&self, prompt: &str) -> String {
        // Check sequences first
        let mut seqs = self.response_sequences.write().expect("lock poisoned");
        for (key, deque) in seqs.iter_mut() {
            if prompt.contains(key.as_str()) {
                if deque.len() > 1 {
                    return deque.pop_front().expect("deque non-empty");
                } else if let Some(last) = deque.front() {
                    return last.clone();
                }
            }
        }
        drop(seqs);

        // Then static rules
        let rules = self.responses.read().expect("lock poisoned");
        for (key, value) in rules.iter() {
            if prompt.contains(key.as_str()) {
                return value.clone();
            }
        }
        self.fallback.clone()
    }

    fn estimate_tokens(text: &str) -> u32 {
        text.split_whitespace().count() as u32
    }

    fn build_usage(&self, prompt: &str, response: &str) -> InferenceUsage {
        let prompt_tokens = Self::estimate_tokens(prompt);
        let completion_tokens = Self::estimate_tokens(response);
        let total_tokens = prompt_tokens + completion_tokens;
        let rates = self.token_cost_rates.read().expect("lock poisoned").clone();

        let prompt_cost = (prompt_tokens as f64 / 1000.0) * rates.input_per_1k_tokens_usd;
        let completion_cost = (completion_tokens as f64 / 1000.0) * rates.output_per_1k_tokens_usd;
        let cost_usd = round_cost(prompt_cost + completion_cost);

        InferenceUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
            cost_usd,
        }
    }
}

fn round_cost(value: f64) -> f64 {
    (value * 1_000_000_000.0).round() / 1_000_000_000.0
}

#[async_trait]
impl ModelOrchestrator for MockLLMBackend {
    fn name(&self) -> &str {
        "MockLLMBackend"
    }

    // -- registration --------------------------------------------------------

    async fn register_model(&self, config: ModelProviderConfig) -> OrchestratorResult<()> {
        self.registered
            .write()
            .expect("lock poisoned")
            .insert(config.model_name);
        Ok(())
    }

    async fn unregister_model(&self, model_id: &str) -> OrchestratorResult<()> {
        self.loaded.write().expect("lock poisoned").remove(model_id);
        self.registered
            .write()
            .expect("lock poisoned")
            .remove(model_id);
        Ok(())
    }

    // -- lifecycle -----------------------------------------------------------

    async fn load_model(&self, model_id: &str) -> OrchestratorResult<()> {
        if !self
            .registered
            .read()
            .expect("lock poisoned")
            .contains(model_id)
        {
            return Err(OrchestratorError::ModelNotFound(model_id.to_string()));
        }
        self.loaded
            .write()
            .expect("lock poisoned")
            .insert(model_id.to_string());
        Ok(())
    }

    async fn unload_model(&self, model_id: &str) -> OrchestratorResult<()> {
        self.loaded.write().expect("lock poisoned").remove(model_id);
        Ok(())
    }

    fn is_model_loaded(&self, model_id: &str) -> bool {
        self.loaded
            .read()
            .expect("lock poisoned")
            .contains(model_id)
    }

    // -- inference -----------------------------------------------------------

    async fn infer(&self, _model_id: &str, input: &str) -> OrchestratorResult<String> {
        self.call_count.fetch_add(1, Ordering::Relaxed);

        // 1. Drain failure queue (FIFO)
        {
            let mut queue = self.failure_queue.write().expect("lock poisoned");
            if let Some(err) = queue.pop_front() {
                return Err(err);
            }
        }

        // 2. Check pattern-based failures
        {
            let patterns = self.failure_patterns.read().expect("lock poisoned");
            for (key, err) in patterns.iter() {
                if input.contains(key.as_str()) {
                    return Err(err.clone());
                }
            }
        }

        // 3. Check rate limit
        {
            let mut rl = self.rate_limit.write().expect("lock poisoned");
            if let Some(limit) = rl.as_mut() {
                limit.window_calls += 1;
                if limit.window_calls > limit.max_calls {
                    return Err(OrchestratorError::Other(format!(
                        "Rate limit exceeded: {} calls (max {})",
                        limit.window_calls, limit.max_calls
                    )));
                }
            }
        }

        let response = self.resolve(input);
        let usage = self.build_usage(input, &response);
        self.usage_history
            .write()
            .expect("lock poisoned")
            .push((input.to_string(), usage));

        Ok(response)
    }

    async fn route_by_type(&self, task: &ModelType) -> OrchestratorResult<String> {
        // Return the first registered model (deterministic since HashSet→Vec)
        let registered = self.registered.read().expect("lock poisoned");
        registered
            .iter()
            .next()
            .cloned()
            .ok_or_else(|| OrchestratorError::NoModelForType(task.to_string()))
    }

    // -- introspection -------------------------------------------------------

    fn get_statistics(&self) -> OrchestratorResult<PoolStatistics> {
        Ok(PoolStatistics {
            loaded_models_count: self.loaded.read().expect("lock poisoned").len(),
            total_memory_usage: 0,
            available_memory: u64::MAX,
            queued_models_count: 0,
            timestamp: chrono::Utc::now(),
        })
    }

    fn list_models(&self) -> Vec<String> {
        self.registered
            .read()
            .expect("lock poisoned")
            .iter()
            .cloned()
            .collect()
    }

    fn list_loaded_models(&self) -> Vec<String> {
        self.loaded
            .read()
            .expect("lock poisoned")
            .iter()
            .cloned()
            .collect()
    }

    // -- memory management ---------------------------------------------------

    async fn trigger_eviction(&self, _target_bytes: u64) -> OrchestratorResult<usize> {
        // Mock: evict everything
        let mut loaded = self.loaded.write().expect("lock poisoned");
        let count = loaded.len();
        loaded.clear();
        Ok(count)
    }

    async fn set_memory_threshold(&self, bytes: u64) -> OrchestratorResult<()> {
        *self.memory_threshold.write().expect("lock poisoned") = bytes;
        Ok(())
    }

    fn get_memory_threshold(&self) -> u64 {
        *self.memory_threshold.read().expect("lock poisoned")
    }

    async fn set_idle_timeout_secs(&self, secs: u64) -> OrchestratorResult<()> {
        *self.idle_timeout_secs.write().expect("lock poisoned") = secs;
        Ok(())
    }

    fn get_idle_timeout_secs(&self) -> u64 {
        *self.idle_timeout_secs.read().expect("lock poisoned")
    }
}
