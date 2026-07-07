use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;

use crate::backend::MockLLMBackend;
use crate::bus::MockAgentBus;
use crate::clock::{Clock, SystemClock};
use crate::tools::MockTool;

use super::types::{
    BenchmarkCaseConfig, BenchmarkCaseResult, BenchmarkReport, BenchmarkThresholds, MetricThreshold,
};

#[derive(Clone)]
pub struct BenchmarkContext {
    pub backend: Arc<MockLLMBackend>,
    pub bus: Arc<MockAgentBus>,
    pub tools: HashMap<String, Arc<MockTool>>,
    pub model_name: String,
}

impl BenchmarkContext {
    pub fn new(
        backend: Arc<MockLLMBackend>,
        bus: Arc<MockAgentBus>,
        tools: HashMap<String, Arc<MockTool>>,
        model_name: impl Into<String>,
    ) -> Self {
        Self {
            backend,
            bus,
            tools,
            model_name: model_name.into(),
        }
    }

    pub fn tool(&self, name: &str) -> Option<Arc<MockTool>> {
        self.tools.get(name).cloned()
    }
}

pub struct BenchmarkRunner {
    suite_name: String,
    clock: Arc<dyn Clock>,
    started_at: Instant,
    cases: Vec<BenchmarkCaseResult>,
}

impl BenchmarkRunner {
    pub fn new(suite_name: impl Into<String>) -> Self {
        Self {
            suite_name: suite_name.into(),
            clock: Arc::new(SystemClock),
            started_at: Instant::now(),
            cases: Vec::new(),
        }
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    pub async fn run_case<Setup, Bench, Fut>(
        mut self,
        config: BenchmarkCaseConfig,
        setup: Setup,
        bench: Bench,
    ) -> Result<Self>
    where
        Setup: Fn() -> BenchmarkContext,
        Bench: Fn(BenchmarkContext) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let iterations = config.iterations.max(1);

        for _ in 0..config.warmup_iterations {
            let context = setup();
            bench(context).await?;
        }

        let mut sample_latencies = Vec::with_capacity(iterations);
        let mut total_infer_calls = 0_u64;
        let mut total_bus_messages = 0_u64;
        let mut total_tool_calls: BTreeMap<String, u64> = BTreeMap::new();

        for _ in 0..iterations {
            let context = setup();
            let start = Instant::now();
            bench(context.clone()).await?;
            let elapsed = duration_to_micros(start.elapsed());
            sample_latencies.push(elapsed);

            total_infer_calls += context.backend.call_count() as u64;
            total_bus_messages += context.bus.message_count().await as u64;

            for (name, tool) in &context.tools {
                let count = tool.call_count().await as u64;
                if count > 0 {
                    *total_tool_calls.entry(name.clone()).or_default() += count;
                }
            }
        }

        let mean_latency_micros = sample_latencies.iter().sum::<u64>() / iterations as u64;
        let peak_latency_micros = sample_latencies.iter().copied().max().unwrap_or(0);
        let infer_calls_per_iteration = total_infer_calls / iterations as u64;
        let bus_messages_per_iteration = total_bus_messages / iterations as u64;
        let tool_calls_per_iteration = total_tool_calls
            .into_iter()
            .map(|(name, total)| (name, total / iterations as u64))
            .collect::<BTreeMap<_, _>>();

        let regressions = evaluate_thresholds(
            &config.thresholds,
            mean_latency_micros,
            peak_latency_micros,
            infer_calls_per_iteration,
            bus_messages_per_iteration,
            &tool_calls_per_iteration,
        );

        self.cases.push(BenchmarkCaseResult {
            name: config.name,
            iterations,
            warmup_iterations: config.warmup_iterations,
            mean_latency_micros,
            peak_latency_micros,
            total_infer_calls,
            infer_calls_per_iteration,
            total_bus_messages,
            bus_messages_per_iteration,
            tool_calls_per_iteration,
            sample_latencies_micros: sample_latencies,
            regressions,
        });

        Ok(self)
    }

    pub fn build(self) -> BenchmarkReport {
        BenchmarkReport {
            suite_name: self.suite_name,
            total_duration_micros: duration_to_micros(self.started_at.elapsed()),
            timestamp: self.clock.now_millis(),
            cases: self.cases,
        }
    }
}

fn evaluate_thresholds(
    thresholds: &BenchmarkThresholds,
    mean_latency_micros: u64,
    peak_latency_micros: u64,
    infer_calls_per_iteration: u64,
    bus_messages_per_iteration: u64,
    tool_calls_per_iteration: &BTreeMap<String, u64>,
) -> Vec<String> {
    let mut regressions = Vec::new();

    push_threshold_failure(
        &mut regressions,
        "mean latency",
        thresholds.max_mean_latency_micros.as_ref(),
        mean_latency_micros,
        "micros",
    );
    push_threshold_failure(
        &mut regressions,
        "peak latency",
        thresholds.max_peak_latency_micros.as_ref(),
        peak_latency_micros,
        "micros",
    );
    push_threshold_failure(
        &mut regressions,
        "infer calls per iteration",
        thresholds.max_infer_calls_per_iteration.as_ref(),
        infer_calls_per_iteration,
        "calls",
    );
    push_threshold_failure(
        &mut regressions,
        "bus messages per iteration",
        thresholds.max_bus_messages_per_iteration.as_ref(),
        bus_messages_per_iteration,
        "messages",
    );

    for tool_metric in &thresholds.max_tool_calls_per_iteration {
        let actual = tool_calls_per_iteration
            .get(&tool_metric.name)
            .copied()
            .unwrap_or(0);

        if actual > tool_metric.calls_per_iteration {
            regressions.push(format!(
                "tool '{}' exceeded threshold: {} calls > {} calls",
                tool_metric.name, actual, tool_metric.calls_per_iteration
            ));
        }
    }

    regressions
}

fn push_threshold_failure(
    regressions: &mut Vec<String>,
    label: &str,
    threshold: Option<&MetricThreshold>,
    actual: u64,
    unit: &str,
) {
    if let Some(threshold) = threshold {
        if actual > threshold.max {
            regressions.push(format!(
                "{} exceeded threshold: {} {} > {} {}",
                label, actual, unit, threshold.max, unit
            ));
        }
    }
}

fn duration_to_micros(duration: std::time::Duration) -> u64 {
    duration.as_micros().min(u64::MAX as u128) as u64
}
