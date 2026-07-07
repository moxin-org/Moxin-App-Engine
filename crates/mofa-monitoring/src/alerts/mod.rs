//! Alert rules engine for `mofa-monitoring`.
//!
//! Declarative SLO-style alerting: define [`Rule`]s over metric names,
//! plug in a [`MetricSource`] (defaulting to the existing
//! `MetricsCollector` or a Prometheus scrape adapter), run the
//! [`Evaluator`] on a tick, and fan the resulting [`AlertEvent`]s through
//! a [`Notifier`] or [`CompositeNotifier`].
//!
//! ```text
//!     ┌──────────────┐    sample()     ┌─────────────┐
//!     │ MetricSource │ ─────────────── │  Evaluator  │
//!     └──────────────┘                 └──────┬──────┘
//!                                             │ AlertEvent
//!                                             ▼
//!                                      ┌──────────────┐
//!                                      │  Notifier    │
//!                                      │  (composite) │
//!                                      └──┬────────┬──┘
//!                                         │        │
//!                                         ▼        ▼
//!                                   LogNotifier CollectingNotifier
//! ```
//!
//! ### State machine (matches Prometheus)
//!
//! ```text
//!                 matched=true,                  matched=true,
//!                 for_duration=0                 for=Δ elapsed
//!    Inactive ──────────────────── Firing ◀────── Pending
//!        │                            │              ▲
//!        │ matched=true, for>0        │ matched=false │
//!        ▼                            ▼              │ matched=true
//!     Pending ───── matched=false ─── Inactive ──────┘
//! ```
//!
//! - `Inactive → Pending`  — silent.
//! - `Pending  → Firing`   — emits `AlertState::Firing`.
//! - `Firing   → Inactive` — emits `AlertState::Resolved`.
//! - `Pending  → Inactive` — silent (condition cleared before soak).
//! - `Inactive → Firing`   (when `for_duration == 0`) — emits
//!   `AlertState::Firing` directly.

pub mod evaluator;
pub mod event;
pub mod notifier;
pub mod rule;
pub mod source;

pub use evaluator::{Evaluator, EvaluatorConfig};
pub use event::{AlertEvent, AlertState};
pub use notifier::{CollectingNotifier, CompositeNotifier, LogNotifier, Notifier};
pub use rule::{ComparisonOp, Condition, Rule, Severity};
pub use source::{InMemoryMetricSource, MetricSample, MetricSource};
