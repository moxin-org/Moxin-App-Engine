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
use crate::scheduler::{AdmissionOutcome, MemoryBudget, MemoryPolicy, MemoryScheduler};
use super::routing::{self, RoutingDecision, RoutingPolicy};
use super::types::{InferenceRequest, InferenceResult, RequestPriority, RoutedBackend};

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
/// - Memory-aware admission control via `MemoryScheduler`
/// - Policy-driven routing (local vs cloud)
/// - LRU model lifecycle management
/// - Deferred queue with age-aware fairness for memory-constrained requests
/// - Automatic cloud failover
pub struct InferenceOrchestrator {
    config: OrchestratorConfig,
    model_pool: ModelPool,
    hardware: HardwareCapability,
    /// Memory-budgeted scheduler: admission control, deferred queue, stability control.
    scheduler: MemoryScheduler,
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
        let scheduler = Self::build_scheduler(&config);

        Self {
            config,
            model_pool,
            hardware,
            scheduler,
        }
    }

    /// Create an orchestrator with explicit hardware capabilities (for testing).
    pub fn with_hardware(config: OrchestratorConfig, hardware: HardwareCapability) -> Self {
        let model_pool = ModelPool::new(config.model_pool_capacity, config.idle_timeout);
        let scheduler = Self::build_scheduler(&config);

        Self {
            config,
            model_pool,
            hardware,
            scheduler,
        }
    }

    /// Build a `MemoryScheduler` from the orchestrator configuration.
    fn build_scheduler(config: &OrchestratorConfig) -> MemoryScheduler {
        let policy = MemoryPolicy::new(
            config.memory_capacity_mb as u64,
            config.defer_threshold,
            config.reject_threshold,
        );
        let budget = MemoryBudget::new(config.memory_capacity_mb as u64);
        MemoryScheduler::new(policy, budget)
    }

    /// The single entry point for inference.
    ///
    /// Agents call this method with an `InferenceRequest`. The orchestrator
    /// evaluates admission via `MemoryScheduler`, routes to the appropriate
    /// backend, manages model lifecycle, and returns the result.
    ///
    /// # Admission flow
    ///
    /// - `Accept` — routes to local or cloud based on policy; allocates scheduler budget.
    /// - `Defer` — enqueues the request in the scheduler's deferred queue and returns
    ///   immediately. The request will be retried when memory is freed (see `unload_model`).
    /// - `Reject` — routes based on policy (cloud fallback or hard rejection).
    ///
    /// In this Phase 1 implementation, actual model execution is simulated.
    /// Real backend integration (MLX, OpenAI) will be wired in Phase 2.
    pub fn infer(&mut self, request: &InferenceRequest) -> InferenceResult {
        // Step 1: Evict idle models to free memory before admission check
        self.model_pool.evict_idle();

        // Step 2: Evaluate admission via MemoryScheduler.
        // High/Critical priority requests bypass the defer band and are admitted
        // whenever projected usage is at or below the reject threshold.
        let decision_meta = self.scheduler.evaluate(request.required_memory_mb as u64);
        let admission = match request.priority {
            RequestPriority::High | RequestPriority::Critical => {
                // Bypass defer band: treat Defer as Accept for high-priority requests
                match decision_meta.outcome {
                    AdmissionOutcome::Defer => AdmissionOutcome::Accept,
                    other => other,
                }
            }
            _ => decision_meta.outcome,
        };

        // Step 3: Handle Defer before routing — enqueue and return early.
        // This replaces the old behaviour of silently sending deferred requests to cloud.
        if admission == AdmissionOutcome::Defer {
            self.scheduler
                .defer(&request.model_id, request.required_memory_mb as u64);
            return InferenceResult {
                output: format!(
                    "[deferred] '{}' queued — memory pressure at {:.0}% ({} MB used). \
                     Will retry when memory is freed.",
                    request.model_id,
                    self.scheduler.usage_percent(),
                    self.scheduler.used_mb(),
                ),
                routed_to: RoutedBackend::Rejected {
                    reason: format!("deferred: {}", decision_meta.reason),
                },
                actual_precision: request.preferred_precision,
            };
        }

        // Step 4: Resolve routing based on policy + admission + hardware
        let routing_decision = routing::resolve(
            &self.config.routing_policy,
            request,
            admission,
            &self.hardware,
            &self.config.cloud_provider,
        );

        // Step 5: Execute based on routing decision
        match &routing_decision {
            RoutingDecision::UseLocal { model_id } => {
                // Load the model if not already loaded; allocate scheduler budget on new load.
                if !self.model_pool.is_loaded(model_id) {
                    self.model_pool.load(
                        model_id,
                        request.required_memory_mb,
                        request.preferred_precision,
                        request.priority,
                    );
                    self.scheduler.allocate(request.required_memory_mb as u64);
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


    /// Get the current memory usage as a fraction of total capacity (0.0–1.0).
    pub fn memory_utilization(&self) -> f64 {
        if self.config.memory_capacity_mb == 0 {
            return 1.0;
        }
        self.scheduler.usage_percent() / 100.0
    }

    /// Get the number of currently loaded models.
    pub fn loaded_model_count(&self) -> usize {
        self.model_pool.len()
    }

    /// Get the total allocated memory (in MB), tracked by the scheduler.
    pub fn allocated_memory_mb(&self) -> usize {
        self.scheduler.used_mb() as usize
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
    ///
    /// After releasing memory, the scheduler's deferred queue is checked so
    /// that any queued request that now fits can be returned to the caller
    /// for retry. Returns the freed memory in MB.
    pub fn unload_model(&mut self, model_id: &str) -> usize {
        let freed = self.model_pool.unload(model_id);
        if freed > 0 {
            self.scheduler.release(freed as u64);
            // Give a waiting deferred request the chance to proceed
            let _ = self.scheduler.try_dequeue();
        }
        freed
    }

    /// Return the next deferred request that fits in currently available memory,
    /// if any. Callers can use this to retry a previously deferred inference request.
    pub fn pop_deferred(&mut self) -> Option<crate::scheduler::DeferredRequest> {
        self.scheduler.try_dequeue()
    }

    /// Number of requests currently waiting in the deferred queue.
    pub fn deferred_count(&self) -> usize {
        self.scheduler.deferred_count()
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
    /// The request is now enqueued in the deferred queue instead of going to cloud.
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

        // Now project 6500+1000 = 7500 / 10000 = 75% → in [70%, 90%) → Defer.
        // Under the new behaviour the request is queued, not sent to cloud.
        let req = InferenceRequest::new("extra-model", "batch", 1_000)
            .with_priority(RequestPriority::Normal);
        let result = orch.infer(&req);
        assert!(
            matches!(result.routed_to, RoutedBackend::Rejected { ref reason } if reason.contains("deferred")),
            "Normal priority in defer band should be enqueued, not cloud-routed"
        );
        assert_eq!(orch.deferred_count(), 1, "Request should be in the deferred queue");
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

    /// Low priority in the defer band: enqueued like Normal (not cloud-routed).
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
        assert!(
            matches!(result.routed_to, RoutedBackend::Rejected { ref reason } if reason.contains("deferred")),
            "Low priority in defer band should be enqueued, not cloud-routed"
        );
        assert_eq!(orch.deferred_count(), 1);
    }

    // ── MemoryScheduler integration tests ───────────────────────────────────────

    /// Verify that a deferred request increments the scheduler's deferred queue.
    #[test]
    fn test_defer_enqueues_to_scheduler() {
        let mut config = test_config();
        config.memory_capacity_mb = 10_000;
        config.defer_threshold = 0.70;
        config.reject_threshold = 0.90;
        let mut orch = InferenceOrchestrator::with_hardware(config, test_hardware());

        // Fill to 65% — next request will land in the defer band.
        orch.infer(&InferenceRequest::new("filler", "fill", 6_500));
        assert_eq!(orch.deferred_count(), 0);

        // 6500+1000 = 75% > 70% defer threshold → Defer
        orch.infer(
            &InferenceRequest::new("pending", "wait", 1_000).with_priority(RequestPriority::Low),
        );
        assert_eq!(orch.deferred_count(), 1, "One request should be in the deferred queue");

        // 6500+1500 = 80% > 70% → also Defer (scheduler budget still 6500 since deferred never allocates)
        orch.infer(
            &InferenceRequest::new("pending2", "wait2", 1_500)
                .with_priority(RequestPriority::Normal),
        );
        assert_eq!(orch.deferred_count(), 2, "Two requests should now be in the deferred queue");
    }

    /// Verify that unloading a model releases scheduler budget.
    #[test]
    fn test_release_after_unload_updates_scheduler() {
        let mut orch = InferenceOrchestrator::with_hardware(test_config(), test_hardware());

        let req = InferenceRequest::new("model-a", "test", 8_000);
        orch.infer(&req);
        assert_eq!(orch.allocated_memory_mb(), 8_000);

        let freed = orch.unload_model("model-a");
        assert_eq!(freed, 8_000);
        assert_eq!(
            orch.allocated_memory_mb(),
            0,
            "Scheduler budget should be zero after unload"
        );
    }

    /// Full deferred-queue round-trip: load model A (fills memory), defer model B,
    /// unload A — the internal try_dequeue() fires and clears model B from the queue.
    #[test]
    fn test_dequeue_after_release() {
        let mut config = test_config();
        config.memory_capacity_mb = 10_000;
        config.defer_threshold = 0.70;
        config.reject_threshold = 0.90;
        let mut orch = InferenceOrchestrator::with_hardware(config, test_hardware());

        // Load model-a: 6500 / 10000 = 65% → Accepted
        orch.infer(&InferenceRequest::new("model-a", "run", 6_500));
        assert_eq!(orch.allocated_memory_mb(), 6_500);

        // model-b: (6500 + 1000) / 10000 = 75% → Defer → enqueued
        let defer_result = orch.infer(
            &InferenceRequest::new("model-b", "pending", 1_000)
                .with_priority(RequestPriority::Normal),
        );
        assert!(
            matches!(defer_result.routed_to, RoutedBackend::Rejected { ref reason } if reason.contains("deferred")),
            "model-b should be deferred"
        );
        assert_eq!(orch.deferred_count(), 1, "One request in the deferred queue");

        // Unload model-a: frees 6500 MB → scheduler.release() + internal try_dequeue()
        // try_dequeue() removes model-b (1000 MB) from the queue since it now fits.
        orch.unload_model("model-a");
        assert_eq!(orch.allocated_memory_mb(), 0, "Scheduler budget should be 0 after unload");
        assert_eq!(
            orch.deferred_count(),
            0,
            "try_dequeue() inside unload_model should have consumed the deferred request"
        );

        // Queue is already empty — pop_deferred returns None
        assert!(orch.pop_deferred().is_none(), "Queue should be empty after automatic dequeue");
    }
}
