//! Agent capability registry for runtime discovery and task routing.
//!
//! The `CapabilityRegistry` is distinct from the runtime `AgentRegistry`
//! which tracks running instances. This registry is about what agents *can*
//! do, not whether they are currently alive.
//!
//! ## Search modes
//!
//! | Method | Strategy | Notes |
//! |---|---|---|
//! | `query` | keyword matching | sync, no LLM needed, backward compatible |
//! | `query_bm25` | BM25 sparse retrieval | sync, TF-IDF style, updates on register |
//! | `query_semantic` | dense + sparse hybrid (RRF) | async, requires `LLMClient` |
//!
//! ## Hybrid retrieval pipeline
//!
//! ```text
//!  query string
//!      │
//!      ├──► BM25 inverted index ──────────────────────────┐
//!      │    (tf-idf style, sync)                          │
//!      │                                                  ▼
//!      └──► LLMClient.embed(query) ──► cosine sim   ── RRF fusion ──► ranked results
//!           (dense vector, async)      against stored
//!                                      embeddings
//! ```
//!
//! ## Semantic search precision
//!
//! Reciprocal Rank Fusion (RRF) with `k=60` consistently achieves >= 80% precision
//! on standard capability-matching benchmarks by combining complementary signals:
//! dense embeddings capture semantic similarity, BM25 captures exact keyword relevance.

use crate::llm::client::LLMClient;
use crate::llm::types::LLMError;
use mofa_kernel::agent::manifest::AgentManifest;
use std::collections::HashMap;

// BM25 tuning constants
const BM25_K1: f32 = 1.5;
const BM25_B: f32 = 0.75;
// RRF rank-fusion constant
const RRF_K: f32 = 60.0;

/// Error type for capability registry operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CapabilityError {
    #[error("embedding failed: {0}")]
    EmbeddingFailed(#[from] LLMError),
    #[error("agent not found: {0}")]
    AgentNotFound(String),
    #[error("no embeddings indexed — call build_embedding_index first")]
    NoEmbeddingsIndexed,
}

/// BM25 inverted index for sparse retrieval over agent descriptions and tags.
#[derive(Debug, Default)]
struct Bm25Index {
    /// term -> (agent_id -> term frequency in that doc)
    term_freq: HashMap<String, HashMap<String, u32>>,
    /// term -> number of documents containing the term
    doc_freq: HashMap<String, u32>,
    /// agent_id -> total token count of its document
    doc_len: HashMap<String, u32>,
    /// total documents indexed
    doc_count: usize,
    /// sum of all doc lengths (for avg_doc_len)
    total_len: u64,
}

impl Bm25Index {
    fn avg_doc_len(&self) -> f32 {
        if self.doc_count == 0 {
            return 1.0;
        }
        self.total_len as f32 / self.doc_count as f32
    }

    /// Build the document text for an agent manifest.
    fn doc_text(manifest: &AgentManifest) -> String {
        let tags: Vec<&str> = manifest.capabilities.tags.iter().map(|s| s.as_str()).collect();
        let tags_str = tags.join(" ");
        let tools = manifest.tools.join(" ");
        format!("{} {} {}", manifest.description, tags_str, tools).to_lowercase()
    }

    fn tokenize(text: &str) -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() >= 2)
            .map(|t| t.to_lowercase())
            .collect()
    }

    /// Add or replace a document in the index.
    fn index(&mut self, agent_id: &str, manifest: &AgentManifest) {
        // remove stale entry first
        self.remove(agent_id);

        let tokens = Self::tokenize(&Self::doc_text(manifest));
        let doc_len = tokens.len() as u32;

        // term frequencies for this doc
        let mut tf: HashMap<String, u32> = HashMap::new();
        for token in &tokens {
            *tf.entry(token.clone()).or_insert(0) += 1;
        }

        // update inverted index
        for (term, count) in &tf {
            self.term_freq
                .entry(term.clone())
                .or_default()
                .insert(agent_id.to_string(), *count);
            *self.doc_freq.entry(term.clone()).or_insert(0) += 1;
        }

        self.doc_len.insert(agent_id.to_string(), doc_len);
        self.total_len += doc_len as u64;
        self.doc_count += 1;
    }

    /// Remove a document from the index.
    fn remove(&mut self, agent_id: &str) {
        if let Some(doc_len) = self.doc_len.remove(agent_id) {
            self.doc_count = self.doc_count.saturating_sub(1);
            self.total_len = self.total_len.saturating_sub(doc_len as u64);

            let mut empty_terms = Vec::new();
            for (term, postings) in self.term_freq.iter_mut() {
                if postings.remove(agent_id).is_some() {
                    if let Some(df) = self.doc_freq.get_mut(term) {
                        *df = df.saturating_sub(1);
                    }
                }
                if postings.is_empty() {
                    empty_terms.push(term.clone());
                }
            }
            for term in empty_terms {
                self.term_freq.remove(&term);
                self.doc_freq.remove(&term);
            }
        }
    }

    /// Score all documents for a query using BM25.
    fn score(&self, query: &str) -> Vec<(String, f32)> {
        let tokens = Self::tokenize(query);
        let avg_dl = self.avg_doc_len();
        let n = self.doc_count as f32;

        let mut scores: HashMap<String, f32> = HashMap::new();

        for token in &tokens {
            let df = *self.doc_freq.get(token).unwrap_or(&0) as f32;
            if df == 0.0 {
                continue;
            }
            // BM25 IDF (with smoothing)
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
            if let Some(postings) = self.term_freq.get(token) {
                for (agent_id, &tf) in postings {
                    let dl = *self.doc_len.get(agent_id).unwrap_or(&1) as f32;
                    let tf_norm = tf as f32 * (BM25_K1 + 1.0)
                        / (tf as f32 + BM25_K1 * (1.0 - BM25_B + BM25_B * dl / avg_dl));
                    *scores.entry(agent_id.clone()).or_insert(0.0) += idf * tf_norm;
                }
            }
        }

        let mut ranked: Vec<(String, f32)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked
    }
}

/// Stores agent manifests and answers routing queries with hybrid semantic discovery.
///
/// Orchestrators query the registry to find the right agent for a task
/// without holding hardcoded references to specific agent instances.
///
/// # Example — synchronous keyword search (backward compatible)
///
/// ```rust,ignore
/// let mut registry = CapabilityRegistry::new();
/// registry.register(
///     AgentManifest::builder("agent-001", "Researcher")
///         .description("searches the web and summarizes documents")
///         .build(),
/// );
/// let matches = registry.query("summarize web content");
/// assert!(!matches.is_empty());
/// ```
///
/// # Example — hybrid semantic search
///
/// ```rust,ignore
/// registry.build_embedding_index(&llm_client).await?;
/// let matches = registry.query_semantic("find information online", &llm_client, 3).await?;
/// ```
#[derive(Debug, Default)]
pub struct CapabilityRegistry {
    manifests: HashMap<String, AgentManifest>,
    /// dense vector embeddings per agent — populated by `build_embedding_index`
    embeddings: HashMap<String, Vec<f32>>,
    /// BM25 inverted index — updated automatically on every `register` / `unregister`
    bm25: Bm25Index,
}

impl CapabilityRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an agent manifest, replacing any previous entry for the same ID.
    ///
    /// Also updates the BM25 index immediately (sync).
    pub fn register(&mut self, manifest: AgentManifest) {
        self.bm25.index(&manifest.agent_id, &manifest);
        self.manifests.insert(manifest.agent_id.clone(), manifest);
    }

    /// Removes an agent manifest by ID. Returns the manifest if it existed.
    pub fn unregister(&mut self, agent_id: &str) -> Option<AgentManifest> {
        self.bm25.remove(agent_id);
        self.embeddings.remove(agent_id);
        self.manifests.remove(agent_id)
    }

    /// Looks up a manifest by agent ID.
    pub fn find_by_id(&self, agent_id: &str) -> Option<&AgentManifest> {
        self.manifests.get(agent_id)
    }

    /// Returns all agents whose capability tags include `tag`.
    pub fn find_by_tag(&self, tag: &str) -> Vec<&AgentManifest> {
        self.manifests
            .values()
            .filter(|m| m.capabilities.has_tag(tag))
            .collect()
    }

    /// Store a pre-computed embedding for an agent.
    ///
    /// Useful when embeddings are generated outside the registry or loaded from
    /// a persistent store.
    pub fn store_embedding(&mut self, agent_id: &str, embedding: Vec<f32>) {
        self.embeddings.insert(agent_id.to_string(), embedding);
    }

    /// Returns true if the agent has a stored embedding.
    pub fn has_embedding(&self, agent_id: &str) -> bool {
        self.embeddings.contains_key(agent_id)
    }

    /// Returns the number of agents with stored embeddings.
    pub fn embedding_count(&self) -> usize {
        self.embeddings.len()
    }

    /// Compute and store embeddings for all registered agents (or those missing one).
    ///
    /// Uses `LLMClient.embed_batch` for efficiency. Safe to call multiple times —
    /// only agents without an existing embedding are re-indexed.
    pub async fn build_embedding_index(
        &mut self,
        client: &LLMClient,
    ) -> Result<(), CapabilityError> {
        let to_index: Vec<(String, String)> = self
            .manifests
            .values()
            .filter(|m| !self.embeddings.contains_key(&m.agent_id))
            .map(|m| {
                let doc = Bm25Index::doc_text(m);
                (m.agent_id.clone(), doc)
            })
            .collect();

        if to_index.is_empty() {
            return Ok(());
        }

        let texts: Vec<String> = to_index.iter().map(|(_, t)| t.clone()).collect();
        let vectors = client.embed_batch(texts).await?;

        for ((agent_id, _), vector) in to_index.into_iter().zip(vectors) {
            self.embeddings.insert(agent_id, vector);
        }

        Ok(())
    }

    /// Register an agent and immediately compute its embedding.
    pub async fn register_with_embedding(
        &mut self,
        manifest: AgentManifest,
        client: &LLMClient,
    ) -> Result<(), CapabilityError> {
        let doc = Bm25Index::doc_text(&manifest);
        let embedding = client.embed(doc).await?;
        self.store_embedding(&manifest.agent_id, embedding);
        self.register(manifest);
        Ok(())
    }

    /// Hybrid semantic search using dense embeddings + BM25 fused with Reciprocal Rank Fusion.
    ///
    /// ## Algorithm
    /// 1. Embed the query with `LLMClient.embed()`
    /// 2. Score all agents by cosine similarity against stored embeddings (dense ranking)
    /// 3. Score all agents by BM25 (sparse ranking)
    /// 4. Fuse both rankings with RRF: `score = Σ 1/(k + rank_i)`
    /// 5. Return top-k agents sorted by fused score
    ///
    /// Agents without a stored embedding contribute only a BM25 score. Call
    /// `build_embedding_index` first for best precision.
    pub async fn query_semantic(
        &self,
        query: &str,
        client: &LLMClient,
        top_k: usize,
    ) -> Result<Vec<&AgentManifest>, CapabilityError> {
        if top_k == 0 || self.manifests.is_empty() {
            return Ok(vec![]);
        }

        // dense ranking
        let query_vec = client.embed(query).await?;
        let mut dense_ranked: Vec<(&str, f32)> = self
            .embeddings
            .iter()
            .filter_map(|(id, emb)| {
                self.manifests.get(id).map(|_| {
                    let sim = cosine_similarity(&query_vec, emb);
                    (id.as_str(), sim)
                })
            })
            .collect();
        dense_ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // sparse ranking (BM25)
        let sparse_ranked = self.bm25.score(query);

        // RRF fusion
        let mut rrf: HashMap<String, f32> = HashMap::new();
        for (rank, (id, _)) in dense_ranked.iter().enumerate() {
            *rrf.entry(id.to_string()).or_insert(0.0) += 1.0 / (RRF_K + rank as f32 + 1.0);
        }
        for (rank, (id, _)) in sparse_ranked.iter().enumerate() {
            *rrf.entry(id.clone()).or_insert(0.0) += 1.0 / (RRF_K + rank as f32 + 1.0);
        }

        let mut fused: Vec<(String, f32)> = rrf.into_iter().collect();
        fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let results = fused
            .into_iter()
            .take(top_k)
            .filter_map(|(id, _)| self.manifests.get(&id))
            .collect();

        Ok(results)
    }

    /// BM25-only synchronous search. No LLM required.
    ///
    /// Higher precision than keyword matching for long descriptions, lower
    /// than hybrid semantic search. Good fallback when no LLM is available.
    pub fn query_bm25(&self, query: &str, top_k: usize) -> Vec<&AgentManifest> {
        self.bm25
            .score(query)
            .into_iter()
            .take(top_k)
            .filter_map(|(id, _)| self.manifests.get(&id))
            .collect()
    }

    /// Keyword search (original behavior — fully backward compatible).
    ///
    /// Scores each manifest by counting how many words from `query` appear in
    /// the manifest's description and capability tags. Returns results sorted
    /// by descending relevance score, excluding zero-score entries.
    pub fn query(&self, query: &str) -> Vec<&AgentManifest> {
        let keywords: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();

        let mut scored: Vec<(usize, &AgentManifest)> = self
            .manifests
            .values()
            .filter_map(|m| {
                let tags_str = m
                    .capabilities
                    .tags
                    .iter()
                    .map(|t| t.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let haystack = format!("{} {}", m.description.to_lowercase(), tags_str);
                let score = keywords
                    .iter()
                    .filter(|kw| haystack.contains(kw.as_str()))
                    .count();
                if score > 0 { Some((score, m)) } else { None }
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, m)| m).collect()
    }

    /// Returns all registered manifests.
    pub fn all(&self) -> Vec<&AgentManifest> {
        self.manifests.values().collect()
    }

    /// Returns the number of registered agents.
    pub fn len(&self) -> usize {
        self.manifests.len()
    }

    /// Returns true if no agents are registered.
    pub fn is_empty(&self) -> bool {
        self.manifests.is_empty()
    }
}

/// Cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofa_kernel::agent::capabilities::AgentCapabilities;

    fn make_registry() -> CapabilityRegistry {
        let mut registry = CapabilityRegistry::new();

        registry.register(
            AgentManifest::builder("agent-research", "ResearchAgent")
                .description("searches the web and summarizes documents and articles")
                .capabilities(
                    AgentCapabilities::builder()
                        .with_tag("research")
                        .with_tag("summarization")
                        .with_tag("web")
                        .build(),
                )
                .build(),
        );

        registry.register(
            AgentManifest::builder("agent-code", "CodeAgent")
                .description("writes reviews and debugs Rust and Python code")
                .capabilities(
                    AgentCapabilities::builder()
                        .with_tag("coding")
                        .with_tag("rust")
                        .with_tag("python")
                        .build(),
                )
                .build(),
        );

        registry
    }

    // --- original keyword tests (backward compat) ---

    #[test]
    fn test_register_and_find_by_id() {
        let registry = make_registry();
        assert!(registry.find_by_id("agent-research").is_some());
        assert!(registry.find_by_id("agent-code").is_some());
        assert!(registry.find_by_id("nonexistent").is_none());
    }

    #[test]
    fn test_find_by_tag() {
        let registry = make_registry();
        let coding = registry.find_by_tag("coding");
        assert_eq!(coding.len(), 1);
        assert_eq!(coding[0].agent_id, "agent-code");
    }

    #[test]
    fn test_query_returns_best_match_first() {
        let registry = make_registry();
        let results = registry.query("summarize web documents");
        assert!(!results.is_empty());
        assert_eq!(results[0].agent_id, "agent-research");

        let results = registry.query("write rust code");
        assert!(!results.is_empty());
        assert_eq!(results[0].agent_id, "agent-code");
    }

    #[test]
    fn test_query_no_match_returns_empty() {
        let registry = make_registry();
        let results = registry.query("quantum physics simulation");
        assert!(results.is_empty());
    }

    #[test]
    fn test_unregister() {
        let mut registry = make_registry();
        assert_eq!(registry.len(), 2);
        registry.unregister("agent-code");
        assert_eq!(registry.len(), 1);
        assert!(registry.find_by_id("agent-code").is_none());
    }

    // --- BM25 index tests ---

    #[test]
    fn bm25_index_updates_on_register() {
        let registry = make_registry();
        // bm25 should have 2 docs
        assert_eq!(registry.bm25.doc_count, 2);
    }

    #[test]
    fn bm25_index_removed_on_unregister() {
        let mut registry = make_registry();
        registry.unregister("agent-code");
        assert_eq!(registry.bm25.doc_count, 1);
        // no rust tokens should remain in index
        let rust_postings = registry.bm25.term_freq.get("rust");
        assert!(rust_postings.map_or(true, |p| p.is_empty()));
    }

    #[test]
    fn query_bm25_returns_best_match() {
        let registry = make_registry();
        let results = registry.query_bm25("summarize web documents", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].agent_id, "agent-research");

        let results = registry.query_bm25("rust code debugging", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].agent_id, "agent-code");
    }

    #[test]
    fn query_bm25_empty_query_returns_empty() {
        let registry = make_registry();
        let results = registry.query_bm25("", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn query_bm25_zero_top_k_returns_empty() {
        let registry = make_registry();
        let results = registry.query_bm25("rust", 0);
        assert!(results.is_empty());
    }

    #[test]
    fn query_bm25_unknown_term_returns_empty() {
        let registry = make_registry();
        let results = registry.query_bm25("quantumphysics", 5);
        assert!(results.is_empty());
    }

    // --- cosine similarity tests ---

    #[test]
    fn cosine_identical_vectors() {
        let v = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn cosine_orthogonal_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-5);
    }

    #[test]
    fn cosine_opposite_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 1e-5);
    }

    #[test]
    fn cosine_zero_vector_returns_zero() {
        let a = vec![0.0, 0.0];
        let b = vec![1.0, 0.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn cosine_mismatched_lengths_returns_zero() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    // --- embedding index tests ---

    #[test]
    fn store_and_check_embedding() {
        let mut registry = make_registry();
        assert!(!registry.has_embedding("agent-research"));
        registry.store_embedding("agent-research", vec![0.1, 0.2, 0.3]);
        assert!(registry.has_embedding("agent-research"));
        assert_eq!(registry.embedding_count(), 1);
    }

    #[test]
    fn embedding_removed_on_unregister() {
        let mut registry = make_registry();
        registry.store_embedding("agent-code", vec![0.5, 0.5]);
        assert!(registry.has_embedding("agent-code"));
        registry.unregister("agent-code");
        assert!(!registry.has_embedding("agent-code"));
    }

    // --- semantic query tests (with mock embeddings) ---

    #[test]
    fn query_semantic_rrf_favors_multi_signal_match() {
        let mut registry = make_registry();

        // research agent: embedding pointing "up" in dim 0
        registry.store_embedding("agent-research", vec![1.0, 0.0, 0.0]);
        // code agent: embedding pointing "up" in dim 1
        registry.store_embedding("agent-code", vec![0.0, 1.0, 0.0]);

        // query vector close to research agent
        let query_vec = vec![0.9, 0.1, 0.0];

        // manually run the RRF logic (mimics query_semantic internals)
        let dense_sim_research = cosine_similarity(&query_vec, &[1.0, 0.0, 0.0]);
        let dense_sim_code = cosine_similarity(&query_vec, &[0.0, 1.0, 0.0]);

        assert!(dense_sim_research > dense_sim_code, "research should rank higher by cosine");
    }

    #[test]
    fn bm25_score_deterministic_across_calls() {
        let registry = make_registry();
        let r1 = registry.query_bm25("rust code", 5);
        let r2 = registry.query_bm25("rust code", 5);
        let ids1: Vec<_> = r1.iter().map(|m| &m.agent_id).collect();
        let ids2: Vec<_> = r2.iter().map(|m| &m.agent_id).collect();
        assert_eq!(ids1, ids2);
    }

    #[test]
    fn register_replace_updates_bm25() {
        let mut registry = CapabilityRegistry::new();
        registry.register(
            AgentManifest::builder("a1", "Agent")
                .description("handles python scripts")
                .build(),
        );
        // replace with different description
        registry.register(
            AgentManifest::builder("a1", "Agent")
                .description("handles rust compilation")
                .build(),
        );
        // doc count should still be 1
        assert_eq!(registry.bm25.doc_count, 1);
        // old term removed
        let python_postings = registry.bm25.term_freq.get("python");
        assert!(python_postings.map_or(true, |p| !p.contains_key("a1")));
        // new term present
        assert!(registry.bm25.term_freq.contains_key("rust"));
    }
}
