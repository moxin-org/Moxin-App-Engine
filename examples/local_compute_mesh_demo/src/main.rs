//! Local Compute Mesh Demo
//!
//! Demonstrates a working inference pipeline with streaming from local provider.
//! Includes routing, workflow execution, and detailed trace output.
//!
//! Run with: cargo run -p local_compute_mesh_demo

use futures::StreamExt;
use mofa_foundation::orchestrator::traits::ModelProvider;
use mofa_local_llm::config::LinuxInferenceConfig;
use mofa_local_llm::provider::LinuxLocalProvider;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

// ============================================================================
// Routing Policy
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoutingPolicy {
    LocalFirstWithCloudFallback,
    CloudOnly,
    LocalOnly,
}

impl Default for RoutingPolicy {
    fn default() -> Self {
        RoutingPolicy::LocalFirstWithCloudFallback
    }
}

impl std::fmt::Display for RoutingPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RoutingPolicy::LocalFirstWithCloudFallback => write!(f, "LocalFirstWithCloudFallback"),
            RoutingPolicy::CloudOnly => write!(f, "CloudOnly"),
            RoutingPolicy::LocalOnly => write!(f, "LocalOnly"),
        }
    }
}

// ============================================================================
// Backend Selection
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Local,
    Cloud,
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Backend::Local => write!(f, "local"),
            Backend::Cloud => write!(f, "cloud"),
        }
    }
}

// ============================================================================
// Trace & Metrics
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub latency_ms: u64,
    pub time_to_first_token_ms: u64,
    pub tokens_streamed: usize,
    pub tokens_per_second: f64,
    pub total_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraceStage {
    stage: String,
    detail: Option<String>,
    timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraceJson {
    request_id: String,
    stages: Vec<TraceStage>,
    metrics: Option<PerformanceMetrics>,
}

// ============================================================================
// Compute Mesh Pipeline
// ============================================================================

struct ComputeMeshPipeline {
    policy: RoutingPolicy,
    trace: Vec<TraceStage>,
}

impl ComputeMeshPipeline {
    fn new(policy: RoutingPolicy) -> Self {
        Self {
            policy,
            trace: Vec::new(),
        }
    }

    fn add_trace(&mut self, stage: &str, detail: Option<String>) {
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        
        self.trace.push(TraceStage {
            stage: stage.to_string(),
            detail,
            timestamp_ms,
        });
    }

    fn select_backend(&mut self) -> Backend {
        self.add_trace("router.policy", Some(self.policy.to_string()));
        
        let backend = match self.policy {
            RoutingPolicy::LocalFirstWithCloudFallback => Backend::Local,
            RoutingPolicy::CloudOnly => Backend::Cloud,
            RoutingPolicy::LocalOnly => Backend::Local,
        };
        
        self.add_trace("router.backend_selection", Some(backend.to_string()));
        backend
    }

    pub async fn execute(&mut self, input: &str) -> Result<(String, PerformanceMetrics), String> {
        // Workflow start
        self.add_trace("workflow.start", None);
        
        // Execute workflow step
        info!("[workflow] executing step: generate_response");
        info!("Prompt: {}", input);
        
        // Route to backend
        let backend = self.select_backend();
        
        info!("Using routing policy: {}", self.policy);
        
        match backend {
            Backend::Local => {
                self.add_trace("inference.start", None);
                
                // Create local provider
                let config = LinuxInferenceConfig::new("demo-model", "C:\\temp\\demo-model");
                
                let mut provider = LinuxLocalProvider::new(config)
                    .map_err(|e| format!("failed to create provider: {}", e))?;
                
                // Load model
                provider.load().await
                    .map_err(|e| format!("failed to load model: {}", e))?;
                
                info!("Model loaded successfully");
                
                // Run streaming inference
                let start_time = Instant::now();
                let stream = provider
                    .infer_stream(input)
                    .await
                    .map_err(|e| format!("inference error: {}", e))?;
                
                let first_token_time = start_time.elapsed();
                let mut tokens_streamed = 0;
                let mut collected_output = String::new();
                
                // Process the stream
                let mut stream = Box::pin(stream);
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(chunk) => {
                            let text = chunk.delta.clone();
                            tokens_streamed += 1;
                            info!("[stream] {}", text);
                            self.add_trace("streaming.tokens", Some(format!("token_{}: {}", tokens_streamed, text)));
                            collected_output.push_str(&text);
                            
                            if chunk.is_done() {
                                self.add_trace("streaming.done", None);
                            }
                        }
                        Err(e) => {
                            tracing::error!("Stream error: {:?}", e);
                            break;
                        }
                    }
                }
                
                // Cleanup
                provider.unload().await
                    .map_err(|e| format!("failed to unload model: {}", e))?;
                
                let total_time = start_time.elapsed();
                let latency_ms = total_time.as_millis() as u64;
                let time_to_first_token_ms = first_token_time.as_millis() as u64;
                let tokens_per_second = if latency_ms > 0 {
                    (tokens_streamed as f64) / (latency_ms as f64 / 1000.0)
                } else {
                    0.0
                };
                
                let metrics = PerformanceMetrics {
                    latency_ms,
                    time_to_first_token_ms,
                    tokens_streamed,
                    tokens_per_second,
                    total_time_ms: latency_ms,
                };
                
                self.add_trace("metrics.latency_ms", Some(latency_ms.to_string()));
                self.add_trace("metrics.tokens_streamed", Some(tokens_streamed.to_string()));
                self.add_trace("metrics.tokens_per_second", Some(format!("{:.1}", tokens_per_second)));
                self.add_trace("workflow.complete", None);
                
                Ok((collected_output, metrics))
            }
            Backend::Cloud => {
                // Cloud backend not implemented in this demo
                Err("Cloud backend not implemented".to_string())
            }
        }
    }

    fn get_trace_json(&self, request_id: &str, metrics: Option<PerformanceMetrics>) -> String {
        let trace = TraceJson {
            request_id: request_id.to_string(),
            stages: self.trace.clone(),
            metrics,
        };
        
        serde_json::to_string_pretty(&trace).unwrap_or_default()
    }
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    info!("========================================");
    info!("  MoFA Compute Mesh Demo");
    info!("  with Benchmarking & Trace");
    info!("========================================");
    info!("");

    // Create a simple prompt
    let prompt = "Explain photosynthesis";

    info!("User prompt: {}", prompt);
    info!("");

    // Create pipeline with routing policy
    let mut pipeline = ComputeMeshPipeline::new(RoutingPolicy::LocalFirstWithCloudFallback);

    // Execute the pipeline
    let (result, metrics) = pipeline.execute(prompt).await?;

    info!("");
    info!("[metrics]");
    info!("[metrics] backend: local");
    info!("[metrics] latency_ms: {}", metrics.latency_ms);
    info!("[metrics] time_to_first_token_ms: {}", metrics.time_to_first_token_ms);
    info!("[metrics] tokens_streamed: {}", metrics.tokens_streamed);
    info!("[metrics] tokens_per_second: {:.1}", metrics.tokens_per_second);
    info!("[metrics] total_time_ms: {}", metrics.total_time_ms);

    // Generate request ID for trace
    let request_id = uuid::Uuid::new_v4().to_string();

    info!("");
    info!("========================================");
    info!("  Demo Complete");
    info!("========================================");
    info!("");
    info!(
        "Result: Processed '{}' with {} tokens (latency: {}ms)",
        prompt, metrics.tokens_streamed, metrics.latency_ms
    );

    // Print trace
    info!("");
    info!("==== Compute Mesh Execution Trace ====");
    
    for stage in &pipeline.trace {
        if let Some(detail) = &stage.detail {
            info!("[trace] {}.{} = {}", stage.stage.split('.').next().unwrap_or(&stage.stage), 
                stage.stage.split('.').nth(1).unwrap_or(""), detail);
        } else {
            info!("[trace] {}", stage.stage);
        }
    }

    info!("");
    info!("Trace JSON: {}", pipeline.get_trace_json(&request_id, Some(metrics)));

    Ok(())
}
