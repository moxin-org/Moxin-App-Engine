//! Metric source abstraction.
//!
//! The alert evaluator is parameterised over a [`MetricSource`] trait so
//! it can be wired against the existing `MetricsCollector`, a Prometheus
//! scrape, a test fixture, or any other backend that can return the
//! current value of a named metric.
//!
//! Implementors are expected to be cheap to query — the evaluator polls
//! every tick. Backends that aggregate from a remote scrape should cache
//! the last snapshot and refresh it out-of-band.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, SystemTime};

/// A point-in-time reading of a metric.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricSample {
    pub value: f64,
    pub observed_at: SystemTime,
}

/// Backend-agnostic metric lookup used by the evaluator.
#[async_trait]
pub trait MetricSource: Send + Sync {
    /// Return the latest sample for `metric_name`, if known.
    ///
    /// Returning `None` has two meanings depending on the rule kind:
    /// for `Threshold`/`RateOfChange` rules the evaluator treats `None`
    /// as "no data, skip this tick"; for `Absent` rules it is exactly
    /// the signal the rule fires on.
    async fn sample(&self, metric_name: &str) -> Option<MetricSample>;
}

/// In-memory test/bench metric source. Tests inject samples via
/// [`InMemoryMetricSource::set`]; downstream evaluators read them via the
/// trait.
#[derive(Debug, Default)]
pub struct InMemoryMetricSource {
    samples: RwLock<HashMap<String, MetricSample>>,
}

impl InMemoryMetricSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or overwrite a sample, stamping it with the current wall
    /// clock.
    pub fn set(&self, metric: impl Into<String>, value: f64) {
        let mut w = self.samples.write().unwrap();
        w.insert(
            metric.into(),
            MetricSample {
                value,
                observed_at: SystemTime::now(),
            },
        );
    }

    /// Insert with a caller-specified observation time. Useful for
    /// exercising `Absent` rules with staleness windows without waiting
    /// real time in a test.
    pub fn set_at(&self, metric: impl Into<String>, value: f64, at: SystemTime) {
        let mut w = self.samples.write().unwrap();
        w.insert(
            metric.into(),
            MetricSample {
                value,
                observed_at: at,
            },
        );
    }

    /// Remove a metric entirely.
    pub fn forget(&self, metric: &str) {
        let mut w = self.samples.write().unwrap();
        w.remove(metric);
    }

    /// How many metrics are currently tracked. Diagnostic helper.
    pub fn len(&self) -> usize {
        self.samples.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[async_trait]
impl MetricSource for InMemoryMetricSource {
    async fn sample(&self, metric_name: &str) -> Option<MetricSample> {
        self.samples.read().unwrap().get(metric_name).cloned()
    }
}

/// Helper that freezes a sample as being observed `ago` before `now`.
pub fn ago(duration: Duration) -> SystemTime {
    SystemTime::now()
        .checked_sub(duration)
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_source_reads_and_writes() {
        let src = InMemoryMetricSource::new();
        assert!(src.is_empty());
        src.set("cpu", 0.5);
        let s = src.sample("cpu").await.unwrap();
        assert_eq!(s.value, 0.5);
    }

    #[tokio::test]
    async fn in_memory_source_missing_returns_none() {
        let src = InMemoryMetricSource::new();
        assert!(src.sample("nope").await.is_none());
    }

    #[tokio::test]
    async fn in_memory_source_forget() {
        let src = InMemoryMetricSource::new();
        src.set("x", 1.0);
        assert!(src.sample("x").await.is_some());
        src.forget("x");
        assert!(src.sample("x").await.is_none());
    }

    #[tokio::test]
    async fn in_memory_source_set_at_preserves_timestamp() {
        let src = InMemoryMetricSource::new();
        let earlier = ago(Duration::from_secs(120));
        src.set_at("stale", 0.0, earlier);
        let s = src.sample("stale").await.unwrap();
        assert_eq!(s.observed_at, earlier);
    }

    #[tokio::test]
    async fn in_memory_source_overwrites_existing() {
        let src = InMemoryMetricSource::new();
        src.set("k", 1.0);
        src.set("k", 2.0);
        assert_eq!(src.sample("k").await.unwrap().value, 2.0);
    }
}
