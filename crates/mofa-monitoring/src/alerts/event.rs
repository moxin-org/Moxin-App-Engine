//! Alert events.
//!
//! An [`AlertEvent`] is the output of the evaluator — a structured record
//! describing a state transition for a given rule. Notifiers consume
//! events; aggregators (dashboards, audit logs) may aggregate them over
//! time.
//!
//! Events carry enough context (labels, annotations, observed value) to
//! render directly in a notifier payload without re-resolving the rule.

use super::rule::{Condition, Severity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// State of a rule at the point the event was generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AlertState {
    /// The rule has just transitioned from `Pending`/`Resolved` into
    /// active firing — `for_duration` has elapsed.
    Firing,
    /// The rule condition was true on this tick but `for_duration` has
    /// not yet elapsed. Notifiers typically ignore these; dashboards may
    /// show them as a "soaking" badge.
    Pending,
    /// The rule fired previously and has now stopped matching.
    Resolved,
}

impl AlertState {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertState::Firing => "firing",
            AlertState::Pending => "pending",
            AlertState::Resolved => "resolved",
        }
    }
}

impl std::fmt::Display for AlertState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single alert event emitted by the evaluator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertEvent {
    /// Name of the rule that emitted this event (unique within a
    /// `RuleSet`).
    pub rule_name: String,
    /// State transition this event reports.
    pub state: AlertState,
    /// Severity copied from the rule for notifier convenience.
    pub severity: Severity,
    /// Snapshot of the rule condition at emission time. Kept by value so
    /// events remain meaningful after a rule is edited or removed.
    pub condition: Condition,
    /// Observed value of the primary metric at the time the event fired.
    /// `None` for conditions that don't have a single scalar (e.g.
    /// `Absent`).
    pub observed_value: Option<f64>,
    /// Merged label set — rule labels plus any evaluator-supplied labels.
    pub labels: HashMap<String, String>,
    /// Annotations copied from the rule.
    pub annotations: HashMap<String, String>,
    /// Wall-clock timestamp of the event.
    pub at: SystemTime,
}

impl AlertEvent {
    /// Convenience for `state == Firing`.
    pub fn is_firing(&self) -> bool {
        self.state == AlertState::Firing
    }

    /// Convenience for `state == Resolved`.
    pub fn is_resolved(&self) -> bool {
        self.state == AlertState::Resolved
    }

    /// Construct a short one-line summary suitable for logs or chat
    /// notifiers.
    pub fn short_summary(&self) -> String {
        let value = self
            .observed_value
            .map(|v| format!(" (observed={v})"))
            .unwrap_or_default();
        format!(
            "[{sev}] {rule}: {state}{value} — {cond}",
            sev = self.severity,
            rule = self.rule_name,
            state = self.state,
            value = value,
            cond = self.condition.summary()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::rule::{ComparisonOp, Condition, Severity};
    use super::*;

    fn sample_event(state: AlertState, value: Option<f64>) -> AlertEvent {
        AlertEvent {
            rule_name: "high-error-rate".into(),
            state,
            severity: Severity::Warning,
            condition: Condition::Threshold {
                metric: "err".into(),
                op: ComparisonOp::Gt,
                threshold: 0.05,
            },
            observed_value: value,
            labels: HashMap::from([("env".into(), "prod".into())]),
            annotations: HashMap::new(),
            at: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn alert_state_display() {
        assert_eq!(AlertState::Firing.to_string(), "firing");
        assert_eq!(AlertState::Pending.to_string(), "pending");
        assert_eq!(AlertState::Resolved.to_string(), "resolved");
    }

    #[test]
    fn is_firing_and_is_resolved_disjoint() {
        let e = sample_event(AlertState::Firing, Some(0.1));
        assert!(e.is_firing());
        assert!(!e.is_resolved());

        let e = sample_event(AlertState::Resolved, Some(0.01));
        assert!(!e.is_firing());
        assert!(e.is_resolved());
    }

    #[test]
    fn short_summary_includes_observed_value() {
        let e = sample_event(AlertState::Firing, Some(0.12));
        let s = e.short_summary();
        assert!(s.contains("0.12"));
        assert!(s.contains("high-error-rate"));
        assert!(s.contains("warning"));
    }

    #[test]
    fn short_summary_without_value() {
        let e = sample_event(AlertState::Firing, None);
        let s = e.short_summary();
        assert!(!s.contains("observed"));
    }

    #[test]
    fn alert_event_json_roundtrip() {
        let e = sample_event(AlertState::Pending, Some(0.05));
        let s = serde_json::to_string(&e).unwrap();
        let parsed: AlertEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, e);
    }
}
