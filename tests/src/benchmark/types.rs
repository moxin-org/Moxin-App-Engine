use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

use crate::report::{TestCaseResult, TestReport, TestStatus};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MetricThreshold {
    pub max: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCallMetric {
    pub name: String,
    pub calls_per_iteration: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BenchmarkThresholds {
    pub max_mean_latency_micros: Option<MetricThreshold>,
    pub max_peak_latency_micros: Option<MetricThreshold>,
    pub max_infer_calls_per_iteration: Option<MetricThreshold>,
    pub max_bus_messages_per_iteration: Option<MetricThreshold>,
    pub max_tool_calls_per_iteration: Vec<ToolCallMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BenchmarkCaseConfig {
    pub name: String,
    pub iterations: usize,
    pub warmup_iterations: usize,
    pub thresholds: BenchmarkThresholds,
}

impl BenchmarkCaseConfig {
    pub fn new(name: impl Into<String>, iterations: usize) -> Self {
        Self {
            name: name.into(),
            iterations,
            warmup_iterations: 0,
            thresholds: BenchmarkThresholds::default(),
        }
    }

    pub fn with_warmup_iterations(mut self, warmup_iterations: usize) -> Self {
        self.warmup_iterations = warmup_iterations;
        self
    }

    pub fn with_thresholds(mut self, thresholds: BenchmarkThresholds) -> Self {
        self.thresholds = thresholds;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BenchmarkCaseResult {
    pub name: String,
    pub iterations: usize,
    pub warmup_iterations: usize,
    pub mean_latency_micros: u64,
    pub peak_latency_micros: u64,
    pub total_infer_calls: u64,
    pub infer_calls_per_iteration: u64,
    pub total_bus_messages: u64,
    pub bus_messages_per_iteration: u64,
    pub tool_calls_per_iteration: BTreeMap<String, u64>,
    pub sample_latencies_micros: Vec<u64>,
    pub regressions: Vec<String>,
}

impl BenchmarkCaseResult {
    pub fn passed(&self) -> bool {
        self.regressions.is_empty()
    }

    pub fn to_test_case_result(&self) -> TestCaseResult {
        let metadata = vec![
            (
                "mean_latency_micros".to_string(),
                self.mean_latency_micros.to_string(),
            ),
            (
                "peak_latency_micros".to_string(),
                self.peak_latency_micros.to_string(),
            ),
            (
                "infer_calls_per_iteration".to_string(),
                self.infer_calls_per_iteration.to_string(),
            ),
            (
                "bus_messages_per_iteration".to_string(),
                self.bus_messages_per_iteration.to_string(),
            ),
        ];

        TestCaseResult {
            name: self.name.clone(),
            status: if self.passed() {
                TestStatus::Passed
            } else {
                TestStatus::Failed
            },
            duration: Duration::from_micros(self.mean_latency_micros),
            error: if self.passed() {
                None
            } else {
                Some(self.regressions.join("; "))
            },
            metadata,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BenchmarkReport {
    pub suite_name: String,
    pub total_duration_micros: u64,
    pub timestamp: u64,
    pub cases: Vec<BenchmarkCaseResult>,
}

impl BenchmarkReport {
    pub fn total(&self) -> usize {
        self.cases.len()
    }

    pub fn failed(&self) -> usize {
        self.cases.iter().filter(|case| !case.passed()).count()
    }

    pub fn passed(&self) -> usize {
        self.cases.iter().filter(|case| case.passed()).count()
    }

    pub fn to_test_report(&self) -> TestReport {
        TestReport {
            suite_name: self.suite_name.clone(),
            results: self
                .cases
                .iter()
                .map(BenchmarkCaseResult::to_test_case_result)
                .collect(),
            total_duration: Duration::from_micros(self.total_duration_micros),
            timestamp: self.timestamp,
        }
    }
}
