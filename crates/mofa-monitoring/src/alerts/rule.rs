//! Alert rule definitions.
//!
//! A [`Rule`] is a declarative specification of a condition to evaluate
//! against a [`MetricSource`]. Rules are backend-agnostic: they know how to
//! extract a numeric value for a named metric, compare it against a
//! threshold, and emit an [`AlertEvent`] when the condition holds for a
//! configured duration (the `for_duration` field, matching Prometheus
//! `for:` semantics).
//!
//! Supported condition families:
//!
//! - [`Condition::Threshold`] — compare a scalar metric to a constant
//!   using [`ComparisonOp`] (`>`, `>=`, `<`, `<=`, `==`, `!=`).
//! - [`Condition::RateOfChange`] — compare the per-second derivative of a
//!   counter over a sliding window against a threshold.
//! - [`Condition::Absent`] — fire when a required metric has not been
//!   observed within a staleness window.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// A complete alerting rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// Unique rule identifier. Used as the natural key for deduplicating
    /// events and tracking active-firing state across evaluations.
    pub name: String,
    /// Human-readable description (shown in notifier payloads).
    pub description: String,
    /// Severity of events emitted by this rule.
    pub severity: Severity,
    /// The condition that must hold for the rule to fire.
    pub condition: Condition,
    /// The condition must hold continuously for this duration before the
    /// rule transitions into the `Firing` state. Zero means fire
    /// immediately on first match.
    pub for_duration: Duration,
    /// Static labels attached to every event emitted by this rule.
    /// Merged into the event payload alongside any labels resolved at
    /// evaluation time.
    pub labels: HashMap<String, String>,
    /// Free-form annotations (human-oriented runbook links, summaries).
    pub annotations: HashMap<String, String>,
}

impl Rule {
    /// Construct a minimal rule. Use the `with_*` builders to add labels
    /// and annotations.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        severity: Severity,
        condition: Condition,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            severity,
            condition,
            for_duration: Duration::ZERO,
            labels: HashMap::new(),
            annotations: HashMap::new(),
        }
    }

    #[must_use]
    pub fn with_for(mut self, duration: Duration) -> Self {
        self.for_duration = duration;
        self
    }

    #[must_use]
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_annotation(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.annotations.insert(key.into(), value.into());
        self
    }

    /// Returns the primary metric name this rule depends on, if the
    /// condition references one. Used for precomputed index lookups.
    pub fn primary_metric(&self) -> Option<&str> {
        match &self.condition {
            Condition::Threshold { metric, .. }
            | Condition::RateOfChange { metric, .. }
            | Condition::Absent { metric, .. } => Some(metric),
        }
    }
}

/// Severity level attached to every event emitted by the rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Severity {
    /// Purely informational — surface in audit trails, don't page.
    Info,
    /// Degraded behaviour. Investigate during business hours.
    Warning,
    /// User-visible impact. Page on-call.
    Critical,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Critical => "critical",
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Comparison operator for threshold conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ComparisonOp {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
    Neq,
}

impl ComparisonOp {
    /// Apply the comparison `value OP threshold`.
    pub fn apply(self, value: f64, threshold: f64) -> bool {
        match self {
            ComparisonOp::Gt => value > threshold,
            ComparisonOp::Gte => value >= threshold,
            ComparisonOp::Lt => value < threshold,
            ComparisonOp::Lte => value <= threshold,
            ComparisonOp::Eq => (value - threshold).abs() < f64::EPSILON,
            ComparisonOp::Neq => (value - threshold).abs() >= f64::EPSILON,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ComparisonOp::Gt => ">",
            ComparisonOp::Gte => ">=",
            ComparisonOp::Lt => "<",
            ComparisonOp::Lte => "<=",
            ComparisonOp::Eq => "==",
            ComparisonOp::Neq => "!=",
        }
    }
}

impl std::fmt::Display for ComparisonOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The evaluable condition of a rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Condition {
    /// Fire when `metric OP threshold` holds. E.g. "error_rate > 0.05".
    Threshold {
        metric: String,
        op: ComparisonOp,
        threshold: f64,
    },
    /// Fire when the per-second derivative of a monotonic counter over
    /// `window` seconds satisfies `OP threshold`. The evaluator is
    /// responsible for maintaining the window samples; see
    /// [`super::evaluator`].
    RateOfChange {
        metric: String,
        op: ComparisonOp,
        threshold: f64,
        window: Duration,
    },
    /// Fire when the metric has not been observed within `staleness`
    /// seconds of the evaluation tick — useful for heartbeat / liveness.
    Absent {
        metric: String,
        staleness: Duration,
    },
}

impl Condition {
    /// Human-readable summary, useful for notifier payloads.
    pub fn summary(&self) -> String {
        match self {
            Condition::Threshold {
                metric,
                op,
                threshold,
            } => format!("{metric} {op} {threshold}"),
            Condition::RateOfChange {
                metric,
                op,
                threshold,
                window,
            } => format!("rate({metric}[{:?}]) {op} {threshold}", window),
            Condition::Absent { metric, staleness } => {
                format!("absent({metric}) for {:?}", staleness)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_ordering() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Critical);
    }

    #[test]
    fn comparison_op_gt() {
        assert!(ComparisonOp::Gt.apply(10.0, 5.0));
        assert!(!ComparisonOp::Gt.apply(5.0, 5.0));
        assert!(!ComparisonOp::Gt.apply(1.0, 5.0));
    }

    #[test]
    fn comparison_op_gte() {
        assert!(ComparisonOp::Gte.apply(10.0, 5.0));
        assert!(ComparisonOp::Gte.apply(5.0, 5.0));
        assert!(!ComparisonOp::Gte.apply(1.0, 5.0));
    }

    #[test]
    fn comparison_op_lt_lte() {
        assert!(ComparisonOp::Lt.apply(1.0, 5.0));
        assert!(!ComparisonOp::Lt.apply(5.0, 5.0));
        assert!(ComparisonOp::Lte.apply(5.0, 5.0));
    }

    #[test]
    fn comparison_op_eq_neq() {
        assert!(ComparisonOp::Eq.apply(5.0, 5.0));
        assert!(!ComparisonOp::Eq.apply(5.0001, 5.0));
        assert!(ComparisonOp::Neq.apply(5.0001, 5.0));
    }

    #[test]
    fn rule_primary_metric_threshold() {
        let r = Rule::new(
            "r1",
            "test",
            Severity::Warning,
            Condition::Threshold {
                metric: "cpu".into(),
                op: ComparisonOp::Gt,
                threshold: 0.8,
            },
        );
        assert_eq!(r.primary_metric(), Some("cpu"));
    }

    #[test]
    fn rule_builder_methods() {
        let r = Rule::new(
            "r1",
            "d",
            Severity::Info,
            Condition::Absent {
                metric: "heartbeat".into(),
                staleness: Duration::from_secs(60),
            },
        )
        .with_for(Duration::from_secs(30))
        .with_label("team", "platform")
        .with_annotation("runbook", "https://example/runbook");

        assert_eq!(r.for_duration, Duration::from_secs(30));
        assert_eq!(r.labels.get("team").unwrap(), "platform");
        assert_eq!(r.annotations.get("runbook").unwrap(), "https://example/runbook");
    }

    #[test]
    fn condition_summary_includes_parameters() {
        let c = Condition::Threshold {
            metric: "err".into(),
            op: ComparisonOp::Gte,
            threshold: 0.05,
        };
        let s = c.summary();
        assert!(s.contains("err"));
        assert!(s.contains(">="));
        assert!(s.contains("0.05"));
    }

    #[test]
    fn rule_json_roundtrip() {
        let r = Rule::new(
            "r",
            "d",
            Severity::Critical,
            Condition::RateOfChange {
                metric: "reqs".into(),
                op: ComparisonOp::Gt,
                threshold: 100.0,
                window: Duration::from_secs(60),
            },
        )
        .with_for(Duration::from_secs(120))
        .with_label("env", "prod");
        let s = serde_json::to_string(&r).unwrap();
        let parsed: Rule = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn severity_display_and_as_str_agree() {
        for s in [Severity::Info, Severity::Warning, Severity::Critical] {
            assert_eq!(format!("{s}"), s.as_str());
        }
    }
}
