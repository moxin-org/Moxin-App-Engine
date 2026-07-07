//! Tracing 导出器
//! Tracing Exporter
//!
//! 支持多种导出格式：Console、Jaeger、OTLP
//! Supports multiple export formats: Console, Jaeger, OTLP

use super::span::SpanData;
use async_trait::async_trait;
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, error, info};

/// 导出器配置
/// Exporter configuration
#[derive(Debug, Clone)]
pub struct ExporterConfig {
    /// 服务名称
    /// Service name
    pub service_name: String,
    /// 批量大小
    /// Batch size
    pub batch_size: usize,
    /// 导出间隔（毫秒）
    /// Export interval (milliseconds)
    pub export_interval_ms: u64,
    /// 最大队列大小
    /// Maximum queue size
    pub max_queue_size: usize,
}

impl Default for ExporterConfig {
    fn default() -> Self {
        Self {
            service_name: "unknown-service".to_string(),
            batch_size: 512,
            export_interval_ms: 5000,
            max_queue_size: 2048,
        }
    }
}

impl ExporterConfig {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            ..Default::default()
        }
    }

    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    pub fn with_export_interval(mut self, interval_ms: u64) -> Self {
        self.export_interval_ms = interval_ms;
        self
    }

    pub fn with_max_queue_size(mut self, max_size: usize) -> Self {
        self.max_queue_size = max_size;
        self
    }
}

/// 追踪导出器 trait
/// Tracing exporter trait
#[async_trait]
pub trait TracingExporter: Send + Sync {
    /// 导出 spans
    /// Export spans
    async fn export(&self, spans: Vec<SpanData>) -> Result<(), String>;

    /// 关闭导出器
    /// Shutdown exporter
    async fn shutdown(&self) -> Result<(), String>;

    /// 强制刷新
    /// Force flush
    async fn force_flush(&self) -> Result<(), String>;
}

/// Console 导出器 - 输出到控制台
/// Console Exporter - Outputs to the console
pub struct ConsoleExporter {
    config: ExporterConfig,
    /// 是否使用 JSON 格式
    /// Whether to use JSON format
    json_format: bool,
    /// 是否只输出摘要
    /// Whether to output summary only
    summary_only: bool,
}

impl ConsoleExporter {
    pub fn new(config: ExporterConfig) -> Self {
        Self {
            config,
            json_format: false,
            summary_only: false,
        }
    }

    pub fn with_json_format(mut self) -> Self {
        self.json_format = true;
        self
    }

    pub fn with_summary_only(mut self) -> Self {
        self.summary_only = true;
        self
    }

    fn format_span(&self, span: &SpanData) -> String {
        if self.json_format {
            serde_json::to_string_pretty(span).unwrap_or_else(|_| format!("{:?}", span))
        } else if self.summary_only {
            let duration = span
                .end_time
                .map(|end| (end - span.start_time).num_milliseconds())
                .unwrap_or(0);
            format!(
                "[{}] {} | trace={} span={} | {}ms | {:?}",
                span.kind,
                span.name,
                span.span_context.trace_id,
                span.span_context.span_id,
                duration,
                span.status
            )
        } else {
            let duration = span
                .end_time
                .map(|end| (end - span.start_time).num_milliseconds())
                .unwrap_or(0);
            let parent = span
                .parent_span_context
                .as_ref()
                .map(|p| p.span_id.to_hex())
                .unwrap_or_else(|| "none".to_string());

            format!(
                r#"
┌─ Span ─────────────────────────────────────────────────────
│ Name:      {}
│ Service:   {}
│ Kind:      {}
│ TraceId:   {}
│ SpanId:    {}
│ ParentId:  {}
│ Duration:  {}ms
│ Status:    {:?}
│ Attributes: {:?}
│ Events:    {} events
└────────────────────────────────────────────────────────────"#,
                span.name,
                span.service_name,
                span.kind,
                span.span_context.trace_id,
                span.span_context.span_id,
                parent,
                duration,
                span.status,
                span.attributes,
                span.events.len()
            )
        }
    }
}

#[async_trait]
impl TracingExporter for ConsoleExporter {
    async fn export(&self, spans: Vec<SpanData>) -> Result<(), String> {
        for span in spans {
            info!("{}", self.format_span(&span));
        }
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), String> {
        info!("Console exporter shutdown");
        Ok(())
    }

    async fn force_flush(&self) -> Result<(), String> {
        Ok(())
    }
}

/// Jaeger 导出器配置
/// Jaeger exporter configuration
#[derive(Debug, Clone)]
pub struct JaegerConfig {
    /// Agent 地址
    /// Agent endpoint address
    pub agent_endpoint: String,
    /// Collector 地址（可选，优先于 agent）
    /// Collector endpoint (optional, takes priority over agent)
    pub collector_endpoint: Option<String>,
    /// 用户名（collector 认证）
    /// Username (collector authentication)
    pub username: Option<String>,
    /// 密码（collector 认证）
    /// Password (collector authentication)
    pub password: Option<String>,
}

impl Default for JaegerConfig {
    fn default() -> Self {
        Self {
            agent_endpoint: "127.0.0.1:6831".to_string(),
            collector_endpoint: None,
            username: None,
            password: None,
        }
    }
}

/// Jaeger 导出器
/// Jaeger Exporter
pub struct JaegerExporter {
    config: ExporterConfig,
    jaeger_config: JaegerConfig,
    buffer: Arc<RwLock<Vec<SpanData>>>,
}

impl JaegerExporter {
    pub fn new(config: ExporterConfig, jaeger_config: JaegerConfig) -> Self {
        Self {
            config,
            jaeger_config,
            buffer: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 将 SpanData 转换为 Jaeger Thrift 格式
    /// Convert SpanData to Jaeger Thrift format
    fn to_jaeger_span(&self, span: &SpanData) -> serde_json::Value {
        let duration_us = span
            .end_time
            .map(|end| (end - span.start_time).num_microseconds().unwrap_or(0))
            .unwrap_or(0);

        let parent_span_id = span
            .parent_span_context
            .as_ref()
            .map(|p| p.span_id.to_hex())
            .unwrap_or_default();

        serde_json::json!({
            "traceIdHigh": 0,
            "traceIdLow": span.span_context.trace_id.to_hex(),
            "spanId": span.span_context.span_id.to_hex(),
            "parentSpanId": parent_span_id,
            "operationName": span.name,
            "references": [],
            "flags": span.span_context.trace_flags.as_u8(),
            "startTime": span.start_time.timestamp_micros(),
            "duration": duration_us,
            "tags": span.attributes.iter().map(|(k, v)| {
                serde_json::json!({
                    "key": k,
                    "type": "string",
                    "value": format!("{:?}", v)
                })
            }).collect::<Vec<_>>(),
            "logs": span.events.iter().map(|e| {
                serde_json::json!({
                    "timestamp": e.timestamp.timestamp_micros(),
                    "fields": [{
                        "key": "event",
                        "type": "string",
                        "value": e.name
                    }]
                })
            }).collect::<Vec<_>>(),
            "process": {
                "serviceName": span.service_name,
                "tags": []
            }
        })
    }

    async fn send_to_collector(&self, spans: &[SpanData]) -> Result<(), String> {
        if let Some(ref endpoint) = self.jaeger_config.collector_endpoint {
            let batch: Vec<_> = spans.iter().map(|s| self.to_jaeger_span(s)).collect();

            debug!(
                "Sending {} spans to Jaeger collector at {}",
                batch.len(),
                endpoint
            );

            let client = Client::new();

            let url = format!("{}/api/traces", endpoint.trim_end_matches('/'));

            let response = client
                .post(&url)
                .json(&batch)
                .send()
                .await
                .map_err(|e| format!("Failed to send spans to Jaeger: {}", e))?;

            if !response.status().is_success() {
                return Err(format!(
                    "Jaeger collector returned non-success status: {}",
                    response.status()
                ));
            }

            info!("Successfully sent {} spans to Jaeger", batch.len());

            Ok(())
        } else {
            Err("No collector endpoint configured".to_string())
        }
    }
}

#[async_trait]
impl TracingExporter for JaegerExporter {
    async fn export(&self, spans: Vec<SpanData>) -> Result<(), String> {
        if spans.is_empty() {
            return Ok(());
        }

        {
            let mut buffer = self.buffer.write().await;
            buffer.extend(spans);

            if buffer.len() >= self.config.batch_size {
                let to_export: Vec<_> = buffer.drain(..).collect();
                drop(buffer);
                return self.send_to_collector(&to_export).await;
            }
        }

        Ok(())
    }

    async fn shutdown(&self) -> Result<(), String> {
        self.force_flush().await?;
        info!("Jaeger exporter shutdown");
        Ok(())
    }

    async fn force_flush(&self) -> Result<(), String> {
        let to_export: Vec<_> = {
            let mut buffer = self.buffer.write().await;
            buffer.drain(..).collect()
        };

        if !to_export.is_empty() {
            self.send_to_collector(&to_export).await?;
        }

        Ok(())
    }
}

/// OTLP 导出器配置
/// OTLP exporter configuration
#[derive(Debug, Clone)]
pub struct OtlpConfig {
    /// Endpoint
    /// Endpoint
    pub endpoint: String,
    /// 协议 (grpc 或 http)
    /// Protocol (grpc or http)
    pub protocol: OtlpProtocol,
    /// Headers
    /// Headers
    pub headers: std::collections::HashMap<String, String>,
    /// 超时（毫秒）
    /// Timeout (milliseconds)
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtlpProtocol {
    Grpc,
    HttpProtobuf,
    HttpJson,
}

impl Default for OtlpConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:4317".to_string(),
            protocol: OtlpProtocol::Grpc,
            headers: std::collections::HashMap::new(),
            timeout_ms: 10000,
        }
    }
}

/// OTLP 导出器
/// OTLP Exporter
pub struct OtlpExporter {
    config: ExporterConfig,
    otlp_config: OtlpConfig,
    buffer: Arc<RwLock<Vec<SpanData>>>,
}

impl OtlpExporter {
    pub fn new(config: ExporterConfig, otlp_config: OtlpConfig) -> Self {
        Self {
            config,
            otlp_config,
            buffer: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 将 SpanData 转换为 OTLP 格式
    /// Convert SpanData to OTLP format
    fn to_otlp_span(&self, span: &SpanData) -> serde_json::Value {
        let _duration_ns = span
            .end_time
            .map(|end| (end - span.start_time).num_nanoseconds().unwrap_or(0))
            .unwrap_or(0);

        let status_code = match &span.status {
            super::span::SpanStatus::Unset => 0,
            super::span::SpanStatus::Ok => 1,
            super::span::SpanStatus::Error { .. } => 2,
        };

        serde_json::json!({
            "traceId": span.span_context.trace_id.to_hex(),
            "spanId": span.span_context.span_id.to_hex(),
            "parentSpanId": span.parent_span_context.as_ref().map(|p| p.span_id.to_hex()),
            "name": span.name,
            "kind": match span.kind {
                super::span::SpanKind::Internal => 1,
                super::span::SpanKind::Server => 2,
                super::span::SpanKind::Client => 3,
                super::span::SpanKind::Producer => 4,
                super::span::SpanKind::Consumer => 5,
            },
            "startTimeUnixNano": span.start_time.timestamp_nanos_opt().unwrap_or(0),
            "endTimeUnixNano": span.end_time.map(|t| t.timestamp_nanos_opt().unwrap_or(0)),
            "attributes": span.attributes.iter().map(|(k, v)| {
                serde_json::json!({
                    "key": k,
                    "value": { "stringValue": format!("{:?}", v) }
                })
            }).collect::<Vec<_>>(),
            "events": span.events.iter().map(|e| {
                serde_json::json!({
                    "timeUnixNano": e.timestamp.timestamp_nanos_opt().unwrap_or(0),
                    "name": e.name,
                    "attributes": e.attributes.iter().map(|(k, v)| {
                        serde_json::json!({
                            "key": k,
                            "value": { "stringValue": format!("{:?}", v) }
                        })
                    }).collect::<Vec<_>>()
                })
            }).collect::<Vec<_>>(),
            "status": {
                "code": status_code,
                "message": match &span.status {
                    super::span::SpanStatus::Error { message } => message.clone(),
                    _ => String::new(),
                }
            }
        })
    }

    async fn send_to_otlp(&self, spans: &[SpanData]) -> Result<(), String> {
        let resource_spans = serde_json::json!({
            "resourceSpans": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": { "stringValue": self.config.service_name }
                    }]
                },
                "scopeSpans": [{
                    "scope": {
                        "name": "mofa-tracing",
                        "version": "0.1.0"
                    },
                    "spans": spans.iter().map(|s| self.to_otlp_span(s)).collect::<Vec<_>>()
                }]
            }]
        });

        let endpoint = &self.otlp_config.endpoint;
        let client = Client::new();

        let url = format!("{}/v1/traces", endpoint.trim_end_matches('/'));

        let mut request = client.post(&url).json(&resource_spans);

        for (key, value) in &self.otlp_config.headers {
            request = request.header(key, value);
        }

        let request = request.timeout(std::time::Duration::from_millis(
            self.otlp_config.timeout_ms,
        ));

        let response = request
            .send()
            .await
            .map_err(|e| format!("Failed to send spans to OTLP endpoint: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "OTLP endpoint returned non-success status: {}",
                response.status()
            ));
        }

        info!("Successfully sent {} spans to OTLP endpoint", spans.len());

        Ok(())
    }
}

#[async_trait]
impl TracingExporter for OtlpExporter {
    async fn export(&self, spans: Vec<SpanData>) -> Result<(), String> {
        if spans.is_empty() {
            return Ok(());
        }

        {
            let mut buffer = self.buffer.write().await;
            buffer.extend(spans);

            if buffer.len() >= self.config.batch_size {
                let to_export: Vec<_> = buffer.drain(..).collect();
                drop(buffer);
                return self.send_to_otlp(&to_export).await;
            }
        }

        Ok(())
    }

    async fn shutdown(&self) -> Result<(), String> {
        self.force_flush().await?;
        info!("OTLP exporter shutdown");
        Ok(())
    }

    async fn force_flush(&self) -> Result<(), String> {
        let to_export: Vec<_> = {
            let mut buffer = self.buffer.write().await;
            buffer.drain(..).collect()
        };

        if !to_export.is_empty() {
            self.send_to_otlp(&to_export).await?;
        }

        Ok(())
    }
}

/// 复合导出器 - 同时导出到多个目标
/// Composite Exporter - Exports to multiple targets simultaneously
pub struct CompositeExporter {
    exporters: Vec<Arc<dyn TracingExporter>>,
}

impl CompositeExporter {
    pub fn new(exporters: Vec<Arc<dyn TracingExporter>>) -> Self {
        Self { exporters }
    }

    pub fn add_exporter(&mut self, exporter: Arc<dyn TracingExporter>) {
        self.exporters.push(exporter);
    }
}

#[async_trait]
impl TracingExporter for CompositeExporter {
    async fn export(&self, spans: Vec<SpanData>) -> Result<(), String> {
        let mut errors = Vec::new();

        for exporter in &self.exporters {
            if let Err(e) = exporter.export(spans.clone()).await {
                errors.push(e);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    async fn shutdown(&self) -> Result<(), String> {
        let mut errors = Vec::new();

        for exporter in &self.exporters {
            if let Err(e) = exporter.shutdown().await {
                errors.push(e);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    async fn force_flush(&self) -> Result<(), String> {
        let mut errors = Vec::new();

        for exporter in &self.exporters {
            if let Err(e) = exporter.force_flush().await {
                errors.push(e);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

/// 异步批处理导出器
/// Asynchronous batch exporter
pub struct BatchExporter {
    exporter: Arc<dyn TracingExporter>,
    sender: mpsc::Sender<SpanData>,
    _task: tokio::task::JoinHandle<()>,
}

impl BatchExporter {
    pub fn new(exporter: Arc<dyn TracingExporter>, config: ExporterConfig) -> Self {
        let (sender, mut receiver) = mpsc::channel::<SpanData>(config.max_queue_size);

        let export_interval = std::time::Duration::from_millis(config.export_interval_ms);
        let batch_size = config.batch_size;
        let exporter_clone = exporter.clone();

        // 启动后台导出任务
        // Start background export task
        let task = tokio::spawn(async move {
            let mut buffer = Vec::with_capacity(batch_size);
            let mut interval = tokio::time::interval(export_interval);

            loop {
                tokio::select! {
                    msg = receiver.recv() => {
                        match msg {
                            Some(span) => {
                                buffer.push(span);
                                if buffer.len() >= batch_size {
                                    let to_export: Vec<_> = std::mem::take(&mut buffer);
                                    if let Err(e) = exporter_clone.export(to_export).await {
                                        error!("Failed to export spans: {}", e);
                                    }
                                }
                            }
                            // All senders dropped: flush remaining spans and exit.
                            None => {
                                if !buffer.is_empty() {
                                    let to_export: Vec<_> = std::mem::take(&mut buffer);
                                    if let Err(e) = exporter_clone.export(to_export).await {
                                        error!("Failed to export spans on shutdown: {}", e);
                                    }
                                }
                                break;
                            }
                        }
                    }
                    _ = interval.tick() => {
                        if !buffer.is_empty() {
                            let to_export: Vec<_> = std::mem::take(&mut buffer);
                            if let Err(e) = exporter_clone.export(to_export).await {
                                error!("Failed to export spans: {}", e);
                            }
                        }
                    }
                }
            }
        });

        Self {
            exporter,
            sender,
            _task: task,
        }
    }

    pub async fn record(&self, span: SpanData) -> Result<(), String> {
        self.sender
            .send(span)
            .await
            .map_err(|e| format!("Failed to send span: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracing::context::{SpanContext, SpanId, TraceFlags, TraceId};
    use crate::tracing::span::{SpanKind, SpanStatus};

    fn create_test_span() -> SpanData {
        SpanData {
            span_context: SpanContext::new(
                TraceId::new(),
                SpanId::new(),
                TraceFlags::SAMPLED,
                false,
            ),
            parent_span_context: None,
            name: "test-span".to_string(),
            kind: SpanKind::Internal,
            start_time: chrono::Utc::now(),
            end_time: Some(chrono::Utc::now()),
            status: SpanStatus::Ok,
            attributes: std::collections::HashMap::new(),
            events: Vec::new(),
            links: Vec::new(),
            service_name: "test-service".to_string(),
        }
    }

    #[tokio::test]
    async fn test_console_exporter() {
        let config = ExporterConfig::new("test-service");
        let exporter = ConsoleExporter::new(config).with_summary_only();

        let spans = vec![create_test_span()];
        let result = exporter.export(spans).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_composite_exporter() {
        let config = ExporterConfig::new("test-service");
        let console = Arc::new(ConsoleExporter::new(config.clone()).with_summary_only());

        let composite = CompositeExporter::new(vec![console]);

        let spans = vec![create_test_span()];
        let result = composite.export(spans).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_jaeger_exporter_buffers_spans() {
        let config = ExporterConfig::new("test-service").with_batch_size(100);
        let jaeger_config = JaegerConfig::default();
        let exporter = JaegerExporter::new(config, jaeger_config);

        // Export fewer spans than batch_size — they should be buffered, not sent.
        let spans = vec![create_test_span(), create_test_span()];
        let result = exporter.export(spans).await;
        assert!(result.is_ok());

        // Verify the exporter can be used as a dyn TracingExporter.
        let dyn_exporter: Arc<dyn TracingExporter> = Arc::new({
            let config = ExporterConfig::new("test-service").with_batch_size(100);
            let jaeger_config = JaegerConfig::default();
            JaegerExporter::new(config, jaeger_config)
        });
        let result = dyn_exporter.export(vec![create_test_span()]).await;
        assert!(result.is_ok());
    }
}
