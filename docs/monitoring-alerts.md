# Monitoring Alert Rules Engine

Declarative SLO-style alerting for `mofa-monitoring`. Operates alongside
the existing dashboard, Prometheus exporter, and OpenTelemetry tracing
layers — this module adds the evaluation + notification loop that
consumes the metrics those layers publish.

---

## Architecture

```mermaid
flowchart LR
    subgraph Sources [Metric sources]
        MC[MetricsCollector]
        PR[Prometheus scrape]
        IM[InMemoryMetricSource<br/>test fixture]
    end

    subgraph Engine [Alerts engine]
        RS[Rule set]
        EV[Evaluator]
    end

    subgraph Sinks [Notifiers]
        LN[LogNotifier]
        CN[CollectingNotifier]
        CO[CompositeNotifier]
    end

    MC -->|MetricSource| EV
    PR -->|MetricSource| EV
    IM -->|MetricSource| EV
    RS --> EV
    EV -->|AlertEvent| CO
    CO --> LN
    CO --> CN
```

The engine is backend-agnostic in both directions:
`MetricSource` lets the evaluator consume from any backend that can
return the current value of a named metric, and `Notifier` lets the
operator wire events to log, webhook, chat, pager, or any composition.

---

## State machine

Matches Prometheus semantics: a rule soaks in `Pending` for the
configured `for_duration` before it is allowed to fire, guarding against
flapping.

```mermaid
stateDiagram-v2
    [*] --> Inactive
    Inactive --> Firing: match && for_duration == 0
    Inactive --> Pending: match && for_duration > 0
    Pending --> Firing: match && elapsed >= for_duration
    Pending --> Inactive: no match (silent)
    Firing --> Firing: match
    Firing --> Inactive: no match (emits Resolved)
```

Only the following transitions emit an `AlertEvent`:

| Transition | Emitted state |
|------------|---------------|
| `Inactive → Firing` | `Firing` |
| `Inactive → Pending` | `Pending` |
| `Pending → Firing` | `Firing` |
| `Firing → Inactive` | `Resolved` |
| `Pending → Inactive` | *silent* |

---

## Rule model

```mermaid
classDiagram
    class Rule {
        +name: String
        +description: String
        +severity: Severity
        +condition: Condition
        +for_duration: Duration
        +labels: HashMap~String,String~
        +annotations: HashMap~String,String~
        +primary_metric() Option~String~
    }
    class Severity {
        <<enumeration>>
        Info
        Warning
        Critical
    }
    class Condition {
        <<enumeration>>
        Threshold
        RateOfChange
        Absent
    }
    class ComparisonOp {
        <<enumeration>>
        Gt
        Gte
        Lt
        Lte
        Eq
        Neq
    }
    Rule --> Severity
    Rule --> Condition
    Condition --> ComparisonOp
```

---

## Condition families

### `Threshold`

Fire when a scalar metric satisfies `value OP threshold`.

```rust
use std::time::Duration;
use mofa_monitoring::alerts::{ComparisonOp, Condition, Rule, Severity};

let rule = Rule::new(
    "high-error-rate",
    "LLM error rate above 5%",
    Severity::Warning,
    Condition::Threshold {
        metric: "llm_error_rate".into(),
        op: ComparisonOp::Gt,
        threshold: 0.05,
    },
)
.with_for(Duration::from_secs(120))
.with_label("team", "platform")
.with_annotation("runbook", "https://runbooks.internal/llm-errors");
```

### `RateOfChange`

Fire when the per-second derivative of a monotonic counter over a sliding
window satisfies `OP threshold`. The evaluator maintains the window
samples internally; configure a sane `max_window_samples` in
`EvaluatorConfig` to cap memory.

### `Absent`

Fire when a metric has not been observed within `staleness` — heartbeat
and liveness checks.

---

## Evaluation flow

```mermaid
sequenceDiagram
    participant Tick as Tick loop
    participant EV as Evaluator
    participant MS as MetricSource
    participant N as Notifier

    Tick->>EV: evaluate()
    loop per rule
        EV->>MS: sample(metric)
        MS-->>EV: Some(MetricSample) | None
        EV->>EV: apply condition
        EV->>EV: update state machine
        alt state transition
            EV-->>Tick: AlertEvent
        end
    end
    Tick->>N: notify(event) per event
```

The evaluator is re-entrant-safe: its state is held under a `Mutex`
keyed by rule name. Production deployments typically call `evaluate()`
from a single tick loop; when scaling out, shard rules across evaluator
instances rather than locking a single evaluator.

---

## Notifiers

| Notifier | Purpose |
|----------|---------|
| `LogNotifier` | Emit through `tracing`. Warning/Critical go to `warn!`; Info to `info!`. Good default alongside richer delivery. |
| `CollectingNotifier` | Bounded in-memory buffer. Powers the dashboard "recent alerts" panel and tests. |
| `CompositeNotifier` | Fan out to multiple notifiers best-effort. |

Future integrations (webhook, Slack, PagerDuty) plug in as additional
`Notifier` implementors without changing the evaluator contract.

---

## Wiring checklist

- [ ] Construct an `Arc<dyn MetricSource>` — either the in-memory fixture,
      a `MetricsCollector` adapter, or a Prometheus scrape client.
- [ ] Build the rule set: `Vec<Rule>`.
- [ ] Instantiate an `Evaluator` (use `with_config` to override
      `max_window_samples` if you run rate-of-change rules over long
      windows).
- [ ] Wire one or more `Notifier`s (typically `CompositeNotifier` over
      `LogNotifier` + a delivery notifier).
- [ ] Tick `evaluator.evaluate()` on a cadence (typically every 15–60s)
      and fan each event to the notifier.

---

## Status

- Rule model, condition families, evaluator, notifier abstraction —
  delivered
- Metric source adapter against the existing `MetricsCollector` —
  follow-up
- Prometheus scrape adapter — follow-up
- Webhook / Slack notifier — follow-up
- YAML rule-file loader — follow-up
