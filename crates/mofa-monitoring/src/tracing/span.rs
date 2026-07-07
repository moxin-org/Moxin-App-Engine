//! Span 定义和管理
//! Span definition and management
//!
//! 实现分布式追踪的 Span 概念
//! Implementation of Span concept for distributed tracing

use super::context::{SpanContext, SpanId, TraceId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Span 类型
/// Span types
///
/// 表示 Span 在分布式追踪系统中的角色。根据 OpenTelemetry 标准定义。
/// Represents the role of a Span in a distributed tracing system. Defined according to OpenTelemetry standards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SpanKind {
    /// 内部操作：在应用内部发生的常规操作。
    /// Internal operation: Regular operations occurring within the application.
    #[default]
    Internal,
    /// 服务器端：处理来自外部的传入请求。
    /// Server side: Handling an incoming request from an external source.
    Server,
    /// 客户端：向外部服务发起传出请求。
    /// Client side: Initiating an outgoing request to an external service.
    Client,
    /// 消息生产者：向队列或主题发送消息。
    /// Message producer: Sending a message to a queue or topic.
    Producer,
    /// 消息消费者：从队列或主题处理消息。
    /// Message consumer: Processing a message from a queue or topic.
    Consumer,
}

impl std::fmt::Display for SpanKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpanKind::Internal => write!(f, "INTERNAL"),
            SpanKind::Server => write!(f, "SERVER"),
            SpanKind::Client => write!(f, "CLIENT"),
            SpanKind::Producer => write!(f, "PRODUCER"),
            SpanKind::Consumer => write!(f, "CONSUMER"),
        }
    }
}

/// Span 状态
/// Span status
///
/// 表示 Span 完成时的结果状态（成功、错误或未设置）。
/// Represents the resulting status of a Span upon completion (Success, Error, or Unset).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum SpanStatus {
    /// 未设置：默认状态，表示没有明确的成功或失败。
    /// Unset: Default status, indicating no explicit success or failure.
    #[default]
    Unset,
    /// 成功：操作已成功完成。
    /// Success: The operation completed successfully.
    Ok,
    /// 错误：操作失败，并附带错误消息。
    /// Error: The operation failed, accompanied by an error message.
    Error { message: String },
}

/// Span 属性值
/// Span attribute value
///
/// 这是一个枚举，涵盖了可以附加到 Span 的各种数据类型，包括基本类型及其数组。
/// An enum covering various data types that can be attached to a Span, including primitives and their arrays.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SpanAttribute {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    StringArray(Vec<String>),
    IntArray(Vec<i64>),
    FloatArray(Vec<f64>),
    BoolArray(Vec<bool>),
}

impl From<&str> for SpanAttribute {
    fn from(v: &str) -> Self {
        SpanAttribute::String(v.to_string())
    }
}

impl From<String> for SpanAttribute {
    fn from(v: String) -> Self {
        SpanAttribute::String(v)
    }
}

impl From<i64> for SpanAttribute {
    fn from(v: i64) -> Self {
        SpanAttribute::Int(v)
    }
}

impl From<i32> for SpanAttribute {
    fn from(v: i32) -> Self {
        SpanAttribute::Int(v as i64)
    }
}

impl From<f64> for SpanAttribute {
    fn from(v: f64) -> Self {
        SpanAttribute::Float(v)
    }
}

impl From<bool> for SpanAttribute {
    fn from(v: bool) -> Self {
        SpanAttribute::Bool(v)
    }
}

impl From<Vec<String>> for SpanAttribute {
    fn from(v: Vec<String>) -> Self {
        SpanAttribute::StringArray(v)
    }
}

/// Span 事件
/// Span event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEvent {
    /// 事件名称
    /// Event name
    pub name: String,
    /// 事件时间
    /// Event timestamp
    pub timestamp: DateTime<Utc>,
    /// 事件属性
    /// Event attributes
    pub attributes: HashMap<String, SpanAttribute>,
}

impl SpanEvent {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            timestamp: Utc::now(),
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(
        mut self,
        key: impl Into<String>,
        value: impl Into<SpanAttribute>,
    ) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

/// Span 链接 - 关联到其他 Span
/// Span link - associate with other Spans
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanLink {
    /// 链接的 SpanContext
    /// Linked SpanContext
    pub span_context: SpanContext,
    /// 链接属性
    /// Link attributes
    pub attributes: HashMap<String, SpanAttribute>,
}

impl SpanLink {
    pub fn new(span_context: SpanContext) -> Self {
        Self {
            span_context,
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(
        mut self,
        key: impl Into<String>,
        value: impl Into<SpanAttribute>,
    ) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

/// Span 数据
/// Span data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanData {
    /// Span 上下文
    /// Span context
    pub span_context: SpanContext,
    /// 父 Span 上下文
    /// Parent Span context
    pub parent_span_context: Option<SpanContext>,
    /// Span 名称
    /// Span name
    pub name: String,
    /// Span 类型
    /// Span kind
    pub kind: SpanKind,
    /// 开始时间
    /// Start time
    pub start_time: DateTime<Utc>,
    /// 结束时间
    /// End time
    pub end_time: Option<DateTime<Utc>>,
    /// 状态
    /// Status
    pub status: SpanStatus,
    /// 属性
    /// Attributes
    pub attributes: HashMap<String, SpanAttribute>,
    /// 事件
    /// Events
    pub events: Vec<SpanEvent>,
    /// 链接
    /// Links
    pub links: Vec<SpanLink>,
    /// 服务名称
    /// Service name
    pub service_name: String,
}

/// Span 内部状态
/// Span internal state
struct SpanInner {
    data: SpanData,
    is_recording: bool,
    is_ended: bool,
}

/// Span - 追踪的基本单元
/// Span - basic unit of tracing
pub struct Span {
    inner: Arc<RwLock<SpanInner>>,
}

impl Span {
    /// 创建新的 Span
    /// Create new Span
    pub(crate) fn new(
        name: impl Into<String>,
        span_context: SpanContext,
        parent_span_context: Option<SpanContext>,
        kind: SpanKind,
        service_name: impl Into<String>,
    ) -> Self {
        let inner = SpanInner {
            data: SpanData {
                span_context,
                parent_span_context,
                name: name.into(),
                kind,
                start_time: Utc::now(),
                end_time: None,
                status: SpanStatus::Unset,
                attributes: HashMap::new(),
                events: Vec::new(),
                links: Vec::new(),
                service_name: service_name.into(),
            },
            is_recording: true,
            is_ended: false,
        };
        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }

    /// 创建非记录的 Span（用于未采样的情况）
    /// Create non-recording Span (for non-sampled cases)
    pub fn non_recording(span_context: SpanContext) -> Self {
        let inner = SpanInner {
            data: SpanData {
                span_context,
                parent_span_context: None,
                name: String::new(),
                kind: SpanKind::Internal,
                start_time: Utc::now(),
                end_time: None,
                status: SpanStatus::Unset,
                attributes: HashMap::new(),
                events: Vec::new(),
                links: Vec::new(),
                service_name: String::new(),
            },
            is_recording: false,
            is_ended: false,
        };
        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }

    /// 获取 SpanContext
    /// Get SpanContext
    pub async fn span_context(&self) -> SpanContext {
        self.inner.read().await.data.span_context.clone()
    }

    /// 获取 Trace ID
    /// Get Trace ID
    pub async fn trace_id(&self) -> TraceId {
        self.inner.read().await.data.span_context.trace_id
    }

    /// 获取 Span ID
    /// Get Span ID
    pub async fn span_id(&self) -> SpanId {
        self.inner.read().await.data.span_context.span_id
    }

    /// 是否正在记录
    /// Check if currently recording
    pub async fn is_recording(&self) -> bool {
        self.inner.read().await.is_recording
    }

    /// 是否已结束
    /// Check if already ended
    pub async fn is_ended(&self) -> bool {
        self.inner.read().await.is_ended
    }

    /// 设置属性
    /// Set attribute
    pub async fn set_attribute(&self, key: impl Into<String>, value: impl Into<SpanAttribute>) {
        let mut inner = self.inner.write().await;
        if inner.is_recording && !inner.is_ended {
            inner.data.attributes.insert(key.into(), value.into());
        }
    }

    /// 批量设置属性
    /// Batch set attributes
    pub async fn set_attributes(
        &self,
        attributes: impl IntoIterator<Item = (String, SpanAttribute)>,
    ) {
        let mut inner = self.inner.write().await;
        if inner.is_recording && !inner.is_ended {
            for (key, value) in attributes {
                inner.data.attributes.insert(key, value);
            }
        }
    }

    /// 添加事件
    /// Add event
    pub async fn add_event(&self, event: SpanEvent) {
        let mut inner = self.inner.write().await;
        if inner.is_recording && !inner.is_ended {
            inner.data.events.push(event);
        }
    }

    /// 添加简单事件
    /// Add simple event
    pub async fn add_event_with_name(&self, name: impl Into<String>) {
        self.add_event(SpanEvent::new(name)).await;
    }

    /// 添加链接
    /// Add link
    pub async fn add_link(&self, link: SpanLink) {
        let mut inner = self.inner.write().await;
        if inner.is_recording && !inner.is_ended {
            inner.data.links.push(link);
        }
    }

    /// 设置状态
    /// Set status
    pub async fn set_status(&self, status: SpanStatus) {
        let mut inner = self.inner.write().await;
        if inner.is_recording && !inner.is_ended {
            inner.data.status = status;
        }
    }

    /// 设置为成功状态
    /// Set as success status
    pub async fn set_ok(&self) {
        self.set_status(SpanStatus::Ok).await;
    }

    /// 设置为错误状态
    /// Set as error status
    pub async fn set_error(&self, message: impl Into<String>) {
        self.set_status(SpanStatus::Error {
            message: message.into(),
        })
        .await;
    }

    /// 记录异常
    /// Record exception
    pub async fn record_exception(&self, error: &dyn std::error::Error) {
        let event = SpanEvent::new("exception")
            .with_attribute("exception.type", std::any::type_name_of_val(error))
            .with_attribute("exception.message", error.to_string());
        self.add_event(event).await;
        self.set_error(error.to_string()).await;
    }

    /// 更新名称
    /// Update name
    pub async fn update_name(&self, name: impl Into<String>) {
        let mut inner = self.inner.write().await;
        if inner.is_recording && !inner.is_ended {
            inner.data.name = name.into();
        }
    }

    /// 结束 Span
    /// End Span
    pub async fn end(&self) {
        let mut inner = self.inner.write().await;
        if !inner.is_ended {
            inner.is_ended = true;
            inner.data.end_time = Some(Utc::now());
        }
    }

    /// 结束 Span 并指定时间
    /// End Span with specific timestamp
    pub async fn end_with_timestamp(&self, timestamp: DateTime<Utc>) {
        let mut inner = self.inner.write().await;
        if !inner.is_ended {
            inner.is_ended = true;
            inner.data.end_time = Some(timestamp);
        }
    }

    /// 获取 Span 数据（用于导出）
    /// Get Span data (for export)
    pub async fn get_data(&self) -> SpanData {
        self.inner.read().await.data.clone()
    }

    /// 获取持续时间（毫秒）
    /// Get duration (milliseconds)
    pub async fn duration_ms(&self) -> Option<i64> {
        let inner = self.inner.read().await;
        inner
            .data
            .end_time
            .map(|end| (end - inner.data.start_time).num_milliseconds())
    }
}

impl Clone for Span {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// Span 构建器
/// Span 构建器
/// Span builder
///
/// 提供流式接口来配置和创建 Span。支持设置类型、父上下文、属性、链接和开始时间。
/// Provides a fluent interface to configure and create Spans. Supports setting kind, parent context, attributes, links, and start time.
///
/// # Example
///
/// ```rust,no_run
/// use mofa_monitoring::tracing::{SpanBuilder, SpanKind};
///
/// let span = SpanBuilder::new("my-operation", "example-service")
///     .with_kind(SpanKind::Server)
///     .with_attribute("http.method", "GET")
///     .with_attribute("http.url", "https://example.com")
///     .start();
/// ```
pub struct SpanBuilder {
    name: String,
    kind: SpanKind,
    parent_context: Option<SpanContext>,
    attributes: HashMap<String, SpanAttribute>,
    links: Vec<SpanLink>,
    start_time: Option<DateTime<Utc>>,
    service_name: String,
}

impl SpanBuilder {
    /// 创建新的 SpanBuilder
    /// Create new SpanBuilder
    pub fn new(name: impl Into<String>, service_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: SpanKind::Internal,
            parent_context: None,
            attributes: HashMap::new(),
            links: Vec::new(),
            start_time: None,
            service_name: service_name.into(),
        }
    }

    /// 设置 Span 类型
    /// Set Span kind
    pub fn with_kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }

    /// 设置父上下文
    /// Set parent context
    pub fn with_parent(mut self, parent: SpanContext) -> Self {
        self.parent_context = Some(parent);
        self
    }

    /// 添加属性
    /// Add attribute
    pub fn with_attribute(
        mut self,
        key: impl Into<String>,
        value: impl Into<SpanAttribute>,
    ) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// 添加多个属性
    /// Add multiple attributes
    pub fn with_attributes(
        mut self,
        attributes: impl IntoIterator<Item = (String, SpanAttribute)>,
    ) -> Self {
        for (key, value) in attributes {
            self.attributes.insert(key, value);
        }
        self
    }

    /// 添加链接
    /// Add link
    pub fn with_link(mut self, link: SpanLink) -> Self {
        self.links.push(link);
        self
    }

    /// 设置开始时间
    /// Set start time
    pub fn with_start_time(mut self, start_time: DateTime<Utc>) -> Self {
        self.start_time = Some(start_time);
        self
    }

    /// 构建 Span
    /// Build Span
    pub fn start(self) -> Span {
        use super::context::{TraceFlags, TraceId};

        let (trace_id, _parent_span_id) = match &self.parent_context {
            Some(parent) => (parent.trace_id, Some(parent.span_id)),
            None => (TraceId::new(), None),
        };

        let span_context = SpanContext::new(trace_id, SpanId::new(), TraceFlags::SAMPLED, false);

        let span = Span::new(
            self.name,
            span_context,
            self.parent_context,
            self.kind,
            self.service_name,
        );

        // 设置属性
        // Set attributes
        let span_clone = span.clone();
        let attributes = self.attributes;
        let links = self.links;

        tokio::spawn(async move {
            for (key, value) in attributes {
                span_clone.set_attribute(key, value).await;
            }
            for link in links {
                span_clone.add_link(link).await;
            }
        });

        span
    }
}

#[cfg(test)]
mod tests {
    use super::super::context::TraceFlags;
    use super::*;

    #[tokio::test]
    async fn test_span_creation() {
        let span_context =
            SpanContext::new(TraceId::new(), SpanId::new(), TraceFlags::SAMPLED, false);

        let span = Span::new(
            "test-span",
            span_context.clone(),
            None,
            SpanKind::Internal,
            "test-service",
        );

        assert!(span.is_recording().await);
        assert!(!span.is_ended().await);
        assert_eq!(span.trace_id().await, span_context.trace_id);
    }

    #[tokio::test]
    async fn test_span_attributes() {
        let span_context =
            SpanContext::new(TraceId::new(), SpanId::new(), TraceFlags::SAMPLED, false);

        let span = Span::new(
            "test-span",
            span_context,
            None,
            SpanKind::Internal,
            "test-service",
        );

        span.set_attribute("key1", "value1").await;
        span.set_attribute("key2", 42i64).await;
        span.set_attribute("key3", true).await;

        let data = span.get_data().await;
        assert_eq!(data.attributes.len(), 3);
    }

    #[tokio::test]
    async fn test_span_events() {
        let span_context =
            SpanContext::new(TraceId::new(), SpanId::new(), TraceFlags::SAMPLED, false);

        let span = Span::new(
            "test-span",
            span_context,
            None,
            SpanKind::Internal,
            "test-service",
        );

        span.add_event_with_name("event1").await;
        span.add_event(SpanEvent::new("event2").with_attribute("attr", "value"))
            .await;

        let data = span.get_data().await;
        assert_eq!(data.events.len(), 2);
    }

    #[tokio::test]
    async fn test_span_end() {
        let span_context =
            SpanContext::new(TraceId::new(), SpanId::new(), TraceFlags::SAMPLED, false);

        let span = Span::new(
            "test-span",
            span_context,
            None,
            SpanKind::Internal,
            "test-service",
        );

        assert!(!span.is_ended().await);
        span.end().await;
        assert!(span.is_ended().await);

        let data = span.get_data().await;
        assert!(data.end_time.is_some());
    }

    #[tokio::test]
    async fn test_span_builder() {
        let span = SpanBuilder::new("test-span", "test-service")
            .with_kind(SpanKind::Server)
            .with_attribute("http.method", "GET")
            .start();

        let data = span.get_data().await;
        assert_eq!(data.name, "test-span");
        assert_eq!(data.kind, SpanKind::Server);
    }
}
