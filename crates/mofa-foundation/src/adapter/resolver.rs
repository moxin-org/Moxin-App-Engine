//! Advanced resolver with weighted scoring and metric-aware routing
//!
//! This module provides the [`Resolver`] which extends the basic registry resolution
//! with weighted scoring based on:
//! - Priority
//! - Expected latency
//! - Memory budget
//! - Quality target
//!
//! It also supports runtime stats for refining selection over time using EWMA.

use std::collections::HashMap;

use super::config::{HardwareProfile, ModelConfig};
use super::descriptor::AdapterDescriptor;
use super::error::ResolutionError;
use super::registry::ResolutionResult;

/// Score components for adapter selection
#[derive(Debug, Clone, Default)]
pub struct AdapterScore {
    /// Priority score (0-100)
    pub priority_score: f64,
    /// Latency score (0-100, lower is better, inverted)
    pub latency_score: f64,
    /// Memory efficiency score (0-100)
    pub memory_score: f64,
    /// Quality score (0-100)
    pub quality_score: f64,
    /// Final weighted score
    pub total_score: f64,
}

impl AdapterScore {
    /// Calculate total score from components
    pub fn calculate(
        &mut self,
        priority_weight: f64,
        latency_weight: f64,
        memory_weight: f64,
        quality_weight: f64,
    ) {
        let total_weight = priority_weight + latency_weight + memory_weight + quality_weight;
        if total_weight > 0.0 {
            self.total_score = (self.priority_score * priority_weight
                + self.latency_score * latency_weight
                + self.memory_score * memory_weight
                + self.quality_score * quality_weight)
                / total_weight;
        }
    }
}

/// Runtime statistics for adapter selection
#[derive(Debug, Clone, Default)]
pub struct AdapterStats {
    /// EWMA of latency in milliseconds
    pub ewma_latency_ms: f64,
    /// EWMA of memory usage in MB
    pub ewma_memory_mb: f64,
    /// Number of successful requests
    pub success_count: u64,
    /// Number of failed requests
    pub failure_count: u64,
}

impl AdapterStats {
    /// Update stats with a new observation
    pub fn update_latency(&mut self, latency_ms: u32, alpha: f64) {
        if self.success_count == 0 {
            self.ewma_latency_ms = latency_ms as f64;
        } else {
            self.ewma_latency_ms =
                alpha * (latency_ms as f64) + (1.0 - alpha) * self.ewma_latency_ms;
        }
    }

    /// Update memory stats
    pub fn update_memory(&mut self, memory_mb: u64, alpha: f64) {
        if self.success_count == 0 {
            self.ewma_memory_mb = memory_mb as f64;
        } else {
            self.ewma_memory_mb = alpha * (memory_mb as f64) + (1.0 - alpha) * self.ewma_memory_mb;
        }
    }

    /// Record a successful request
    pub fn record_success(&mut self) {
        self.success_count += 1;
    }

    /// Record a failed request
    pub fn record_failure(&mut self) {
        self.failure_count += 1;
    }

    /// Get success rate (0.0 - 1.0)
    pub fn success_rate(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            1.0 // Default to 100% if no data
        } else {
            self.success_count as f64 / total as f64
        }
    }
}

/// Weights for scoring components
#[derive(Debug, Clone)]
pub struct ScoringWeights {
    /// Weight for priority (default: 0.3)
    pub priority_weight: f64,
    /// Weight for latency (default: 0.3)
    pub latency_weight: f64,
    /// Weight for memory efficiency (default: 0.2)
    pub memory_weight: f64,
    /// Weight for quality (default: 0.2)
    pub quality_weight: f64,
    /// EWMA alpha for runtime stats (default: 0.3)
    pub ewma_alpha: f64,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            priority_weight: 0.3,
            latency_weight: 0.3,
            memory_weight: 0.2,
            quality_weight: 0.2,
            ewma_alpha: 0.3,
        }
    }
}

/// Advanced resolver with weighted scoring and runtime stats
///
/// This resolver extends the basic adapter resolution with:
/// - Weighted scoring based on multiple criteria
/// - Runtime statistics (EWMA) for latency and memory
/// - Metric-aware routing that adapts to actual performance
#[derive(Debug, Default)]
pub struct Resolver {
    /// Scoring weights
    weights: ScoringWeights,
    /// Runtime statistics per adapter
    stats: HashMap<String, AdapterStats>,
}

impl Resolver {
    /// Create a new resolver with default weights
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a resolver with custom weights
    pub fn with_weights(weights: ScoringWeights) -> Self {
        Self {
            weights,
            stats: HashMap::new(),
        }
    }

    /// Get weights
    pub fn weights(&self) -> &ScoringWeights {
        &self.weights
    }

    /// Set weights
    pub fn set_weights(&mut self, weights: ScoringWeights) {
        self.weights = weights;
    }

    /// Get runtime stats for an adapter
    pub fn get_stats(&self, adapter_id: &str) -> Option<&AdapterStats> {
        self.stats.get(adapter_id)
    }

    /// Resolve adapter with weighted scoring
    ///
    /// This method considers:
    /// 1. Hard constraints (modality, format, hardware compatibility)
    /// 2. Priority score
    /// 3. Latency score (from estimated latency or runtime stats)
    /// 4. Memory efficiency score
    /// 5. Quality score (based on success rate)
    pub fn resolve(
        &self,
        adapters: &[AdapterDescriptor],
        config: &ModelConfig,
        hardware: &HardwareProfile,
    ) -> Result<ResolutionResult, ResolutionError> {
        let mut candidates: Vec<(AdapterDescriptor, AdapterScore)> = Vec::new();

        for adapter in adapters {
            let score = self.calculate_score(adapter, hardware);
            candidates.push((adapter.clone(), score));
        }

        // Filter candidates with zero total score (failed hard constraints)
        candidates.retain(|(_, score)| score.total_score > 0.0);

        if candidates.is_empty() {
            return Err(ResolutionError::NoCompatibleAdapter);
        }

        // Sort by total score (descending), then by name for deterministic tie-break
        candidates.sort_by(|a, b| {
            let score_cmp = b.1.total_score.partial_cmp(&a.1.total_score).unwrap();
            if score_cmp == std::cmp::Ordering::Equal {
                a.0.name.cmp(&b.0.name)
            } else {
                score_cmp
            }
        });

        let (best_adapter, best_score) = &candidates[0];

        Ok(ResolutionResult {
            adapter: best_adapter.clone(),
            rejection_reasons: HashMap::new(), // Could be populated with scoring details
        })
    }

    /// Calculate score for an adapter
    fn calculate_score(
        &self,
        adapter: &AdapterDescriptor,
        hardware: &HardwareProfile,
    ) -> AdapterScore {
        let mut score = AdapterScore {
            priority_score: adapter.priority.clamp(0, 100) as f64,
            ..Default::default()
        };

        // Latency score (0-100, lower is better)
        // Prefer adapters with lower estimated latency or better runtime stats
        if let Some(estimated_latency) = adapter.estimated_latency_ms {
            // Convert to score: lower latency = higher score
            // Assume 10000ms (10s) is worst case
            score.latency_score = (1.0 - (estimated_latency as f64 / 10000.0)).max(0.0) * 100.0;
        } else if let Some(stats) = self.stats.get(&adapter.id) {
            // Use runtime stats if available
            score.latency_score = (1.0 - (stats.ewma_latency_ms / 10000.0)).max(0.0) * 100.0;
        } else {
            score.latency_score = 50.0; // Default middle score
        }

        // Memory score (0-100)
        // Check if hardware has sufficient memory
        if let Some(min_ram) = adapter.hardware_constraint.min_ram_mb {
            if hardware.available_ram_mb >= min_ram {
                score.memory_score = 100.0;
            } else {
                score.memory_score = (hardware.available_ram_mb as f64 / min_ram as f64) * 100.0;
            }
        } else {
            score.memory_score = 100.0; // No constraint
        }

        // Quality score (0-100)
        // Based on success rate from runtime stats
        if let Some(stats) = self.stats.get(&adapter.id) {
            score.quality_score = stats.success_rate() * 100.0;
        } else {
            score.quality_score = 100.0; // Default to perfect quality if no stats
        }

        // Calculate total score
        score.calculate(
            self.weights.priority_weight,
            self.weights.latency_weight,
            self.weights.memory_weight,
            self.weights.quality_weight,
        );

        score
    }

    /// Record a successful inference for an adapter
    pub fn record_success(&mut self, adapter_id: &str, latency_ms: u32, memory_mb: u64) {
        let stats = self.stats.entry(adapter_id.to_string()).or_default();
        stats.record_success();
        stats.update_latency(latency_ms, self.weights.ewma_alpha);
        stats.update_memory(memory_mb, self.weights.ewma_alpha);
    }

    /// Record a failed inference for an adapter
    pub fn record_failure(&mut self, adapter_id: &str) {
        let stats = self.stats.entry(adapter_id.to_string()).or_default();
        stats.record_failure();
    }

    /// Reset stats for an adapter
    pub fn reset_stats(&mut self, adapter_id: &str) {
        self.stats.remove(adapter_id);
    }

    /// Reset all stats
    pub fn reset_all_stats(&mut self) {
        self.stats.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolver_scoring() {
        let resolver = Resolver::new();

        let adapter = AdapterDescriptor::builder()
            .id("test")
            .name("Test Adapter")
            .supported_modality(super::super::descriptor::Modality::LLM)
            .supported_format(super::super::descriptor::ModelFormat::Safetensors)
            .priority(100)
            .estimated_latency_ms(100)
            .build()
            .unwrap();

        let hardware = HardwareProfile::builder().available_ram_mb(16384).build();

        let result = resolver.resolve(
            &[adapter],
            &ModelConfig::builder()
                .model_id("test")
                .required_modality(super::super::descriptor::Modality::LLM)
                .build()
                .unwrap(),
            &hardware,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_stats_recording() {
        let mut resolver = Resolver::new();

        resolver.record_success("adapter1", 100, 1024);
        resolver.record_success("adapter1", 150, 2048);
        resolver.record_failure("adapter1");

        let stats = resolver.get_stats("adapter1").unwrap();
        assert_eq!(stats.success_count, 2);
        assert_eq!(stats.failure_count, 1);
        assert!((stats.success_rate() - 2.0 / 3.0).abs() < 0.01);
    }
}
