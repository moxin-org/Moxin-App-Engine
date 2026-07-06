//! Memory drift testing harness built on top of real MoFA memory components.

use anyhow::Result;
use mofa_foundation::agent::components::{
    EpisodicMemory, HashEmbedder, Memory, MemoryItem, MemoryStats, Message, SemanticMemory,
};
use mofa_kernel::agent::components::memory::MemoryValue;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Snapshot of a single session's history for drift/isolation assertions.
#[derive(Debug, Clone)]
pub struct SessionMemorySnapshot {
    pub session_id: String,
    pub messages: Vec<Message>,
}

/// Small harness for testing cross-session memory retention and isolation.
pub struct MemoryDriftHarness {
    backend: MemoryBackend,
    known_sessions: BTreeSet<String>,
}

enum MemoryBackend {
    Episodic(EpisodicMemory),
    Semantic(SemanticMemory),
}

impl Default for MemoryDriftHarness {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryDriftHarness {
    pub fn new() -> Self {
        Self {
            backend: MemoryBackend::Episodic(EpisodicMemory::new()),
            known_sessions: BTreeSet::new(),
        }
    }

    pub fn with_semantic_memory() -> Self {
        Self {
            // Use the built-in hash embedder so semantic-memory tests stay fully local and deterministic.
            backend: MemoryBackend::Semantic(SemanticMemory::new(Arc::new(
                HashEmbedder::with_128_dims(),
            ))),
            known_sessions: BTreeSet::new(),
        }
    }

    pub async fn record_message(&mut self, session_id: &str, message: Message) -> Result<()> {
        self.known_sessions.insert(session_id.to_string());
        match &mut self.backend {
            MemoryBackend::Episodic(memory) => memory.add_to_history(session_id, message).await?,
            MemoryBackend::Semantic(memory) => memory.add_to_history(session_id, message).await?,
        }
        Ok(())
    }

    pub async fn record_turn(
        &mut self,
        session_id: &str,
        user_text: &str,
        assistant_text: &str,
    ) -> Result<()> {
        self.record_message(session_id, Message::user(user_text))
            .await?;
        self.record_message(session_id, Message::assistant(assistant_text))
            .await?;
        Ok(())
    }

    pub async fn history(&self, session_id: &str) -> Result<Vec<Message>> {
        Ok(match &self.backend {
            MemoryBackend::Episodic(memory) => memory.get_history(session_id).await?,
            MemoryBackend::Semantic(memory) => memory.get_history(session_id).await?,
        })
    }

    pub async fn session_snapshot(&self, session_id: &str) -> Result<SessionMemorySnapshot> {
        Ok(SessionMemorySnapshot {
            session_id: session_id.to_string(),
            messages: self.history(session_id).await?,
        })
    }

    pub async fn all_session_snapshots(&self) -> Result<Vec<SessionMemorySnapshot>> {
        let mut snapshots = Vec::new();
        for session_id in self.session_ids() {
            snapshots.push(self.session_snapshot(&session_id).await?);
        }
        Ok(snapshots)
    }

    pub async fn recent_episode_texts(&self, limit: usize) -> Result<Vec<String>> {
        match &self.backend {
            MemoryBackend::Episodic(memory) => Ok(memory
                .get_recent_episodes(limit)
                .into_iter()
                .map(|episode| episode.message.content.clone())
                .collect()),
            MemoryBackend::Semantic(memory) => {
                // Semantic memory does not expose cross-session episodes directly, so rebuild
                // a recent view from known session histories.
                let mut messages: Vec<Message> = Vec::new();
                for session_id in &self.known_sessions {
                    messages.extend(memory.get_history(session_id).await?);
                }
                messages.sort_by_key(|message| message.timestamp);
                let total = messages.len();
                let start = total.saturating_sub(limit);
                Ok(messages[start..]
                    .iter()
                    .map(|message| message.content.clone())
                    .collect())
            }
        }
    }

    pub fn session_ids(&self) -> Vec<String> {
        self.known_sessions.iter().cloned().collect()
    }

    pub async fn clear_session(&mut self, session_id: &str) -> Result<()> {
        self.known_sessions.remove(session_id);
        match &mut self.backend {
            MemoryBackend::Episodic(memory) => memory.clear_history(session_id).await?,
            MemoryBackend::Semantic(memory) => memory.clear_history(session_id).await?,
        }
        Ok(())
    }

    pub async fn clear_all(&mut self) -> Result<()> {
        self.known_sessions.clear();
        match &mut self.backend {
            MemoryBackend::Episodic(memory) => memory.clear().await?,
            MemoryBackend::Semantic(memory) => memory.clear().await?,
        }
        Ok(())
    }

    pub async fn stats(&self) -> Result<MemoryStats> {
        Ok(match &self.backend {
            MemoryBackend::Episodic(memory) => memory.stats().await?,
            MemoryBackend::Semantic(memory) => memory.stats().await?,
        })
    }

    pub async fn store_text(&mut self, key: &str, value: &str) -> Result<()> {
        match &mut self.backend {
            // Store text through the real memory trait so retrieval/search semantics stay aligned
            // with the underlying backend implementation.
            MemoryBackend::Episodic(memory) => memory.store(key, MemoryValue::text(value)).await?,
            MemoryBackend::Semantic(memory) => memory.store(key, MemoryValue::text(value)).await?,
        }
        Ok(())
    }

    // Exact KV lookup helper for testing direct retrieval separately from search based recall
    pub async fn retrieve_text(&self, key: &str) -> Result<Option<String>> {
        let value = match &self.backend {
            MemoryBackend::Episodic(memory) => memory.retrieve(key).await?,
            MemoryBackend::Semantic(memory) => memory.retrieve(key).await?,
        };

        Ok(value.and_then(|value| value.as_text().map(str::to_string)))
    }

    pub async fn search_texts(&self, query: &str, limit: usize) -> Result<Vec<String>> {
        let results: Vec<MemoryItem> = match &self.backend {
            MemoryBackend::Episodic(memory) => memory.search(query, limit).await?,
            MemoryBackend::Semantic(memory) => memory.search(query, limit).await?,
        };
        Ok(results
            .into_iter()
            .filter_map(|item| item.value.as_text().map(str::to_string))
            .collect())
    }

    pub fn memory_type(&self) -> &'static str {
        match self.backend {
            MemoryBackend::Episodic(_) => "episodic",
            MemoryBackend::Semantic(_) => "semantic",
        }
    }
}
