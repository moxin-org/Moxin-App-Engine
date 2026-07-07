//! Unified Inference Orchestrator.
//!
//! This is the central control plane for inference in MoFA. It composes
//! the routing policy, model pool, and memory scheduler into a single
//! entry point that agents use to request inference without knowing
//! whether execution happens locally or in the cloud.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │           InferenceOrchestrator              │
//! │                                             │
//! │  RoutingPolicy  →  MemoryBudget             │
//! │        ↓                ↓                   │
//! │     ModelPool    →  AdmissionCheck           │
//! │        ↓                ↓                   │
//! │     LocalExec  ←→  CloudFallback            │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! # Phase 1 Scope
//!
//! Phase 1 focuses on deterministic routing and lifecycle control.
//! Precision adaptation (f16→q8→q4 downgrade) and deferred-queue
//! scheduling will be introduced in Phase 2.

use std::time::Duration;

use crate::hardware::{HardwareCapability, detect_hardware};

mod duration_secs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        duration.as_secs().serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

use super::model_pool::ModelPool;
use super::routing::{self, RoutingDecision, RoutingPolicy};
use super::types::{InferenceRequest, InferenceResult, RequestPriority, RoutedBackend};
use crate::scheduler::AdmissionOutcome;

/// Configuration for the `InferenceOrchestrator`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OrchestratorConfig {
    /// Total memory budget for local models (in MB)
    pub memory_capacity_mb: usize,
    /// Fraction of capacity above which new requests are deferred (0.0–1.0)
    pub defer_threshold: f64,
    /// Fraction of capacity above which new requests are rejected (0.0–1.0)
    pub reject_threshold: f64,
    /// Maximum number of models that can be concurrently loaded
    pub model_pool_capacity: usize,
    /// Models idle longer than this duration are candidates for eviction
    #[serde(with = "duration_secs")]
    pub idle_timeout: Duration,
    /// The routing policy governing local vs cloud decisions
    pub routing_policy: RoutingPolicy,
    /// The cloud provider to use for fallback (e.g., "openai")
    pub cloud_provider: String,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            memory_capacity_mb: 16384, // 16 GB
            defer_threshold: 0.75,
            reject_threshold: 0.90,
            model_pool_capacity: 5,
            idle_timeout: Duration::from_secs(300),
            routing_policy: RoutingPolicy::default(),
            cloud_provider: "openai".to_string(),
        }
    }
}

/// The unified inference orchestrator.
///
/// Provides a single entry point (`infer`) for agents to request inference.
/// Internally handles:
/// - Memory-aware admission control
/// - Policy-driven routing (local vs cloud)
/// - LRU model lifecycle management
/// - Automatic cloud failover
///
/// Memory tracking is derived from `ModelPool` as the single source of truth.
/// There is no separate `allocated_mb` counter — this prevents inconsistency.
pub struct InferenceOrchestrator {
    config: OrchestratorConfig,
    model_pool: ModelPool,
    hardware: HardwareCapability,
}

impl InferenceOrchestrator {
    /// Create a new orchestrator with the given configuration.
    ///
    /// Hardware capabilities are auto-detected at construction time.
    /// The memory capacity automatically defaults to the host machine's total unified/VRAM memory.
    pub fn new(mut config: OrchestratorConfig) -> Self {
        let hardware = detect_hardware();

        // Dynamically override memory capacity with actual unified memory (MB)
        if config.memory_capacity_mb == 16384 {
            config.memory_capacity_mb = (hardware.total_memory_bytes / 1_000_000) as usize;
        }

        let model_pool = ModelPool::new(config.model_pool_capacity, config.idle_timeout);

        Self {
            config,
            model_pool,
            hardware,
        }
    }

    /// Create an orchestrator with explicit hardware capabilities (for testing).
    pub fn with_hardware(config: OrchestratorConfig, hardware: HardwareCapability) -> Self {
        let model_pool = ModelPool::new(config.model_pool_capacity, config.idle_timeout);

        Self {
            config,
            model_pool,
            hardware,
        }
    }

    /// The single entry point for inference.
    ///
    /// Agents call this method with an `InferenceRequest`. The orchestrator
    /// evaluates admission, routes to the appropriate backend, manages model
    /// lifecycle, and returns the result.
    ///
    /// In this Phase 1 implementation, actual model execution is simulated.
    /// Real backend integration (MLX, OpenAI) will be wired in Phase 2.
    pub fn infer(&mut self, request: &InferenceRequest) -> InferenceResult {
        // Step 1: Evict idle models to free memory before admission check
        self.model_pool.evict_idle();

        // Step 2: Evaluate admission based on current memory state
        // Memory is always derived from ModelPool (single source of truth)
        let mut admission = self.evaluate_admission(request);

        // Critical requests can reclaim memory via priority-aware eviction before
        // we decide to reject/fallback. This is skipped for CloudOnly policy
        // because local admission does not participate in routing there.
        if request.priority == RequestPriority::Critical
            && admission == AdmissionOutcome::Reject
            && self.config.routing_policy != RoutingPolicy::CloudOnly
        {
            let (post_reclaim_admission, evicted_models) =
                self.attempt_critical_reclamation(request);
            if evicted_models > 0 {
                tracing::warn!(
                    model_id = %request.model_id,
                    evicted_models,
                    "critical request triggered priority-aware reclamation before routing"
                );
            }
            admission = post_reclaim_admission;
        }

        // Step 3: Resolve routing based on policy + admission + hardware
        let decision = routing::resolve(
            &self.config.routing_policy,
            request,
            admission,
            &self.hardware,
            &self.config.cloud_provider,
        );

        // Step 4: Execute based on routing decision
        match &decision {
            RoutingDecision::UseLocal { model_id } => {
                // Load the model if not already loaded
                if !self.model_pool.is_loaded(model_id) {
                    self.model_pool.load(
                        model_id,
                        request.required_memory_mb,
                        request.preferred_precision,
                        request.priority,
                    );
                } else {
                    self.model_pool.touch(model_id);
                }

                InferenceResult {
                    output: format!(
                        "[local:{}] Inference result for: {}",
                        model_id, request.prompt
                    ),
                    routed_to: RoutedBackend::Local {
                        model_id: model_id.clone(),
                    },
                    actual_precision: request.preferred_precision,
                }
            }
            RoutingDecision::UseLocalDegraded {
                model_id,
                degraded_precision,
                quality_warning,
            } => {
                // Estimate degraded memory footprint
                let degraded_memory_mb = ((request.required_memory_mb as f64)
                    * (degraded_precision.bytes_per_param()
                        / request.preferred_precision.bytes_per_param()))
                .ceil() as usize;

                // Load the model at degraded precision
                if !self.model_pool.is_loaded(model_id) {
                    self.model_pool.load(
                        model_id,
                        degraded_memory_mb,
                        *degraded_precision,
                        request.priority,
                    );
                } else {
                    self.model_pool.touch(model_id);
                }

                tracing::warn!(
                    model_id,
                    from = %request.preferred_precision,
                    to = %degraded_precision,
                    "{}",
                    quality_warning
                );

                InferenceResult {
                    output: format!(
                        "[local-degraded:{}@{}] Inference result for: {}",
                        model_id, degraded_precision, request.prompt
                    ),
                    routed_to: RoutedBackend::Local {
                        model_id: model_id.clone(),
                    },
                    actual_precision: *degraded_precision,
                }
            }
            RoutingDecision::UseCloud { provider } => InferenceResult {
                output: format!(
                    "[cloud:{}] Inference result for: {}",
                    provider, request.prompt
                ),
                routed_to: RoutedBackend::Cloud {
                    provider: provider.clone(),
                },
                actual_precision: request.preferred_precision,
            },
            RoutingDecision::Rejected { reason } => InferenceResult {
                output: format!("[rejected] {}", reason),
                routed_to: RoutedBackend::Rejected {
                    reason: reason.clone(),
                },
                actual_precision: request.preferred_precision,
            },
        }
    }

    /// Phase-1 simulated streaming entry point.
    ///
    /// Stream inference result as a [`BoxTokenStream`].
    ///
    /// Each [`StreamChunk`] emitted carries the incremental text delta from the
    /// backend. The final chunk has [`StreamChunk::finish_reason`] set to
    /// [`FinishReason::Stop`].
    ///
    /// # Current implementation note
    ///
    /// Until the local model pool exposes native token-by-token decoding, this
    /// method calls [`infer`] to obtain the full output and then re-emits it
    /// word-by-word as a simulated stream. Real LLM providers accessed via the
    /// cloud fallback path will be wired in Phase 2 to use `chat_stream()`
    /// directly, bypassing the full-output round-trip.
    pub fn infer_stream(
        &mut self,
        request: &InferenceRequest,
    ) -> (InferenceResult, mofa_kernel::llm::streaming::BoxTokenStream) {
        use futures::StreamExt;
        use mofa_kernel::llm::streaming::{BoxTokenStream, StreamChunk, StreamError};
        use mofa_kernel::llm::types::FinishReason;

        // Run full admission/routing logic to get the complete output.
        let base_result = self.infer(request);
        let output_str = base_result.output.clone();

        // Build word-level chunks from the full output string.
        let mut words: Vec<Result<StreamChunk, StreamError>> = output_str
            .split_whitespace()
            .map(|w| Ok(StreamChunk::text(format!("{w} "))))
            .collect();

        // Append a terminal done chunk with finish_reason = Stop.
        words.push(Ok(StreamChunk::done(FinishReason::Stop)));

        let stream: BoxTokenStream = Box::pin(futures::stream::iter(words));
        (base_result, stream)
    }

    /// Compatibility accessor: returns the text-only stream used by legacy callers.
    ///
    /// **Deprecated** — prefer [`infer_stream`] which returns a fully typed
    /// [`BoxTokenStream`]. This method will be removed once all call sites are
    /// updated.
    #[deprecated(note = "Use infer_stream() which returns BoxTokenStream")]
    pub fn infer_stream_text(
        &mut self,
        request: &InferenceRequest,
    ) -> (
        InferenceResult,
        std::pin::Pin<Box<dyn futures::Stream<Item = String> + Send + Sync>>,
    ) {
        let base_result = self.infer(request);
        let output_str = base_result.output.clone();
        let words: Vec<String> = output_str
            .split_whitespace()
            .map(|w| format!("{w} "))
            .collect();
        let stream = futures::stream::iter(words);
        (base_result, Box::pin(stream))
    }

    /// Evaluate whether a local backend can admit this request based on
    /// current memory usage, configured thresholds, and **request priority**.
    ///
    /// # Priority semantics
    ///
    /// - `Low` / `Normal`: standard dual-threshold hysteresis — may return
    ///   [`AdmissionOutcome::Defer`] when usage is in the `[defer, reject)` band.
    /// - `High`: bypasses the Deferred band — admitted directly whenever usage
    ///   is at or below `reject_threshold` (skips the defer zone entirely).
    /// - `Critical`: same bypass as `High`; the caller (orchestrator) is
    ///   responsible for attempting priority-weighted eviction before a final
    ///   rejection.
    ///
    /// Memory is always read from ModelPool — no separate counter to get out of sync.
    fn evaluate_admission(&self, request: &InferenceRequest) -> AdmissionOutcome {
        let current_mb = self.model_pool.total_memory_mb();
        let projected_mb = current_mb + request.required_memory_mb;
        let capacity = self.config.memory_capacity_mb;

        if capacity == 0 {
            return AdmissionOutcome::Reject;
        }

        let projected_usage = projected_mb as f64 / capacity as f64;

        match request.priority {
            // High and Critical bypass the Deferred hysteresis band:
            // they are admitted whenever memory is below the reject ceiling.
            RequestPriority::High | RequestPriority::Critical => {
                if projected_usage <= self.config.reject_threshold {
                    AdmissionOutcome::Accept
                } else {
                    AdmissionOutcome::Reject
                }
            }
            // Low and Normal use standard dual-threshold hysteresis.
            RequestPriority::Low | RequestPriority::Normal => {
                if projected_usage <= self.config.defer_threshold {
                    AdmissionOutcome::Accept
                } else if projected_usage <= self.config.reject_threshold {
                    // Deferred: memory is tight but may be reclaimable via eviction.
                    // Phase 2 will add a queue-based scheduler with retry logic.
                    AdmissionOutcome::Defer
                } else {
                    AdmissionOutcome::Reject
                }
            }
        }
    }

    /// Attempt to reclaim memory for a critical request by evicting
    /// lower-resistance models (priority-aware LRU) before final routing.
    ///
    /// Returns the post-reclamation admission outcome and number of models evicted.
    fn attempt_critical_reclamation(
        &mut self,
        request: &InferenceRequest,
    ) -> (AdmissionOutcome, usize) {
        // Fast-fail: if request cannot fit under reject threshold even on empty pool,
        // do not evict existing models pointlessly.
        if self.config.memory_capacity_mb == 0 {
            return (AdmissionOutcome::Reject, 0);
        }
        let request_usage =
            request.required_memory_mb as f64 / self.config.memory_capacity_mb as f64;
        if request_usage > self.config.reject_threshold {
            return (AdmissionOutcome::Reject, 0);
        }

        let mut evicted_models = 0usize;
        while self.evaluate_admission(request) == AdmissionOutcome::Reject {
            if self
                .model_pool
                .evict_lru_for_priority(request.priority)
                .is_none()
            {
                break;
            }
            evicted_models += 1;
        }

        (self.evaluate_admission(request), evicted_models)
    }

    /// Get the current memory usage as a fraction of total capacity (0.0–1.0).
    pub fn memory_utilization(&self) -> f64 {
        if self.config.memory_capacity_mb == 0 {
            return 1.0;
        }
        self.model_pool.total_memory_mb() as f64 / self.config.memory_capacity_mb as f64
    }

    /// Get the number of currently loaded models.
    pub fn loaded_model_count(&self) -> usize {
        self.model_pool.len()
    }

    /// Get the total allocated memory (in MB), derived from ModelPool.
    pub fn allocated_memory_mb(&self) -> usize {
        self.model_pool.total_memory_mb()
    }

    /// Get a reference to the detected hardware capabilities.
    pub fn hardware(&self) -> &HardwareCapability {
        &self.hardware
    }

    /// Get a reference to the active routing policy.
    pub fn routing_policy(&self) -> &RoutingPolicy {
        &self.config.routing_policy
    }

    /// Manually unload a model from the pool, freeing its memory.
    pub fn unload_model(&mut self, model_id: &str) -> usize {
        self.model_pool.unload(model_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{CpuFamily, GpuType, HardwareCapability, OsClassification};

    fn test_hardware() -> HardwareCapability {
        HardwareCapability {
            os: OsClassification::MacOS,
            cpu_family: CpuFamily::AppleSilicon,
            gpu_available: true,
            gpu_type: Some(GpuType::Metal),
            total_memory_bytes: 32_000_000_000,
            available_memory_bytes: 16_000_000_000,
        }
    }

    fn test_config() -> OrchestratorConfig {
        OrchestratorConfig {
            memory_capacity_mb: 24576, // 24 GB
            defer_threshold: 0.75,
            reject_threshold: 0.90,
            model_pool_capacity: 5,
            idle_timeout: Duration::from_secs(300),
            routing_policy: RoutingPolicy::LocalFirstWithCloudFallback,
            cloud_provider: "openai".to_string(),
        }
    }

    #[test]
    fn test_local_inference_happy_path() {
        let mut orch = InferenceOrchestrator::with_hardware(test_config(), test_hardware());

        let request = InferenceRequest::new("llama-3-7b", "Hello world", 7168);
        let result = orch.infer(&request);

        assert_eq!(
            result.routed_to,
            RoutedBackend::Local {
                model_id: "llama-3-7b".into()
            }
        );
        assert!(result.output.contains("local:llama-3-7b"));
        assert_eq!(orch.loaded_model_count(), 1);
        assert_eq!(orch.allocated_memory_mb(), 7168);
    }

    #[test]
    fn test_cloud_fallback_when_memory_full() {
        let mut config = test_config();
        config.memory_capacity_mb = 24576; // 24 GB
        let mut orch = InferenceOrchestrator::with_hardware(config, test_hardware());

        // Load a model that uses ~54% of capacity (under 75% defer threshold)
        // 13312 / 24576 = 54.2% → Accepted
        let req1 = InferenceRequest::new("llama-3-13b", "First", 13312);
        let result1 = orch.infer(&req1);
        assert_eq!(
            result1.routed_to,
            RoutedBackend::Local {
                model_id: "llama-3-13b".into()
            }
        );

        // Second model: (13312 + 10240) / 24576 = 95.8% → exceeds 90% reject → cloud
        let req2 = InferenceRequest::new("mistral-7b", "Second", 10240);
        let result2 = orch.infer(&req2);
        assert_eq!(
            result2.routed_to,
            RoutedBackend::Cloud {
                provider: "openai".into()
            }
        );
    }

    #[test]
    fn test_local_only_rejects_when_full() {
        let mut config = test_config();
        config.memory_capacity_mb = 16384; // 16 GB
        config.routing_policy = RoutingPolicy::LocalOnly;
        let mut orch = InferenceOrchestrator::with_hardware(config, test_hardware());

        // Fill memory: 10000 / 16384 = 61% → Accepted
        let req1 = InferenceRequest::new("model-a", "test", 10000);
        orch.infer(&req1);

        // Second: (10000 + 6000) / 16384 = 97.6% → Rejected
        // With LocalOnly policy, rejection means no cloud fallback
        let req2 = InferenceRequest::new("model-b", "test", 6000);
        let result = orch.infer(&req2);
        assert!(matches!(result.routed_to, RoutedBackend::Rejected { .. }));
        assert!(result.output.contains("rejected"));
    }

    #[test]
    fn test_memory_utilization_tracking() {
        let mut config = test_config();
        config.memory_capacity_mb = 10000;
        let mut orch = InferenceOrchestrator::with_hardware(config, test_hardware());

        assert_eq!(orch.memory_utilization(), 0.0);

        let req = InferenceRequest::new("model-a", "test", 5000);
        orch.infer(&req);

        assert!((orch.memory_utilization() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_model_unloading_frees_memory() {
        let mut orch = InferenceOrchestrator::with_hardware(test_config(), test_hardware());

        let req = InferenceRequest::new("model-a", "test", 8000);
        orch.infer(&req);
        assert_eq!(orch.allocated_memory_mb(), 8000);

        let freed = orch.unload_model("model-a");
        assert_eq!(freed, 8000);
        assert_eq!(orch.allocated_memory_mb(), 0);
        assert_eq!(orch.loaded_model_count(), 0);
    }

    #[test]
    fn test_cloud_only_always_routes_to_cloud() {
        let mut config = test_config();
        config.routing_policy = RoutingPolicy::CloudOnly;
        let mut orch = InferenceOrchestrator::with_hardware(config, test_hardware());

        let req = InferenceRequest::new("llama-3-7b", "Hello", 7168);
        let result = orch.infer(&req);

        assert_eq!(
            result.routed_to,
            RoutedBackend::Cloud {
                provider: "openai".into()
            }
        );
        // Model should NOT be loaded locally
        assert_eq!(orch.loaded_model_count(), 0);
    }

    #[test]
    fn test_orchestrator_config_serde_roundtrip() {
        let config = OrchestratorConfig {
            memory_capacity_mb: 32768,
            defer_threshold: 0.80,
            reject_threshold: 0.95,
            model_pool_capacity: 10,
            idle_timeout: Duration::from_secs(600),
            routing_policy: RoutingPolicy::CostOptimized,
            cloud_provider: "anthropic".to_string(),
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: OrchestratorConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, config);
    }

    #[test]
    fn test_orchestrator_config_default_serde_roundtrip() {
        let config = OrchestratorConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let back: OrchestratorConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, config);
    }

    #[test]
    fn test_idle_timeout_serializes_as_seconds() {
        let config = OrchestratorConfig {
            idle_timeout: Duration::from_secs(120),
            ..OrchestratorConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["idle_timeout"], serde_json::json!(120));
    }

    // ── Priority-aware admission tests ──────────────────────────────────────────

    /// Normal priority: pre-fill so projected usage is in the [defer, reject) band.
    /// Result should be cloud fallback (Deferred → cloud under LocalFirstWithCloudFallback).
    #[test]
    fn test_normal_priority_deferred_in_defer_band() {
        let mut config = test_config();
        // Use tight thresholds and small capacity for deterministic math.
        config.memory_capacity_mb = 10_000;
        config.defer_threshold = 0.70;
        config.reject_threshold = 0.90;
        let mut orch = InferenceOrchestrator::with_hardware(config, test_hardware());

        // Fill to 65% with a Normal-priority request (admitted locally).
        let fill = InferenceRequest::new("base-model", "warmup", 6_500);
        orch.infer(&fill);
        assert_eq!(orch.allocated_memory_mb(), 6_500);

        // Now project 6500+1000 = 7500 / 10000 = 75% → in [70%, 90%) → Deferred → cloud.
        let req = InferenceRequest::new("extra-model", "batch", 1_000)
            .with_priority(RequestPriority::Normal);
        let result = orch.infer(&req);
        assert_eq!(
            result.routed_to,
            RoutedBackend::Cloud {
                provider: "openai".into()
            },
            "Normal priority in defer band should fall back to cloud"
        );
    }

    /// High priority: same memory conditions as above, but bypass the defer band.
    /// The request should be admitted locally even though usage is in [defer, reject).
    #[test]
    fn test_high_priority_bypasses_defer_band() {
        let mut config = test_config();
        config.memory_capacity_mb = 10_000;
        config.defer_threshold = 0.70;
        config.reject_threshold = 0.90;
        let mut orch = InferenceOrchestrator::with_hardware(config, test_hardware());

        // Fill to 65% (< 70% defer → Accepted locally).
        let fill = InferenceRequest::new("base-model", "warmup", 6_500);
        orch.infer(&fill);
        assert_eq!(orch.allocated_memory_mb(), 6_500);

        // Project 6500+1000 = 7500 / 10000 = 75% → in defer band.
        // High priority bypasses defer → Accepted locally.
        let req = InferenceRequest::new("realtime-model", "urgent", 1_000)
            .with_priority(RequestPriority::High);
        let result = orch.infer(&req);
        assert_eq!(
            result.routed_to,
            RoutedBackend::Local {
                model_id: "realtime-model".into()
            },
            "High priority should bypass defer band and be admitted locally"
        );
    }

    /// Critical priority: same bypass behaviour as High in the defer band.
    #[test]
    fn test_critical_priority_bypasses_defer_band() {
        let mut config = test_config();
        config.memory_capacity_mb = 10_000;
        config.defer_threshold = 0.70;
        config.reject_threshold = 0.90;
        let mut orch = InferenceOrchestrator::with_hardware(config, test_hardware());

        let fill = InferenceRequest::new("base-model", "warmup", 6_500);
        orch.infer(&fill);
        assert_eq!(orch.allocated_memory_mb(), 6_500);

        // Project 75% — in defer band. Critical bypasses → Accepted locally.
        let req = InferenceRequest::new("voice-model", "CRITICAL", 1_000)
            .with_priority(RequestPriority::Critical);
        let result = orch.infer(&req);
        assert_eq!(
            result.routed_to,
            RoutedBackend::Local {
                model_id: "voice-model".into()
            },
            "Critical priority should bypass defer band and be admitted locally"
        );
    }

    /// High priority above the reject ceiling still falls back to cloud.
    /// Priority bypass only applies within [defer, reject); above reject, all priorities fail.
    #[test]
    fn test_high_priority_above_reject_threshold_falls_back_to_cloud() {
        let mut config = test_config();
        config.memory_capacity_mb = 10_000;
        config.defer_threshold = 0.70;
        config.reject_threshold = 0.90;
        let mut orch = InferenceOrchestrator::with_hardware(config, test_hardware());

        let fill = InferenceRequest::new("base-model", "warmup", 6_500);
        orch.infer(&fill);
        assert_eq!(orch.allocated_memory_mb(), 6_500);

        // Project 6500+3000 = 9500 / 10000 = 95% → above 90% reject.
        // Even High priority cannot override rejection → cloud fallback.
        let req = InferenceRequest::new("huge-model", "urgent", 3_000)
            .with_priority(RequestPriority::High);
        let result = orch.infer(&req);
        assert_eq!(
            result.routed_to,
            RoutedBackend::Cloud {
                provider: "openai".into()
            },
            "High priority above reject threshold should fall back to cloud"
        );
    }

    /// Low priority in the defer band behaves identically to Normal (falls back to cloud).
    #[test]
    fn test_low_priority_deferred_same_as_normal() {
        let mut config = test_config();
        config.memory_capacity_mb = 10_000;
        config.defer_threshold = 0.70;
        config.reject_threshold = 0.90;
        let mut orch = InferenceOrchestrator::with_hardware(config, test_hardware());

        let fill = InferenceRequest::new("base-model", "warmup", 6_500);
        orch.infer(&fill);

        let req = InferenceRequest::new("batch-model", "batch job", 1_000)
            .with_priority(RequestPriority::Low);
        let result = orch.infer(&req);
        assert_eq!(
            result.routed_to,
            RoutedBackend::Cloud {
                provider: "openai".into()
            },
            "Low priority in defer band should fall back to cloud"
        );
    }

    #[test]
    fn test_critical_priority_reclaims_memory_before_fallback() {
        let mut config = test_config();
        config.memory_capacity_mb = 10_000;
        config.defer_threshold = 0.70;
        config.reject_threshold = 0.90;
        let mut orch = InferenceOrchestrator::with_hardware(config, test_hardware());

        // 60% usage -> admitted locally and loaded.
        let fill = InferenceRequest::new("base-model", "warmup", 6_000)
            .with_priority(RequestPriority::Low);
        let fill_result = orch.infer(&fill);
        assert!(matches!(fill_result.routed_to, RoutedBackend::Local { .. }));
        assert_eq!(orch.allocated_memory_mb(), 6_000);

        // Projected usage = 100% -> initial reject for Critical.
        // Orchestrator should evict lower-priority model and admit locally.
        let critical = InferenceRequest::new("critical-model", "urgent", 4_000)
            .with_priority(RequestPriority::Critical);
        let result = orch.infer(&critical);
        assert_eq!(
            result.routed_to,
            RoutedBackend::Local {
                model_id: "critical-model".into()
            }
        );
        assert_eq!(orch.loaded_model_count(), 1);
        assert_eq!(orch.allocated_memory_mb(), 4_000);
    }

    #[test]
    fn test_critical_priority_does_not_evict_when_request_cannot_fit_even_empty_pool() {
        let mut config = test_config();
        config.memory_capacity_mb = 10_000;
        config.defer_threshold = 0.70;
        config.reject_threshold = 0.90;
        config.routing_policy = RoutingPolicy::LocalOnly;
        let mut orch = InferenceOrchestrator::with_hardware(config, test_hardware());

        let fill = InferenceRequest::new("base-model", "warmup", 6_000);
        let fill_result = orch.infer(&fill);
        assert!(matches!(fill_result.routed_to, RoutedBackend::Local { .. }));

        // 9_500 / 10_000 = 95% > reject threshold (90%), so impossible even if empty.
        // Existing models should not be evicted.
        let critical = InferenceRequest::new("oversized-critical", "urgent", 9_500)
            .with_priority(RequestPriority::Critical);
        let result = orch.infer(&critical);
        assert!(matches!(result.routed_to, RoutedBackend::Rejected { .. }));

        // base-model should still be present
        assert_eq!(orch.unload_model("base-model"), 6_000);
    }
}
