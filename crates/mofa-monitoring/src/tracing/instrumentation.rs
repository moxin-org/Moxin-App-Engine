//! Automatic Tracing Integration
//!
//! Providing automatic tracing functionality for Agents and Workflows

use super::context::SpanContext;
use super::exporter::{ConsoleExporter, ExporterConfig};
use super::propagator::{HeaderCarrier, TracePropagator, W3CTraceContextPropagator};
use super::span::{Span, SpanAttribute, SpanEvent, SpanKind};
use super::tracer::{SimpleSpanProcessor, Tracer, TracerConfig, TracerProvider};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// High-level tracing helper for MoFA agents.
///
/// Wraps a low-level [`Tracer`] with agent-aware span naming conventions
/// (`agent.<id>.<operation>`) and automatically attaches `agent.id` and
/// `agent.operation` attributes to every span.
///
/// # Examples
/// ```rust,no_run
/// use std::sync::Arc;
/// use mofa_monitoring::tracing::{
///     AgentTracer, ConsoleExporter, ExporterConfig, SimpleSpanProcessor,
///     TracerConfig, Tracer,
/// };
///
/// # async fn example() {
/// let exporter = Arc::new(ConsoleExporter::new(ExporterConfig::new("my-agent")));
/// let processor = Arc::new(SimpleSpanProcessor::new(exporter));
/// let tracer = Arc::new(Tracer::new(TracerConfig::new("my-agent"), processor));
/// let agent_tracer = AgentTracer::new(tracer);
///
/// let span = agent_tracer.start_operation("agent-001", "llm.call", None).await;
/// // ... perform work ...
/// agent_tracer.end_span(&span).await;
/// # }
/// ```
pub struct AgentTracer {
    tracer: Arc<Tracer>,
    propagator: Arc<dyn TracePropagator>,
    /// Currently active spans
    active_spans: Arc<RwLock<HashMap<String, Span>>>,
}

impl AgentTracer {
    pub fn new(tracer: Arc<Tracer>) -> Self {
        Self {
            tracer,
            propagator: Arc::new(W3CTraceContextPropagator::new()),
            active_spans: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_propagator(mut self, propagator: Arc<dyn TracePropagator>) -> Self {
        self.propagator = propagator;
        self
    }

    /// Start tracing an Agent operation
    pub async fn start_operation(
        &self,
        agent_id: &str,
        operation: &str,
        parent_context: Option<&SpanContext>,
    ) -> Span {
        let span_name = format!("agent.{}.{}", agent_id, operation);
        let span = self
            .tracer
            .start_span_with_kind(&span_name, SpanKind::Internal, parent_context);

        // Set Agent related attributes
        span.set_attribute("agent.id", agent_id).await;
        span.set_attribute("agent.operation", operation).await;

        // Store active span
        let span_id = span.span_id().await.to_hex();
        self.active_spans
            .write()
            .await
            .insert(span_id, span.clone());

        span
    }

    /// Trace message sending
    pub async fn trace_message_send(
        &self,
        agent_id: &str,
        target_agent: &str,
        message_type: &str,
        parent: Option<&SpanContext>,
    ) -> Span {
        let span = self.tracer.start_span_with_kind(
            format!("agent.{}.send.{}", agent_id, message_type),
            SpanKind::Producer,
            parent,
        );

        span.set_attribute("messaging.system", "mofa").await;
        span.set_attribute("messaging.operation", "send").await;
        span.set_attribute("messaging.destination", target_agent)
            .await;
        span.set_attribute("agent.id", agent_id).await;
        span.set_attribute("message.type", message_type).await;

        span
    }

    /// Trace message receiving
    pub async fn trace_message_receive(
        &self,
        agent_id: &str,
        source_agent: &str,
        message_type: &str,
        parent: Option<&SpanContext>,
    ) -> Span {
        let span = self.tracer.start_span_with_kind(
            format!("agent.{}.receive.{}", agent_id, message_type),
            SpanKind::Consumer,
            parent,
        );

        span.set_attribute("messaging.system", "mofa").await;
        span.set_attribute("messaging.operation", "receive").await;
        span.set_attribute("messaging.source", source_agent).await;
        span.set_attribute("agent.id", agent_id).await;
        span.set_attribute("message.type", message_type).await;

        span
    }

    /// Trace task execution
    pub async fn trace_task_execution(
        &self,
        agent_id: &str,
        task_id: &str,
        task_type: &str,
        parent: Option<&SpanContext>,
    ) -> Span {
        let span = self.tracer.start_span_with_kind(
            format!("agent.{}.task.{}", agent_id, task_type),
            SpanKind::Internal,
            parent,
        );

        span.set_attribute("task.id", task_id).await;
        span.set_attribute("task.type", task_type).await;
        span.set_attribute("agent.id", agent_id).await;

        span
    }

    /// End span and export
    pub async fn end_span(&self, span: &Span) {
        let span_id = span.span_id().await.to_hex();
        self.active_spans.write().await.remove(&span_id);
        self.tracer.end_span(span).await;
    }

    /// Inject trace context into headers
    pub fn inject_context(&self, span_context: &SpanContext, carrier: &mut dyn HeaderCarrier) {
        self.propagator.inject(span_context, carrier);
    }

    /// Extract trace context from headers
    pub fn extract_context(&self, carrier: &dyn HeaderCarrier) -> Option<SpanContext> {
        self.propagator.extract(carrier)
    }

    /// Get Tracer
    pub fn tracer(&self) -> Arc<Tracer> {
        self.tracer.clone()
    }
}

/// Workflow Tracer
pub struct WorkflowTracer {
    tracer: Arc<Tracer>,
    /// Root span of workflow execution
    workflow_spans: Arc<RwLock<HashMap<String, Span>>>,
    /// Spans of node execution
    node_spans: Arc<RwLock<HashMap<String, Span>>>,
}

impl WorkflowTracer {
    pub fn new(tracer: Arc<Tracer>) -> Self {
        Self {
            tracer,
            workflow_spans: Arc::new(RwLock::new(HashMap::new())),
            node_spans: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start tracing workflow execution
    pub async fn start_workflow(
        &self,
        workflow_id: &str,
        workflow_name: &str,
        execution_id: &str,
    ) -> Span {
        let span = self.tracer.start_span_with_kind(
            format!("workflow.{}", workflow_name),
            SpanKind::Internal,
            None,
        );

        span.set_attribute("workflow.id", workflow_id).await;
        span.set_attribute("workflow.name", workflow_name).await;
        span.set_attribute("workflow.execution_id", execution_id)
            .await;

        span.add_event_with_name("workflow_started").await;

        self.workflow_spans
            .write()
            .await
            .insert(execution_id.to_string(), span.clone());

        span
    }

    /// Trace node execution
    pub async fn start_node(
        &self,
        execution_id: &str,
        node_id: &str,
        node_name: &str,
        node_type: &str,
    ) -> Span {
        let parent = self.workflow_spans.read().await.get(execution_id).cloned();
        let parent_context = if let Some(ref p) = parent {
            Some(p.span_context().await)
        } else {
            None
        };

        let span = self.tracer.start_span_with_kind(
            format!("workflow.node.{}", node_name),
            SpanKind::Internal,
            parent_context.as_ref(),
        );

        span.set_attribute("workflow.node.id", node_id).await;
        span.set_attribute("workflow.node.name", node_name).await;
        span.set_attribute("workflow.node.type", node_type).await;
        span.set_attribute("workflow.execution_id", execution_id)
            .await;

        span.add_event_with_name("node_started").await;

        let key = format!("{}:{}", execution_id, node_id);
        self.node_spans.write().await.insert(key, span.clone());

        span
    }

    /// End node execution
    pub async fn end_node(
        &self,
        execution_id: &str,
        node_id: &str,
        success: bool,
        error: Option<&str>,
    ) {
        let key = format!("{}:{}", execution_id, node_id);
        if let Some(span) = self.node_spans.write().await.remove(&key) {
            if success {
                span.set_ok().await;
                span.add_event_with_name("node_completed").await;
            } else {
                span.set_error(error.unwrap_or("Unknown error")).await;
                span.add_event(
                    SpanEvent::new("node_failed")
                        .with_attribute("error.message", error.unwrap_or("Unknown error")),
                )
                .await;
            }
            self.tracer.end_span(&span).await;
        }
    }

    /// End workflow execution
    pub async fn end_workflow(&self, execution_id: &str, success: bool, error: Option<&str>) {
        if let Some(span) = self.workflow_spans.write().await.remove(execution_id) {
            if success {
                span.set_ok().await;
                span.add_event_with_name("workflow_completed").await;
            } else {
                span.set_error(error.unwrap_or("Unknown error")).await;
                span.add_event(
                    SpanEvent::new("workflow_failed")
                        .with_attribute("error.message", error.unwrap_or("Unknown error")),
                )
                .await;
            }
            self.tracer.end_span(&span).await;
        }
    }

    /// Record workflow event
    pub async fn record_event(&self, execution_id: &str, event_name: &str) {
        if let Some(span) = self.workflow_spans.read().await.get(execution_id) {
            span.add_event_with_name(event_name).await;
        }
    }

    /// Set workflow attribute
    pub async fn set_attribute(
        &self,
        execution_id: &str,
        key: impl Into<String>,
        value: impl Into<SpanAttribute>,
    ) {
        if let Some(span) = self.workflow_spans.read().await.get(execution_id) {
            span.set_attribute(key, value).await;
        }
    }

    /// Get Tracer
    pub fn tracer(&self) -> Arc<Tracer> {
        self.tracer.clone()
    }
}

/// Message Tracer - For tracing message passing between Agents
pub struct MessageTracer {
    tracer: Arc<Tracer>,
    propagator: Arc<dyn TracePropagator>,
}

impl MessageTracer {
    pub fn new(tracer: Arc<Tracer>) -> Self {
        Self {
            tracer,
            propagator: Arc::new(W3CTraceContextPropagator::new()),
        }
    }

    /// Trace message sending, returns headers containing trace context
    pub async fn trace_send(
        &self,
        message_type: &str,
        source: &str,
        destination: &str,
        parent: Option<&SpanContext>,
    ) -> (Span, HashMap<String, String>) {
        let span = self.tracer.start_span_with_kind(
            format!("message.send.{}", message_type),
            SpanKind::Producer,
            parent,
        );

        span.set_attribute("messaging.system", "mofa").await;
        span.set_attribute("messaging.operation", "send").await;
        span.set_attribute("messaging.message_type", message_type)
            .await;
        span.set_attribute("messaging.source", source).await;
        span.set_attribute("messaging.destination", destination)
            .await;

        // Inject trace context
        let mut headers = HashMap::new();
        let span_context = span.span_context().await;
        self.propagator.inject(&span_context, &mut headers);

        (span, headers)
    }

    /// Trace message receiving
    pub async fn trace_receive(
        &self,
        message_type: &str,
        source: &str,
        destination: &str,
        headers: &HashMap<String, String>,
    ) -> Span {
        // Extract trace context
        let parent_context = self.propagator.extract(headers);

        let span = self.tracer.start_span_with_kind(
            format!("message.receive.{}", message_type),
            SpanKind::Consumer,
            parent_context.as_ref(),
        );

        span.set_attribute("messaging.system", "mofa").await;
        span.set_attribute("messaging.operation", "receive").await;
        span.set_attribute("messaging.message_type", message_type)
            .await;
        span.set_attribute("messaging.source", source).await;
        span.set_attribute("messaging.destination", destination)
            .await;

        span
    }

    /// End message tracing
    pub async fn end_span(&self, span: &Span) {
        self.tracer.end_span(span).await;
    }
}

/// Wraps any agent type `A` and provides a `traced_operation` method that
/// automatically starts/ends a span around the given async closure.
///
/// Prefer this over manual `start_operation`/`end_span` calls when you don't
/// need to keep a span reference beyond the operation's lifetime.
pub struct TracedAgent<A> {
    agent: A,
    tracer: Arc<AgentTracer>,
    agent_id: String,
}

impl<A> TracedAgent<A> {
    pub fn new(agent: A, tracer: Arc<AgentTracer>, agent_id: impl Into<String>) -> Self {
        Self {
            agent,
            tracer,
            agent_id: agent_id.into(),
        }
    }

    pub fn agent(&self) -> &A {
        &self.agent
    }

    pub fn agent_mut(&mut self) -> &mut A {
        &mut self.agent
    }

    pub fn tracer(&self) -> Arc<AgentTracer> {
        self.tracer.clone()
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Execute operation with tracing
    pub async fn traced_operation<F, Fut, R>(
        &self,
        operation: &str,
        parent: Option<&SpanContext>,
        f: F,
    ) -> R
    where
        F: FnOnce(&A) -> Fut,
        Fut: std::future::Future<Output = R>,
    {
        let span = self
            .tracer
            .start_operation(&self.agent_id, operation, parent)
            .await;

        let result = f(&self.agent).await;

        self.tracer.end_span(&span).await;
        result
    }
}

/// Traced Workflow Wrapper
pub struct TracedWorkflow<W> {
    workflow: W,
    tracer: Arc<WorkflowTracer>,
    workflow_id: String,
    workflow_name: String,
}

impl<W> TracedWorkflow<W> {
    pub fn new(
        workflow: W,
        tracer: Arc<WorkflowTracer>,
        workflow_id: impl Into<String>,
        workflow_name: impl Into<String>,
    ) -> Self {
        Self {
            workflow,
            tracer,
            workflow_id: workflow_id.into(),
            workflow_name: workflow_name.into(),
        }
    }

    pub fn workflow(&self) -> &W {
        &self.workflow
    }

    pub fn workflow_mut(&mut self) -> &mut W {
        &mut self.workflow
    }

    pub fn tracer(&self) -> Arc<WorkflowTracer> {
        self.tracer.clone()
    }
}

/// Convenience wrapper: starts a span, calls `f` with it, then ends the span.
///
/// Use this instead of manually calling `start_operation`/`end_span` when
/// you don't need to access the span outside the closure.
///
/// # Examples
/// ```rust,no_run
/// use std::sync::Arc;
/// use mofa_monitoring::tracing::{
///     AgentTracer, ConsoleExporter, ExporterConfig, SimpleSpanProcessor,
///     TracerConfig, Tracer, trace_agent_operation,
/// };
///
/// # async fn example() {
/// # let exporter = Arc::new(ConsoleExporter::new(ExporterConfig::new("x")));
/// # let processor = Arc::new(SimpleSpanProcessor::new(exporter));
/// # let tracer = Arc::new(Tracer::new(TracerConfig::new("x"), processor));
/// let agent_tracer = AgentTracer::new(tracer);
///
/// let answer = trace_agent_operation(
///     &agent_tracer, "agent-001", "llm.call", None,
///     |_span| async move { "response".to_string() },
/// ).await;
/// # }
/// ```
pub async fn trace_agent_operation<F, Fut, R>(
    tracer: &AgentTracer,
    agent_id: &str,
    operation: &str,
    parent: Option<&SpanContext>,
    f: F,
) -> R
where
    F: FnOnce(Span) -> Fut,
    Fut: std::future::Future<Output = R>,
{
    let span = tracer.start_operation(agent_id, operation, parent).await;
    let result = f(span.clone()).await;
    tracer.end_span(&span).await;
    result
}

/// Helper function: trace Workflow execution
pub async fn trace_workflow_execution<F, Fut, R>(
    tracer: &WorkflowTracer,
    workflow_id: &str,
    workflow_name: &str,
    execution_id: &str,
    f: F,
) -> R
where
    F: FnOnce(Span) -> Fut,
    Fut: std::future::Future<Output = R>,
{
    let span = tracer
        .start_workflow(workflow_id, workflow_name, execution_id)
        .await;
    let result = f(span).await;
    tracer.end_workflow(execution_id, true, None).await;
    result
}

/// Create default tracing setup
pub fn create_default_tracing(
    service_name: &str,
) -> (Arc<TracerProvider>, Arc<AgentTracer>, Arc<WorkflowTracer>) {
    let exporter_config = ExporterConfig::new(service_name);
    let exporter = Arc::new(ConsoleExporter::new(exporter_config).with_summary_only());
    let processor = Arc::new(SimpleSpanProcessor::new(exporter));

    let tracer_config = TracerConfig::new(service_name);
    let provider = Arc::new(TracerProvider::new(tracer_config, processor));

    // Need to use futures to synchronously get the tracer
    let tracer = futures::executor::block_on(provider.default_tracer());

    let agent_tracer = Arc::new(AgentTracer::new(tracer.clone()));
    let workflow_tracer = Arc::new(WorkflowTracer::new(tracer));

    (provider, agent_tracer, workflow_tracer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_tracer() {
        let config = ExporterConfig::new("test-service");
        let exporter = Arc::new(ConsoleExporter::new(config).with_summary_only());
        let processor = Arc::new(SimpleSpanProcessor::new(exporter));

        let tracer_config = TracerConfig::new("test-service");
        let tracer = Arc::new(Tracer::new(tracer_config, processor));

        let agent_tracer = AgentTracer::new(tracer);

        let span = agent_tracer
            .start_operation("agent-1", "process", None)
            .await;
        assert!(span.is_recording().await);

        agent_tracer.end_span(&span).await;
        assert!(span.is_ended().await);
    }

    #[tokio::test]
    async fn test_workflow_tracer() {
        let config = ExporterConfig::new("test-service");
        let exporter = Arc::new(ConsoleExporter::new(config).with_summary_only());
        let processor = Arc::new(SimpleSpanProcessor::new(exporter));

        let tracer_config = TracerConfig::new("test-service");
        let tracer = Arc::new(Tracer::new(tracer_config, processor));

        let workflow_tracer = WorkflowTracer::new(tracer);

        let span = workflow_tracer
            .start_workflow("wf-1", "Test Workflow", "exec-1")
            .await;
        assert!(span.is_recording().await);

        let node_span = workflow_tracer
            .start_node("exec-1", "node-1", "Node 1", "task")
            .await;
        assert!(node_span.is_recording().await);

        workflow_tracer
            .end_node("exec-1", "node-1", true, None)
            .await;
        workflow_tracer.end_workflow("exec-1", true, None).await;
    }

    #[tokio::test]
    async fn test_message_tracer() {
        let config = ExporterConfig::new("test-service");
        let exporter = Arc::new(ConsoleExporter::new(config).with_summary_only());
        let processor = Arc::new(SimpleSpanProcessor::new(exporter));

        let tracer_config = TracerConfig::new("test-service");
        let tracer = Arc::new(Tracer::new(tracer_config, processor));

        let message_tracer = MessageTracer::new(tracer);

        // Send message
        let (send_span, headers) = message_tracer
            .trace_send("task_request", "agent-1", "agent-2", None)
            .await;

        // Verify headers contain trace information
        assert!(headers.contains_key("traceparent"));

        // Receive message
        let receive_span = message_tracer
            .trace_receive("task_request", "agent-1", "agent-2", &headers)
            .await;

        // Verify trace_id is the same
        assert_eq!(send_span.trace_id().await, receive_span.trace_id().await);

        message_tracer.end_span(&send_span).await;
        message_tracer.end_span(&receive_span).await;
    }
}
