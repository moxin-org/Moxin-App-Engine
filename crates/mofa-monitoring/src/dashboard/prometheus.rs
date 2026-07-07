//! Prometheus metrics export bridge for dashboard metrics.
//!
//! Renders a [`MetricsSnapshot`] into the Prometheus text exposition format
//! and serves it from `GET /metrics`.
//!
//! ## LLM Metrics Exported
//!
//! | Metric | Type | Labels |
//! |--------|------|--------|
//! | `mofa_llm_requests_total` | counter | `provider`, `model` |
//! | `mofa_llm_errors_total` | counter | `provider`, `model` |
//! | `mofa_llm_input_tokens_total` | counter | `provider`, `model` |
//! | `mofa_llm_output_tokens_total` | counter | `provider`, `model` |
//! | `mofa_llm_tokens_per_second` | gauge | `provider`, `model` |
//! | `mofa_llm_latency_seconds` | gauge | `provider`, `model` |
//! | `mofa_llm_time_to_first_token_seconds` | gauge | `provider`, `model` |
//! | `mofa_llm_request_duration_seconds` | histogram | `provider`, `model` |
//!
//! ## Practical PromQL Examples
//!
//! ```promql
//! # Token consumption rate (cost proxy)
//! rate(mofa_llm_input_tokens_total[1m]) + rate(mofa_llm_output_tokens_total[1m])
//!
//! # Error rate per model
//! rate(mofa_llm_errors_total[5m]) / rate(mofa_llm_requests_total[5m])
//!
//! # P95 request latency
//! histogram_quantile(0.95, rate(mofa_llm_request_duration_seconds_bucket[5m]))
//! ```

use super::metrics::{MetricValue, MetricsCollector, MetricsSnapshot};
use axum::body::Bytes;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{debug, warn};

const OTHER_LABEL_VALUE: &str = "__other__";
const MOFA_METRICS_CONTRACT_VERSION: &str = "v1alpha1";
const MAX_CARDINALITY_LIMIT_PER_DIMENSION: usize = 1_000;

/// Cardinality limits for exported label dimensions.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CardinalityLimits {
    /// Maximum number of distinct `agent_id` series.
    pub agent_id: usize,
    /// Maximum number of distinct `workflow_id` series.
    pub workflow_id: usize,
    /// Maximum number of distinct `plugin_id`/`tool_name` series.
    pub plugin_or_tool: usize,
    /// Maximum number of distinct `(provider, model)` series.
    pub provider_model: usize,
}

impl Default for CardinalityLimits {
    fn default() -> Self {
        Self {
            agent_id: 100,
            workflow_id: 100,
            plugin_or_tool: 100,
            provider_model: 50,
        }
    }
}

impl CardinalityLimits {
    pub fn with_agent_id(mut self, limit: usize) -> Self {
        self.agent_id = sanitize_limit(limit, "agent_id");
        self
    }

    pub fn with_workflow_id(mut self, limit: usize) -> Self {
        self.workflow_id = sanitize_limit(limit, "workflow_id");
        self
    }

    pub fn with_plugin_or_tool(mut self, limit: usize) -> Self {
        self.plugin_or_tool = sanitize_limit(limit, "plugin_or_tool");
        self
    }

    pub fn with_provider_model(mut self, limit: usize) -> Self {
        self.provider_model = sanitize_limit(limit, "provider_model");
        self
    }
}

/// Prometheus export configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PrometheusExportConfig {
    /// Refresh interval for the background cache worker.
    pub refresh_interval: Duration,
    /// Label cardinality limits.
    pub cardinality: CardinalityLimits,
    /// Exported metrics-contract version marker.
    pub contract_version: String,
    /// Emit OTel-aligned GenAI alias metrics in addition to mofa_* metrics.
    pub enable_otel_genai_aliases: bool,
}

impl Default for PrometheusExportConfig {
    fn default() -> Self {
        // Environment level rollout control while keeping safe defaults.
        let contract_version = read_contract_version_from_env()
            .unwrap_or_else(|_| MOFA_METRICS_CONTRACT_VERSION.to_string());
        let enable_otel_genai_aliases = read_otel_aliases_from_env()
            .unwrap_or(true);

        Self {
            refresh_interval: Duration::from_secs(1),
            cardinality: CardinalityLimits::default(),
            contract_version,
            enable_otel_genai_aliases,
        }
        .validate()
    }
}

impl PrometheusExportConfig {
    pub fn with_refresh_interval(mut self, refresh_interval: Duration) -> Self {
        self.refresh_interval = sanitize_refresh_interval(refresh_interval);
        self
    }

    pub fn with_cardinality(mut self, cardinality: CardinalityLimits) -> Self {
        self.cardinality = sanitize_cardinality_limits(cardinality);
        self
    }

    pub fn with_contract_version(mut self, version: impl Into<String>) -> Self {
        self.contract_version = sanitize_contract_version(version.into());
        self
    }

    pub fn with_otel_genai_aliases(mut self, enabled: bool) -> Self {
        self.enable_otel_genai_aliases = enabled;
        self
    }

    /// Apply all supported environment-variable overrides.
    pub fn with_env_overrides(mut self) -> Self {
        if let Ok(version) = read_contract_version_from_env() {
            self.contract_version = version;
        }
        self = self.with_otel_genai_aliases_from_env();
        self
    }

    /// Apply OTel alias toggle from environment when present.
    pub fn with_otel_genai_aliases_from_env(mut self) -> Self {
        if let Some(enabled) = read_otel_aliases_from_env() {
            self.enable_otel_genai_aliases = enabled;
        }
        self
    }

    /// Normalize and clamp all config fields to safe values.
    pub fn validate(mut self) -> Self {
        self.refresh_interval = sanitize_refresh_interval(self.refresh_interval);
        self.cardinality = sanitize_cardinality_limits(self.cardinality.clone());
        self.contract_version = sanitize_contract_version(self.contract_version);
        self
    }
}

fn sanitize_limit(limit: usize, name: &str) -> usize {
    if limit == 0 {
        warn!("{name} cardinality limit was 0; clamping to 1");
        1
    } else if limit > MAX_CARDINALITY_LIMIT_PER_DIMENSION {
        warn!(
            "{name} cardinality limit {} exceeds max {}; clamping",
            limit, MAX_CARDINALITY_LIMIT_PER_DIMENSION
        );
        MAX_CARDINALITY_LIMIT_PER_DIMENSION
    } else {
        limit
    }
}

fn sanitize_cardinality_limits(mut limits: CardinalityLimits) -> CardinalityLimits {
    limits.agent_id = sanitize_limit(limits.agent_id, "agent_id");
    limits.workflow_id = sanitize_limit(limits.workflow_id, "workflow_id");
    limits.plugin_or_tool = sanitize_limit(limits.plugin_or_tool, "plugin_or_tool");
    limits.provider_model = sanitize_limit(limits.provider_model, "provider_model");
    limits
}

fn sanitize_refresh_interval(refresh_interval: Duration) -> Duration {
    if refresh_interval.is_zero() {
        warn!(
            "PrometheusExportConfig::with_refresh_interval received zero duration; clamping to 1ms"
        );
        Duration::from_millis(1)
    } else {
        refresh_interval
    }
}

fn sanitize_contract_version(version: String) -> String {
    let trimmed = version.trim();
    if trimmed.is_empty() {
        warn!(
            "PrometheusExportConfig::with_contract_version received empty value; defaulting to {}",
            MOFA_METRICS_CONTRACT_VERSION
        );
        MOFA_METRICS_CONTRACT_VERSION.to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_bool_env(raw: String) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => {
            warn!(
                "invalid boolean env value '{}' for MOFA_METRICS_ENABLE_OTEL_ALIASES; using default",
                raw
            );
            None
        }
    }
}

fn read_contract_version_from_env() -> Result<String, std::env::VarError> {
    std::env::var("MOFA_METRICS_CONTRACT_VERSION").map(sanitize_contract_version)
}

fn read_otel_aliases_from_env() -> Option<bool> {
    std::env::var("MOFA_METRICS_ENABLE_OTEL_ALIASES")
        .ok()
        .and_then(parse_bool_env)
}

/// Errors returned by the Prometheus exporter lifecycle.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PrometheusExportError {
    #[error("prometheus exporter internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone)]
struct HistogramSample {
    count: u64,
    sum: f64,
    bucket_counts: Vec<u64>, // cumulative
}

impl HistogramSample {
    fn new(bucket_bounds: &[f64]) -> Self {
        Self {
            count: 0,
            sum: 0.0,
            bucket_counts: vec![0; bucket_bounds.len()],
        }
    }

    fn observe(&mut self, value: f64, bucket_bounds: &[f64]) {
        if !value.is_finite() || value < 0.0 {
            return;
        }
        self.count = self.count.saturating_add(1);
        self.sum += value;
        for (idx, bound) in bucket_bounds.iter().enumerate() {
            if value <= *bound {
                self.bucket_counts[idx] = self.bucket_counts[idx].saturating_add(1);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct SeriesHistogram {
    labels: Vec<(String, String)>,
    sample: HistogramSample,
}

#[derive(Debug)]
struct LabeledHistogramStore {
    bucket_bounds: Vec<f64>,
    series: HashMap<String, SeriesHistogram>,
    label_limit: usize,
    overflow_labels: Vec<(String, String)>,
}

impl LabeledHistogramStore {
    fn new(
        bucket_bounds: Vec<f64>,
        label_limit: usize,
        overflow_labels: Vec<(String, String)>,
    ) -> Self {
        Self {
            bucket_bounds,
            series: HashMap::new(),
            label_limit,
            overflow_labels,
        }
    }

    fn observe(
        &mut self,
        key: String,
        labels: Vec<(String, String)>,
        value: f64,
        dropped_series_counter: &mut u64,
    ) {
        if let Some(series) = self.series.get_mut(&key) {
            series.sample.observe(value, &self.bucket_bounds);
            return;
        }

        if self.series.len() >= self.label_limit {
            let overflow_key = self.overflow_key();
            let overflow = self
                .series
                .entry(overflow_key)
                .or_insert_with(|| SeriesHistogram {
                    labels: self.overflow_labels.clone(),
                    sample: HistogramSample::new(&self.bucket_bounds),
                });
            overflow.sample.observe(value, &self.bucket_bounds);
            *dropped_series_counter = dropped_series_counter.saturating_add(1);
            return;
        }

        let mut sample = HistogramSample::new(&self.bucket_bounds);
        sample.observe(value, &self.bucket_bounds);
        self.series.insert(key, SeriesHistogram { labels, sample });
    }

    fn overflow_key(&self) -> String {
        self.overflow_labels
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("|")
    }
}

#[derive(Debug)]
struct DurationHistogram {
    bounds: Vec<f64>,
    sample: HistogramSample,
}

impl DurationHistogram {
    fn new(bounds: Vec<f64>) -> Self {
        let sample = HistogramSample::new(&bounds);
        Self { bounds, sample }
    }

    fn observe(&mut self, value_seconds: f64) {
        self.sample.observe(value_seconds, &self.bounds);
    }
}

#[derive(Debug)]
struct LatencyStores {
    agent_execution: LabeledHistogramStore,
    tool_call: LabeledHistogramStore,
    llm_request: LabeledHistogramStore,
}

impl LatencyStores {
    fn new(limits: &CardinalityLimits) -> Self {
        Self {
            agent_execution: LabeledHistogramStore::new(
                vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0],
                limits.agent_id,
                vec![("agent_id".to_string(), OTHER_LABEL_VALUE.to_string())],
            ),
            tool_call: LabeledHistogramStore::new(
                vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0],
                limits.plugin_or_tool,
                vec![("tool_name".to_string(), OTHER_LABEL_VALUE.to_string())],
            ),
            llm_request: LabeledHistogramStore::new(
                vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0],
                limits.provider_model,
                vec![
                    ("provider".to_string(), OTHER_LABEL_VALUE.to_string()),
                    ("model".to_string(), OTHER_LABEL_VALUE.to_string()),
                ],
            ),
        }
    }

    fn observe_snapshot(
        &mut self,
        snapshot: &MetricsSnapshot,
        dropped: &mut DroppedSeriesCounters,
    ) {
        for agent in &snapshot.agents {
            self.agent_execution.observe(
                format!("agent:{}", agent.agent_id),
                vec![("agent_id".to_string(), agent.agent_id.clone())],
                agent.avg_task_duration_ms / 1000.0,
                &mut dropped.agent_id,
            );
        }

        for plugin in &snapshot.plugins {
            self.tool_call.observe(
                format!("tool:{}", plugin.name),
                vec![("tool_name".to_string(), plugin.name.clone())],
                plugin.avg_response_time_ms / 1000.0,
                &mut dropped.plugin_or_tool,
            );
        }

        for llm in &snapshot.llm_metrics {
            let key = format!("{}:{}", llm.provider_name, llm.model_name);
            self.llm_request.observe(
                key,
                vec![
                    ("provider".to_string(), llm.provider_name.clone()),
                    ("model".to_string(), llm.model_name.clone()),
                ],
                llm.avg_latency_ms / 1000.0,
                &mut dropped.provider_model,
            );
        }
    }
}

#[derive(Debug, Default)]
struct DroppedSeriesCounters {
    agent_id: u64,
    workflow_id: u64,
    plugin_or_tool: u64,
    provider_model: u64,
}

impl DroppedSeriesCounters {
    fn add_assign(&mut self, rhs: &DroppedSeriesCounters) {
        self.agent_id = self.agent_id.saturating_add(rhs.agent_id);
        self.workflow_id = self.workflow_id.saturating_add(rhs.workflow_id);
        self.plugin_or_tool = self.plugin_or_tool.saturating_add(rhs.plugin_or_tool);
        self.provider_model = self.provider_model.saturating_add(rhs.provider_model);
    }
}

/// Prometheus exporter bridge that caches rendered exposition text.
pub struct PrometheusExporter {
    collector: Arc<MetricsCollector>,
    config: PrometheusExportConfig,
    cached_body: Arc<RwLock<Bytes>>,
    render_duration_histogram: Arc<RwLock<DurationHistogram>>,
    dropped_series_total: Arc<RwLock<DroppedSeriesCounters>>,
    latency_histograms: Arc<RwLock<LatencyStores>>,
    refresh_failures: AtomicU64,
}

impl PrometheusExporter {
    pub fn new(collector: Arc<MetricsCollector>, mut config: PrometheusExportConfig) -> Self {
        config.refresh_interval = sanitize_refresh_interval(config.refresh_interval);
        config.cardinality = sanitize_cardinality_limits(config.cardinality);

        Self {
            collector,
            config: config.clone(),
            cached_body: Arc::new(RwLock::new(Bytes::new())),
            render_duration_histogram: Arc::new(RwLock::new(DurationHistogram::new(vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0,
            ]))),
            dropped_series_total: Arc::new(RwLock::new(DroppedSeriesCounters::default())),
            latency_histograms: Arc::new(RwLock::new(LatencyStores::new(&config.cardinality))),
            refresh_failures: AtomicU64::new(0),
        }
    }

    /// Starts the background cache refresh worker.
    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let refresh_interval = self.config.refresh_interval;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(refresh_interval);
            loop {
                ticker.tick().await;
                if let Err(err) = self.refresh_once().await {
                    self.refresh_failures.fetch_add(1, AtomicOrdering::Relaxed);
                    warn!("prometheus exporter refresh failed: {err}");
                }
            }
        })
    }

    pub async fn refresh_once(&self) -> Result<(), PrometheusExportError> {
        let snapshot = self.collector.current().await;
        let refresh_unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        let render_start = Instant::now();
        let mut dropped_this_render = DroppedSeriesCounters::default();

        {
            let mut latency = self.latency_histograms.write().await;
            latency.observe_snapshot(&snapshot, &mut dropped_this_render);
        }

        let mut body = self
            .render_snapshot(&snapshot, &mut dropped_this_render)
            .await;

        let render_duration = render_start.elapsed().as_secs_f64();
        {
            let mut histogram = self.render_duration_histogram.write().await;
            histogram.observe(render_duration);
        }
        {
            let mut dropped_total = self.dropped_series_total.write().await;
            dropped_total.add_assign(&dropped_this_render);
        }

        self.append_exporter_internal_metrics(&mut body, refresh_unix_seconds)
            .await;
        *self.cached_body.write().await = Bytes::from(body);

        debug!("prometheus cache refreshed in {:.6}s", render_duration);
        Ok(())
    }

    /// Returns the current Prometheus payload from cache.
    pub async fn render_cached(&self) -> Bytes {
        self.cached_body.read().await.clone()
    }

    async fn render_snapshot(
        &self,
        snapshot: &MetricsSnapshot,
        dropped: &mut DroppedSeriesCounters,
    ) -> String {
        let mut out = String::with_capacity(16 * 1024);

        render_contract_info(&mut out, &self.config.contract_version);
        render_agent_metrics(&mut out, snapshot, &self.config.cardinality, dropped);
        render_workflow_metrics(&mut out, snapshot, &self.config.cardinality, dropped);
        render_plugin_metrics(&mut out, snapshot, &self.config.cardinality, dropped);
        render_llm_metrics(
            &mut out,
            snapshot,
            &self.config.cardinality,
            dropped,
            self.config.enable_otel_genai_aliases,
        );
        render_system_metrics(&mut out, snapshot);
        render_custom_metrics(&mut out, snapshot);

        let latency = self.latency_histograms.read().await;
        render_labeled_histogram_store(
            &mut out,
            "mofa_agent_execution_duration_seconds",
            "Rolling distribution of agent execution duration in seconds",
            &latency.agent_execution,
        );
        render_labeled_histogram_store(
            &mut out,
            "mofa_tool_call_duration_seconds",
            "Rolling distribution of tool/plugin call duration in seconds",
            &latency.tool_call,
        );
        render_labeled_histogram_store(
            &mut out,
            "mofa_llm_request_duration_seconds",
            "Rolling distribution of LLM request duration in seconds",
            &latency.llm_request,
        );
        if self.config.enable_otel_genai_aliases {
            render_labeled_histogram_store(
                &mut out,
                "gen_ai_client_operation_duration_seconds",
                "OTel-aligned rolling distribution of GenAI operation duration in seconds",
                &latency.llm_request,
            );
        }

        out
    }

    async fn append_exporter_internal_metrics(
        &self,
        out: &mut String,
        last_refresh_unix_seconds: f64,
    ) {
        let render_hist = self.render_duration_histogram.read().await;
        write_metric_header(
            out,
            "mofa_exporter_render_duration_seconds",
            "Distribution of Prometheus payload render duration",
            "histogram",
        );
        append_histogram_lines(
            out,
            "mofa_exporter_render_duration_seconds",
            &[],
            &render_hist.bounds,
            &render_hist.sample,
        );

        let dropped = self.dropped_series_total.read().await;
        write_metric_header(
            out,
            "mofa_exporter_dropped_series_total",
            "Total dropped high-cardinality time series by label dimension",
            "counter",
        );
        append_gauge_line(
            out,
            "mofa_exporter_dropped_series_total",
            &[("label".to_string(), "agent_id".to_string())],
            dropped.agent_id as f64,
        );
        append_gauge_line(
            out,
            "mofa_exporter_dropped_series_total",
            &[("label".to_string(), "workflow_id".to_string())],
            dropped.workflow_id as f64,
        );
        append_gauge_line(
            out,
            "mofa_exporter_dropped_series_total",
            &[("label".to_string(), "plugin_or_tool".to_string())],
            dropped.plugin_or_tool as f64,
        );
        append_gauge_line(
            out,
            "mofa_exporter_dropped_series_total",
            &[("label".to_string(), "provider_model".to_string())],
            dropped.provider_model as f64,
        );

        write_metric_header(
            out,
            "mofa_exporter_refresh_failures_total",
            "Total background refresh failures for the Prometheus exporter",
            "counter",
        );
        append_gauge_line(
            out,
            "mofa_exporter_refresh_failures_total",
            &[],
            self.refresh_failures.load(AtomicOrdering::Relaxed) as f64,
        );

        write_metric_header(
            out,
            "mofa_exporter_last_refresh_timestamp_seconds",
            "Unix timestamp of the last successful /metrics payload refresh",
            "gauge",
        );
        append_gauge_line(
            out,
            "mofa_exporter_last_refresh_timestamp_seconds",
            &[],
            last_refresh_unix_seconds,
        );
    }
}

// Emit the contract version marker so dashboards/alerts can pin schema expectations.
fn render_contract_info(out: &mut String, contract_version: &str) {
    write_metric_header(
        out,
        "mofa_metrics_contract_info",
        "MoFA Prometheus metrics contract version",
        "gauge",
    );
    append_gauge_line(
        out,
        "mofa_metrics_contract_info",
        &[("version".to_string(), contract_version.to_string())],
        1.0,
    );
}

#[derive(Clone)]
struct LabeledValue {
    labels: Vec<(String, String)>,
    ranking_value: f64,
    sample_value: f64,
}

#[derive(Copy, Clone)]
enum OverflowAggregation {
    Sum,
    Mean,
}

fn render_agent_metrics(
    out: &mut String,
    snapshot: &MetricsSnapshot,
    limits: &CardinalityLimits,
    dropped: &mut DroppedSeriesCounters,
) {
    let mut task_totals = Vec::with_capacity(snapshot.agents.len());
    let mut task_failed = Vec::with_capacity(snapshot.agents.len());
    let mut task_in_progress = Vec::with_capacity(snapshot.agents.len());
    let mut avg_duration = Vec::with_capacity(snapshot.agents.len());
    let mut messages_sent = Vec::with_capacity(snapshot.agents.len());
    let mut messages_received = Vec::with_capacity(snapshot.agents.len());

    for agent in &snapshot.agents {
        let labels = vec![("agent_id".to_string(), agent.agent_id.clone())];
        task_totals.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: agent.tasks_completed as f64,
            sample_value: agent.tasks_completed as f64,
        });
        task_failed.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: agent.tasks_failed as f64,
            sample_value: agent.tasks_failed as f64,
        });
        task_in_progress.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: agent.tasks_in_progress as f64,
            sample_value: agent.tasks_in_progress as f64,
        });
        avg_duration.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: agent.avg_task_duration_ms,
            sample_value: agent.avg_task_duration_ms / 1000.0,
        });
        messages_sent.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: agent.messages_sent as f64,
            sample_value: agent.messages_sent as f64,
        });
        messages_received.push(LabeledValue {
            labels,
            ranking_value: agent.messages_received as f64,
            sample_value: agent.messages_received as f64,
        });
    }

    write_metric_header(
        out,
        "mofa_agent_tasks_total",
        "Total tasks completed by agent",
        "counter",
    );
    for series in limit_series(
        task_totals,
        limits.agent_id,
        &mut dropped.agent_id,
        OverflowAggregation::Sum,
    ) {
        append_gauge_line(
            out,
            "mofa_agent_tasks_total",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_agent_tasks_failed_total",
        "Total failed tasks by agent",
        "counter",
    );
    for series in limit_series(
        task_failed,
        limits.agent_id,
        &mut dropped.agent_id,
        OverflowAggregation::Sum,
    ) {
        append_gauge_line(
            out,
            "mofa_agent_tasks_failed_total",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_agent_tasks_in_progress",
        "Current in-progress tasks by agent",
        "gauge",
    );
    for series in limit_series(
        task_in_progress,
        limits.agent_id,
        &mut dropped.agent_id,
        OverflowAggregation::Sum,
    ) {
        append_gauge_line(
            out,
            "mofa_agent_tasks_in_progress",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_agent_response_time_seconds",
        "Average task duration by agent in seconds",
        "gauge",
    );
    for series in limit_series(
        avg_duration,
        limits.agent_id,
        &mut dropped.agent_id,
        OverflowAggregation::Mean,
    ) {
        append_gauge_line(
            out,
            "mofa_agent_response_time_seconds",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_agent_messages_sent_total",
        "Total messages sent by agent",
        "counter",
    );
    for series in limit_series(
        messages_sent,
        limits.agent_id,
        &mut dropped.agent_id,
        OverflowAggregation::Sum,
    ) {
        append_gauge_line(
            out,
            "mofa_agent_messages_sent_total",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_agent_messages_received_total",
        "Total messages received by agent",
        "counter",
    );
    for series in limit_series(
        messages_received,
        limits.agent_id,
        &mut dropped.agent_id,
        OverflowAggregation::Sum,
    ) {
        append_gauge_line(
            out,
            "mofa_agent_messages_received_total",
            &series.labels,
            series.sample_value,
        );
    }
}

fn render_workflow_metrics(
    out: &mut String,
    snapshot: &MetricsSnapshot,
    limits: &CardinalityLimits,
    dropped: &mut DroppedSeriesCounters,
) {
    let mut executions = Vec::with_capacity(snapshot.workflows.len());
    let mut success = Vec::with_capacity(snapshot.workflows.len());
    let mut failures = Vec::with_capacity(snapshot.workflows.len());
    let mut avg_duration = Vec::with_capacity(snapshot.workflows.len());
    let mut running = Vec::with_capacity(snapshot.workflows.len());

    for workflow in &snapshot.workflows {
        let labels = vec![("workflow_id".to_string(), workflow.workflow_id.clone())];
        executions.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: workflow.total_executions as f64,
            sample_value: workflow.total_executions as f64,
        });
        success.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: workflow.successful_executions as f64,
            sample_value: workflow.successful_executions as f64,
        });
        failures.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: workflow.failed_executions as f64,
            sample_value: workflow.failed_executions as f64,
        });
        avg_duration.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: workflow.avg_execution_time_ms,
            sample_value: workflow.avg_execution_time_ms / 1000.0,
        });
        running.push(LabeledValue {
            labels,
            ranking_value: workflow.running_instances as f64,
            sample_value: workflow.running_instances as f64,
        });
    }

    write_metric_header(
        out,
        "mofa_workflow_executions_total",
        "Total workflow executions",
        "counter",
    );
    for series in limit_series(
        executions,
        limits.workflow_id,
        &mut dropped.workflow_id,
        OverflowAggregation::Sum,
    ) {
        append_gauge_line(
            out,
            "mofa_workflow_executions_total",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_workflow_executions_success_total",
        "Total successful workflow executions",
        "counter",
    );
    for series in limit_series(
        success,
        limits.workflow_id,
        &mut dropped.workflow_id,
        OverflowAggregation::Sum,
    ) {
        append_gauge_line(
            out,
            "mofa_workflow_executions_success_total",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_workflow_executions_failed_total",
        "Total failed workflow executions",
        "counter",
    );
    for series in limit_series(
        failures,
        limits.workflow_id,
        &mut dropped.workflow_id,
        OverflowAggregation::Sum,
    ) {
        append_gauge_line(
            out,
            "mofa_workflow_executions_failed_total",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_workflow_duration_seconds",
        "Average workflow execution duration in seconds",
        "gauge",
    );
    for series in limit_series(
        avg_duration,
        limits.workflow_id,
        &mut dropped.workflow_id,
        OverflowAggregation::Mean,
    ) {
        append_gauge_line(
            out,
            "mofa_workflow_duration_seconds",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_workflow_active",
        "Currently running workflow instances",
        "gauge",
    );
    for series in limit_series(
        running,
        limits.workflow_id,
        &mut dropped.workflow_id,
        OverflowAggregation::Sum,
    ) {
        append_gauge_line(
            out,
            "mofa_workflow_active",
            &series.labels,
            series.sample_value,
        );
    }
}

fn render_plugin_metrics(
    out: &mut String,
    snapshot: &MetricsSnapshot,
    limits: &CardinalityLimits,
    dropped: &mut DroppedSeriesCounters,
) {
    let mut calls = Vec::with_capacity(snapshot.plugins.len());
    let mut errors = Vec::with_capacity(snapshot.plugins.len());
    let mut avg_duration = Vec::with_capacity(snapshot.plugins.len());

    for plugin in &snapshot.plugins {
        let labels = vec![("tool_name".to_string(), plugin.name.clone())];
        calls.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: plugin.call_count as f64,
            sample_value: plugin.call_count as f64,
        });
        errors.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: plugin.error_count as f64,
            sample_value: plugin.error_count as f64,
        });
        avg_duration.push(LabeledValue {
            labels,
            ranking_value: plugin.avg_response_time_ms,
            sample_value: plugin.avg_response_time_ms / 1000.0,
        });
    }

    write_metric_header(
        out,
        "mofa_tool_calls_total",
        "Total tool/plugin call count",
        "counter",
    );
    for series in limit_series(
        calls,
        limits.plugin_or_tool,
        &mut dropped.plugin_or_tool,
        OverflowAggregation::Sum,
    ) {
        append_gauge_line(
            out,
            "mofa_tool_calls_total",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_tool_errors_total",
        "Total tool/plugin errors",
        "counter",
    );
    for series in limit_series(
        errors,
        limits.plugin_or_tool,
        &mut dropped.plugin_or_tool,
        OverflowAggregation::Sum,
    ) {
        append_gauge_line(
            out,
            "mofa_tool_errors_total",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_tool_response_time_seconds",
        "Average tool/plugin response duration in seconds",
        "gauge",
    );
    for series in limit_series(
        avg_duration,
        limits.plugin_or_tool,
        &mut dropped.plugin_or_tool,
        OverflowAggregation::Mean,
    ) {
        append_gauge_line(
            out,
            "mofa_tool_response_time_seconds",
            &series.labels,
            series.sample_value,
        );
    }
}

fn render_llm_metrics(
    out: &mut String,
    snapshot: &MetricsSnapshot,
    limits: &CardinalityLimits,
    dropped: &mut DroppedSeriesCounters,
    emit_otel_genai_aliases: bool,
) {
    let mut requests = Vec::with_capacity(snapshot.llm_metrics.len());
    let mut tokens_per_second = Vec::with_capacity(snapshot.llm_metrics.len());
    let mut errors = Vec::with_capacity(snapshot.llm_metrics.len());
    let mut latency = Vec::with_capacity(snapshot.llm_metrics.len());
    let mut input_tokens = Vec::with_capacity(snapshot.llm_metrics.len());
    let mut output_tokens = Vec::with_capacity(snapshot.llm_metrics.len());
    let mut time_to_first_token = Vec::with_capacity(snapshot.llm_metrics.len());

    for llm in &snapshot.llm_metrics {
        let labels = vec![
            ("provider".to_string(), llm.provider_name.clone()),
            ("model".to_string(), llm.model_name.clone()),
        ];
        requests.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: llm.total_requests as f64,
            sample_value: llm.total_requests as f64,
        });
        tokens_per_second.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: llm.tokens_per_second.unwrap_or_default(),
            sample_value: llm.tokens_per_second.unwrap_or_default(),
        });
        errors.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: llm.failed_requests as f64,
            sample_value: llm.failed_requests as f64,
        });
        latency.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: llm.avg_latency_ms,
            sample_value: llm.avg_latency_ms / 1000.0,
        });
        input_tokens.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: llm.prompt_tokens as f64,
            sample_value: llm.prompt_tokens as f64,
        });
        output_tokens.push(LabeledValue {
            labels: labels.clone(),
            ranking_value: llm.completion_tokens as f64,
            sample_value: llm.completion_tokens as f64,
        });
        let ttft_s = llm.time_to_first_token_ms.unwrap_or(0.0) / 1000.0;
        time_to_first_token.push(LabeledValue {
            labels,
            ranking_value: ttft_s,
            sample_value: ttft_s,
        });
    }

    write_metric_header(
        out,
        "mofa_llm_requests_total",
        "Total LLM requests",
        "counter",
    );
    for series in limit_series(
        requests,
        limits.provider_model,
        &mut dropped.provider_model,
        OverflowAggregation::Sum,
    ) {
        append_gauge_line(
            out,
            "mofa_llm_requests_total",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_llm_tokens_per_second",
        "LLM generation speed in tokens per second",
        "gauge",
    );
    for series in limit_series(
        tokens_per_second,
        limits.provider_model,
        &mut dropped.provider_model,
        OverflowAggregation::Mean,
    ) {
        append_gauge_line(
            out,
            "mofa_llm_tokens_per_second",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(out, "mofa_llm_errors_total", "Total LLM errors", "counter");
    for series in limit_series(
        errors,
        limits.provider_model,
        &mut dropped.provider_model,
        OverflowAggregation::Sum,
    ) {
        append_gauge_line(
            out,
            "mofa_llm_errors_total",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_llm_latency_seconds",
        "Average LLM request latency in seconds",
        "gauge",
    );
    for series in limit_series(
        latency,
        limits.provider_model,
        &mut dropped.provider_model,
        OverflowAggregation::Mean,
    ) {
        append_gauge_line(
            out,
            "mofa_llm_latency_seconds",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_llm_input_tokens_total",
        "Cumulative prompt tokens sent to the LLM provider",
        "counter",
    );
    let limited_input_tokens = limit_series(
        input_tokens,
        limits.provider_model,
        &mut dropped.provider_model,
        OverflowAggregation::Sum,
    );
    for series in &limited_input_tokens {
        append_gauge_line(
            out,
            "mofa_llm_input_tokens_total",
            &series.labels,
            series.sample_value,
        );
    }

    write_metric_header(
        out,
        "mofa_llm_output_tokens_total",
        "Cumulative completion tokens received from the LLM provider",
        "counter",
    );
    let limited_output_tokens = limit_series(
        output_tokens,
        limits.provider_model,
        &mut dropped.provider_model,
        OverflowAggregation::Sum,
    );
    for series in &limited_output_tokens {
        append_gauge_line(
            out,
            "mofa_llm_output_tokens_total",
            &series.labels,
            series.sample_value,
        );
    }

    if emit_otel_genai_aliases {
        // OTel-aligned alias: unify token usage with a token_type label dimension.
        write_metric_header(
            out,
            "gen_ai_client_token_usage_total",
            "Cumulative GenAI token usage grouped by token_type",
            "counter",
        );
        for series in &limited_input_tokens {
            let mut labels = series.labels.clone();
            labels.push(("token_type".to_string(), "input".to_string()));
            append_gauge_line(out, "gen_ai_client_token_usage_total", &labels, series.sample_value);
        }
        for series in &limited_output_tokens {
            let mut labels = series.labels.clone();
            labels.push(("token_type".to_string(), "output".to_string()));
            append_gauge_line(out, "gen_ai_client_token_usage_total", &labels, series.sample_value);
        }
    }

    write_metric_header(
        out,
        "mofa_llm_time_to_first_token_seconds",
        "Time to first token for streaming requests in seconds (0 for non-streaming)",
        "gauge",
    );
    for series in limit_series(
        time_to_first_token,
        limits.provider_model,
        &mut dropped.provider_model,
        OverflowAggregation::Mean,
    ) {
        append_gauge_line(
            out,
            "mofa_llm_time_to_first_token_seconds",
            &series.labels,
            series.sample_value,
        );
    }
}

fn render_system_metrics(out: &mut String, snapshot: &MetricsSnapshot) {
    write_metric_header(
        out,
        "mofa_system_cpu_percent",
        "System CPU usage percentage",
        "gauge",
    );
    append_gauge_line(
        out,
        "mofa_system_cpu_percent",
        &[],
        snapshot.system.cpu_usage,
    );

    write_metric_header(
        out,
        "mofa_system_memory_bytes",
        "System memory used in bytes",
        "gauge",
    );
    append_gauge_line(
        out,
        "mofa_system_memory_bytes",
        &[],
        snapshot.system.memory_used as f64,
    );

    write_metric_header(
        out,
        "mofa_system_memory_total_bytes",
        "System total memory in bytes",
        "gauge",
    );
    append_gauge_line(
        out,
        "mofa_system_memory_total_bytes",
        &[],
        snapshot.system.memory_total as f64,
    );

    write_metric_header(
        out,
        "mofa_system_uptime_seconds",
        "System/process uptime in seconds",
        "gauge",
    );
    append_gauge_line(
        out,
        "mofa_system_uptime_seconds",
        &[],
        snapshot.system.uptime_secs as f64,
    );

    write_metric_header(
        out,
        "mofa_system_thread_count",
        "System thread count",
        "gauge",
    );
    append_gauge_line(
        out,
        "mofa_system_thread_count",
        &[],
        snapshot.system.thread_count as f64,
    );
}

fn render_custom_metrics(out: &mut String, snapshot: &MetricsSnapshot) {
    let mut custom = snapshot.custom.iter().collect::<Vec<_>>();
    custom.sort_by(|(left, _), (right, _)| left.cmp(right));

    let mut collision_counts: HashMap<String, usize> = HashMap::new();

    for (name, value) in custom {
        let sanitized = sanitize_metric_name(name);
        let next = collision_counts.entry(sanitized.clone()).or_insert(0);
        let metric_name = if *next == 0 {
            sanitized
        } else {
            format!("{sanitized}_{}", *next)
        };
        *next += 1;

        match value {
            MetricValue::Integer(v) => {
                write_metric_header(
                    out,
                    &metric_name,
                    "Custom metric exported from MetricsRegistry",
                    "gauge",
                );
                append_gauge_line(out, &metric_name, &[], *v as f64);
            }
            MetricValue::Float(v) => {
                write_metric_header(
                    out,
                    &metric_name,
                    "Custom metric exported from MetricsRegistry",
                    "gauge",
                );
                append_gauge_line(out, &metric_name, &[], *v);
            }
            MetricValue::Histogram(hist) => {
                write_metric_header(
                    out,
                    &metric_name,
                    "Custom histogram metric exported from MetricsRegistry",
                    "histogram",
                );
                let mut sorted = hist.buckets.clone();
                sorted.sort_by(|(left, _), (right, _)| {
                    left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
                });
                let mut cumulative = Vec::with_capacity(sorted.len());
                for (_, count) in &sorted {
                    cumulative.push(*count);
                }
                let bounds = sorted.iter().map(|(bound, _)| *bound).collect::<Vec<_>>();
                let sample = HistogramSample {
                    count: hist.count,
                    sum: hist.sum,
                    bucket_counts: cumulative,
                };
                append_histogram_lines(out, &metric_name, &[], &bounds, &sample);
            }
        }
    }
}

fn render_labeled_histogram_store(
    out: &mut String,
    metric_name: &str,
    help: &str,
    store: &LabeledHistogramStore,
) {
    write_metric_header(out, metric_name, help, "histogram");

    let mut series_entries = store.series.values().collect::<Vec<_>>();
    series_entries.sort_by(|a, b| compare_label_set(&a.labels, &b.labels));

    for series in series_entries {
        append_histogram_lines(
            out,
            metric_name,
            &series.labels,
            &store.bucket_bounds,
            &series.sample,
        );
    }
}

fn limit_series(
    mut values: Vec<LabeledValue>,
    limit: usize,
    dropped_counter: &mut u64,
    aggregation: OverflowAggregation,
) -> Vec<LabeledValue> {
    let effective_limit = limit.max(1);

    if values.len() <= effective_limit {
        values.sort_by(|a, b| compare_label_set(&a.labels, &b.labels));
        return values;
    }

    values.sort_by(|a, b| {
        b.ranking_value
            .partial_cmp(&a.ranking_value)
            .unwrap_or(Ordering::Equal)
            .then_with(|| compare_label_set(&a.labels, &b.labels))
    });

    let mut kept = values.drain(..effective_limit).collect::<Vec<_>>();
    let overflow = values;

    let overflow_count = overflow.len() as u64;
    *dropped_counter = dropped_counter.saturating_add(overflow_count);

    if overflow_count > 0 {
        let overflow_sum = overflow
            .iter()
            .map(|entry| entry.sample_value)
            .fold(0.0, |acc, v| acc + v);
        let overflow_value = match aggregation {
            OverflowAggregation::Sum => overflow_sum,
            OverflowAggregation::Mean => overflow_sum / (overflow_count as f64),
        };

        // Preserve original label keys; replace values with __other__.
        let mut label_keys = if let Some(first) = kept.first().or_else(|| overflow.first()) {
            first
                .labels
                .iter()
                .map(|(k, _)| (k.clone(), OTHER_LABEL_VALUE.to_string()))
                .collect::<Vec<_>>()
        } else {
            vec![("label".to_string(), OTHER_LABEL_VALUE.to_string())]
        };

        if label_keys.is_empty() {
            label_keys.push(("label".to_string(), OTHER_LABEL_VALUE.to_string()));
        }

        kept.push(LabeledValue {
            labels: label_keys,
            ranking_value: overflow_value,
            sample_value: overflow_value,
        });
    }

    kept.sort_by(|a, b| compare_label_set(&a.labels, &b.labels));
    kept
}

fn compare_label_set(a: &[(String, String)], b: &[(String, String)]) -> Ordering {
    for ((ak, av), (bk, bv)) in a.iter().zip(b.iter()) {
        match ak.cmp(bk) {
            Ordering::Less => return Ordering::Less,
            Ordering::Greater => return Ordering::Greater,
            Ordering::Equal => {}
        }
        match av.cmp(bv) {
            Ordering::Less => return Ordering::Less,
            Ordering::Greater => return Ordering::Greater,
            Ordering::Equal => {}
        }
    }

    a.len().cmp(&b.len())
}

fn sanitize_metric_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == ':' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        return "mofa_custom_metric".to_string();
    }

    // Prometheus requires the first character to match [a-zA-Z_:].
    if let Some(first) = out.chars().next()
        && !(first.is_ascii_alphabetic() || first == '_' || first == ':')
    {
        return format!("mofa_custom_{out}");
    }

    out
}

fn write_metric_header(out: &mut String, name: &str, help: &str, metric_type: &str) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {metric_type}");
}

fn append_gauge_line(out: &mut String, name: &str, labels: &[(String, String)], value: f64) {
    if !value.is_finite() {
        return;
    }

    if labels.is_empty() {
        let _ = writeln!(out, "{name} {}", format_float(value));
        return;
    }

    let rendered_labels = labels
        .iter()
        .map(|(k, v)| {
            let escaped = escape_label_value(v);
            format!("{k}=\"{escaped}\"")
        })
        .collect::<Vec<_>>()
        .join(",");
    let _ = writeln!(out, "{name}{{{rendered_labels}}} {}", format_float(value));
}

fn append_histogram_lines(
    out: &mut String,
    base_name: &str,
    labels: &[(String, String)],
    bounds: &[f64],
    sample: &HistogramSample,
) {
    for (idx, bound) in bounds.iter().enumerate() {
        let mut with_le = labels.to_vec();
        with_le.push(("le".to_string(), format_float(*bound)));
        append_gauge_line(
            out,
            &format!("{base_name}_bucket"),
            &with_le,
            sample.bucket_counts.get(idx).copied().unwrap_or_default() as f64,
        );
    }

    let mut with_inf = labels.to_vec();
    with_inf.push(("le".to_string(), "+Inf".to_string()));
    append_gauge_line(
        out,
        &format!("{base_name}_bucket"),
        &with_inf,
        sample.count as f64,
    );
    append_gauge_line(out, &format!("{base_name}_sum"), labels, sample.sum);
    append_gauge_line(
        out,
        &format!("{base_name}_count"),
        labels,
        sample.count as f64,
    );
}

fn format_float(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn escape_label_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::StatusCode, routing::get};
    use tokio::time::timeout;
    use tower::ServiceExt;

    fn sample_snapshot() -> MetricsSnapshot {
        MetricsSnapshot {
            system: super::super::metrics::SystemMetrics {
                cpu_usage: 40.5,
                memory_used: 1024,
                memory_total: 4096,
                uptime_secs: 77,
                thread_count: 5,
                timestamp: 1,
            },
            agents: vec![super::super::metrics::AgentMetrics {
                agent_id: "agent-1".to_string(),
                name: "Agent One".to_string(),
                tasks_completed: 42,
                tasks_failed: 1,
                tasks_in_progress: 2,
                avg_task_duration_ms: 120.0,
                messages_sent: 9,
                messages_received: 8,
                ..Default::default()
            }],
            workflows: vec![],
            plugins: vec![],
            llm_metrics: vec![],
            timestamp: 2,
            custom: HashMap::new(),
        }
    }

    async fn seed_collector_from_snapshot(collector: &MetricsCollector, snapshot: MetricsSnapshot) {
        for agent in snapshot.agents {
            collector.update_agent(agent).await;
        }
        for workflow in snapshot.workflows {
            collector.update_workflow(workflow).await;
        }
        for plugin in snapshot.plugins {
            collector.update_plugin(plugin).await;
        }
        for llm in snapshot.llm_metrics {
            collector.update_llm(llm).await;
        }
        let _ = collector.collect().await;
    }

    #[tokio::test]
    async fn renders_prometheus_headers_and_labels() {
        let collector = Arc::new(MetricsCollector::new(Default::default()));
        seed_collector_from_snapshot(&collector, sample_snapshot()).await;

        let exporter = PrometheusExporter::new(collector, PrometheusExportConfig::default());
        exporter.refresh_once().await.expect("refresh");
        let output = exporter.render_cached().await;
        let output = std::str::from_utf8(output.as_ref()).expect("utf8");

        assert!(output.contains("# HELP mofa_agent_tasks_total"));
        assert!(output.contains("# TYPE mofa_agent_tasks_total counter"));
        assert!(output.contains("mofa_agent_tasks_total{agent_id=\"agent-1\"} 42"));
        assert!(output.contains("# HELP mofa_metrics_contract_info"));
        assert!(
            output.contains("mofa_metrics_contract_info{version=\"v1alpha1\"} 1"),
            "expected contract version marker in exported payload"
        );
        assert!(output.contains("# HELP gen_ai_client_operation_duration_seconds"));
        assert!(output.contains("# HELP mofa_system_cpu_percent"));
    }

    #[tokio::test]
    async fn renders_configured_contract_version_marker() {
        let collector = Arc::new(MetricsCollector::new(Default::default()));
        seed_collector_from_snapshot(&collector, sample_snapshot()).await;

        let exporter = PrometheusExporter::new(
            collector,
            PrometheusExportConfig::default().with_contract_version("v9beta2"),
        );
        exporter.refresh_once().await.expect("refresh");
        let output = exporter.render_cached().await;
        let output = std::str::from_utf8(output.as_ref()).expect("utf8");

        assert!(
            output.contains("mofa_metrics_contract_info{version=\"v9beta2\"} 1"),
            "expected configured contract version marker; output was:\n{output}"
        );
    }

    #[tokio::test]
    async fn enforces_cardinality_limits_with_other_bucket() {
        let mut snapshot = sample_snapshot();
        snapshot.agents = (0..5)
            .map(|idx| super::super::metrics::AgentMetrics {
                agent_id: format!("agent-{idx}"),
                tasks_completed: (idx + 1) as u64,
                avg_task_duration_ms: (idx * 10) as f64,
                ..Default::default()
            })
            .collect();

        let collector = Arc::new(MetricsCollector::new(Default::default()));
        seed_collector_from_snapshot(&collector, snapshot).await;

        let exporter = PrometheusExporter::new(
            collector,
            PrometheusExportConfig {
                refresh_interval: Duration::from_millis(50),
                cardinality: CardinalityLimits {
                    agent_id: 2,
                    ..Default::default()
                },
                ..Default::default()
            },
        );

        exporter.refresh_once().await.expect("refresh");
        let output = exporter.render_cached().await;
        let output = std::str::from_utf8(output.as_ref()).expect("utf8");

        assert!(output.contains("agent_id=\"__other__\""));
        assert!(output.contains("mofa_exporter_dropped_series_total{label=\"agent_id\"}"));
    }

    #[tokio::test]
    async fn provides_deterministic_order_for_equal_ranked_series() {
        let entries = vec![
            LabeledValue {
                labels: vec![("agent_id".to_string(), "b".to_string())],
                ranking_value: 10.0,
                sample_value: 10.0,
            },
            LabeledValue {
                labels: vec![("agent_id".to_string(), "a".to_string())],
                ranking_value: 10.0,
                sample_value: 10.0,
            },
        ];

        let mut dropped_a = 0;
        let mut dropped_b = 0;
        let first = limit_series(entries.clone(), 2, &mut dropped_a, OverflowAggregation::Sum);
        let second = limit_series(entries, 2, &mut dropped_b, OverflowAggregation::Sum);

        assert_eq!(first[0].labels[0].1, "a");
        assert_eq!(second[0].labels[0].1, "a");
    }

    #[tokio::test]
    async fn zero_limits_are_clamped_and_preserve_dimension_keys() {
        let mut snapshot = sample_snapshot();
        snapshot.agents = (0..3)
            .map(|idx| super::super::metrics::AgentMetrics {
                agent_id: format!("agent-{idx}"),
                tasks_completed: (idx + 1) as u64,
                ..Default::default()
            })
            .collect();

        let collector = Arc::new(MetricsCollector::new(Default::default()));
        seed_collector_from_snapshot(&collector, snapshot).await;

        let exporter = PrometheusExporter::new(
            collector,
            PrometheusExportConfig {
                refresh_interval: Duration::from_millis(20),
                cardinality: CardinalityLimits {
                    agent_id: 0,
                    workflow_id: 0,
                    plugin_or_tool: 0,
                    provider_model: 0,
                },
                ..Default::default()
            },
        );
        exporter.refresh_once().await.expect("refresh");
        let output = exporter.render_cached().await;
        let output = std::str::from_utf8(output.as_ref()).expect("utf8");

        assert!(output.contains("mofa_agent_tasks_total{agent_id=\"__other__\"}"));
        assert!(!output.contains("mofa_agent_tasks_total{label=\"__other__\"}"));
    }

    #[tokio::test]
    async fn overflow_uses_mean_for_average_metrics() {
        let mut snapshot = sample_snapshot();
        snapshot.agents = vec![
            super::super::metrics::AgentMetrics {
                agent_id: "a".to_string(),
                avg_task_duration_ms: 1_000.0,
                ..Default::default()
            },
            super::super::metrics::AgentMetrics {
                agent_id: "b".to_string(),
                avg_task_duration_ms: 2_000.0,
                ..Default::default()
            },
            super::super::metrics::AgentMetrics {
                agent_id: "c".to_string(),
                avg_task_duration_ms: 9_000.0,
                ..Default::default()
            },
        ];

        let collector = Arc::new(MetricsCollector::new(Default::default()));
        seed_collector_from_snapshot(&collector, snapshot).await;

        let exporter = PrometheusExporter::new(
            collector,
            PrometheusExportConfig::default()
                .with_refresh_interval(Duration::from_millis(10))
                .with_cardinality(CardinalityLimits::default().with_agent_id(1)),
        );
        exporter.refresh_once().await.expect("refresh");
        let output = exporter.render_cached().await;
        let output = std::str::from_utf8(output.as_ref()).expect("utf8");

        assert!(output.contains("mofa_agent_response_time_seconds{agent_id=\"__other__\"} 1.5"));
    }

    #[tokio::test]
    async fn serves_metrics_route() {
        let collector = Arc::new(MetricsCollector::new(Default::default()));
        seed_collector_from_snapshot(&collector, sample_snapshot()).await;

        let exporter = Arc::new(PrometheusExporter::new(
            collector,
            PrometheusExportConfig::default(),
        ));
        exporter.refresh_once().await.expect("refresh");

        let app = Router::new().route(
            "/metrics",
            get({
                let exporter = exporter.clone();
                move || {
                    let exporter = exporter.clone();
                    async move {
                        (
                            [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
                            exporter.render_cached().await,
                        )
                    }
                }
            }),
        );

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/metrics")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(content_type.starts_with("text/plain"));
    }

    #[tokio::test]
    async fn concurrent_scrapes_do_not_block_updates() {
        let collector = Arc::new(MetricsCollector::new(Default::default()));
        let exporter = Arc::new(PrometheusExporter::new(
            collector.clone(),
            PrometheusExportConfig {
                refresh_interval: Duration::from_millis(20),
                ..Default::default()
            },
        ));

        let worker = exporter.clone().start();

        let updater = {
            let collector = collector.clone();
            tokio::spawn(async move {
                for idx in 0..200u64 {
                    collector
                        .update_agent(super::super::metrics::AgentMetrics {
                            agent_id: format!("agent-{idx}"),
                            tasks_completed: idx,
                            avg_task_duration_ms: idx as f64,
                            ..Default::default()
                        })
                        .await;
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            })
        };

        let mut scrapers = Vec::new();
        for _ in 0..25 {
            let exporter = exporter.clone();
            scrapers.push(tokio::spawn(async move {
                for _ in 0..20 {
                    let payload = exporter.render_cached().await;
                    let payload = std::str::from_utf8(payload.as_ref()).expect("utf8");
                    assert!(payload.contains("mofa_exporter_last_refresh_timestamp_seconds"));
                }
            }));
        }

        timeout(Duration::from_secs(8), async {
            let _ = updater.await;
            for scraper in scrapers {
                let _ = scraper.await;
            }
        })
        .await
        .expect("concurrency test timed out");

        worker.abort();
    }

    #[test]
    fn escapes_label_values() {
        let escaped = escape_label_value("a\"b\\c\n");
        assert_eq!(escaped, "a\\\"b\\\\c\\n");
    }

    #[test]
    fn sanitizes_metric_names_with_invalid_first_char() {
        assert_eq!(sanitize_metric_name("1foo"), "mofa_custom_1foo");
        assert_eq!(sanitize_metric_name(""), "mofa_custom_metric");
    }

    #[test]
    fn zero_refresh_interval_is_clamped() {
        let config = PrometheusExportConfig::default().with_refresh_interval(Duration::ZERO);
        assert_eq!(config.refresh_interval, Duration::from_millis(1));
    }

    #[test]
    fn empty_contract_version_defaults() {
        let config = PrometheusExportConfig::default().with_contract_version("   ");
        assert_eq!(config.contract_version, MOFA_METRICS_CONTRACT_VERSION);
    }

    #[test]
    fn cardinality_limits_are_clamped_to_safety_maximum() {
        let config = PrometheusExportConfig::default().with_cardinality(CardinalityLimits {
            agent_id: usize::MAX,
            workflow_id: usize::MAX,
            plugin_or_tool: usize::MAX,
            provider_model: usize::MAX,
        });

        assert_eq!(
            config.cardinality.agent_id,
            MAX_CARDINALITY_LIMIT_PER_DIMENSION
        );
        assert_eq!(
            config.cardinality.workflow_id,
            MAX_CARDINALITY_LIMIT_PER_DIMENSION
        );
        assert_eq!(
            config.cardinality.plugin_or_tool,
            MAX_CARDINALITY_LIMIT_PER_DIMENSION
        );
        assert_eq!(
            config.cardinality.provider_model,
            MAX_CARDINALITY_LIMIT_PER_DIMENSION
        );
    }

    #[test]
    fn custom_metric_name_collisions_are_disambiguated() {
        let mut snapshot = sample_snapshot();
        snapshot
            .custom
            .insert("foo-bar".to_string(), MetricValue::Integer(1));
        snapshot
            .custom
            .insert("foo_bar".to_string(), MetricValue::Integer(2));
        let mut output = String::new();
        render_custom_metrics(&mut output, &snapshot);

        assert!(output.contains("# HELP foo_bar "));
        assert!(output.contains("# HELP foo_bar_1 "));
    }

    #[test]
    fn test_render_llm_input_tokens_exported() {
        let mut snapshot = sample_snapshot();
        snapshot.llm_metrics = vec![super::super::metrics::LLMMetrics {
            provider_name: "openai".to_string(),
            model_name: "gpt-4".to_string(),
            prompt_tokens: 1000,
            ..Default::default()
        }];
        let mut output = String::new();
        let limits = CardinalityLimits::default();
        let mut dropped = DroppedSeriesCounters::default();
        render_llm_metrics(&mut output, &snapshot, &limits, &mut dropped, true);

        assert!(
            output.contains("# HELP mofa_llm_input_tokens_total"),
            "missing HELP line for input tokens"
        );
        assert!(
            output.contains("# TYPE mofa_llm_input_tokens_total counter"),
            "missing TYPE line for input tokens"
        );
        assert!(
            output
                .contains("mofa_llm_input_tokens_total{provider=\"openai\",model=\"gpt-4\"} 1000"),
            "missing or wrong value for input tokens; output was:\n{output}"
        );
    }

    #[test]
    fn test_render_llm_output_tokens_exported() {
        let mut snapshot = sample_snapshot();
        snapshot.llm_metrics = vec![super::super::metrics::LLMMetrics {
            provider_name: "anthropic".to_string(),
            model_name: "claude-3-opus".to_string(),
            completion_tokens: 512,
            ..Default::default()
        }];
        let mut output = String::new();
        let limits = CardinalityLimits::default();
        let mut dropped = DroppedSeriesCounters::default();
        render_llm_metrics(&mut output, &snapshot, &limits, &mut dropped, true);

        assert!(
            output.contains("# HELP mofa_llm_output_tokens_total"),
            "missing HELP line for output tokens"
        );
        assert!(
            output.contains("# TYPE mofa_llm_output_tokens_total counter"),
            "missing TYPE line for output tokens"
        );
        assert!(
            output.contains(
                "mofa_llm_output_tokens_total{provider=\"anthropic\",model=\"claude-3-opus\"} 512"
            ),
            "missing or wrong value for output tokens; output was:\n{output}"
        );
    }

    #[test]
    fn test_render_llm_time_to_first_token_exported() {
        let mut snapshot = sample_snapshot();
        snapshot.llm_metrics = vec![super::super::metrics::LLMMetrics {
            provider_name: "openai".to_string(),
            model_name: "gpt-4".to_string(),
            time_to_first_token_ms: Some(42.0),
            ..Default::default()
        }];
        let mut output = String::new();
        let limits = CardinalityLimits::default();
        let mut dropped = DroppedSeriesCounters::default();
        render_llm_metrics(&mut output, &snapshot, &limits, &mut dropped, true);

        assert!(
            output.contains("# HELP mofa_llm_time_to_first_token_seconds"),
            "missing HELP line for time_to_first_token; output was:\n{output}"
        );
        // 42 ms = 0.042 s — check with float tolerance via string search
        assert!(
            output.contains(
                "mofa_llm_time_to_first_token_seconds{provider=\"openai\",model=\"gpt-4\"} 0.042"
            ),
            "wrong value for time_to_first_token; output was:\n{output}"
        );
    }

    #[test]
    fn test_render_llm_otel_token_usage_alias_exported() {
        let mut snapshot = sample_snapshot();
        snapshot.llm_metrics = vec![super::super::metrics::LLMMetrics {
            provider_name: "openai".to_string(),
            model_name: "gpt-4".to_string(),
            prompt_tokens: 100,
            completion_tokens: 25,
            ..Default::default()
        }];
        let mut output = String::new();
        let limits = CardinalityLimits::default();
        let mut dropped = DroppedSeriesCounters::default();
        render_llm_metrics(&mut output, &snapshot, &limits, &mut dropped, true);

        assert!(
            output.contains("# HELP gen_ai_client_token_usage_total"),
            "missing HELP line for OTel token usage alias; output was:\n{output}"
        );
        assert!(
            output.contains("# TYPE gen_ai_client_token_usage_total counter"),
            "missing TYPE line for OTel token usage alias; output was:\n{output}"
        );
        assert!(
            output.contains(
                "gen_ai_client_token_usage_total{provider=\"openai\",model=\"gpt-4\",token_type=\"input\"} 100"
            ),
            "missing input token alias line; output was:\n{output}"
        );
        assert!(
            output.contains(
                "gen_ai_client_token_usage_total{provider=\"openai\",model=\"gpt-4\",token_type=\"output\"} 25"
            ),
            "missing output token alias line; output was:\n{output}"
        );
    }

    #[test]
    fn test_render_llm_without_otel_aliases() {
        let mut snapshot = sample_snapshot();
        snapshot.llm_metrics = vec![super::super::metrics::LLMMetrics {
            provider_name: "openai".to_string(),
            model_name: "gpt-4".to_string(),
            prompt_tokens: 100,
            completion_tokens: 25,
            ..Default::default()
        }];
        let mut output = String::new();
        let limits = CardinalityLimits::default();
        let mut dropped = DroppedSeriesCounters::default();
        render_llm_metrics(&mut output, &snapshot, &limits, &mut dropped, false);

        assert!(
            !output.contains("gen_ai_client_token_usage_total"),
            "OTel alias should be disabled; output was:\n{output}"
        );
    }
}
