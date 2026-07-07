//! Core data types for test reports.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Outcome of a single test case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
}

impl TestStatus {
    pub fn is_passed(&self) -> bool {
        matches!(self, Self::Passed)
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed)
    }

    pub fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped)
    }
}

impl std::fmt::Display for TestStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Passed => write!(f, "passed"),
            Self::Failed => write!(f, "failed"),
            Self::Skipped => write!(f, "skipped"),
        }
    }
}

/// Result of a single test case execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCaseResult {
    pub name: String,
    pub status: TestStatus,
    #[serde(with = "duration_millis")]
    pub duration: Duration,
    pub error: Option<String>,
    pub metadata: Vec<(String, String)>,
}

/// Aggregated report for a test suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestReport {
    pub suite_name: String,
    pub results: Vec<TestCaseResult>,
    #[serde(with = "duration_millis")]
    pub total_duration: Duration,
    pub timestamp: u64,
}

mod duration_millis {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(d: &Duration, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_u64(d.as_millis() as u64)
    }

    pub fn deserialize<'de, D>(d: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let ms = u64::deserialize(d)?;
        Ok(Duration::from_millis(ms))
    }
}

impl TestReport {
    /// Merge multiple reports into a single aggregated report.
    /// Results are concatenated in order. Total duration is the max of all suites.
    /// Timestamp is taken from the first report (or 0 if empty).
    pub fn merge(suite_name: impl Into<String>, reports: &[TestReport]) -> TestReport {
        let mut results = Vec::new();
        let mut total_duration = Duration::from_secs(0);
        let timestamp = reports.first().map(|r| r.timestamp).unwrap_or(0);

        for report in reports {
            results.extend(report.results.clone());
            if report.total_duration > total_duration {
                total_duration = report.total_duration;
            }
        }

        TestReport {
            suite_name: suite_name.into(),
            results,
            total_duration,
            timestamp,
        }
    }

    /// Total number of test cases.
    pub fn total(&self) -> usize {
        self.results.len()
    }

    /// Number of passed test cases.
    pub fn passed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.status == TestStatus::Passed)
            .count()
    }

    /// Number of failed test cases.
    pub fn failed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.status == TestStatus::Failed)
            .count()
    }

    /// Number of skipped test cases.
    pub fn skipped(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.status == TestStatus::Skipped)
            .count()
    }

    /// Pass rate as a fraction in `[0.0, 1.0]`. Returns `1.0` for an empty suite.
    pub fn pass_rate(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 1.0;
        }
        self.passed() as f64 / total as f64
    }

    /// The `n` slowest test cases, sorted by descending duration.
    pub fn slowest(&self, n: usize) -> Vec<&TestCaseResult> {
        let mut sorted: Vec<&TestCaseResult> = self.results.iter().collect();
        sorted.sort_by(|a, b| b.duration.cmp(&a.duration));
        sorted.truncate(n);
        sorted
    }

    /// All test cases that failed.
    pub fn failures(&self) -> Vec<&TestCaseResult> {
        self.results
            .iter()
            .filter(|r| r.status == TestStatus::Failed)
            .collect()
    }
}
