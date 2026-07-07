//! Shared types for the unified inference orchestration layer.
//!
//! These types define the request/response contract that agents use
//! to interact with the `InferenceOrchestrator`, regardless of whether
//! inference runs on a local backend or a cloud provider.

use std::fmt;

/// Priority level for an inference request.
///
/// Controls how the [`InferenceOrchestrator`] handles the request under memory
/// pressure. **All levels are actively enforced** by admission control:
///
/// | Level | Admission behaviour |
/// |-------|---------------------|
/// | `Low` / `Normal` | Standard dual-threshold hysteresis — may be **Deferred** when usage is between `defer_threshold` (0.75) and `reject_threshold` (0.90). |
/// | `High` | Bypasses the Deferred band — admitted directly if usage ≤ `reject_threshold`. |
/// | `Critical` | Same as `High`; additionally triggers priority-weighted LRU eviction to free space before rejecting. |
///
/// # Eviction ordering
///
/// When `evict_lru()` is invoked to free space for a new model load, models
/// that were loaded for **lower-priority** sessions are candidates for eviction
/// before models loaded for **higher-priority** sessions, regardless of recency.
///
/// [`InferenceOrchestrator`]: crate::inference::orchestrator::InferenceOrchestrator
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum RequestPriority {
    /// Background tasks and batch processing.
    ///
    /// Requests may be Deferred or evicted first under memory pressure.
    Low,
    /// Default priority for interactive requests.
    ///
    /// Subject to standard dual-threshold hysteresis admission control.
    #[default]
    Normal,
    /// Latency-sensitive requests (e.g., real-time chat, voice pipelines).
    ///
    /// Bypasses the Deferred band — admitted if memory usage ≤ `reject_threshold`.
    High,
    /// System-critical requests that must never be unnecessarily deferred.
    ///
    /// Bypasses the Deferred band and triggers priority-weighted eviction to
    /// free space before falling back to cloud or rejecting outright.
    Critical,
}

impl fmt::Display for RequestPriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Normal => write!(f, "normal"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

/// Model precision / quantization level.
///
/// Ordered from highest quality (most memory) to lowest quality (least memory).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Precision {
    F32,
    F16,
    Q8,
    Q4,
}

impl fmt::Display for Precision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::F32 => write!(f, "f32"),
            Self::F16 => write!(f, "f16"),
            Self::Q8 => write!(f, "q8"),
            Self::Q4 => write!(f, "q4"),
        }
    }
}

impl Precision {
    /// Returns the next lower precision level, or `None` if already at
    /// maximum compression (Q4).
    ///
    /// Degradation order: `F32 → F16 → Q8 → Q4`
    pub fn next_lower(&self) -> Option<Precision> {
        match self {
            Precision::F32 => Some(Precision::F16),
            Precision::F16 => Some(Precision::Q8),
            Precision::Q8 => Some(Precision::Q4),
            Precision::Q4 => None, // Already at maximum compression
        }
    }

    /// Approximate bytes per parameter at this precision level.
    ///
    /// Used by the degradation ladder to estimate memory footprint at
    /// different quantization levels.
    pub fn bytes_per_param(&self) -> f64 {
        match self {
            Precision::F32 => 4.0,
            Precision::F16 => 2.0,
            Precision::Q8 => 1.0,
            Precision::Q4 => 0.5,
        }
    }
}

/// An inference request submitted by an agent.
///
/// Agents construct this and pass it to `InferenceOrchestrator::infer()`.
/// The orchestrator handles all backend selection, admission control,
/// and failover logic transparently.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InferenceRequest {
    /// Identifier of the model to use (e.g., "llama-3-13b", "gpt-4")
    pub model_id: String,
    /// The prompt or input text for inference
    pub prompt: String,
    /// Estimated memory required to load this model (in MB)
    pub required_memory_mb: usize,
    /// Request priority for admission control.
    ///
    /// Determines how the orchestrator handles this request under memory pressure.
    /// See [`RequestPriority`] for detailed semantics of each level.
    pub priority: RequestPriority,
    /// Preferred precision level (orchestrator may downgrade under pressure)
    pub preferred_precision: Precision,
    /// Optional cap for completion length.
    ///
    /// This is forwarded from API-level request parameters so downstream
    /// providers can enforce generation limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Optional sampling temperature.
    ///
    /// This is forwarded from API-level request parameters so downstream
    /// providers can control generation stochasticity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

impl InferenceRequest {
    /// Create a new inference request with default priority and precision.
    pub fn new(model_id: impl Into<String>, prompt: impl Into<String>, memory_mb: usize) -> Self {
        Self {
            model_id: model_id.into(),
            prompt: prompt.into(),
            required_memory_mb: memory_mb,
            priority: RequestPriority::default(),
            preferred_precision: Precision::F16,
            max_tokens: None,
            temperature: None,
        }
    }

    /// Set the request priority.
    pub fn with_priority(mut self, priority: RequestPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Set the preferred precision.
    pub fn with_precision(mut self, precision: Precision) -> Self {
        self.preferred_precision = precision;
        self
    }

    /// Set maximum generated tokens.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Set sampling temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }
}

/// The result of an inference request after orchestration.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InferenceResult {
    /// The generated output text
    pub output: String,
    /// Where the inference actually ran
    pub routed_to: RoutedBackend,
    /// The actual precision used (may differ from requested if downgraded)
    pub actual_precision: Precision,
}

/// Describes where an inference request was actually executed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RoutedBackend {
    /// Ran on a local backend (e.g., MLX, Candle)
    Local { model_id: String },
    /// Ran on a cloud provider (e.g., OpenAI, Anthropic)
    Cloud { provider: String },
    /// Request was rejected by all backends under current policy
    Rejected { reason: String },
}

impl fmt::Display for RoutedBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local { model_id } => write!(f, "local({})", model_id),
            Self::Cloud { provider } => write!(f, "cloud({})", provider),
            Self::Rejected { reason } => write!(f, "rejected({})", reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_builder() {
        let req = InferenceRequest::new("llama-3-13b", "Hello world", 13312)
            .with_priority(RequestPriority::High)
            .with_precision(Precision::Q8)
            .with_max_tokens(256)
            .with_temperature(0.7);

        assert_eq!(req.model_id, "llama-3-13b");
        assert_eq!(req.required_memory_mb, 13312);
        assert_eq!(req.priority, RequestPriority::High);
        assert_eq!(req.preferred_precision, Precision::Q8);
        assert_eq!(req.max_tokens, Some(256));
        assert_eq!(req.temperature, Some(0.7));
    }

    #[test]
    fn test_priority_ordering() {
        assert!(RequestPriority::Critical > RequestPriority::High);
        assert!(RequestPriority::High > RequestPriority::Normal);
        assert!(RequestPriority::Normal > RequestPriority::Low);
    }

    #[test]
    fn test_request_priority_display() {
        assert_eq!(format!("{}", RequestPriority::Low), "low");
        assert_eq!(format!("{}", RequestPriority::Normal), "normal");
        assert_eq!(format!("{}", RequestPriority::High), "high");
        assert_eq!(format!("{}", RequestPriority::Critical), "critical");
    }

    #[test]
    fn test_precision_display() {
        assert_eq!(format!("{}", Precision::F32), "f32");
        assert_eq!(format!("{}", Precision::F16), "f16");
        assert_eq!(format!("{}", Precision::Q8), "q8");
        assert_eq!(format!("{}", Precision::Q4), "q4");
    }

    #[test]
    fn test_request_priority_serde_roundtrip() {
        for variant in [
            RequestPriority::Low,
            RequestPriority::Normal,
            RequestPriority::High,
            RequestPriority::Critical,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: RequestPriority = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn test_precision_serde_roundtrip() {
        for variant in [Precision::F32, Precision::F16, Precision::Q8, Precision::Q4] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: Precision = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn test_routed_backend_serde_roundtrip() {
        let variants = vec![
            RoutedBackend::Local {
                model_id: "llama-3".into(),
            },
            RoutedBackend::Cloud {
                provider: "openai".into(),
            },
            RoutedBackend::Rejected {
                reason: "no capacity".into(),
            },
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).unwrap();
            let back: RoutedBackend = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn test_inference_request_serde_roundtrip() {
        let req = InferenceRequest::new("llama-3-13b", "Hello world", 13312)
            .with_priority(RequestPriority::High)
            .with_precision(Precision::Q8)
            .with_max_tokens(128)
            .with_temperature(0.4);
        let json = serde_json::to_string(&req).unwrap();
        let back: InferenceRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn test_inference_request_deserialize_without_generation_fields() {
        let json = r#"{
            "model_id":"m",
            "prompt":"p",
            "required_memory_mb":1024,
            "priority":"Normal",
            "preferred_precision":"F16"
        }"#;
        let req: InferenceRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.max_tokens, None);
        assert_eq!(req.temperature, None);
    }

    #[test]
    fn test_inference_result_serde_roundtrip() {
        let result = InferenceResult {
            output: "Hello from local".into(),
            routed_to: RoutedBackend::Local {
                model_id: "llama-3".into(),
            },
            actual_precision: Precision::F16,
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: InferenceResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, result);
    }

    #[test]
    fn test_routed_backend_display() {
        let local = RoutedBackend::Local {
            model_id: "llama-3".into(),
        };
        let cloud = RoutedBackend::Cloud {
            provider: "openai".into(),
        };
        let rejected = RoutedBackend::Rejected {
            reason: "no capacity".into(),
        };
        assert_eq!(format!("{}", local), "local(llama-3)");
        assert_eq!(format!("{}", cloud), "cloud(openai)");
        assert_eq!(format!("{}", rejected), "rejected(no capacity)");
    }
}
