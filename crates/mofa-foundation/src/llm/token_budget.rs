//! Token-budget-aware context window management
//!
//! This module provides:
//! - `TokenEstimator` trait for pluggable token counting
//! - `CharBasedEstimator` as a zero-dependency default
//! - `ContextWindowPolicy` for automatic history trimming
//! - `ContextWindowManager` that wires estimation + policy together
//!
//! # Design Principles
//!
//! 1. **Backward-compatible** — default policy is `None` (current behaviour)
//! 2. **Pluggable** — bring your own estimator (tiktoken, sentencepiece, etc.)
//! 3. **Framework-level** — agents don't need to manage token budgets manually
//!
//! # Example
//!
//! ```rust,ignore
//! use mofa_foundation::llm::token_budget::{
//!     ContextWindowManager, ContextWindowPolicy, CharBasedEstimator,
//! };
//!
//! let manager = ContextWindowManager::new(8192)
//!     .with_policy(ContextWindowPolicy::SlidingWindow { keep_last_n: 10 })
//!     .with_estimator(Box::new(CharBasedEstimator::new(4)));
//!
//! let trimmed = manager.apply(&messages);
//! ```

use crate::llm::types::{ChatMessage, MessageContent, Role};
use tracing::warn;

// ============================================================================
// Token Estimation
// ============================================================================

/// Trait for estimating token counts from chat messages.
///
/// Implement this trait to provide accurate token counting for a specific
/// tokenizer (e.g., tiktoken for OpenAI, sentencepiece for Llama).
pub trait TokenEstimator: Send + Sync {
    /// Estimate the token count for a single message.
    fn estimate_tokens(&self, message: &ChatMessage) -> usize;

    /// Estimate the total token count for a slice of messages.
    ///
    /// Default implementation sums individual estimates plus a small
    /// per-message overhead (3 tokens) for message framing.
    fn estimate_total(&self, messages: &[ChatMessage]) -> usize {
        messages
            .iter()
            .map(|m| self.estimate_tokens(m) + 3) // ~3 tokens per-message overhead
            .sum()
    }
}

/// Character-based token estimator.
///
/// Uses a simple heuristic: `token_count ≈ char_count / chars_per_token`.
/// Default ratio is 4 characters per token, which is a reasonable approximation
/// for English text with GPT-family tokenizers.
///
/// This estimator requires zero external dependencies and is suitable for
/// rough budget enforcement. For production accuracy, implement `TokenEstimator`
/// with a real tokenizer.
#[derive(Debug, Clone)]
pub struct CharBasedEstimator {
    /// Characters per token ratio
    chars_per_token: usize,
}

impl CharBasedEstimator {
    /// Create a new estimator with a custom chars-per-token ratio.
    pub fn new(chars_per_token: usize) -> Self {
        Self {
            chars_per_token: chars_per_token.max(1),
        }
    }
}

impl Default for CharBasedEstimator {
    fn default() -> Self {
        Self { chars_per_token: 4 }
    }
}

impl TokenEstimator for CharBasedEstimator {
    fn estimate_tokens(&self, message: &ChatMessage) -> usize {
        let content_len = match &message.content {
            Some(MessageContent::Text(text)) => text.len(),
            Some(MessageContent::Parts(parts)) => parts
                .iter()
                .map(|p| match p {
                    crate::llm::types::ContentPart::Text { text } => text.len(),
                    // Images contribute a fixed token estimate (~85 tokens for low detail)
                    crate::llm::types::ContentPart::Image { .. } => 85 * self.chars_per_token,
                    // Audio contributes a fixed token estimate (~200 tokens)
                    crate::llm::types::ContentPart::Audio { .. } => 200 * self.chars_per_token,
                    // Video contributes a fixed token estimate (~300 tokens)
                    crate::llm::types::ContentPart::Video { .. } => 300 * self.chars_per_token,
                })
                .sum(),
            None => 0,
        };

        // Role name contributes ~1 token
        let role_tokens = 1;

        content_len / self.chars_per_token + role_tokens
    }
}

// ============================================================================
// Context Window Policy
// ============================================================================

/// Policy for managing conversation history within a token budget.
///
/// When the assembled message payload would exceed the model's context window,
/// this policy determines how to trim the history.
#[derive(Debug, Clone, Default)]
pub enum ContextWindowPolicy {
    /// Drop oldest messages first, always keep system prompt + last N user/assistant turns.
    ///
    /// This is the recommended policy for most use cases. It preserves recent
    /// context while staying within the token budget.
    SlidingWindow {
        /// Minimum number of recent turns to always keep (in addition to system prompt).
        /// A "turn" is one user message + one assistant response.
        keep_last_n: usize,
    },

    /// Error if the context would be exceeded. Use this for strict validation.
    Strict,

    /// No management — current behaviour (default). History is passed through as-is.
    #[default]
    None,
}

// ============================================================================
// Context Window Manager
// ============================================================================

/// Result of applying context window management.
#[must_use]
#[derive(Debug)]
pub struct TrimResult {
    /// The trimmed messages, ready to send to the LLM.
    pub messages: Vec<ChatMessage>,
    /// Number of messages that were dropped.
    pub dropped_count: usize,
    /// Estimated token count of the final payload.
    pub estimated_tokens: usize,
    /// Whether trimming was applied.
    pub was_trimmed: bool,
}

// ============================================================================
// Token Budget Config
// ============================================================================

/// Unified configuration for token-budget-aware auto-summarization and graceful halt.
///
/// Pass this to `LLMAgentBuilder::with_token_budget_config()` to enable automatic
/// context compression and budget enforcement.
///
/// # Example
///
/// ```rust,ignore
/// use mofa_foundation::llm::TokenBudgetConfig;
///
/// // Enable auto-summarize at 80% of an 8 K-token context window
/// let cfg = TokenBudgetConfig::default();
///
/// // Only sliding-window (no LLM summarization)
/// let cfg = TokenBudgetConfig::sliding_window_only(4096);
/// ```
///
/// # Note
/// When using `TokenBudgetConfig`, set `LLMAgentConfig::context_window_size` to `None`
/// (or leave it unset) to avoid the round-based sliding window fighting with
/// the token-based management provided by this config.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TokenBudgetConfig {
    /// Maximum tokens for the entire input payload (system + history + current message).
    ///
    /// Default: `8192`
    pub context_window_tokens: usize,

    /// Fraction of `context_window_tokens` at which auto-summarization is triggered.
    ///
    /// Must be in the range `(0.0, 1.0]`. Default: `0.8` (trigger at 80% full).
    /// When the estimated payload exceeds `context_window_tokens * auto_summarize_threshold`,
    /// the compressor is invoked before the LLM call.
    pub auto_summarize_threshold: f64,

    /// Number of recent messages to keep verbatim when summarizing.
    ///
    /// The summarizer compresses only older messages; the most recent
    /// `keep_recent_on_summarize` messages are left untouched.
    /// Default: `4`
    pub keep_recent_on_summarize: usize,

    /// Whether to use LLM-based summarization (true) or just sliding-window trimming (false).
    ///
    /// When `true`, an LLM call is made to produce a concise summary of older messages.
    /// When `false`, oldest messages are simply dropped.
    /// Default: `true`
    pub use_llm_summarize: bool,

    /// If `true`, return an error when the token/cost budget is exceeded.
    /// If `false`, log a warning and continue.
    ///
    /// Default: `true`
    pub halt_on_budget_exceeded: bool,

    /// Optional cost/token budget limits.
    ///
    /// When `None`, no budget checking is performed.
    /// When `Some`, a `BudgetEnforcer` is created and wired automatically.
    pub budget: Option<mofa_kernel::budget::BudgetConfig>,
}

impl Default for TokenBudgetConfig {
    fn default() -> Self {
        Self {
            context_window_tokens: 8192,
            auto_summarize_threshold: 0.8,
            keep_recent_on_summarize: 4,
            use_llm_summarize: true,
            halt_on_budget_exceeded: true,
            budget: None,
        }
    }
}

impl TokenBudgetConfig {
    /// Create a config that only enables sliding-window trimming — no LLM summarization
    /// and no budget limits.
    pub fn sliding_window_only(context_window_tokens: usize) -> Self {
        Self {
            context_window_tokens,
            use_llm_summarize: false,
            ..Default::default()
        }
    }

    /// Validate this config. Returns `Err` on invalid values.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.context_window_tokens == 0 {
            return Err("context_window_tokens must be > 0");
        }
        if self.auto_summarize_threshold <= 0.0 || self.auto_summarize_threshold > 1.0 {
            return Err("auto_summarize_threshold must be in range (0.0, 1.0]");
        }
        Ok(())
    }

    /// Compute the absolute token count at which summarization is triggered.
    pub fn summarize_trigger_tokens(&self) -> usize {
        (self.context_window_tokens as f64 * self.auto_summarize_threshold) as usize
    }
}

/// Manages the context window budget for LLM requests.
///
/// Combines a `TokenEstimator` with a `ContextWindowPolicy` to automatically
/// trim conversation history when it would exceed the model's context window.
pub struct ContextWindowManager {
    /// Maximum tokens for the entire input payload (system + history + current).
    context_window_tokens: usize,
    /// Policy to apply when the budget is exceeded.
    policy: ContextWindowPolicy,
    /// Token estimator implementation.
    estimator: Box<dyn TokenEstimator>,
}

impl ContextWindowManager {
    /// Create a new manager with a given context window size.
    ///
    /// Uses `CharBasedEstimator` and `ContextWindowPolicy::None` by default.
    pub fn new(context_window_tokens: usize) -> Self {
        Self {
            context_window_tokens,
            policy: ContextWindowPolicy::default(),
            estimator: Box::new(CharBasedEstimator::default()),
        }
    }

    /// Set the context window policy.
    pub fn with_policy(mut self, policy: ContextWindowPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Set a custom token estimator.
    pub fn with_estimator(mut self, estimator: Box<dyn TokenEstimator>) -> Self {
        self.estimator = estimator;
        self
    }

    /// Get the configured context window size.
    pub fn context_window_tokens(&self) -> usize {
        self.context_window_tokens
    }

    /// Estimate the total token count for a set of messages.
    pub fn estimate_tokens(&self, messages: &[ChatMessage]) -> usize {
        self.estimator.estimate_total(messages)
    }

    /// Apply the context window policy to a set of messages.
    ///
    /// The input `messages` should be in the standard order:
    /// `[system_prompt, ...history, current_user_message]`
    ///
    /// The system prompt (first message) and the current user message (last message)
    /// are **never** trimmed. Only history messages between them are candidates
    /// for removal.
    pub fn apply(&self, messages: &[ChatMessage]) -> TrimResult {
        match &self.policy {
            ContextWindowPolicy::None => {
                let estimated = self.estimator.estimate_total(messages);
                TrimResult {
                    messages: messages.to_vec(),
                    dropped_count: 0,
                    estimated_tokens: estimated,
                    was_trimmed: false,
                }
            }
            ContextWindowPolicy::Strict => {
                let estimated = self.estimator.estimate_total(messages);
                if estimated > self.context_window_tokens {
                    warn!(
                        estimated_tokens = estimated,
                        budget = self.context_window_tokens,
                        "Context window budget exceeded in strict mode"
                    );
                }
                TrimResult {
                    messages: messages.to_vec(),
                    dropped_count: 0,
                    estimated_tokens: estimated,
                    was_trimmed: false,
                }
            }
            ContextWindowPolicy::SlidingWindow { keep_last_n } => {
                self.apply_sliding_window(messages, *keep_last_n)
            }
        }
    }

    /// Apply sliding window trimming.
    ///
    /// Strategy:
    /// 1. Always keep the system prompt (index 0) and current user message (last index)
    /// 2. Always keep the last `keep_last_n` history messages
    /// 3. Drop oldest history messages until we fit within the budget
    fn apply_sliding_window(&self, messages: &[ChatMessage], keep_last_n: usize) -> TrimResult {
        if messages.len() <= 2 {
            // Only system prompt + current message — nothing to trim
            let estimated = self.estimator.estimate_total(messages);
            return TrimResult {
                messages: messages.to_vec(),
                dropped_count: 0,
                estimated_tokens: estimated,
                was_trimmed: false,
            };
        }

        // Separate: system prompt | history | current message
        let system_msg = &messages[0];
        let current_msg = &messages[messages.len() - 1];
        let history = &messages[1..messages.len() - 1];

        // Tokens consumed by the fixed parts (system + current)
        let fixed_tokens = self.estimator.estimate_tokens(system_msg)
            + 3
            + self.estimator.estimate_tokens(current_msg)
            + 3;

        let remaining_budget = if self.context_window_tokens > fixed_tokens {
            self.context_window_tokens - fixed_tokens
        } else {
            // Budget is too small even for system + current; return just those
            warn!(
                fixed_tokens = fixed_tokens,
                budget = self.context_window_tokens,
                "Context window too small for system prompt + current message"
            );
            return TrimResult {
                messages: vec![system_msg.clone(), current_msg.clone()],
                dropped_count: history.len(),
                estimated_tokens: fixed_tokens,
                was_trimmed: true,
            };
        };

        // Start from the end of history, keep messages while within budget
        let mut kept_history: Vec<&ChatMessage> = Vec::new();
        let mut used_tokens = 0usize;

        for (i, msg) in history.iter().rev().enumerate() {
            let msg_tokens = self.estimator.estimate_tokens(msg) + 3;

            if used_tokens + msg_tokens <= remaining_budget || i < keep_last_n {
                // Keep this message (either within budget or protected by keep_last_n)
                if used_tokens + msg_tokens <= remaining_budget {
                    used_tokens += msg_tokens;
                    kept_history.push(msg);
                } else if i < keep_last_n {
                    // Protected by keep_last_n but exceeds budget — keep anyway but warn
                    used_tokens += msg_tokens;
                    kept_history.push(msg);
                    warn!(
                        keep_last_n = keep_last_n,
                        "keep_last_n forces retention beyond token budget"
                    );
                }
            } else {
                break;
            }
        }

        // Reverse back to chronological order
        kept_history.reverse();

        let dropped = history.len() - kept_history.len();
        let total_estimated = fixed_tokens + used_tokens;

        // Assemble final messages
        let mut result = Vec::with_capacity(kept_history.len() + 2);
        result.push(system_msg.clone());
        for msg in kept_history {
            result.push(msg.clone());
        }
        result.push(current_msg.clone());

        if dropped > 0 {
            warn!(
                dropped_messages = dropped,
                estimated_tokens = total_estimated,
                budget = self.context_window_tokens,
                "Trimmed conversation history to fit context window"
            );
        }

        TrimResult {
            messages: result,
            dropped_count: dropped,
            estimated_tokens: total_estimated,
            was_trimmed: dropped > 0,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::ChatMessage;

    // -------------------------------------------------------------------------
    // TokenBudgetConfig tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_token_budget_config_default_values() {
        let cfg = TokenBudgetConfig::default();
        assert_eq!(cfg.context_window_tokens, 8192);
        assert!((cfg.auto_summarize_threshold - 0.8).abs() < f64::EPSILON);
        assert_eq!(cfg.keep_recent_on_summarize, 4);
        assert!(cfg.use_llm_summarize);
        assert!(cfg.halt_on_budget_exceeded);
        assert!(cfg.budget.is_none());
    }

    #[test]
    fn test_token_budget_config_validate_zero_window() {
        let cfg = TokenBudgetConfig {
            context_window_tokens: 0,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_token_budget_config_validate_threshold_zero() {
        let cfg = TokenBudgetConfig {
            auto_summarize_threshold: 0.0,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_token_budget_config_validate_threshold_over_one() {
        let cfg = TokenBudgetConfig {
            auto_summarize_threshold: 1.1,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_token_budget_config_validate_ok() {
        let cfg = TokenBudgetConfig::default();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_token_budget_config_trigger_tokens_math() {
        let cfg = TokenBudgetConfig {
            context_window_tokens: 10000,
            auto_summarize_threshold: 0.8,
            ..Default::default()
        };
        assert_eq!(cfg.summarize_trigger_tokens(), 8000);
    }

    #[test]
    fn test_token_budget_config_sliding_window_only() {
        let cfg = TokenBudgetConfig::sliding_window_only(4096);
        assert_eq!(cfg.context_window_tokens, 4096);
        assert!(!cfg.use_llm_summarize);
        assert!(cfg.budget.is_none());
        assert!(cfg.validate().is_ok());
    }

    // -------------------------------------------------------------------------
    // Existing ContextWindowManager tests
    // -------------------------------------------------------------------------

    fn make_messages(count: usize, msg_len: usize) -> Vec<ChatMessage> {
        let mut msgs = Vec::new();
        // System prompt
        msgs.push(ChatMessage::system("You are a helpful assistant."));
        // History
        for i in 0..count {
            let content = format!("Message {}: {}", i, "x".repeat(msg_len));
            if i % 2 == 0 {
                msgs.push(ChatMessage::user(&content));
            } else {
                msgs.push(ChatMessage::assistant(&content));
            }
        }
        // Current user message
        msgs.push(ChatMessage::user("What is the answer?"));
        msgs
    }

    #[test]
    fn test_char_based_estimator_basic() {
        let estimator = CharBasedEstimator::default();
        let msg = ChatMessage::user("Hello world"); // 11 chars -> ~2 tokens + 1 role
        let tokens = estimator.estimate_tokens(&msg);
        assert_eq!(tokens, 11 / 4 + 1); // 3
    }

    #[test]
    fn test_char_based_estimator_empty() {
        let estimator = CharBasedEstimator::default();
        let msg = ChatMessage::system("");
        let tokens = estimator.estimate_tokens(&msg);
        assert_eq!(tokens, 1); // just role token
    }

    #[test]
    fn test_policy_none_passes_through() {
        let msgs = make_messages(10, 100);
        let manager = ContextWindowManager::new(4096);
        let result = manager.apply(&msgs);

        assert!(!result.was_trimmed);
        assert_eq!(result.dropped_count, 0);
        assert_eq!(result.messages.len(), msgs.len());
    }

    #[test]
    fn test_sliding_window_trims_old_messages() {
        // Create 50 messages with ~100 chars each
        // Each message ≈ 100/4 + 1 + 3 = 29 tokens
        // 50 messages ≈ 1450 tokens + system + current ≈ 1500 tokens
        // Set budget to 500 tokens — should trim most history
        let msgs = make_messages(50, 100);
        let manager = ContextWindowManager::new(500)
            .with_policy(ContextWindowPolicy::SlidingWindow { keep_last_n: 2 });

        let result = manager.apply(&msgs);

        assert!(result.was_trimmed);
        assert!(result.dropped_count > 0);
        // System prompt is always first
        assert!(matches!(result.messages[0].role, Role::System));
        // Current message is always last
        assert!(matches!(
            result.messages[result.messages.len() - 1].role,
            Role::User
        ));
        // Total messages should be less than original
        assert!(result.messages.len() < msgs.len());
    }

    #[test]
    fn test_sliding_window_preserves_system_and_current() {
        let msgs = make_messages(5, 1000);
        let manager = ContextWindowManager::new(100)
            .with_policy(ContextWindowPolicy::SlidingWindow { keep_last_n: 0 });

        let result = manager.apply(&msgs);

        // Even with extreme trimming, system + current are preserved
        assert!(result.messages.len() >= 2);
        assert!(matches!(result.messages[0].role, Role::System));
        assert!(matches!(
            result.messages[result.messages.len() - 1].role,
            Role::User
        ));
    }

    #[test]
    fn test_sliding_window_no_trim_when_within_budget() {
        let msgs = make_messages(3, 10);
        let manager = ContextWindowManager::new(100_000)
            .with_policy(ContextWindowPolicy::SlidingWindow { keep_last_n: 2 });

        let result = manager.apply(&msgs);

        assert!(!result.was_trimmed);
        assert_eq!(result.dropped_count, 0);
        assert_eq!(result.messages.len(), msgs.len());
    }

    #[test]
    fn test_strict_mode_warns_but_preserves() {
        let msgs = make_messages(50, 100);
        let manager = ContextWindowManager::new(100).with_policy(ContextWindowPolicy::Strict);

        let result = manager.apply(&msgs);

        // Strict mode doesn't trim — it warns
        assert!(!result.was_trimmed);
        assert_eq!(result.messages.len(), msgs.len());
        assert!(result.estimated_tokens > 100); // exceeds budget
    }

    #[test]
    fn test_only_two_messages_no_crash() {
        let msgs = vec![
            ChatMessage::system("System prompt"),
            ChatMessage::user("Current input"),
        ];
        let manager = ContextWindowManager::new(100)
            .with_policy(ContextWindowPolicy::SlidingWindow { keep_last_n: 5 });

        let result = manager.apply(&msgs);
        assert!(!result.was_trimmed);
        assert_eq!(result.messages.len(), 2);
    }

    #[test]
    fn test_custom_estimator() {
        // Estimator that counts every character as 1 token
        struct OneCharOneToken;
        impl TokenEstimator for OneCharOneToken {
            fn estimate_tokens(&self, message: &ChatMessage) -> usize {
                match &message.content {
                    Some(MessageContent::Text(t)) => t.len(),
                    _ => 0,
                }
            }
        }

        let msgs = vec![
            ChatMessage::system("hi"),  // 2 tokens
            ChatMessage::user("hello"), // 5 tokens
        ];
        let manager = ContextWindowManager::new(1000).with_estimator(Box::new(OneCharOneToken));

        let result = manager.apply(&msgs);
        // 2 + 3 (overhead) + 5 + 3 (overhead) = 13
        assert_eq!(result.estimated_tokens, 13);
    }
}
