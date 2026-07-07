//! Tracer 和 TracerProvider
//! Tracer and TracerProvider
//!
//! 提供追踪器的创建和管理
//! Provides creation and management of tracers

use super::context::{SpanContext, SpanId, TraceFlags, TraceId};
use super::exporter::TracingExporter;
use super::propagator::TracePropagator;
use super::span::{Span, SpanBuilder, SpanData, SpanKind};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

/// 采样策略
/// Sampling strategy
#[derive(Debug, Clone, Default)]
pub enum SamplingStrategy {
    /// 始终采样
    /// Always sample
    #[default]
    AlwaysOn,
    /// 从不采样
    /// Never sample
    AlwaysOff,
    /// 按概率采样
    /// Probabilistic sampling
    Probabilistic(f64),
    /// 基于速率限制采样
    /// Rate-limiting based sampling
    RateLimiting {
        traces_per_second: u64,
        /// Holds (timestamp_secs << 32) | (count)
        state: Arc<AtomicU64>,
    },
    /// 父级决定
    /// Parent-based decision
    ParentBased { root: Box<SamplingStrategy> },
}

impl SamplingStrategy {
    /// 判断是否应该采样
    /// Determine if sampling should occur
    pub fn should_sample(
        &self,
        parent_context: Option<&SpanContext>,
        trace_id: &TraceId,
        _name: &str,
    ) -> bool {
        match self {
            SamplingStrategy::AlwaysOn => true,
            SamplingStrategy::AlwaysOff => false,
            SamplingStrategy::Probabilistic(probability) => {
                let hash = trace_id
                    .as_bytes()
                    .iter()
                    .fold(0u64, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u64));
                (hash as f64 / u64::MAX as f64) < *probability
            }
            SamplingStrategy::RateLimiting {
                traces_per_second,
                state,
            } => {
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let mut current = state.load(Ordering::Relaxed);
                loop {
                    let current_secs = current >> 32;
                    let current_count = current & 0xFFFFFFFF;

                    let (new_secs, new_count) = if current_secs != now_secs {
                        // New second window
                        (now_secs, 1)
                    } else {
                        // Same second window
                        if current_count >= *traces_per_second {
                            return false; // Rate limit exceeded
                        }
                        (current_secs, current_count + 1)
                    };

                    let new_state = (new_secs << 32) | new_count;
                    match state.compare_exchange_weak(
                        current,
                        new_state,
                        Ordering::SeqCst,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => return true,
                        Err(v) => current = v,
                    }
                }
            }
            SamplingStrategy::ParentBased { root } => {
                if let Some(parent) = parent_context {
                    parent.is_sampled()
                } else {
                    root.should_sample(None, trace_id, _name)
                }
            }
        }
    }
}

/// Tracer 配置
/// Tracer configuration
/// 
/// 用于定义追踪器的全局设置，包括服务名称、版本、环境以及采样策略等。
/// Used to define global settings for tracers, including service name, version, environment, and sampling strategy.
#[derive(Debug, Clone)]
pub struct TracerConfig {
    /// 服务名称
    /// Service name
    pub service_name: String,
    /// 服务版本
    /// Service version
    pub service_version: Option<String>,
    /// 环境
    /// Environment
    pub environment: Option<String>,
    /// 采样策略
    /// Sampling strategy
    pub sampling_strategy: SamplingStrategy,
    /// 最大属性数
    /// Maximum number of attributes
    pub max_attributes: usize,
    /// 最大事件数
    /// Maximum number of events
    pub max_events: usize,
    /// 最大链接数
    /// Maximum number of links
    pub max_links: usize,
}

impl Default for TracerConfig {
    fn default() -> Self {
        Self {
            service_name: "unknown-service".to_string(),
            service_version: None,
            environment: None,
            sampling_strategy: SamplingStrategy::AlwaysOn,
            max_attributes: 128,
            max_events: 128,
            max_links: 128,
        }
    }
}

impl TracerConfig {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            ..Default::default()
        }
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.service_version = Some(version.into());
        self
    }

    pub fn with_environment(mut self, env: impl Into<String>) -> Self {
        self.environment = Some(env.into());
        self
    }

    pub fn with_sampling_strategy(mut self, strategy: SamplingStrategy) -> Self {
        self.sampling_strategy = strategy;
        self
    }
}

/// Span 处理器 trait
/// Span processor trait
/// 
/// 定义了 Span 生命周期中的回调接口。实现此 trait 的处理器可以决定如何处理 Span 数据（如导出、过滤、缓冲等）。
/// Defines callback interfaces during the Span lifecycle. Processors implementing this trait decide how to handle Span data (e.g., exporting, filtering, buffering).
#[async_trait::async_trait]
pub trait SpanProcessor: Send + Sync {
    /// Span 开始时调用
    /// Called when a Span starts
    async fn on_start(&self, span: &Span, parent_context: Option<&SpanContext>);
    /// Span 结束时调用
    /// Called when a Span ends
    async fn on_end(&self, span: SpanData);
    /// 关闭处理器
    /// Shutdown the processor
    async fn shutdown(&self) -> Result<(), String>;
    /// 强制刷新
    /// Force flush
    async fn force_flush(&self) -> Result<(), String>;
}

/// 简单 Span 处理器 - 直接导出
/// Simple Span Processor - Export directly
/// 
/// 每当一个 Span 结束时，立即调用导出器将其同步导出。适用于调试或低流量场景。
/// Immediately calls the exporter to synchronously export each Span as it ends. Suitable for debugging or low-traffic scenarios.
pub struct SimpleSpanProcessor {
    exporter: Arc<dyn TracingExporter>,
}

impl SimpleSpanProcessor {
    pub fn new(exporter: Arc<dyn TracingExporter>) -> Self {
        Self { exporter }
    }
}

#[async_trait::async_trait]
impl SpanProcessor for SimpleSpanProcessor {
    async fn on_start(&self, _span: &Span, _parent_context: Option<&SpanContext>) {
        // 简单处理器不在开始时做任何事
        // Simple processor does nothing on start
    }

    async fn on_end(&self, span: SpanData) {
        if let Err(e) = self.exporter.export(vec![span]).await {
            tracing::error!("Failed to export span: {}", e);
        }
    }

    async fn shutdown(&self) -> Result<(), String> {
        self.exporter.shutdown().await
    }

    async fn force_flush(&self) -> Result<(), String> {
        self.exporter.force_flush().await
    }
}

/// 批处理 Span 处理器
/// Batch Span Processor
/// 
/// 将 Span 数据缓冲在内存中，并在达到批次大小或最大队列限制时异步导出。适用于生产环境以减少导出开销。
/// Buffers Span data in memory and asynchronously exports it when batch size or max queue limits are reached. Suitable for production to reduce export overhead.
pub struct BatchSpanProcessor {
    exporter: Arc<dyn TracingExporter>,
    buffer: Arc<RwLock<Vec<SpanData>>>,
    batch_size: usize,
    max_queue_size: usize,
    dropped_count: AtomicU64,
}

impl BatchSpanProcessor {
    pub fn new(
        exporter: Arc<dyn TracingExporter>,
        batch_size: usize,
        max_queue_size: usize,
    ) -> Self {
        Self {
            exporter,
            buffer: Arc::new(RwLock::new(Vec::new())),
            batch_size,
            max_queue_size,
            dropped_count: AtomicU64::new(0),
        }
    }

    async fn maybe_export(&self) -> Result<(), String> {
        let to_export: Option<Vec<SpanData>> = {
            let mut buffer = self.buffer.write().await;
            if buffer.len() >= self.batch_size {
                Some(buffer.drain(..).collect())
            } else {
                None
            }
        };

        let dropped = self.dropped_count.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            tracing::warn!(
                dropped_spans = dropped,
                max_queue_size = self.max_queue_size,
                "BatchSpanProcessor: dropped spans since last flush because buffer was full"
            );
        }

        if let Some(spans) = to_export {
            self.exporter.export(spans).await?;
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl SpanProcessor for BatchSpanProcessor {
    async fn on_start(&self, _span: &Span, _parent_context: Option<&SpanContext>) {
        // 批处理器不在开始时做任何事
        // Batch processor does nothing on start
    }

    async fn on_end(&self, span: SpanData) {
        {
            let mut buffer = self.buffer.write().await;
            if buffer.len() < self.max_queue_size {
                buffer.push(span);
            } else {
                self.dropped_count.fetch_add(1, Ordering::Relaxed);
            }
        }

        if let Err(e) = self.maybe_export().await {
            tracing::error!("Failed to export spans: {}", e);
        }
    }

    async fn shutdown(&self) -> Result<(), String> {
        self.force_flush().await?;
        self.exporter.shutdown().await
    }

    async fn force_flush(&self) -> Result<(), String> {
        let to_export: Vec<SpanData> = {
            let mut buffer = self.buffer.write().await;
            buffer.drain(..).collect()
        };

        let dropped = self.dropped_count.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            tracing::warn!(
                dropped_spans = dropped,
                max_queue_size = self.max_queue_size,
                "BatchSpanProcessor: dropped spans since last flush because buffer was full"
            );
        }

        if !to_export.is_empty() {
            self.exporter.export(to_export).await?;
        }

        self.exporter.force_flush().await
    }
}

/// Tracer - 追踪器
/// Tracer - Tracing component
/// 
/// Tracer 是追踪系统的核心入口点，用于创建和管理 Span。每个 Tracer 通常关联到一个特定的服务或组件。
/// The Tracer is the core entry point of the tracing system, used to create and manage Spans. Each Tracer is typically associated with a specific service or component.
pub struct Tracer {
    config: TracerConfig,
    processor: Arc<dyn SpanProcessor>,
}

impl Tracer {
    pub fn new(config: TracerConfig, processor: Arc<dyn SpanProcessor>) -> Self {
        Self { config, processor }
    }

    /// 创建新的根 Span
    /// Create a new root Span
    pub fn start_span(&self, name: impl Into<String>) -> Span {
        self.start_span_with_kind(name, SpanKind::Internal, None)
    }

    /// 创建带类型的 Span
    /// Create a Span with a specific kind
    pub fn start_span_with_kind(
        &self,
        name: impl Into<String>,
        kind: SpanKind,
        parent: Option<&SpanContext>,
    ) -> Span {
        let name = name.into();
        let trace_id = parent.map(|p| p.trace_id).unwrap_or_default();

        // 检查是否应该采样
        // Check if sampling should occur
        let should_sample = self
            .config
            .sampling_strategy
            .should_sample(parent, &trace_id, &name);

        let trace_flags = if should_sample {
            TraceFlags::SAMPLED
        } else {
            TraceFlags::NONE
        };

        let span_context = SpanContext::new(trace_id, SpanId::new(), trace_flags, false);

        if !should_sample {
            return Span::non_recording(span_context);
        }

        let span = Span::new(
            name,
            span_context.clone(),
            parent.cloned(),
            kind,
            &self.config.service_name,
        );

        // 通知处理器
        // Notify processor
        let processor = self.processor.clone();
        let span_clone = span.clone();
        let parent_clone = parent.cloned();
        tokio::spawn(async move {
            processor.on_start(&span_clone, parent_clone.as_ref()).await;
        });

        span
    }

    /// 创建子 Span
    /// Create a child Span
    pub fn start_child_span(&self, name: impl Into<String>, parent: &SpanContext) -> Span {
        self.start_span_with_kind(name, SpanKind::Internal, Some(parent))
    }

    /// 使用 SpanBuilder 创建 Span
    /// Create a Span using SpanBuilder
    pub fn span_builder(&self, name: impl Into<String>) -> SpanBuilder {
        SpanBuilder::new(name, &self.config.service_name)
    }

    /// 结束 Span 并导出
    /// End Span and export
    pub async fn end_span(&self, span: &Span) {
        span.end().await;
        if span.is_recording().await {
            let data = span.get_data().await;
            self.processor.on_end(data).await;
        }
    }

    /// 获取服务名称
    /// Get service name
    pub fn service_name(&self) -> &str {
        &self.config.service_name
    }

    /// 关闭 Tracer
    /// Shutdown Tracer
    pub async fn shutdown(&self) -> Result<(), String> {
        self.processor.shutdown().await
    }

    /// 强制刷新
    /// Force flush
    pub async fn force_flush(&self) -> Result<(), String> {
        self.processor.force_flush().await
    }
}

/// Tracer Provider - 管理多个 Tracer
/// Tracer Provider - Manages multiple Tracers
/// 
/// 负责维护追踪配置和 Span 处理器，并按名称提供 `Tracer` 实例。它通常由应用生命周期全局持有。
/// Responsible for maintaining tracing configurations and Span processors, and providing `Tracer` instances by name. It is typically held globally by the application lifecycle.
pub struct TracerProvider {
    config: TracerConfig,
    processor: Arc<dyn SpanProcessor>,
    tracers: Arc<RwLock<HashMap<String, Arc<Tracer>>>>,
    propagator: Arc<dyn TracePropagator>,
}

impl TracerProvider {
    pub fn new(config: TracerConfig, processor: Arc<dyn SpanProcessor>) -> Self {
        use super::propagator::W3CTraceContextPropagator;

        Self {
            config,
            processor,
            tracers: Arc::new(RwLock::new(HashMap::new())),
            propagator: Arc::new(W3CTraceContextPropagator::new()),
        }
    }

    pub fn with_propagator(mut self, propagator: Arc<dyn TracePropagator>) -> Self {
        self.propagator = propagator;
        self
    }

    /// 获取或创建 Tracer
    /// Get or create a Tracer
    pub async fn tracer(&self, name: &str) -> Arc<Tracer> {
        // Fast path: read lock only
        {
            let tracers = self.tracers.read().await;
            if let Some(tracer) = tracers.get(name) {
                return tracer.clone();
            }
        }

        // Slow path: acquire write lock and re-check to avoid creating
        // duplicate tracers under concurrent access
        let mut tracers = self.tracers.write().await;
        if let Some(tracer) = tracers.get(name) {
            return tracer.clone();
        }

        let tracer = Arc::new(Tracer::new(
            TracerConfig {
                service_name: name.to_string(),
                ..self.config.clone()
            },
            self.processor.clone(),
        ));

        tracers.insert(name.to_string(), tracer.clone());
        tracer
    }

    /// 获取默认 Tracer
    /// Get default Tracer
    pub async fn default_tracer(&self) -> Arc<Tracer> {
        self.tracer(&self.config.service_name).await
    }

    /// 获取传播器
    /// Get propagator
    pub fn propagator(&self) -> Arc<dyn TracePropagator> {
        self.propagator.clone()
    }

    /// 关闭 Provider
    /// Shutdown Provider
    pub async fn shutdown(&self) -> Result<(), String> {
        self.processor.shutdown().await
    }
}

/// 全局 Tracer
/// Global Tracer
pub struct GlobalTracer {
    provider: Arc<RwLock<Option<Arc<TracerProvider>>>>,
}

impl GlobalTracer {
    /// 创建新的全局 Tracer 实例
    /// Create a new GlobalTracer instance
    pub fn new() -> Self {
        Self {
            provider: Arc::new(RwLock::new(None)),
        }
    }

    /// 设置全局 TracerProvider
    /// Set the global TracerProvider
    pub async fn set_provider(&self, provider: Arc<TracerProvider>) {
        let mut guard = self.provider.write().await;
        *guard = Some(provider);
    }

    /// 获取全局 TracerProvider
    /// Get the global TracerProvider
    pub async fn provider(&self) -> Option<Arc<TracerProvider>> {
        let guard = self.provider.read().await;
        guard.clone()
    }

    /// 获取默认 Tracer
    /// Get default Tracer
    pub async fn tracer(&self) -> Option<Arc<Tracer>> {
        let provider = self.provider().await?;
        Some(provider.default_tracer().await)
    }

    /// 获取指定名称的 Tracer
    /// Get Tracer with specified name
    pub async fn tracer_with_name(&self, name: &str) -> Option<Arc<Tracer>> {
        let provider = self.provider().await?;
        Some(provider.tracer(name).await)
    }
}

impl Default for GlobalTracer {
    fn default() -> Self {
        Self::new()
    }
}

// 全局静态实例
// Global static instance
lazy_static::lazy_static! {
    static ref GLOBAL_TRACER: GlobalTracer = GlobalTracer::new();
}

/// 获取全局 Tracer
/// Get the global Tracer
pub fn global_tracer() -> &'static GlobalTracer {
    &GLOBAL_TRACER
}

/// 设置全局 TracerProvider
/// Set global TracerProvider
pub async fn set_global_tracer_provider(provider: Arc<TracerProvider>) {
    GLOBAL_TRACER.set_provider(provider).await;
}

/// 获取全局默认 Tracer
/// Get global default Tracer
pub async fn get_tracer() -> Option<Arc<Tracer>> {
    GLOBAL_TRACER.tracer().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracing::exporter::{ConsoleExporter, ExporterConfig};

    #[tokio::test]
    async fn test_tracer_creation() {
        let config = ExporterConfig::new("test-service");
        let exporter = Arc::new(ConsoleExporter::new(config).with_summary_only());
        let processor = Arc::new(SimpleSpanProcessor::new(exporter));

        let tracer_config = TracerConfig::new("test-service");
        let tracer = Tracer::new(tracer_config, processor);

        let span = tracer.start_span("test-operation");
        assert!(span.is_recording().await);

        tracer.end_span(&span).await;
        assert!(span.is_ended().await);
    }

    #[tokio::test]
    async fn test_tracer_provider() {
        let config = ExporterConfig::new("test-service");
        let exporter = Arc::new(ConsoleExporter::new(config).with_summary_only());
        let processor = Arc::new(SimpleSpanProcessor::new(exporter));

        let tracer_config = TracerConfig::new("test-service");
        let provider = TracerProvider::new(tracer_config, processor);

        let tracer1 = provider.tracer("service-a").await;
        let tracer2 = provider.tracer("service-a").await;

        // 应该返回相同的 tracer
        // Should return the same tracer
        assert_eq!(tracer1.service_name(), tracer2.service_name());
    }

    #[test]
    fn test_sampling_always_on() {
        let strategy = SamplingStrategy::AlwaysOn;
        assert!(strategy.should_sample(None, &TraceId::new(), "test"));
    }

    #[test]
    fn test_sampling_always_off() {
        let strategy = SamplingStrategy::AlwaysOff;
        assert!(!strategy.should_sample(None, &TraceId::new(), "test"));
    }

    #[test]
    fn test_sampling_probabilistic() {
        let strategy = SamplingStrategy::Probabilistic(0.5);
        let mut sampled_count = 0;
        let iterations = 1000;

        for _ in 0..iterations {
            if strategy.should_sample(None, &TraceId::new(), "test") {
                sampled_count += 1;
            }
        }

        // 应该大约有一半被采样
        // Approximately half should be sampled
        let ratio = sampled_count as f64 / iterations as f64;
        assert!(ratio > 0.3 && ratio < 0.7);
    }
}
