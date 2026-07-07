//! Alert rule evaluator.
//!
//! The [`Evaluator`] owns a set of rules and a [`MetricSource`]. On each
//! call to [`Evaluator::evaluate`] it resolves every rule against the
//! current metric snapshot and emits the state-transition events.
//!
//! State tracking matches Prometheus semantics: a rule is `Pending` as
//! soon as its condition starts matching, transitions to `Firing` once
//! the condition has held continuously for at least `for_duration`, and
//! emits a `Resolved` event the first tick the condition stops matching
//! after having fired.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use super::event::{AlertEvent, AlertState};
use super::rule::{Condition, Rule};
use super::source::{MetricSample, MetricSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleState {
    Inactive,
    Pending {
        since: SystemTime,
    },
    Firing,
}

#[derive(Debug)]
struct RuleRuntime {
    state: RuleState,
    /// Sliding window of samples for rate-of-change conditions. Each entry
    /// is `(observed_at, cumulative_value)`. The deque is trimmed per
    /// evaluation to the configured window.
    window: VecDeque<(SystemTime, f64)>,
}

impl RuleRuntime {
    fn new() -> Self {
        Self {
            state: RuleState::Inactive,
            window: VecDeque::new(),
        }
    }
}

/// Configuration knobs for the evaluator itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluatorConfig {
    /// Maximum number of samples held per rate-of-change window before
    /// older entries are evicted, regardless of their age. Guards
    /// unbounded memory growth if a rule's `window` is misconfigured.
    pub max_window_samples: usize,
}

impl Default for EvaluatorConfig {
    fn default() -> Self {
        Self {
            max_window_samples: 1024,
        }
    }
}

pub struct Evaluator<S: MetricSource + ?Sized> {
    rules: Vec<Rule>,
    runtimes: Mutex<HashMap<String, RuleRuntime>>,
    source: Arc<S>,
    config: EvaluatorConfig,
}

impl<S: MetricSource + ?Sized> Evaluator<S> {
    pub fn new(rules: Vec<Rule>, source: Arc<S>) -> Self {
        Self::with_config(rules, source, EvaluatorConfig::default())
    }

    pub fn with_config(rules: Vec<Rule>, source: Arc<S>, config: EvaluatorConfig) -> Self {
        let mut runtimes = HashMap::with_capacity(rules.len());
        for r in &rules {
            runtimes.insert(r.name.clone(), RuleRuntime::new());
        }
        Self {
            rules,
            runtimes: Mutex::new(runtimes),
            source,
            config,
        }
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Evaluate every rule once. Returns the events produced on this
    /// tick — typically a small handful, since only state transitions
    /// emit.
    pub async fn evaluate(&self) -> Vec<AlertEvent> {
        let now = SystemTime::now();
        let mut events = Vec::new();

        for rule in &self.rules {
            let (matched, observed) = match &rule.condition {
                Condition::Threshold {
                    metric,
                    op,
                    threshold,
                } => match self.source.sample(metric).await {
                    Some(sample) => (op.apply(sample.value, *threshold), Some(sample.value)),
                    None => (false, None),
                },
                Condition::RateOfChange {
                    metric,
                    op,
                    threshold,
                    window,
                } => match self.source.sample(metric).await {
                    Some(sample) => self.evaluate_rate(rule, sample, *op, *threshold, *window, now),
                    None => (false, None),
                },
                Condition::Absent { metric, staleness } => match self.source.sample(metric).await {
                    Some(sample) => {
                        let is_stale = now
                            .duration_since(sample.observed_at)
                            .map(|d| d > *staleness)
                            .unwrap_or(false);
                        (is_stale, Some(sample.value))
                    }
                    None => (true, None),
                },
            };

            if let Some(event) = self.transition(rule, matched, observed, now) {
                events.push(event);
            }
        }
        events
    }

    fn evaluate_rate(
        &self,
        rule: &Rule,
        sample: MetricSample,
        op: super::rule::ComparisonOp,
        threshold: f64,
        window: Duration,
        now: SystemTime,
    ) -> (bool, Option<f64>) {
        let mut runtimes = self.runtimes.lock().unwrap();
        let rt = runtimes
            .entry(rule.name.clone())
            .or_insert_with(RuleRuntime::new);

        rt.window.push_back((sample.observed_at, sample.value));
        // Trim by age first.
        while let Some(&(t, _)) = rt.window.front() {
            let too_old = now
                .duration_since(t)
                .map(|d| d > window)
                .unwrap_or(false);
            if too_old {
                rt.window.pop_front();
            } else {
                break;
            }
        }
        // Then by count.
        while rt.window.len() > self.config.max_window_samples {
            rt.window.pop_front();
        }

        if rt.window.len() < 2 {
            return (false, Some(sample.value));
        }
        let (t0, v0) = rt.window.front().copied().unwrap();
        let (t1, v1) = rt.window.back().copied().unwrap();
        let secs = match t1.duration_since(t0) {
            Ok(d) if !d.is_zero() => d.as_secs_f64(),
            _ => return (false, Some(sample.value)),
        };
        let rate = (v1 - v0) / secs;
        (op.apply(rate, threshold), Some(rate))
    }

    fn transition(
        &self,
        rule: &Rule,
        matched: bool,
        observed: Option<f64>,
        now: SystemTime,
    ) -> Option<AlertEvent> {
        let mut runtimes = self.runtimes.lock().unwrap();
        let rt = runtimes
            .entry(rule.name.clone())
            .or_insert_with(RuleRuntime::new);

        let next = match (rt.state, matched) {
            (RuleState::Inactive, true) => {
                if rule.for_duration.is_zero() {
                    RuleState::Firing
                } else {
                    RuleState::Pending { since: now }
                }
            }
            (RuleState::Pending { since }, true) => {
                let elapsed = now.duration_since(since).unwrap_or(Duration::ZERO);
                if elapsed >= rule.for_duration {
                    RuleState::Firing
                } else {
                    RuleState::Pending { since }
                }
            }
            (_, false) => RuleState::Inactive,
            (RuleState::Firing, true) => RuleState::Firing,
        };

        let prev = rt.state;
        rt.state = next;

        // Only state transitions emit events.
        let emit_state = match (prev, next) {
            (RuleState::Inactive, RuleState::Pending { .. }) => Some(AlertState::Pending),
            (RuleState::Inactive, RuleState::Firing) => Some(AlertState::Firing),
            (RuleState::Pending { .. }, RuleState::Firing) => Some(AlertState::Firing),
            (RuleState::Firing, RuleState::Inactive) => Some(AlertState::Resolved),
            (RuleState::Pending { .. }, RuleState::Inactive) => None,
            _ => None,
        };

        emit_state.map(|state| AlertEvent {
            rule_name: rule.name.clone(),
            state,
            severity: rule.severity,
            condition: rule.condition.clone(),
            observed_value: observed,
            labels: rule.labels.clone(),
            annotations: rule.annotations.clone(),
            at: now,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::rule::{ComparisonOp, Condition, Rule, Severity};
    use super::super::source::InMemoryMetricSource;
    use super::*;

    fn threshold_rule(name: &str, metric: &str, thr: f64) -> Rule {
        Rule::new(
            name,
            "",
            Severity::Warning,
            Condition::Threshold {
                metric: metric.into(),
                op: ComparisonOp::Gt,
                threshold: thr,
            },
        )
    }

    #[tokio::test]
    async fn evaluator_fires_threshold_immediately_when_for_zero() {
        let src = Arc::new(InMemoryMetricSource::new());
        src.set("x", 10.0);
        let ev = Evaluator::new(vec![threshold_rule("r1", "x", 5.0)], src.clone());
        let out = ev.evaluate().await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].state, AlertState::Firing);
        assert_eq!(out[0].observed_value, Some(10.0));
    }

    #[tokio::test]
    async fn evaluator_does_not_fire_when_condition_false() {
        let src = Arc::new(InMemoryMetricSource::new());
        src.set("x", 1.0);
        let ev = Evaluator::new(vec![threshold_rule("r1", "x", 5.0)], src);
        let out = ev.evaluate().await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn evaluator_emits_resolved_once_condition_clears() {
        let src = Arc::new(InMemoryMetricSource::new());
        src.set("x", 10.0);
        let ev = Evaluator::new(vec![threshold_rule("r1", "x", 5.0)], src.clone());
        let fired = ev.evaluate().await;
        assert_eq!(fired[0].state, AlertState::Firing);
        src.set("x", 1.0);
        let resolved = ev.evaluate().await;
        assert_eq!(resolved[0].state, AlertState::Resolved);
    }

    #[tokio::test]
    async fn evaluator_emits_pending_then_firing_when_for_nonzero() {
        let src = Arc::new(InMemoryMetricSource::new());
        src.set("x", 10.0);
        let rule = threshold_rule("r1", "x", 5.0).with_for(Duration::from_millis(50));
        let ev = Evaluator::new(vec![rule], src.clone());

        let first = ev.evaluate().await;
        assert_eq!(first[0].state, AlertState::Pending);

        tokio::time::sleep(Duration::from_millis(60)).await;
        let second = ev.evaluate().await;
        assert_eq!(second[0].state, AlertState::Firing);
    }

    #[tokio::test]
    async fn evaluator_absent_rule_fires_when_metric_missing() {
        let src = Arc::new(InMemoryMetricSource::new());
        let rule = Rule::new(
            "heartbeat-gone",
            "",
            Severity::Critical,
            Condition::Absent {
                metric: "heartbeat".into(),
                staleness: Duration::from_secs(5),
            },
        );
        let ev = Evaluator::new(vec![rule], src);
        let out = ev.evaluate().await;
        assert_eq!(out[0].state, AlertState::Firing);
    }

    #[tokio::test]
    async fn evaluator_absent_rule_fires_on_stale_sample() {
        let src = Arc::new(InMemoryMetricSource::new());
        let earlier = super::super::source::ago(Duration::from_secs(30));
        src.set_at("heartbeat", 1.0, earlier);
        let rule = Rule::new(
            "heartbeat-stale",
            "",
            Severity::Critical,
            Condition::Absent {
                metric: "heartbeat".into(),
                staleness: Duration::from_secs(5),
            },
        );
        let ev = Evaluator::new(vec![rule], src);
        let out = ev.evaluate().await;
        assert_eq!(out[0].state, AlertState::Firing);
    }

    #[tokio::test]
    async fn evaluator_absent_rule_silent_when_fresh() {
        let src = Arc::new(InMemoryMetricSource::new());
        src.set("heartbeat", 1.0);
        let rule = Rule::new(
            "heartbeat-ok",
            "",
            Severity::Critical,
            Condition::Absent {
                metric: "heartbeat".into(),
                staleness: Duration::from_secs(60),
            },
        );
        let ev = Evaluator::new(vec![rule], src);
        let out = ev.evaluate().await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn evaluator_rate_of_change_positive() {
        let src = Arc::new(InMemoryMetricSource::new());
        let rule = Rule::new(
            "rate-up",
            "",
            Severity::Info,
            Condition::RateOfChange {
                metric: "reqs".into(),
                op: ComparisonOp::Gt,
                threshold: 5.0,
                window: Duration::from_secs(60),
            },
        );
        let ev = Evaluator::new(vec![rule], src.clone());

        let t0 = super::super::source::ago(Duration::from_secs(10));
        src.set_at("reqs", 100.0, t0);
        ev.evaluate().await;

        src.set("reqs", 200.0);
        let out = ev.evaluate().await;
        assert_eq!(out[0].state, AlertState::Firing);
    }

    #[tokio::test]
    async fn evaluator_silent_on_missing_primary_metric() {
        let src = Arc::new(InMemoryMetricSource::new());
        let ev = Evaluator::new(vec![threshold_rule("r", "nope", 0.0)], src);
        let out = ev.evaluate().await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn evaluator_does_not_double_fire() {
        let src = Arc::new(InMemoryMetricSource::new());
        src.set("x", 10.0);
        let ev = Evaluator::new(vec![threshold_rule("r", "x", 5.0)], src);
        let first = ev.evaluate().await;
        let second = ev.evaluate().await;
        assert_eq!(first[0].state, AlertState::Firing);
        assert!(second.is_empty(), "no event expected while state is stable");
    }

    #[tokio::test]
    async fn evaluator_pending_then_cleared_emits_no_event() {
        let src = Arc::new(InMemoryMetricSource::new());
        src.set("x", 10.0);
        let rule = threshold_rule("r", "x", 5.0).with_for(Duration::from_secs(60));
        let ev = Evaluator::new(vec![rule], src.clone());
        let first = ev.evaluate().await;
        assert_eq!(first[0].state, AlertState::Pending);
        src.set("x", 1.0);
        let second = ev.evaluate().await;
        assert!(second.is_empty(), "pending → inactive is silent");
    }
}
