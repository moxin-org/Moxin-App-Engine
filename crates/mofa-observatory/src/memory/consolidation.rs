use super::{episodic::EpisodicMemory, semantic::SemanticMemory};
use anyhow::Result;
use reqwest::Client;
use std::sync::Arc;

/// Consolidates recent episodic memories into semantic facts.
///
/// Runs as a background Tokio task every `interval_secs` seconds.
/// When at least `episodes_per_consolidation` new episodes exist,
/// calls an LLM to extract key facts and stores them in `SemanticMemory`.
pub struct MemoryConsolidationEngine {
    episodic: Arc<EpisodicMemory>,
    semantic: Arc<SemanticMemory>,
    api_base: String,
    api_key: String,
    pub episodes_per_consolidation: usize,
    pub interval_secs: u64,
    client: Client,
}

impl MemoryConsolidationEngine {
    pub fn new(
        episodic: Arc<EpisodicMemory>,
        semantic: Arc<SemanticMemory>,
        api_base: impl Into<String>,
        api_key: impl Into<String>,
        episodes_per_consolidation: usize,
    ) -> Self {
        Self {
            episodic,
            semantic,
            api_base: api_base.into(),
            api_key: api_key.into(),
            episodes_per_consolidation,
            interval_secs: 30,
            client: Client::new(),
        }
    }

    /// Start the consolidation loop in the background.
    pub fn spawn(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(self.interval_secs));
            let mut run_count = 0u64;
            loop {
                interval.tick().await;
                let limit = self.episodes_per_consolidation as i64;
                match self.episodic.recent(limit).await {
                    Ok(episodes) if episodes.len() >= self.episodes_per_consolidation => {
                        match self.consolidate(&episodes).await {
                            Ok(n) => {
                                run_count += 1;
                                tracing::info!(
                                    run = run_count,
                                    episodes = episodes.len(),
                                    facts_extracted = n,
                                    "Memory consolidation complete"
                                );
                            }
                            Err(e) => tracing::warn!("Consolidation failed: {e}"),
                        }
                    }
                    Ok(_) => {} // Not enough episodes yet
                    Err(e) => tracing::warn!("Failed to load episodes for consolidation: {e}"),
                }
            }
        })
    }

    async fn consolidate(
        &self,
        episodes: &[super::episodic::Episode],
    ) -> Result<usize> {
        let conversation = episodes
            .iter()
            .map(|e| format!("{}: {}", e.role, e.content))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "Extract 3-7 key facts from this conversation as a JSON array of strings.\n\
             Each fact should be a standalone sentence. Return ONLY the JSON array.\n\n\
             Conversation:\n{conversation}"
        );

        let resp: serde_json::Value = self
            .client
            .post(format!("{}/chat/completions", self.api_base))
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": prompt}],
                "temperature": 0.2
            }))
            .send()
            .await?
            .json()
            .await?;

        let content = resp["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("[]");

        // Strip markdown code fences if present
        let cleaned = content.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();

        let facts: Vec<String> = serde_json::from_str(cleaned).unwrap_or_default();
        let n = facts.len();
        for fact in facts {
            if let Err(e) = self.semantic.insert(fact, 0.7).await {
                tracing::warn!("Failed to store consolidated fact: {e}");
            }
        }
        Ok(n)
    }
}
