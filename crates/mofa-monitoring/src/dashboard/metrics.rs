//! Metrics collection and aggregation
//!
//! Provides metrics collection for the monitoring dashboard

use mofa_kernel::bus::metrics::{
    MessageBusMetrics as KernelBusMetrics, MessageBusObserver, SharedCounters,
};
use mofa_kernel::metrics::LLMMetricsSource;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Metric type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricType {
    /// Counter (monotonically increasing)
    Counter,
    /// Gauge (can go up or down)
    Gauge,
    /// Histogram (distribution of values)
    Histogram,
    /// Summary (percentiles)
    Summary,
}

/// Metric value
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MetricValue {
    Integer(i64),
    Float(f64),
    Histogram(HistogramData),
}

impl MetricValue {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            MetricValue::Integer(v) => Some(*v),
            MetricValue::Float(v) => Some(*v as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            MetricValue::Integer(v) => Some(*v as f64),
            MetricValue::Float(v) => Some(*v),
            _ => None,
        }
    }
}

/// Histogram data
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistogramData {
    pub count: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub buckets: Vec<(f64, u64)>, // (upper_bound, count)
}

/// Counter metric
pub struct Counter {
    value: AtomicU64,
    name: String,
    description: String,
    labels: HashMap<String, String>,
}

impl Counter {
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            value: AtomicU64::new(0),
            name: name.to_string(),
            description: description.to_string(),
            labels: HashMap::new(),
        }
    }

    pub fn with_label(mut self, key: &str, value: &str) -> Self {
        self.labels.insert(key.to_string(), value.to_string());
        self
    }

    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_by(&self, n: u64) {
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}

/// Gauge metric
pub struct Gauge {
    value: AtomicI64,
    name: String,
    description: String,
    labels: HashMap<String, String>,
}

impl Gauge {
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            value: AtomicI64::new(0),
            name: name.to_string(),
            description: description.to_string(),
            labels: HashMap::new(),
        }
    }

    pub fn with_label(mut self, key: &str, value: &str) -> Self {
        self.labels.insert(key.to_string(), value.to_string());
        self
    }

    pub fn set(&self, val: i64) {
        self.value.store(val, Ordering::Relaxed);
    }

    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dec(&self) {
        self.value.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn add(&self, n: i64) {
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    pub fn get(&self) -> i64 {
        self.value.load(Ordering::Relaxed)
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Histogram metric
pub struct Histogram {
    name: String,
    description: String,
    labels: HashMap<String, String>,
    buckets: Vec<f64>,
    data: RwLock<HistogramData>,
}

impl Histogram {
    pub fn new(name: &str, description: &str, buckets: Vec<f64>) -> Self {
        let bucket_counts: Vec<(f64, u64)> = buckets.iter().map(|&b| (b, 0)).collect();
        Self {
            name: name.to_string(),
            description: description.to_string(),
            labels: HashMap::new(),
            buckets,
            data: RwLock::new(HistogramData {
                count: 0,
                sum: 0.0,
                min: f64::MAX,
                max: f64::MIN,
                buckets: bucket_counts,
            }),
        }
    }

    pub fn with_label(mut self, key: &str, value: &str) -> Self {
        self.labels.insert(key.to_string(), value.to_string());
        self
    }

    pub async fn observe(&self, value: f64) {
        let mut data = self.data.write().await;
        data.count += 1;
        data.sum += value;
        data.min = data.min.min(value);
        data.max = data.max.max(value);

        // Update buckets
        for (bound, count) in &mut data.buckets {
            if value <= *bound {
                *count += 1;
            }
        }
    }

    pub async fn get_data(&self) -> HistogramData {
        self.data.read().await.clone()
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// System metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemMetrics {
    /// CPU usage percentage
    pub cpu_usage: f64,
    /// Memory usage in bytes
    pub memory_used: u64,
    /// Total memory in bytes
    pub memory_total: u64,
    /// Uptime in seconds
    pub uptime_secs: u64,
    /// Number of active threads
    pub thread_count: u32,
    /// Timestamp
    pub timestamp: u64,
}

/// Agent metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentMetrics {
    /// Agent ID
    pub agent_id: String,
    /// Agent name
    pub name: String,
    /// Current state
    pub state: String,
    /// Tasks completed
    pub tasks_completed: u64,
    /// Tasks failed
    pub tasks_failed: u64,
    /// Tasks in progress
    pub tasks_in_progress: u32,
    /// Average task duration in ms
    pub avg_task_duration_ms: f64,
    /// Messages sent
    pub messages_sent: u64,
    /// Messages received
    pub messages_received: u64,
    /// Last activity timestamp
    pub last_activity: u64,
}

/// Workflow metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowMetrics {
    /// Workflow ID
    pub workflow_id: String,
    /// Workflow name
    pub name: String,
    /// Current status
    pub status: String,
    /// Total executions
    pub total_executions: u64,
    /// Successful executions
    pub successful_executions: u64,
    /// Failed executions
    pub failed_executions: u64,
    /// Average execution time in ms
    pub avg_execution_time_ms: f64,
    /// Currently running instances
    pub running_instances: u32,
    /// Nodes in workflow
    pub node_count: u32,
}

/// Plugin metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginMetrics {
    /// Plugin ID
    pub plugin_id: String,
    /// Plugin name
    pub name: String,
    /// Plugin version
    pub version: String,
    /// Current state
    pub state: String,
    /// Call count
    pub call_count: u64,
    /// Error count
    pub error_count: u64,
    /// Average response time in ms
    pub avg_response_time_ms: f64,
    /// Last reload timestamp
    pub last_reload: Option<u64>,
    /// Reload count
    pub reload_count: u32,
}

/// LLM Metrics - specialized metrics for LLM inference
///
/// Separate from PluginMetrics because LLM-specific metrics (tokens/s, TTFT, etc.)
/// are fundamentally different from generic plugin metrics and require
/// their own collection and reporting pipeline.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LLMMetrics {
    /// Plugin ID
    pub plugin_id: String,
    /// LLM Provider name (e.g., "OpenAI", "Anthropic")
    pub provider_name: String,
    /// Model name (e.g., "gpt-4", "claude-3-opus")
    pub model_name: String,
    /// Current state
    pub state: String,
    /// Total requests made
    pub total_requests: u64,
    /// Successful requests
    pub successful_requests: u64,
    /// Failed requests
    pub failed_requests: u64,
    /// Total tokens processed
    pub total_tokens: u64,
    /// Prompt tokens
    pub prompt_tokens: u64,
    /// Completion/generation tokens
    pub completion_tokens: u64,
    /// Average latency in milliseconds
    pub avg_latency_ms: f64,
    /// Tokens per second (generation speed)
    pub tokens_per_second: Option<f64>,
    /// Time to first token in ms (for streaming)
    pub time_to_first_token_ms: Option<f64>,
    /// Requests per minute (throughput)
    pub requests_per_minute: f64,
    /// Error rate percentage
    pub error_rate: f64,
    /// Last request timestamp
    pub last_request_timestamp: u64,
}

/// Metrics snapshot
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// System metrics
    pub system: SystemMetrics,
    /// Agent metrics
    pub agents: Vec<AgentMetrics>,
    /// Workflow metrics
    pub workflows: Vec<WorkflowMetrics>,
    /// Plugin metrics (generic)
    pub plugins: Vec<PluginMetrics>,
    /// LLM metrics (model-specific inference metrics)
    pub llm_metrics: Vec<LLMMetrics>,
    /// Message bus metrics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_bus: Option<KernelBusMetrics>,
    /// Snapshot timestamp
    pub timestamp: u64,
    /// Custom metrics
    pub custom: HashMap<String, MetricValue>,
}

/// Metrics configuration
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    /// Collection interval
    pub collection_interval: Duration,
    /// History retention
    pub history_retention: Duration,
    /// Enable system metrics
    pub enable_system_metrics: bool,
    /// Enable agent metrics
    pub enable_agent_metrics: bool,
    /// Enable workflow metrics
    pub enable_workflow_metrics: bool,
    /// Enable plugin metrics
    pub enable_plugin_metrics: bool,
    /// Enable LLM metrics (model inference metrics)
    pub enable_llm_metrics: bool,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            collection_interval: Duration::from_secs(5),
            history_retention: Duration::from_secs(3600), // 1 hour
            enable_system_metrics: true,
            enable_agent_metrics: true,
            enable_workflow_metrics: true,
            enable_plugin_metrics: true,
            enable_llm_metrics: true,
        }
    }
}

/// Metrics registry
pub struct MetricsRegistry {
    counters: RwLock<HashMap<String, Arc<Counter>>>,
    gauges: RwLock<HashMap<String, Arc<Gauge>>>,
    histograms: RwLock<HashMap<String, Arc<Histogram>>>,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self {
            counters: RwLock::new(HashMap::new()),
            gauges: RwLock::new(HashMap::new()),
            histograms: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register_counter(&self, counter: Counter) -> Arc<Counter> {
        let name = counter.name.clone();
        let arc = Arc::new(counter);
        self.counters.write().await.insert(name, arc.clone());
        arc
    }

    pub async fn register_gauge(&self, gauge: Gauge) -> Arc<Gauge> {
        let name = gauge.name.clone();
        let arc = Arc::new(gauge);
        self.gauges.write().await.insert(name, arc.clone());
        arc
    }

    pub async fn register_histogram(&self, histogram: Histogram) -> Arc<Histogram> {
        let name = histogram.name.clone();
        let arc = Arc::new(histogram);
        self.histograms.write().await.insert(name, arc.clone());
        arc
    }

    pub async fn get_counter(&self, name: &str) -> Option<Arc<Counter>> {
        self.counters.read().await.get(name).cloned()
    }

    pub async fn get_gauge(&self, name: &str) -> Option<Arc<Gauge>> {
        self.gauges.read().await.get(name).cloned()
    }

    pub async fn get_histogram(&self, name: &str) -> Option<Arc<Histogram>> {
        self.histograms.read().await.get(name).cloned()
    }

    pub async fn collect_all(&self) -> HashMap<String, MetricValue> {
        let mut result = HashMap::new();

        for (name, counter) in self.counters.read().await.iter() {
            result.insert(name.clone(), MetricValue::Integer(counter.get() as i64));
        }

        for (name, gauge) in self.gauges.read().await.iter() {
            result.insert(name.clone(), MetricValue::Integer(gauge.get()));
        }

        for (name, histogram) in self.histograms.read().await.iter() {
            let data = histogram.get_data().await;
            result.insert(name.clone(), MetricValue::Histogram(data));
        }

        result
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics collector
pub struct MetricsCollector {
    config: MetricsConfig,
    registry: Arc<MetricsRegistry>,
    /// Current snapshot
    current_snapshot: Arc<RwLock<MetricsSnapshot>>,
    /// Historical snapshots
    history: Arc<RwLock<Vec<MetricsSnapshot>>>,
    /// Start time
    start_time: Instant,
    /// Agent metrics storage
    agent_metrics: Arc<RwLock<HashMap<String, AgentMetrics>>>,
    /// Workflow metrics storage
    workflow_metrics: Arc<RwLock<HashMap<String, WorkflowMetrics>>>,
    /// Plugin metrics storage
    plugin_metrics: Arc<RwLock<HashMap<String, PluginMetrics>>>,
    /// LLM metrics storage (model-specific inference metrics)
    llm_metrics: Arc<RwLock<HashMap<String, LLMMetrics>>>,
    /// LLM metrics source for pulling from persistence
    llm_metrics_source: Option<Arc<dyn LLMMetricsSource>>,
    /// Provider name for LLM metrics (e.g., "OpenAI", "Anthropic")
    provider_name: String,
    /// Cached system info (using std sync RwLock for sync access)
    system: Arc<StdRwLock<System>>,
    /// Message bus counters (optional)
    bus_counters: Option<SharedCounters>,
}

impl MetricsCollector {
    pub fn new(config: MetricsConfig) -> Self {
        Self {
            config,
            registry: Arc::new(MetricsRegistry::new()),
            current_snapshot: Arc::new(RwLock::new(MetricsSnapshot::default())),
            history: Arc::new(RwLock::new(Vec::new())),
            start_time: Instant::now(),
            agent_metrics: Arc::new(RwLock::new(HashMap::new())),
            workflow_metrics: Arc::new(RwLock::new(HashMap::new())),
            plugin_metrics: Arc::new(RwLock::new(HashMap::new())),
            llm_metrics: Arc::new(RwLock::new(HashMap::new())),
            llm_metrics_source: None,
            provider_name: "unknown".to_string(),
            system: Arc::new(StdRwLock::new(System::new_all())),
            bus_counters: None,
        }
    }

    /// Set message bus counters for automatic collection.
    pub fn with_bus_counters(&mut self, counters: SharedCounters) {
        self.bus_counters = Some(counters);
    }

    /// Set LLM metrics source for pulling from persistence
    pub fn with_llm_metrics_source(
        mut self,
        source: Arc<dyn LLMMetricsSource>,
        provider_name: String,
    ) -> Self {
        self.llm_metrics_source = Some(source);
        self.provider_name = provider_name;
        self
    }

    pub fn registry(&self) -> Arc<MetricsRegistry> {
        self.registry.clone()
    }

    /// Update agent metrics
    pub async fn update_agent(&self, metrics: AgentMetrics) {
        let mut agents = self.agent_metrics.write().await;
        agents.insert(metrics.agent_id.clone(), metrics);
    }

    /// Update LLM metrics
    pub async fn update_llm(&self, metrics: LLMMetrics) {
        let mut llm = self.llm_metrics.write().await;
        llm.insert(metrics.plugin_id.clone(), metrics);
    }

    /// Remove LLM metrics
    pub async fn remove_llm(&self, plugin_id: &str) {
        let mut llm = self.llm_metrics.write().await;
        llm.remove(plugin_id);
    }

    /// Update workflow metrics
    pub async fn update_workflow(&self, metrics: WorkflowMetrics) {
        let mut workflows = self.workflow_metrics.write().await;
        workflows.insert(metrics.workflow_id.clone(), metrics);
    }

    /// Update plugin metrics
    pub async fn update_plugin(&self, metrics: PluginMetrics) {
        let mut plugins = self.plugin_metrics.write().await;
        plugins.insert(metrics.plugin_id.clone(), metrics);
    }

    /// Remove agent metrics
    pub async fn remove_agent(&self, agent_id: &str) {
        let mut agents = self.agent_metrics.write().await;
        agents.remove(agent_id);
    }

    /// Collect current system metrics
    ///
    /// Offloads the blocking `sysinfo::System::refresh_all()` call to
    /// Tokio's blocking thread pool via `spawn_blocking`, preventing it
    /// from stalling the async worker threads every collection interval.
    async fn collect_system_metrics(&self) -> SystemMetrics {
        let system = self.system.clone();
        let uptime_secs = self.start_time.elapsed().as_secs();

        tokio::task::spawn_blocking(move || {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            // Handle potential RwLock poisoning gracefully instead of panicking
            let mut sys = match system.write() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    tracing::error!(
                        "RwLock poisoned in metrics collection - recovering with poisoned data"
                    );
                    // Recover the data even though the lock is poisoned
                    // This is safe for metrics collection - we prefer stale data over no data
                    poisoned.into_inner()
                }
            };
            sys.refresh_all();

            let pid = Pid::from_u32(std::process::id());
            let (cpu_usage, memory_used, thread_count) = sys
                .process(pid)
                .map(|p| {
                    (
                        p.cpu_usage() as f64,
                        p.memory(),
                        p.tasks().iter().count() as u32,
                    )
                })
                .unwrap_or((0.0, 0, 0));

            SystemMetrics {
                cpu_usage,
                memory_used,
                memory_total: sys.total_memory(),
                uptime_secs,
                thread_count,
                timestamp: now,
            }
        })
        .await
        .unwrap_or_default()
    }

    /// Collect a snapshot
    pub async fn collect(&self) -> MetricsSnapshot {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let system = if self.config.enable_system_metrics {
            self.collect_system_metrics().await
        } else {
            SystemMetrics::default()
        };

        let agents: Vec<AgentMetrics> = if self.config.enable_agent_metrics {
            self.agent_metrics.read().await.values().cloned().collect()
        } else {
            Vec::new()
        };

        let workflows: Vec<WorkflowMetrics> = if self.config.enable_workflow_metrics {
            self.workflow_metrics
                .read()
                .await
                .values()
                .cloned()
                .collect()
        } else {
            Vec::new()
        };

        let plugins: Vec<PluginMetrics> = if self.config.enable_plugin_metrics {
            self.plugin_metrics.read().await.values().cloned().collect()
        } else {
            Vec::new()
        };

        let llm_metrics: Vec<LLMMetrics> = if self.config.enable_llm_metrics {
            if let Some(source) = &self.llm_metrics_source {
                match source.get_llm_statistics().await {
                    Ok(stats) => vec![LLMMetrics {
                        plugin_id: "llm".to_string(),
                        provider_name: self.provider_name.clone(),
                        model_name: "default".to_string(),
                        state: "running".to_string(),
                        total_requests: stats.total_requests,
                        successful_requests: stats.successful_requests,
                        failed_requests: stats.failed_requests,
                        total_tokens: stats.total_tokens,
                        prompt_tokens: stats.prompt_tokens,
                        completion_tokens: stats.completion_tokens,
                        avg_latency_ms: stats.avg_latency_ms,
                        tokens_per_second: stats.tokens_per_second,
                        time_to_first_token_ms: None,
                        requests_per_minute: 0.0,
                        error_rate: if stats.total_requests > 0 {
                            (stats.failed_requests as f64 / stats.total_requests as f64) * 100.0
                        } else {
                            0.0
                        },
                        last_request_timestamp: 0,
                    }],
                    Err(e) => {
                        tracing::warn!("Failed to fetch LLM metrics from source: {}", e);
                        self.llm_metrics.read().await.values().cloned().collect()
                    }
                }
            } else {
                self.llm_metrics.read().await.values().cloned().collect()
            }
        } else {
            Vec::new()
        };

        let custom = self.registry.collect_all().await;

        let message_bus = self.bus_counters.as_ref().map(|c| c.snapshot());

        let snapshot = MetricsSnapshot {
            system,
            agents,
            workflows,
            plugins,
            llm_metrics,
            message_bus,
            timestamp: now,
            custom,
        };

        // Update current snapshot
        {
            let mut current = self.current_snapshot.write().await;
            *current = snapshot.clone();
        }

        // Add to history
        {
            let mut history = self.history.write().await;
            history.push(snapshot.clone());

            // Trim old history
            let retention_secs = self.config.history_retention.as_secs();
            history.retain(|s| now - s.timestamp < retention_secs);
        }

        snapshot
    }

    /// Get current snapshot
    pub async fn current(&self) -> MetricsSnapshot {
        self.current_snapshot.read().await.clone()
    }

    /// Get history
    pub async fn history(&self, limit: Option<usize>) -> Vec<MetricsSnapshot> {
        let history = self.history.read().await;
        match limit {
            Some(n) => history.iter().rev().take(n).cloned().collect(),
            None => history.clone(),
        }
    }

    /// Start background collection
    pub fn start_collection(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let interval = self.config.collection_interval;
        info!("Starting metrics collection with interval {:?}", interval);

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                self.collect().await;
                debug!("Collected metrics snapshot");
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_counter() {
        let counter = Counter::new("test_counter", "A test counter");
        assert_eq!(counter.get(), 0);

        counter.inc();
        assert_eq!(counter.get(), 1);

        counter.inc_by(5);
        assert_eq!(counter.get(), 6);
    }

    #[tokio::test]
    async fn test_gauge() {
        let gauge = Gauge::new("test_gauge", "A test gauge");
        assert_eq!(gauge.get(), 0);

        gauge.set(100);
        assert_eq!(gauge.get(), 100);

        gauge.inc();
        assert_eq!(gauge.get(), 101);

        gauge.dec();
        assert_eq!(gauge.get(), 100);
    }

    #[tokio::test]
    async fn test_histogram() {
        let histogram = Histogram::new(
            "test_histogram",
            "A test histogram",
            vec![10.0, 50.0, 100.0, 500.0, 1000.0],
        );

        histogram.observe(5.0).await;
        histogram.observe(25.0).await;
        histogram.observe(75.0).await;

        let data = histogram.get_data().await;
        assert_eq!(data.count, 3);
        assert_eq!(data.sum, 105.0);
        assert_eq!(data.min, 5.0);
        assert_eq!(data.max, 75.0);
    }

    #[tokio::test]
    async fn test_metrics_registry() {
        let registry = MetricsRegistry::new();

        let counter = Counter::new("requests_total", "Total requests");
        let counter = registry.register_counter(counter).await;
        counter.inc();

        let retrieved = registry.get_counter("requests_total").await.unwrap();
        assert_eq!(retrieved.get(), 1);
    }

    #[tokio::test]
    async fn test_metrics_collector() {
        let config = MetricsConfig::default();
        let collector = MetricsCollector::new(config);

        // Add agent metrics
        collector
            .update_agent(AgentMetrics {
                agent_id: "agent-1".to_string(),
                name: "Test Agent".to_string(),
                state: "running".to_string(),
                tasks_completed: 10,
                ..Default::default()
            })
            .await;

        let snapshot = collector.collect().await;
        assert_eq!(snapshot.agents.len(), 1);
        assert_eq!(snapshot.agents[0].agent_id, "agent-1");
    }
}
