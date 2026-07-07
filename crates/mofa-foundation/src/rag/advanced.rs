use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchConfig {
    pub dense_weight: f64,
    pub sparse_weight: f64,
    pub fusion_method: FusionMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FusionMethod {
    RRF, // Reciprocal Rank Fusion
    WeightedSum,
    Coherence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankConfig {
    pub model_name: String,
    pub top_k: usize,
    pub batch_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptiveChunkingConfig {
    pub min_chunk_size: usize,
    pub max_chunk_size: usize,
    pub overlap_tokens: usize,
    pub preserve_structure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryExpansionConfig {
    pub enabled: bool,
    pub expansion_rounds: usize,
    pub include_original: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiVectorConfig {
    pub representations: Vec<RepresentationType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RepresentationType {
    Full,
    Summary,
    Keywords,
    Entities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedRagConfig {
    pub hybrid_search: Option<HybridSearchConfig>,
    pub reranking: Option<RerankConfig>,
    pub adaptive_chunking: Option<AdaptiveChunkingConfig>,
    pub query_expansion: Option<QueryExpansionConfig>,
    pub multi_vector: Option<MultiVectorConfig>,
}

impl Default for HybridSearchConfig {
    fn default() -> Self {
        Self {
            dense_weight: 0.5,
            sparse_weight: 0.5,
            fusion_method: FusionMethod::RRF,
        }
    }
}

impl Default for RerankConfig {
    fn default() -> Self {
        Self {
            model_name: "cross-encoder/ms-marco-MiniLM-L-6-v2".to_string(),
            top_k: 10,
            batch_size: 32,
        }
    }
}

impl Default for AdaptiveChunkingConfig {
    fn default() -> Self {
        Self {
            min_chunk_size: 100,
            max_chunk_size: 1000,
            overlap_tokens: 50,
            preserve_structure: true,
        }
    }
}

impl Default for QueryExpansionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            expansion_rounds: 2,
            include_original: true,
        }
    }
}

impl Default for MultiVectorConfig {
    fn default() -> Self {
        Self {
            representations: vec![RepresentationType::Full, RepresentationType::Summary],
        }
    }
}

impl Default for AdvancedRagConfig {
    fn default() -> Self {
        Self {
            hybrid_search: Some(HybridSearchConfig::default()),
            reranking: Some(RerankConfig::default()),
            adaptive_chunking: Some(AdaptiveChunkingConfig::default()),
            query_expansion: Some(QueryExpansionConfig::default()),
            multi_vector: Some(MultiVectorConfig::default()),
        }
    }
}
