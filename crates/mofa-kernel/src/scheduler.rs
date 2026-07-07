//! Scheduler kernel contract — traits, types, and errors for periodic agent execution.
//!
//! # Architecture
//!
//! This module defines the complete kernel-level contract for scheduled agent execution.
//! Per MoFA's microkernel rules:
//!
//! - **Trait definitions** live here in `mofa-kernel`.
//! - **Concrete implementations** (`CronScheduler`) live in `mofa-foundation`.
//! - The kernel must never depend on foundation.
//!
//! Everything in this file can be compiled and unit-tested without a running tokio
//! runtime except for the single `#[tokio::test]` that exercises `ScheduleHandle`.

use crate::agent::types::AgentInput;

// ---------------------------------------------------------------------------
// Clock abstraction (injectable for testing)
// ---------------------------------------------------------------------------

/// Provides the current wall-clock time as Unix-epoch milliseconds.
///
/// Injecting this through [`CronScheduler`](mofa_foundation) rather than calling
/// `SystemTime::now()` directly makes timing-sensitive code deterministic in tests.
/// See INSTRUCTIONS.md §IV.3 — "Timestamp generation logic MUST be abstracted".
pub trait Clock: Send + Sync {
    /// Returns the current time as milliseconds since the Unix epoch.
    fn now_millis(&self) -> u64;
}

// ---------------------------------------------------------------------------
// ScheduleDefinition
// ---------------------------------------------------------------------------

/// Describes one periodic execution slot registered with an [`AgentScheduler`].
///
/// Exactly one of `cron_expression` or `interval_ms` must be set.
/// Both set, or neither set, is a construction error caught at call time via
/// [`ScheduleDefinition::new_interval`] or [`ScheduleDefinition::new_cron`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct ScheduleDefinition {
    /// Globally unique identifier for this schedule. Used in all management calls.
    pub schedule_id: String,
    /// The agent that will be invoked on each tick.
    pub agent_id: String,
    /// A standard cron expression (`"0 */5 * * * *"` = every 5 minutes).
    /// Mutually exclusive with `interval_ms`.
    pub cron_expression: Option<String>,
    /// Fixed interval in milliseconds between ticks.
    /// Mutually exclusive with `cron_expression`.
    pub interval_ms: Option<u64>,
    /// Maximum number of concurrent runs of this schedule at any instant.
    /// Must be ≥ 1. Defaults to 1 (serialised runs).
    pub max_concurrent: usize,
    /// The [`AgentInput`] template cloned and sent to the agent on every tick.
    pub input_template: AgentInput,
    /// What to do when a tick fires while the previous run still holds a concurrency slot.
    pub missed_tick_policy: MissedTickPolicy,
}

impl ScheduleDefinition {
    /// Construct an interval-based schedule.
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::InvalidInterval`] if `interval_ms` is 0.
    /// Returns [`SchedulerError::InvalidConcurrency`] if `max_concurrent` is 0.
    pub fn new_interval(
        schedule_id: impl Into<String>,
        agent_id: impl Into<String>,
        interval_ms: u64,
        max_concurrent: usize,
        input_template: AgentInput,
        policy: MissedTickPolicy,
    ) -> Result<Self, SchedulerError> {
        if interval_ms == 0 {
            return Err(SchedulerError::InvalidInterval(
                "interval_ms must be > 0".into(),
            ));
        }
        if max_concurrent == 0 {
            return Err(SchedulerError::InvalidConcurrency);
        }
        Ok(Self {
            schedule_id: schedule_id.into(),
            agent_id: agent_id.into(),
            cron_expression: None,
            interval_ms: Some(interval_ms),
            max_concurrent,
            input_template,
            missed_tick_policy: policy,
        })
    }

    /// Construct a cron-expression-based schedule.
    ///
    /// This constructor validates that the expression string is non-empty. Full
    /// syntactic validation is performed by the `cron` crate inside `CronScheduler`
    /// at registration time — kernel deliberately has no dependency on that crate.
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::InvalidCron`] if `cron_expression` is empty.
    /// Returns [`SchedulerError::InvalidConcurrency`] if `max_concurrent` is 0.
    pub fn new_cron(
        schedule_id: impl Into<String>,
        agent_id: impl Into<String>,
        cron_expression: impl Into<String>,
        max_concurrent: usize,
        input_template: AgentInput,
        policy: MissedTickPolicy,
    ) -> Result<Self, SchedulerError> {
        let expr = cron_expression.into();
        if expr.is_empty() {
            return Err(SchedulerError::InvalidCron(
                expr,
                "expression must not be empty".into(),
            ));
        }
        if max_concurrent == 0 {
            return Err(SchedulerError::InvalidConcurrency);
        }
        Ok(Self {
            schedule_id: schedule_id.into(),
            agent_id: agent_id.into(),
            cron_expression: Some(expr),
            interval_ms: None,
            max_concurrent,
            input_template,
            missed_tick_policy: policy,
        })
    }
}

// ---------------------------------------------------------------------------
// MissedTickPolicy
// ---------------------------------------------------------------------------

/// Controls what happens when a new tick fires while the previous execution is
/// still occupying a concurrency slot.
///
/// Analogous to [`tokio::time::MissedTickBehavior`] but expressed as a first-class
/// framework type so it is serialisable and can appear in monitoring dashboards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum MissedTickPolicy {
    /// Silently discard the missed tick. The schedule resumes at the next
    /// normally-scheduled instant.
    ///
    /// Use this for idempotent workloads where running more frequently than expected
    /// is worse than running less frequently (e.g. rate-limited external API calls).
    Skip,
    /// Queue every missed tick and fire them all immediately once a slot opens.
    ///
    /// Use this for audit/notification agents where no event must be lost,
    /// accepting that a burst of executions may occur on recovery.
    Burst,
    /// Fire exactly one catch-up tick immediately after the previous run finishes,
    /// then resume the normal schedule.
    ///
    /// A middle ground between [`MissedTickPolicy::Skip`] and [`MissedTickPolicy::Burst`].
    DelaySingle,
}

// ---------------------------------------------------------------------------
// ScheduleHandle
// ---------------------------------------------------------------------------

/// Returned by [`AgentScheduler::register`]. The background task for this schedule
/// runs until the handle is dropped or [`.cancel()`](ScheduleHandle::cancel) is called.
///
/// The handle is intentionally not `Clone` — ownership models the exclusive right
/// to cancel a schedule.
pub struct ScheduleHandle {
    /// The schedule this handle controls. Exposed for logging/display purposes.
    pub schedule_id: String,
    pub(crate) cancel_tx: tokio::sync::oneshot::Sender<()>,
}

impl ScheduleHandle {
    /// Create a new handle (used by `CronScheduler` in foundation).
    pub fn new(
        schedule_id: impl Into<String>,
        cancel_tx: tokio::sync::oneshot::Sender<()>,
    ) -> Self {
        Self {
            schedule_id: schedule_id.into(),
            cancel_tx,
        }
    }

    /// Cancel the schedule immediately.
    ///
    /// Returns `true` if the cancellation signal was delivered (the background task
    /// was still running), `false` if it had already stopped.
    pub fn cancel(self) -> bool {
        self.cancel_tx.send(()).is_ok()
    }
}

impl std::fmt::Debug for ScheduleHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScheduleHandle")
            .field("schedule_id", &self.schedule_id)
            .field("cancel_tx", &"<channel>")
            .finish()
    }
}

// ---------------------------------------------------------------------------
// ScheduleInfo
// ---------------------------------------------------------------------------

/// A point-in-time snapshot of a schedule's runtime state.
///
/// Returned by [`AgentScheduler::list`] and used by monitoring dashboards and the
/// `mofa agent schedule list` CLI subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ScheduleInfo {
    /// Unique identifier of the schedule.
    pub schedule_id: String,
    /// The agent invoked on each tick.
    pub agent_id: String,
    /// Unix-epoch milliseconds of the next predicted tick, if the schedule is active.
    pub next_run_ms: Option<u64>,
    /// Unix-epoch milliseconds when the most recent tick fired.
    pub last_run_ms: Option<u64>,
    /// Number of consecutive failed executions without an intervening success.
    pub consecutive_failures: u32,
    /// Whether the schedule is currently paused via [`AgentScheduler::pause`].
    pub is_paused: bool,
}

impl ScheduleInfo {
    /// Create a new ScheduleInfo for monitoring purposes.
    pub fn new(
        schedule_id: impl Into<String>,
        agent_id: impl Into<String>,
        next_run_ms: Option<u64>,
        last_run_ms: Option<u64>,
        consecutive_failures: u32,
        is_paused: bool,
    ) -> Self {
        Self {
            schedule_id: schedule_id.into(),
            agent_id: agent_id.into(),
            next_run_ms,
            last_run_ms,
            consecutive_failures,
            is_paused,
        }
    }
}

// ---------------------------------------------------------------------------
// AgentScheduler trait
// ---------------------------------------------------------------------------

/// A service that manages multiple periodic agent executions.
///
/// # Architecture note
///
/// This trait is defined in `mofa-kernel`; the concrete implementation
/// (`CronScheduler`) lives in `mofa-foundation`. This separation ensures the kernel
/// has no dependency on `tokio::time`, the `cron` crate, or `AgentRegistry`.
///
/// Callers that need to be generic over the scheduler backend (e.g. tests using a
/// `MockScheduler`) depend only on this trait.
#[allow(async_fn_in_trait)]
pub trait AgentScheduler: Send + Sync {
    /// Register a new schedule. Returns a [`ScheduleHandle`] that, when dropped or
    /// cancelled, stops the background task.
    ///
    /// # Errors
    ///
    /// - [`SchedulerError::AlreadyExists`] — `def.schedule_id` is already registered.
    /// - [`SchedulerError::AgentNotFound`] — the agent is not in the registry.
    /// - [`SchedulerError::InvalidCron`] — the cron expression is syntactically invalid.
    async fn register(&self, def: ScheduleDefinition) -> Result<ScheduleHandle, SchedulerError>;

    /// Stop and remove a schedule.
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::NotFound`] if the schedule ID is not known.
    async fn unregister(&self, schedule_id: &str) -> Result<(), SchedulerError>;

    /// Snapshot the state of all currently registered schedules.
    async fn list(&self) -> Vec<ScheduleInfo>;

    /// Suppress tick execution for a schedule without removing it.
    ///
    /// Missed ticks during a pause are discarded regardless of [`MissedTickPolicy`].
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::NotFound`] if the schedule ID is not known.
    async fn pause(&self, schedule_id: &str) -> Result<(), SchedulerError>;

    /// Re-enable a previously paused schedule.
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::NotFound`] if the schedule ID is not known.
    async fn resume(&self, schedule_id: &str) -> Result<(), SchedulerError>;
}

// ---------------------------------------------------------------------------
// SchedulerError
// ---------------------------------------------------------------------------

/// All errors that can be returned by [`AgentScheduler`] operations.
///
/// Marked `#[non_exhaustive]` so that new variants can be added in future minor
/// releases without breaking callers that match exhaustively.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SchedulerError {
    /// The provided cron expression cannot be parsed.
    #[error("Invalid cron expression '{0}': {1}")]
    InvalidCron(String, String),

    /// `interval_ms` was set to 0, which would create a busy-spin loop.
    #[error("Invalid interval: {0}")]
    InvalidInterval(String),

    /// `max_concurrent` was set to 0, which makes the schedule permanently blocked.
    #[error("max_concurrent must be >= 1")]
    InvalidConcurrency,

    /// A management call referenced a schedule ID that is not registered.
    #[error("Schedule '{0}' not found")]
    NotFound(String),

    /// The `agent_id` in a [`ScheduleDefinition`] is not present in the `AgentRegistry`.
    #[error("Agent '{0}' is not registered in the agent registry")]
    AgentNotFound(String),

    /// All concurrency slots for a schedule are occupied; the tick was dropped.
    #[error("Concurrency limit reached for schedule '{0}'")]
    ConcurrencyLimit(String),

    /// A `register` call was made with a `schedule_id` that is already active.
    #[error("Schedule '{0}' is already registered")]
    AlreadyExists(String),

    /// Persistence operation failed (save/load from disk).
    #[error("Persistence error: {0}")]
    PersistenceError(String),
}

// ---------------------------------------------------------------------------
// ScheduledAgentRunner
// ---------------------------------------------------------------------------

/// Minimal execution interface required by [`CronScheduler`] to fire an agent
/// by ID.
///
/// Lives in `mofa-kernel` so that `mofa-foundation`'s `CronScheduler` can hold
/// an `Arc<dyn ScheduledAgentRunner>` without importing the concrete
/// `ExecutionEngine` from `mofa-runtime`, which would create a cyclic crate
/// dependency.  `mofa-runtime`'s `ExecutionEngine` implements this trait.
#[async_trait::async_trait]
pub trait ScheduledAgentRunner: Send + Sync {
    /// Run the agent identified by `agent_id` with the given `input`.
    ///
    /// Errors are boxed so that callers in `mofa-foundation` do not need to
    /// know the concrete error type defined in `mofa-runtime`.
    async fn run_scheduled(
        &self,
        agent_id: &str,
        input: AgentInput,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // 1. Error display messages are human-readable
    // ------------------------------------------------------------------

    #[test]
    fn scheduler_error_display_invalid_cron() {
        let e = SchedulerError::InvalidCron("bad expr".into(), "unexpected token".into());
        assert_eq!(
            e.to_string(),
            "Invalid cron expression 'bad expr': unexpected token"
        );
    }

    #[test]
    fn scheduler_error_display_not_found() {
        let e = SchedulerError::NotFound("daily-report".into());
        assert!(e.to_string().contains("daily-report"));
    }

    #[test]
    fn scheduler_error_display_agent_not_found() {
        let e = SchedulerError::AgentNotFound("summariser".into());
        assert!(e.to_string().contains("summariser"));
    }

    #[test]
    fn scheduler_error_display_already_exists() {
        let e = SchedulerError::AlreadyExists("hourly-run".into());
        assert!(e.to_string().contains("hourly-run"));
    }

    #[test]
    fn scheduler_error_display_concurrency_limit() {
        let e = SchedulerError::ConcurrencyLimit("high-freq".into());
        assert!(e.to_string().contains("high-freq"));
    }

    // ------------------------------------------------------------------
    // 2. MissedTickPolicy round-trips through JSON
    // ------------------------------------------------------------------

    #[test]
    fn missed_tick_policy_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        for policy in [
            MissedTickPolicy::Skip,
            MissedTickPolicy::Burst,
            MissedTickPolicy::DelaySingle,
        ] {
            let json = serde_json::to_string(&policy)?;
            let back: MissedTickPolicy = serde_json::from_str(&json)?;
            assert_eq!(policy, back, "round-trip failed for {:?}", policy);
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // 3. ScheduleDefinition::new_interval — validation
    // ------------------------------------------------------------------

    #[test]
    fn schedule_definition_rejects_zero_interval() {
        let result = ScheduleDefinition::new_interval(
            "s1",
            "agent-1",
            0,
            1,
            AgentInput::text("ping"),
            MissedTickPolicy::Skip,
        );
        assert!(matches!(result, Err(SchedulerError::InvalidInterval(_))));
    }

    #[test]
    fn schedule_definition_rejects_zero_concurrency_interval() {
        let result = ScheduleDefinition::new_interval(
            "s1",
            "agent-1",
            1000,
            0,
            AgentInput::text("ping"),
            MissedTickPolicy::Skip,
        );
        assert!(matches!(result, Err(SchedulerError::InvalidConcurrency)));
    }

    #[test]
    fn schedule_definition_new_interval_ok() {
        let def = ScheduleDefinition::new_interval(
            "every-second",
            "my-agent",
            1_000,
            2,
            AgentInput::text("tick"),
            MissedTickPolicy::Burst,
        )
        .unwrap();
        assert_eq!(def.schedule_id, "every-second");
        assert_eq!(def.interval_ms, Some(1_000));
        assert!(def.cron_expression.is_none());
        assert_eq!(def.max_concurrent, 2);
    }

    // ------------------------------------------------------------------
    // 4. ScheduleDefinition::new_cron — validation
    // ------------------------------------------------------------------

    #[test]
    fn schedule_definition_rejects_empty_cron() {
        let result = ScheduleDefinition::new_cron(
            "s1",
            "agent-1",
            "",
            1,
            AgentInput::text("ping"),
            MissedTickPolicy::Skip,
        );
        assert!(matches!(result, Err(SchedulerError::InvalidCron(_, _))));
    }

    #[test]
    fn schedule_definition_rejects_zero_concurrency_cron() {
        let result = ScheduleDefinition::new_cron(
            "s1",
            "agent-1",
            "0 * * * * *",
            0,
            AgentInput::text("ping"),
            MissedTickPolicy::DelaySingle,
        );
        assert!(matches!(result, Err(SchedulerError::InvalidConcurrency)));
    }

    #[test]
    fn schedule_definition_new_cron_ok() {
        let def = ScheduleDefinition::new_cron(
            "five-min",
            "my-agent",
            "0 */5 * * * *",
            1,
            AgentInput::text("tick"),
            MissedTickPolicy::Skip,
        )
        .unwrap();
        assert_eq!(def.schedule_id, "five-min");
        assert_eq!(def.cron_expression.as_deref(), Some("0 */5 * * * *"));
        assert!(def.interval_ms.is_none());
    }

    // ------------------------------------------------------------------
    // 5. ScheduleDefinition round-trips through JSON
    // ------------------------------------------------------------------

    #[test]
    fn schedule_definition_json_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let def = ScheduleDefinition::new_interval(
            "rt-test",
            "agent-rt",
            500,
            3,
            AgentInput::text("data"),
            MissedTickPolicy::DelaySingle,
        )?;
        let json = serde_json::to_string(&def)?;
        let back: ScheduleDefinition = serde_json::from_str(&json)?;
        assert_eq!(back.schedule_id, "rt-test");
        assert_eq!(back.interval_ms, Some(500));
        assert_eq!(back.max_concurrent, 3);
        assert_eq!(back.missed_tick_policy, MissedTickPolicy::DelaySingle);
        Ok(())
    }

    // ------------------------------------------------------------------
    // 6. ScheduleHandle::cancel returns false after receiver dropped
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn schedule_handle_cancel_returns_false_when_receiver_dropped() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        drop(rx); // simulate background task already finished
        let handle = ScheduleHandle::new("test-handle", tx);
        assert!(!handle.cancel());
    }

    #[tokio::test]
    async fn schedule_handle_cancel_returns_true_when_receiver_alive() {
        let (tx, _rx) = tokio::sync::oneshot::channel::<()>();
        let handle = ScheduleHandle::new("test-handle", tx);
        // _rx is still in scope, so the send should succeed
        assert!(handle.cancel());
    }
}
