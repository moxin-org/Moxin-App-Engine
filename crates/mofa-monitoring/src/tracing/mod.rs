//! Distributed tracing module
//!
//! Provides distributed tracing functionality, supporting:
//! - Trace Context propagation
//! - Span management
//! - Multiple exporters (Console, Jaeger, OTLP)
//! - Automatic tracing for Agents and Workflows
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use mofa_monitoring::tracing::{
//!     AgentTracer, ConsoleExporter, SimpleSpanProcessor, Tracer, TracerConfig, TracerProvider,
//! };
//!
//! let exporter = Arc::new(ConsoleExporter::new());
//! let processor = Arc::new(SimpleSpanProcessor::new(exporter));
//! let provider = Arc::new(TracerProvider::new(TracerConfig::new("my-agent"), processor));
//! let tracer = Arc::new(Tracer::new(provider));
//! let _agent_tracer = AgentTracer::new(tracer);
//! ```
//!
//! # Utility Helpers
//!
//! Use `trace_agent_operation` and `trace_workflow_execution` to attach tracing
//! around custom logic blocks without manually creating and ending spans.

mod context;
mod exporter;
mod instrumentation;
#[cfg(feature = "otlp-metrics")]
mod metrics_exporter;
mod propagator;
mod span;
mod tracer;

pub use context::{SpanContext, SpanId, TraceContext, TraceFlags, TraceId, TraceState};
pub use exporter::{
    ConsoleExporter, ExporterConfig, JaegerExporter, OtlpExporter, TracingExporter,
};
pub use instrumentation::{
    AgentTracer, MessageTracer, TracedAgent, TracedWorkflow, WorkflowTracer, trace_agent_operation,
    trace_workflow_execution,
};
#[cfg(feature = "otlp-metrics")]
pub use metrics_exporter::{
    CardinalityLimits, OtlpExporterHandles, OtlpMetricsExporter, OtlpMetricsExporterConfig,
    OtlpMetricsExporterError,
};
pub use propagator::{B3Propagator, HeaderCarrier, TracePropagator, W3CTraceContextPropagator};
pub use span::{Span, SpanAttribute, SpanBuilder, SpanEvent, SpanKind, SpanLink, SpanStatus};
pub use tracer::{
    BatchSpanProcessor, GlobalTracer, SamplingStrategy, SimpleSpanProcessor, SpanProcessor, Tracer,
    TracerConfig, TracerProvider,
};
