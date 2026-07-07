//! Example: Agent Distributed Tracing
//!
//! Demonstrates end-to-end span creation using the AgentTracer.
//! When the `tracing-otel` feature is enabled, spans are exported
//! to the configured backend. Without it, the no-op path is used.

use mofa_foundation::tracing::{
    AgentTracer, MetricsCollector, TracingConfig, TracingExporter, get_baggage, set_baggage,
};

fn main() {
    let config = TracingConfig {
        service_name: "example-agent".to_string(),
        exporter: TracingExporter::Stdout,
        sampling_ratio: 1.0,
        max_attributes: 128,
    };

    let tracer = AgentTracer::new("agent-001".to_string(), config);
    let metrics = MetricsCollector::new();

    // Propagate context via baggage
    set_baggage("session-id", "sess-abc123");
    println!("Session: {:?}", get_baggage("session-id"));

    // Thought span
    println!("Starting thought span...");
    let mut span = tracer.start_thought_span("analyzing user request");
    span.set_attribute("thought.complexity", "medium");
    println!("  span name: {}", span.name());
    drop(span);
    metrics.increment_counter("thoughts", 1);

    // Tool call span
    println!("Starting tool call span...");
    let mut span = tracer.start_tool_call_span("web_search");
    span.set_attribute("tool.query", "mofa framework rust");
    tracer.record_observation(&mut span, "found 10 results", "web");
    drop(span);
    metrics.increment_counter("tool_calls", 1);
    metrics.record_histogram("tool_latency_ms", 42.5);

    // Action span with error
    println!("Starting action span (with error)...");
    let mut span = tracer.start_action_span("parse_result");
    tracer.record_error(&mut span, "unexpected JSON format");
    drop(span);
    metrics.increment_counter("errors", 1);

    // Print metrics summary
    println!("\nMetrics:");
    println!("  thoughts:     {}", metrics.counter("thoughts"));
    println!("  tool_calls:   {}", metrics.counter("tool_calls"));
    println!("  errors:       {}", metrics.counter("errors"));
    let latencies = metrics.histogram_values("tool_latency_ms");
    if !latencies.is_empty() {
        let avg = latencies.iter().sum::<f64>() / latencies.len() as f64;
        println!("  avg latency:  {:.1}ms", avg);
    }
}
