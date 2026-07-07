//! Memory-budgeted admission control for inference requests.
//!
//! Provides [`MemoryScheduler`], which combines threshold-based admission control,
//! a fairness queue for deferred requests, and hysteresis-based stability tracking.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────┐
//! │   Adapter Registry      │  ← static: "which backends can run this model?"
//! └──────────┬──────────────┘
//!            │ candidates
//!            ▼
//! ┌─────────────────────────┐
//! │   Memory Scheduler      │  ← dynamic: "should we admit this request now?"
//! └──────────┬──────────────┘
//!            │ Accept / Defer / Reject
//!            ▼
//! ┌─────────────────────────┐
//! │   Inference Execution   │
//! └─────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use mofa_foundation::scheduler::{MemoryScheduler, MemoryPolicy, MemoryBudget};
//!
//! let policy = MemoryPolicy::default();
//! let budget = MemoryBudget::new(16_384); // 16 GB
//! let mut scheduler = MemoryScheduler::new(policy, budget);
//!
//! let decision = scheduler.evaluate(2048);
//! match decision.outcome {
//!     AdmissionOutcome::Accept => { scheduler.allocate(2048); }
//!     AdmissionOutcome::Defer  => { scheduler.defer("req-1", 2048); }
//!     AdmissionOutcome::Reject => { /* drop request */ }
//! }
//! ```

// The following submodule files live alongside memory.rs in the scheduler/ directory.
// Using #[path] lets them stay in place without requiring a scheduler/memory/ subdirectory.
#[path = "admission.rs"]
mod admission;
#[path = "budget.rs"]
mod budget;
#[path = "clock.rs"]
pub mod clock;
#[path = "deferred.rs"]
mod deferred;
#[path = "stability.rs"]
mod stability;

pub use admission::{AdmissionDecision, AdmissionOutcome};
pub use budget::MemoryBudget;
pub use clock::SystemClock;
pub use deferred::{DeferredQueue, DeferredRequest};
pub use stability::StabilityControl;

use tracing::warn;

// ============================================================================
// Memory Policy
// ============================================================================

/// Threshold-based memory policy for admission control.
///
/// Defines three zones:
/// - **Accept zone**: usage ≤ `defer_threshold` → accept immediately.
/// - **Defer zone**: `defer_threshold` < usage ≤ `reject_threshold` → queue for retry.
/// - **Reject zone**: usage > `reject_threshold` → reject outright.
#[derive(Debug, Clone)]
pub struct MemoryPolicy {
    /// Total memory capacity in MB.
    pub capacity_mb: u64,
    /// Fraction of capacity at which deferral begins (0.0–1.0).
    pub defer_at: f64,
    /// Fraction of capacity at which rejection begins (0.0–1.0).
    pub reject_at: f64,
    /// Maximum number of deferred requests.
    pub max_deferred: usize,
    /// Maximum retry attempts before a deferred request is rejected.
    pub max_retries: u32,
}

impl Default for MemoryPolicy {
    fn default() -> Self {
        Self {
            capacity_mb: 16_384, // 16 GB
            defer_at: 0.75,      // defer above 75%
            reject_at: 0.90,     // reject above 90%
            max_deferred: 100,
            max_retries: 3,
        }
    }
}

impl MemoryPolicy {
    /// Create a policy with explicit capacity and thresholds.
    pub fn new(capacity_mb: u64, defer_at: f64, reject_at: f64) -> Self {
        Self {
            capacity_mb,
            defer_at: defer_at.clamp(0.0, 1.0),
            reject_at: reject_at.clamp(defer_at, 1.0),
            ..Default::default()
        }
    }

    /// Absolute MB threshold for deferral.
    pub fn defer_threshold_mb(&self) -> u64 {
        (self.capacity_mb as f64 * self.defer_at) as u64
    }

    /// Absolute MB threshold for rejection.
    pub fn reject_threshold_mb(&self) -> u64 {
        (self.capacity_mb as f64 * self.reject_at) as u64
    }
}

// ============================================================================
// Memory Scheduler
// ============================================================================

/// Memory-budgeted admission controller for inference requests.
///
/// Combines a [`MemoryPolicy`], [`MemoryBudget`], [`StabilityControl`], and
/// [`DeferredQueue`] to provide admission control.
#[derive(Debug)]
pub struct MemoryScheduler {
    policy: MemoryPolicy,
    budget: MemoryBudget,
    stability: StabilityControl,
    deferred: DeferredQueue,
    active_count: usize,
}

impl MemoryScheduler {
    /// Create a new scheduler with the given policy and budget.
    pub fn new(policy: MemoryPolicy, budget: MemoryBudget) -> Self {
        let max_deferred = policy.max_deferred;
        let max_retries = policy.max_retries;
        Self {
            policy,
            budget,
            stability: StabilityControl::default(),
            deferred: DeferredQueue::new(max_deferred, max_retries),
            active_count: 0,
        }
    }

    /// Create a scheduler with default policy for a given total memory.
    pub fn with_capacity(capacity_mb: u64) -> Self {
        let policy = MemoryPolicy {
            capacity_mb,
            ..Default::default()
        };
        let budget = MemoryBudget::new(capacity_mb);
        Self::new(policy, budget)
    }

    /// Evaluate whether a request requiring `required_mb` should be admitted.
    ///
    /// This is a **read-only** check — it does not allocate memory.
    /// Call [`allocate`](Self::allocate) after an `Accept` decision to reserve memory.
    pub fn evaluate(&self, required_mb: u64) -> AdmissionDecision {
        let current = self.budget.used_mb();
        let projected = current.saturating_add(required_mb);
        let available = self.budget.available_mb();

        if projected > self.policy.reject_threshold_mb() {
            AdmissionDecision {
                outcome: AdmissionOutcome::Reject,
                reason: format!(
                    "Projected usage {}MB exceeds reject threshold {}MB",
                    projected,
                    self.policy.reject_threshold_mb()
                ),
                current_usage_mb: current,
                required_mb,
                available_mb: available,
            }
        } else if projected > self.policy.defer_threshold_mb() {
            AdmissionDecision {
                outcome: AdmissionOutcome::Defer,
                reason: format!(
                    "Projected usage {}MB exceeds defer threshold {}MB",
                    projected,
                    self.policy.defer_threshold_mb()
                ),
                current_usage_mb: current,
                required_mb,
                available_mb: available,
            }
        } else {
            AdmissionDecision {
                outcome: AdmissionOutcome::Accept,
                reason: "Within budget".to_string(),
                current_usage_mb: current,
                required_mb,
                available_mb: available,
            }
        }
    }

    /// Allocate memory for an accepted request.
    ///
    /// Returns `true` if allocation succeeded, `false` if insufficient memory.
    pub fn allocate(&mut self, amount_mb: u64) -> bool {
        if self.budget.allocate(amount_mb) {
            self.active_count += 1;
            true
        } else {
            false
        }
    }

    /// Release memory when a request completes.
    pub fn release(&mut self, amount_mb: u64) {
        self.budget.release(amount_mb);
        self.active_count = self.active_count.saturating_sub(1);
    }

    /// Defer a request (add to the fairness queue).
    ///
    /// Returns `true` if the request was queued, `false` if the queue is full.
    pub fn defer(&mut self, id: impl Into<String>, required_mb: u64) -> bool {
        let request = DeferredRequest::new(id.into(), required_mb);
        let ok = self.deferred.enqueue(request);
        if !ok {
            warn!("Deferred queue is full, cannot defer request");
        }
        ok
    }

    /// Try to dequeue the oldest request that fits in available memory.
    ///
    /// Uses age-aware fairness to prevent starvation of small requests.
    pub fn try_dequeue(&mut self) -> Option<DeferredRequest> {
        let available = self.budget.available_mb();
        self.deferred.dequeue_oldest_fitting(available)
    }

    /// Drain expired requests (exceeded max retries).
    pub fn drain_expired(&mut self) -> Vec<DeferredRequest> {
        self.deferred.drain_expired()
    }

    /// Check if the stability control allows a profile switch.
    pub fn can_switch_profile(&self) -> bool {
        self.stability.can_switch()
    }

    /// Record a profile switch for stability cooldown.
    pub fn record_switch(&mut self) {
        self.stability.record_switch();
    }

    /// Check if a memory change is significant (exceeds hysteresis threshold).
    pub fn is_significant_change(&self, new_usage_mb: u64) -> bool {
        self.stability.is_significant_change(new_usage_mb)
    }

    /// Update the stability control's memory reading.
    pub fn update_memory_reading(&mut self, usage_mb: u64) {
        self.stability.update_reading(usage_mb);
    }

    // -- Accessors --

    /// Current memory usage in MB.
    pub fn used_mb(&self) -> u64 {
        self.budget.used_mb()
    }

    /// Available memory in MB.
    pub fn available_mb(&self) -> u64 {
        self.budget.available_mb()
    }

    /// Usage as a percentage (0.0–100.0).
    pub fn usage_percent(&self) -> f64 {
        self.budget.usage_percent()
    }

    /// Number of currently active requests.
    pub fn active_count(&self) -> usize {
        self.active_count
    }

    /// Number of deferred requests waiting in the queue.
    pub fn deferred_count(&self) -> usize {
        self.deferred.len()
    }

    /// Get the policy reference.
    pub fn policy(&self) -> &MemoryPolicy {
        &self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_scheduler_evaluate_overflow() {
        // Set up a budget of 16GB
        let mut scheduler = MemoryScheduler::with_capacity(16_384);
        
        // Use up 8GB
        assert!(scheduler.allocate(8192));
        assert_eq!(scheduler.used_mb(), 8192);

        // Try to allocate an obscenely large amount that would overflow u64
        // If the code uses `current + required_mb` (unchecked), `8192 + (u64::MAX - 100)`
        // would overflow to 8091. This is less than the defer/reject thresholds, so it would
        // incorrectly return `Accept` or `Defer`.
        let attack_size = u64::MAX - 100;
        
        let decision = scheduler.evaluate(attack_size);
        
        // It should correctly saturate at u64::MAX and reject the massive request
        assert_eq!(decision.outcome, AdmissionOutcome::Reject);
    }
}
