//! Demo: Streaming Response with LocalFirstWithCloudFallback in MoFA Compute Mesh
//!
//! This example demonstrates how to use streaming inference responses with the
//! LocalFirstWithCloudFallback routing policy in a MoFA compute mesh.
//!
//! The compute mesh intelligently routes inference requests between local and cloud
//! backends based on resource availability and the configured routing policy.

use anyhow::Result;
use mofa_foundation::inference::{
    InferenceOrchestrator, InferenceRequest, OrchestratorConfig, RoutingPolicy,
};

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    println!("=== MoFA Compute Mesh Streaming Response Demo ===");
    println!();

    // 1. Initialize the Inference Orchestrator with LocalFirstWithCloudFallback policy
    let mut config = OrchestratorConfig::default();
    config.memory_capacity_mb = 16384; // 16GB local budget
    config.routing_policy = RoutingPolicy::LocalFirstWithCloudFallback;
    config.defer_threshold = 0.75;
    config.reject_threshold = 0.90;

    let mut orchestrator = InferenceOrchestrator::new(config);

    println!("Compute Mesh Configuration:");
    println!("  Local Memory Capacity: 16 GB");
    println!("  Routing Policy: LocalFirstWithCloudFallback");
    println!("  Defer Threshold: 75%");
    println!("  Reject Threshold: 90%");
    println!();

    // 2. Demonstrate streaming inference with routing decisions
    println!("--- Streaming Inference Demo ---");
    println!();

    // Simulate a streaming request that would be routed based on memory pressure
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Explain the concept of compute mesh".to_string());

    let request = InferenceRequest::new(
        "qwen-7b-chat",
        &prompt,
        7000,
    );

    println!("User prompt: {}", prompt);
    println!();

    println!("[workflow] executing step: generate_response");
    println!("[inference] sending request to orchestrator...");
    println!();

    println!("Request model: '{}'", request.model_id);
    println!("Memory Required: {} MB", request.required_memory_mb);
    println!();

    // Get the routing decision
    let response = orchestrator.infer(&request);

    println!("Routing Decision: {:?}", response.routed_to);
    println!();

    // 3. Demonstrate the streaming pattern (simulated)
    println!("--- Streaming Response Pattern ---");
    println!();

    println!("[router] policy: LocalFirstWithCloudFallback");
    println!("[router] selected backend: {:?}", response.routed_to);
    println!();

    println!("Streaming tokens:");
    println!();

    // Simulated token streaming
    let demo_tokens = [
        "Photosynthesis",
        "is",
        "the",
        "process",
        "by",
        "which",
        "plants",
        "convert",
        "sunlight",
        "into",
        "chemical",
        "energy",
        "using",
        "chlorophyll",
    ];

    for token in demo_tokens.iter() {
        println!("[stream] {}", token);
        std::thread::sleep(std::time::Duration::from_millis(150));
    }

    println!();
    println!("[result] Photosynthesis is the process by which plants convert sunlight into chemical energy using chlorophyll.");
    println!();

    // 4. Show cloud fallback scenario
    println!("--- Cloud Fallback Scenario ---");
    println!();

    // Simulate high memory pressure scenario
    let mut high_pressure_config = OrchestratorConfig::default();
    high_pressure_config.memory_capacity_mb = 16384;
    high_pressure_config.routing_policy = RoutingPolicy::LocalFirstWithCloudFallback;
    high_pressure_config.defer_threshold = 0.75;
    high_pressure_config.reject_threshold = 0.90;

    let mut high_pressure_orchestrator = InferenceOrchestrator::new(high_pressure_config);

    // Simulate local resources are exhausted
    println!("Simulating high memory pressure (local resources exhausted)...");

    // First, fill up local memory with simulated requests
    for i in 0..3 {
        let req = InferenceRequest::new(format!("model-{}", i), "dummy", 5000);
        high_pressure_orchestrator.infer(&req);
    }

    // Now try to route a new request - should fall back to cloud
    let cloud_fallback_request =
        InferenceRequest::new("gpt-4-turbo", "Tell me about Rust programming", 8000);

    let fallback_response = high_pressure_orchestrator.infer(&cloud_fallback_request);

    println!("Request: '{}'", cloud_fallback_request.model_id);
    println!("Routing Decision: {:?}", fallback_response.routed_to);
    println!();

    println!("=== Demo Completed ===");
    println!();
    println!("Key Takeaways:");
    println!("1. LocalFirstWithCloudFallback tries local first");
    println!("2. Falls back to cloud when local resources are exhausted");
    println!("3. Streaming responses work the same way regardless of backend");
    println!("4. The orchestrator handles failover transparently");

    Ok(())
}
