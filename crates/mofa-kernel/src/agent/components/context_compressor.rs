//! Context compression component
//!
//! Defines the interface for managing conversation length when interacting with LLMs.
//! When accumulated context exceeds token limits, a ContextCompressor intelligently
//! trims or summarises the message history so agents can run indefinitely.
//!
//! # Architecture
//!
//! This module only contains the trait definition and associated data types.
//! Concrete implementations live in `mofa-foundation`.
//!
//! # Example
//!
//! ```rust,ignore
//! use mofa_kernel::agent::components::context_compressor::{ContextCompressor, CompressionStrategy};
//! use mofa_kernel::agent::error::AgentResult;
//!
//! async fn ensure_fits(compressor: &dyn ContextCompressor, messages: Vec<ChatMessage>) -> AgentResult<Vec<ChatMessage>> {
//!     let tokens = compressor.count_tokens(&messages);
//!     if tokens > 4096 {
//!         compressor.compress(messages, 4096).await
//!     } else {
//!         Ok(messages)
//!     }
//! }
//! ```

use crate::agent::error::AgentResult;
use crate::agent::types::ChatMessage;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// The strategy a compressor uses to reduce context length.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub enum CompressionStrategy {
    /// Discard older messages, always keeping the system prompt and the most
    /// recent `window_size` non-system messages.
    SlidingWindow {
        /// Number of recent non-system messages to retain.
        window_size: usize,
    },
    /// Use the LLM itself to summarise older portions of the conversation,
    /// replacing them with a single condensed assistant message.
    Summarize,
    /// Use semantic similarity (embeddings) to identify and merge redundant
    /// messages while preserving diverse information.
    Semantic {
        /// Similarity threshold above which messages are considered redundant (0.0-1.0).
        similarity_threshold: f32,
        /// Number of recent messages to always keep uncompressed.
        keep_recent: usize,
    },
    /// Hierarchically compress messages based on importance scores (recency,
    /// relevance, information density, role importance).
    Hierarchical {
        /// Number of recent messages to always keep uncompressed.
        keep_recent: usize,
    },
    /// Combine multiple compression strategies adaptively.
    Hybrid {
        /// List of strategy names to combine.
        strategies: Vec<String>,
    },
}

/// Metrics about a compression operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompressionMetrics {
    /// Number of tokens before compression.
    pub tokens_before: usize,
    /// Number of tokens after compression.
    pub tokens_after: usize,
    /// Number of messages before compression.
    pub messages_before: usize,
    /// Number of messages after compression.
    pub messages_after: usize,
    /// Compression ratio (tokens_after / tokens_before), 0.0-1.0.
    pub compression_ratio: f64,
    /// Token reduction percentage (0.0-100.0).
    pub token_reduction_percent: f64,
    /// Message reduction percentage (0.0-100.0).
    pub message_reduction_percent: f64,
}

impl CompressionMetrics {
    /// Create metrics from before/after counts.
    pub fn new(
        tokens_before: usize,
        tokens_after: usize,
        messages_before: usize,
        messages_after: usize,
    ) -> Self {
        let compression_ratio = if tokens_before > 0 {
            tokens_after as f64 / tokens_before as f64
        } else {
            1.0
        };
        let token_reduction_percent = if tokens_before > 0 {
            100.0 * (1.0 - compression_ratio)
        } else {
            0.0
        };
        let message_reduction_percent = if messages_before > 0 {
            100.0 * (1.0 - messages_after as f64 / messages_before as f64)
        } else {
            0.0
        };

        Self {
            tokens_before,
            tokens_after,
            messages_before,
            messages_after,
            compression_ratio,
            token_reduction_percent,
            message_reduction_percent,
        }
    }

    /// Whether compression actually occurred (tokens were reduced).
    pub fn was_compressed(&self) -> bool {
        self.tokens_after < self.tokens_before
    }

    /// Number of tokens saved.
    pub fn tokens_saved(&self) -> usize {
        self.tokens_before.saturating_sub(self.tokens_after)
    }
}

/// Result of a compression operation, including metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionResult {
    /// Compressed messages.
    pub messages: Vec<ChatMessage>,
    /// Compression metrics.
    pub metrics: CompressionMetrics,
    /// Name of the compression strategy used.
    pub strategy_name: String,
}

impl CompressionResult {
    /// Create a result from messages and metrics.
    pub fn new(
        messages: Vec<ChatMessage>,
        metrics: CompressionMetrics,
        strategy_name: String,
    ) -> Self {
        Self {
            messages,
            metrics,
            strategy_name,
        }
    }

    /// Extract just the messages (for backward compatibility).
    pub fn into_messages(self) -> Vec<ChatMessage> {
        self.messages
    }
}

/// Trait for context compression implementations.
///
/// Implementors decide *how* to shorten a message list when it grows beyond
/// `max_tokens`.  Multiple strategies ship with `mofa-foundation`:
/// - [`SlidingWindowCompressor`](mofa_foundation::agent::components::SlidingWindowCompressor)
/// - [`SummarizingCompressor`](mofa_foundation::agent::components::SummarizingCompressor)
/// - [`SemanticCompressor`](mofa_foundation::agent::components::SemanticCompressor)
/// - [`HierarchicalCompressor`](mofa_foundation::agent::components::HierarchicalCompressor)
/// - [`HybridCompressor`](mofa_foundation::agent::components::HybridCompressor)
#[async_trait]
pub trait ContextCompressor: Send + Sync {
    /// Shorten `messages` so that the estimated token count fits within
    /// `max_tokens`.  The system prompt (role `"system"`) must always be
    /// preserved; the most recent messages must be kept when possible.
    ///
    /// If the conversation is already within the budget, return it unchanged.
    ///
    /// Returns a `CompressionResult` containing the compressed messages and
    /// detailed metrics about the compression operation.
    async fn compress_with_metrics(
        &self,
        messages: Vec<ChatMessage>,
        max_tokens: usize,
    ) -> AgentResult<CompressionResult>;

    /// Shorten `messages` so that the estimated token count fits within
    /// `max_tokens`.  The system prompt (role `"system"`) must always be
    /// preserved; the most recent messages must be kept when possible.
    ///
    /// If the conversation is already within the budget, return it unchanged.
    ///
    /// This is a convenience method that returns just the messages.
    /// For detailed metrics, use `compress_with_metrics` instead.
    async fn compress(
        &self,
        messages: Vec<ChatMessage>,
        max_tokens: usize,
    ) -> AgentResult<Vec<ChatMessage>> {
        let result = self.compress_with_metrics(messages, max_tokens).await?;
        Ok(result.messages)
    }

    /// Estimate the number of tokens consumed by a slice of messages.
    ///
    /// The default implementation uses the `chars / 4` heuristic which is a
    /// reasonable approximation for English text with GPT-family tokenisers.
    /// Override this method to plug in a tiktoken-style counter.
    fn count_tokens(&self, messages: &[ChatMessage]) -> usize {
        messages
            .iter()
            .filter_map(|m| m.content.as_ref())
            .map(|c| c.len() / 4 + 1)
            .sum()
    }

    /// The compression strategy this compressor uses.
    fn strategy(&self) -> CompressionStrategy;

    /// A short human-readable name for this compressor (used in logs).
    fn name(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── CompressionMetrics::new ──────────────────────────────────────────

    #[test]
    fn metrics_full_compression() {
        let m = CompressionMetrics::new(1000, 0, 20, 1);
        assert_eq!(m.compression_ratio, 0.0);
        assert!((m.token_reduction_percent - 100.0).abs() < f64::EPSILON);
        assert!((m.message_reduction_percent - 95.0).abs() < f64::EPSILON);
        assert!(m.was_compressed());
        assert_eq!(m.tokens_saved(), 1000);
    }

    #[test]
    fn metrics_no_compression() {
        let m = CompressionMetrics::new(500, 500, 10, 10);
        assert!((m.compression_ratio - 1.0).abs() < f64::EPSILON);
        assert!((m.token_reduction_percent - 0.0).abs() < f64::EPSILON);
        assert!((m.message_reduction_percent - 0.0).abs() < f64::EPSILON);
        assert!(!m.was_compressed());
        assert_eq!(m.tokens_saved(), 0);
    }

    #[test]
    fn metrics_partial_compression() {
        let m = CompressionMetrics::new(1000, 400, 10, 6);
        assert!((m.compression_ratio - 0.4).abs() < f64::EPSILON);
        assert!((m.token_reduction_percent - 60.0).abs() < f64::EPSILON);
        assert!((m.message_reduction_percent - 40.0).abs() < f64::EPSILON);
        assert!(m.was_compressed());
        assert_eq!(m.tokens_saved(), 600);
    }

    #[test]
    fn metrics_zero_tokens_before() {
        let m = CompressionMetrics::new(0, 0, 0, 0);
        // When tokens_before is 0, ratio defaults to 1.0 and reduction to 0.0
        assert!((m.compression_ratio - 1.0).abs() < f64::EPSILON);
        assert!((m.token_reduction_percent - 0.0).abs() < f64::EPSILON);
        assert!((m.message_reduction_percent - 0.0).abs() < f64::EPSILON);
        assert!(!m.was_compressed());
        assert_eq!(m.tokens_saved(), 0);
    }

    #[test]
    fn metrics_tokens_after_exceeds_before() {
        // Edge case: tokens_after > tokens_before (shouldn't happen in practice
        // but tests the saturating_sub in tokens_saved)
        let m = CompressionMetrics::new(100, 200, 5, 10);
        assert!(m.compression_ratio > 1.0);
        assert!(!m.was_compressed());
        assert_eq!(m.tokens_saved(), 0); // saturating_sub prevents underflow
    }

    // ── CompressionStrategy construction ─────────────────────────────────

    #[test]
    fn strategy_sliding_window() {
        let s = CompressionStrategy::SlidingWindow { window_size: 10 };
        let json = serde_json::to_string(&s).unwrap();
        let recovered: CompressionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, s);
    }

    #[test]
    fn strategy_summarize_roundtrip() {
        let s = CompressionStrategy::Summarize;
        let json = serde_json::to_string(&s).unwrap();
        let recovered: CompressionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, s);
    }

    #[test]
    fn strategy_semantic() {
        let s = CompressionStrategy::Semantic {
            similarity_threshold: 0.85,
            keep_recent: 5,
        };
        let json = serde_json::to_string(&s).unwrap();
        let recovered: CompressionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, s);
    }

    #[test]
    fn strategy_hierarchical() {
        let s = CompressionStrategy::Hierarchical { keep_recent: 3 };
        let json = serde_json::to_string(&s).unwrap();
        let recovered: CompressionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, s);
    }

    #[test]
    fn strategy_hybrid() {
        let s = CompressionStrategy::Hybrid {
            strategies: vec!["sliding_window".into(), "summarize".into()],
        };
        let json = serde_json::to_string(&s).unwrap();
        let recovered: CompressionStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, s);
    }

    // ── CompressionResult ────────────────────────────────────────────────

    #[test]
    fn compression_result_into_messages() {
        let msgs = vec![ChatMessage {
            role: "assistant".into(),
            content: Some("summary".into()),
            tool_call_id: None,
            tool_calls: None,
        }];
        let metrics = CompressionMetrics::new(500, 100, 10, 1);
        let result = CompressionResult::new(msgs.clone(), metrics, "test".into());
        assert_eq!(result.strategy_name, "test");
        let extracted = result.into_messages();
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].content.as_deref(), Some("summary"));
    }

    // ── CompressionMetrics serde ─────────────────────────────────────────

    #[test]
    fn compression_metrics_serde_roundtrip() {
        let m = CompressionMetrics::new(800, 200, 15, 4);
        let json = serde_json::to_string(&m).unwrap();
        let recovered: CompressionMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.tokens_before, 800);
        assert_eq!(recovered.tokens_after, 200);
        assert_eq!(recovered.messages_before, 15);
        assert_eq!(recovered.messages_after, 4);
        assert_eq!(recovered, m);
    }
}
