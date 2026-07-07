use anyhow::Result;
use instant_distance::{Builder, HnswMap, Point, Search};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

/// A stored semantic fact with importance score.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fact {
    pub id: String,
    pub content: String,
    pub importance: f32,
}

/// A floating-point embedding vector, implementing the `instant_distance::Point` trait.
#[derive(Clone)]
struct EmbeddingPoint(Vec<f32>);

impl Point for EmbeddingPoint {
    fn distance(&self, other: &Self) -> f32 {
        // Cosine distance: 1 − cos(θ)
        let dot: f32 = self.0.iter().zip(&other.0).map(|(a, b)| a * b).sum();
        let norm_a: f32 = self.0.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = other.0.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            1.0
        } else {
            1.0 - dot / (norm_a * norm_b)
        }
    }
}

/// Semantic memory backed by an in-memory HNSW index.
///
/// Facts are embedded via an OpenAI-compatible `/v1/embeddings` endpoint
/// and searched with approximate nearest-neighbour lookup (<100ms p99 at 10k entries).
pub struct SemanticMemory {
    index: Arc<RwLock<Option<HnswMap<EmbeddingPoint, usize>>>>,
    facts: Arc<RwLock<Vec<Fact>>>,
    embeddings: Arc<RwLock<Vec<EmbeddingPoint>>>,
    api_base: String,
    api_key: String,
    client: Client,
}

impl SemanticMemory {
    pub fn new(api_base: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            index: Arc::new(RwLock::new(None)),
            facts: Arc::new(RwLock::new(Vec::new())),
            embeddings: Arc::new(RwLock::new(Vec::new())),
            api_base: api_base.into(),
            api_key: api_key.into(),
            client: Client::new(),
        }
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let resp: serde_json::Value = self
            .client
            .post(format!("{}/embeddings", self.api_base))
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({
                "input": text,
                "model": "text-embedding-3-small"
            }))
            .send()
            .await?
            .json()
            .await?;
        let emb: Vec<f32> =
            serde_json::from_value(resp["data"][0]["embedding"].clone())?;
        Ok(emb)
    }

    fn rebuild_index(&self) {
        let embeddings = self.embeddings.read().unwrap();
        let n = embeddings.len();
        if n == 0 {
            return;
        }
        let values: Vec<usize> = (0..n).collect();
        let built = Builder::default().build(embeddings.clone(), values);
        *self.index.write().unwrap() = Some(built);
    }

    pub async fn insert(&self, content: String, importance: f32) -> Result<()> {
        let embedding = self.embed(&content).await?;
        let point = EmbeddingPoint(embedding);
        {
            let mut facts = self.facts.write().unwrap();
            let mut embeddings = self.embeddings.write().unwrap();
            facts.push(Fact {
                id: uuid::Uuid::new_v4().to_string(),
                content,
                importance,
            });
            embeddings.push(point);
        }
        self.rebuild_index();
        Ok(())
    }

    pub async fn search(&self, query: &str, top_k: usize) -> Result<Vec<Fact>> {
        let query_emb = self.embed(query).await?;
        let query_point = EmbeddingPoint(query_emb);
        let index = self.index.read().unwrap();
        let Some(hnsw) = index.as_ref() else {
            return Ok(vec![]);
        };
        let facts = self.facts.read().unwrap();
        let mut search = Search::default();
        let results: Vec<Fact> = hnsw
            .search(&query_point, &mut search)
            .take(top_k)
            .map(|item| facts[*item.value].clone())
            .collect();
        Ok(results)
    }

    /// Insert a pre-computed embedding (for tests/stubs that don't call the API).
    /// Does NOT rebuild the index — call `finalize_index()` after bulk inserts.
    pub fn insert_with_embedding(&self, content: String, importance: f32, embedding: Vec<f32>) {
        let point = EmbeddingPoint(embedding);
        let mut facts = self.facts.write().unwrap();
        let mut embeddings = self.embeddings.write().unwrap();
        facts.push(Fact {
            id: uuid::Uuid::new_v4().to_string(),
            content,
            importance,
        });
        embeddings.push(point);
    }

    /// Build the HNSW index from all embeddings inserted so far.
    /// Call this once after bulk inserts via `insert_with_embedding`.
    pub fn finalize_index(&self) {
        self.rebuild_index();
    }

    /// Search using a pre-computed query embedding (for tests/stubs).
    pub fn search_with_embedding(&self, query_emb: Vec<f32>, top_k: usize) -> Vec<Fact> {
        let query_point = EmbeddingPoint(query_emb);
        let index = self.index.read().unwrap();
        let Some(hnsw) = index.as_ref() else {
            return vec![];
        };
        let facts = self.facts.read().unwrap();
        let mut search = Search::default();
        hnsw.search(&query_point, &mut search)
            .take(top_k)
            .map(|item| facts[*item.value].clone())
            .collect()
    }
}
