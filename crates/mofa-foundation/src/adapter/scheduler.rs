//! Memory-budgeted scheduler for inference orchestration
//!
//! This module provides the scheduler policy for admission control under memory constraints.
//! It implements a two-phase approach:
//!
//! Phase 1: Rule-based baseline
//! - Admission decisions: Accept, Defer, Reject
//! - Deterministic memory thresholds
//! - Stability controls (cooldown/hysteresis)
//! - Deferred queue fairness
//!
//! Phase 2: Quant-driven optimization (future)
//! - Utility-based profile selection
//! - Weighted utility objective
//! - EWMA-based memory headroom estimate

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::config::HardwareProfile;
use super::descriptor::AdapterDescriptor;
use super::error::AdapterError;

/// Admission decision result
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdmissionDecision {
    /// Request is accepted for processing
    Accept,
    /// Request should be retried later
    Defer,
    /// Request is rejected
    Reject,
}

impl AdmissionDecision {
    /// Check if the decision is final (no retry possible)
    pub fn is_final(&self) -> bool {
        matches!(self, AdmissionDecision::Reject)
    }

    /// Check if the request can be retried
    pub fn is_retryable(&self) -> bool {
        matches!(self, AdmissionDecision::Defer)
    }
}

impl std::fmt::Display for AdmissionDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdmissionDecision::Accept => write!(f, "Accept"),
            AdmissionDecision::Defer => write!(f, "Defer"),
            AdmissionDecision::Reject => write!(f, "Reject"),
        }
    }
}

/// Reason for admission decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdmissionReason {
    /// The decision made
    pub decision: AdmissionDecision,
    /// Human-readable reason
    pub message: String,
    /// Current memory usage in MB
    pub current_memory_mb: u64,
    /// Required memory in MB
    pub required_memory_mb: u64,
    /// Available memory headroom in MB
    pub available_memory_mb: u64,
}

impl AdmissionReason {
    /// Create an accept reason
    pub fn accept(current: u64, required: u64, available: u64) -> Self {
        Self {
            decision: AdmissionDecision::Accept,
            message: "Memory available for request".to_string(),
            current_memory_mb: current,
            required_memory_mb: required,
            available_memory_mb: available,
        }
    }

    /// Create a defer reason
    pub fn defer(current: u64, required: u64, available: u64) -> Self {
        Self {
            decision: AdmissionDecision::Defer,
            message: "Insufficient memory, request queued for retry".to_string(),
            current_memory_mb: current,
            required_memory_mb: required,
            available_memory_mb: available,
        }
    }

    /// Create a reject reason
    pub fn reject(current: u64, required: u64, available: u64) -> Self {
        Self {
            decision: AdmissionDecision::Reject,
            message: "Request exceeds memory limits".to_string(),
            current_memory_mb: current,
            required_memory_mb: required,
            available_memory_mb: available,
        }
    }
}

/// Memory thresholds for admission control
#[derive(Debug, Clone)]
pub struct MemoryThresholds {
    /// Maximum memory usage before rejecting new requests (MB)
    pub max_memory_mb: u64,
    /// Memory level to start deferring requests (MB)
    pub defer_threshold_mb: u64,
    /// Memory level to accept requests without hesitation (MB)
    pub accept_threshold_mb: u64,
}

impl Default for MemoryThresholds {
    fn default() -> Self {
        Self {
            max_memory_mb: 16 * 1024,       // 16 GB
            defer_threshold_mb: 14 * 1024,  // 14 GB
            accept_threshold_mb: 12 * 1024, // 12 GB
        }
    }
}

impl MemoryThresholds {
    /// Create custom thresholds
    pub fn new(max: u64, defer: u64, accept: u64) -> Self {
        Self {
            max_memory_mb: max,
            defer_threshold_mb: defer,
            accept_threshold_mb: accept,
        }
    }

    /// Check if memory is within acceptable range
    pub fn check_memory(&self, current_usage: u64, required: u64) -> AdmissionDecision {
        let projected = current_usage + required;

        // Reject if projected exceeds max
        if projected > self.max_memory_mb {
            return AdmissionDecision::Reject;
        }

        // Defer if projected exceeds defer threshold
        if projected > self.defer_threshold_mb {
            return AdmissionDecision::Defer;
        }

        // Accept if within acceptable range
        AdmissionDecision::Accept
    }
}

/// Stability controls to prevent rapid profile switching
#[derive(Debug, Clone)]
pub struct StabilityControl {
    /// Cooldown period after a profile switch (ms)
    pub cooldown_ms: u64,
    /// Hysteresis threshold for memory headroom (MB)
    pub hysteresis_mb: u64,
    /// Last profile switch timestamp
    last_switch: Option<Instant>,
    /// Last memory reading for hysteresis
    last_memory: Option<u64>,
}

impl Default for StabilityControl {
    fn default() -> Self {
        Self {
            cooldown_ms: 5000,  // 5 seconds
            hysteresis_mb: 512, // 512 MB
            last_switch: None,
            last_memory: None,
        }
    }
}

impl StabilityControl {
    /// Check if a profile switch is allowed
    pub fn can_switch(&self) -> bool {
        match self.last_switch {
            Some(last) => last.elapsed() > Duration::from_millis(self.cooldown_ms),
            None => true,
        }
    }

    /// Record a profile switch
    pub fn record_switch(&mut self) {
        self.last_switch = Some(Instant::now());
    }

    /// Check if memory change is significant (beyond hysteresis)
    pub fn is_significant_change(&self, current_memory: u64) -> bool {
        match self.last_memory {
            Some(last) => {
                let diff = current_memory.abs_diff(last);
                diff > self.hysteresis_mb
            }
            None => true,
        }
    }

    /// Update last memory reading
    pub fn update_memory(&mut self, memory: u64) {
        self.last_memory = Some(memory);
    }
}

/// A deferred request waiting in the queue
#[derive(Debug, Clone)]
pub struct DeferredRequest {
    /// Unique request identifier
    pub id: String,
    /// Required memory in MB
    pub required_memory_mb: u64,
    /// When the request was first deferred
    pub enqueued_at: Instant,
    /// Number of retry attempts
    pub retry_count: u32,
    /// Associated adapter descriptor
    pub adapter: AdapterDescriptor,
}

impl DeferredRequest {
    /// Create a new deferred request
    pub fn new(id: String, required: u64, adapter: AdapterDescriptor) -> Self {
        Self {
            id,
            required_memory_mb: required,
            enqueued_at: Instant::now(),
            retry_count: 0,
            adapter,
        }
    }

    /// Get wait time in milliseconds
    pub fn wait_time_ms(&self) -> u64 {
        self.enqueued_at.elapsed().as_millis() as u64
    }

    /// Increment retry count
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }
}

/// Fairness-aware deferred queue
#[derive(Debug, Clone, Default)]
pub struct DeferredQueue {
    /// Queue of deferred requests
    requests: VecDeque<DeferredRequest>,
    /// Maximum size of the queue
    max_size: usize,
    /// Maximum retry attempts before rejecting
    max_retries: u32,
}

impl DeferredQueue {
    /// Create a new deferred queue
    pub fn new(max_size: usize, max_retries: u32) -> Self {
        Self {
            requests: VecDeque::new(),
            max_size,
            max_retries,
        }
    }

    /// Add a request to the queue
    pub fn enqueue(&mut self, request: DeferredRequest) -> bool {
        if self.requests.len() < self.max_size {
            self.requests.push_back(request);
            true
        } else {
            false
        }
    }

    /// Get the next request that can be processed (oldest first, respects max retries)
    pub fn dequeue(&mut self, available_memory: u64) -> Option<DeferredRequest> {
        // Find the oldest request that fits in available memory
        let mut best_index: Option<usize> = None;
        let mut oldest_time: Option<Instant> = None;

        for (i, req) in self.requests.iter().enumerate() {
            if req.required_memory_mb <= available_memory && req.retry_count < self.max_retries {
                match oldest_time {
                    Some(time) if req.enqueued_at < time => {
                        oldest_time = Some(req.enqueued_at);
                        best_index = Some(i);
                    }
                    None => {
                        oldest_time = Some(req.enqueued_at);
                        best_index = Some(i);
                    }
                    _ => {}
                }
            }
        }

        // Remove and return the best request
        if let Some(index) = best_index {
            self.requests.remove(index)
        } else {
            None
        }
    }

    /// Get all requests that have exceeded max retries
    pub fn get_expired(&mut self) -> Vec<DeferredRequest> {
        let expired: Vec<_> = self
            .requests
            .iter()
            .filter(|r| r.retry_count >= self.max_retries)
            .cloned()
            .collect();

        // Remove expired requests from queue
        self.requests.retain(|r| r.retry_count < self.max_retries);

        expired
    }

    /// Get current queue size
    pub fn size(&self) -> usize {
        self.requests.len()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    /// Get all pending requests (for debugging/inspection)
    pub fn pending_requests(&self) -> Vec<&DeferredRequest> {
        self.requests.iter().collect()
    }
}

/// Memory budget for inference requests
#[derive(Debug, Clone)]
pub struct MemoryBudget {
    /// Total available memory in MB
    pub total_memory_mb: u64,
    /// Current usage in MB
    pub current_usage_mb: u64,
    /// Reserved memory in MB
    pub reserved_mb: u64,
}

impl MemoryBudget {
    /// Create a new memory budget
    pub fn new(total: u64) -> Self {
        Self {
            total_memory_mb: total,
            current_usage_mb: 0,
            reserved_mb: 0,
        }
    }

    /// Create from hardware profile
    pub fn from_hardware(hardware: &HardwareProfile) -> Self {
        let total = hardware.available_ram_mb;
        Self::new(total)
    }

    /// Get available memory
    pub fn available(&self) -> u64 {
        self.total_memory_mb
            .saturating_sub(self.current_usage_mb)
            .saturating_sub(self.reserved_mb)
    }

    /// Allocate memory for a request
    pub fn allocate(&mut self, amount: u64) -> bool {
        if self.available() >= amount {
            self.current_usage_mb += amount;
            true
        } else {
            false
        }
    }

    /// Release memory after request completes
    pub fn release(&mut self, amount: u64) {
        self.current_usage_mb = self.current_usage_mb.saturating_sub(amount);
    }

    /// Reserve memory (for system usage)
    pub fn reserve(&mut self, amount: u64) {
        self.reserved_mb = self.reserved_mb.saturating_add(amount);
    }

    /// Get usage percentage
    pub fn usage_percent(&self) -> f64 {
        let total = self.total_memory_mb as f64;
        if total > 0.0 {
            (self.current_usage_mb as f64 / total) * 100.0
        } else {
            0.0
        }
    }
}

impl Default for MemoryBudget {
    fn default() -> Self {
        Self::new(8192) // Default 8GB
    }
}

/// Scheduler policy configuration
#[derive(Debug, Clone)]
pub struct SchedulerPolicy {
    /// Memory thresholds
    pub thresholds: MemoryThresholds,
    /// Stability controls
    pub stability: StabilityControl,
    /// Maximum deferred queue size
    pub max_deferred: usize,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Enable fairness (age-aware dequeue)
    pub enable_fairness: bool,
}

impl Default for SchedulerPolicy {
    fn default() -> Self {
        Self {
            thresholds: MemoryThresholds::default(),
            stability: StabilityControl::default(),
            max_deferred: 100,
            max_retries: 3,
            enable_fairness: true,
        }
    }
}

impl SchedulerPolicy {
    /// Create a new scheduler policy with custom thresholds
    pub fn with_thresholds(thresholds: MemoryThresholds) -> Self {
        Self {
            thresholds,
            ..Default::default()
        }
    }

    /// Create a strict policy (lower thresholds)
    pub fn strict() -> Self {
        Self {
            thresholds: MemoryThresholds::new(8 * 1024, 7 * 1024, 6 * 1024),
            stability: StabilityControl {
                cooldown_ms: 10000,
                hysteresis_mb: 1024,
                ..Default::default()
            },
            max_deferred: 50,
            max_retries: 2,
            enable_fairness: true,
        }
    }

    /// Create a lenient policy (higher thresholds)
    pub fn lenient() -> Self {
        Self {
            thresholds: MemoryThresholds::new(32 * 1024, 28 * 1024, 24 * 1024),
            stability: StabilityControl {
                cooldown_ms: 2000,
                hysteresis_mb: 256,
                ..Default::default()
            },
            max_deferred: 200,
            max_retries: 5,
            enable_fairness: true,
        }
    }
}

/// The scheduler for memory-budgeted inference
#[derive(Debug)]
pub struct Scheduler {
    /// Scheduler policy
    policy: SchedulerPolicy,
    /// Memory budget
    memory: MemoryBudget,
    /// Deferred queue
    deferred: DeferredQueue,
    /// Current active requests count
    active_requests: usize,
}

impl Scheduler {
    /// Create a new scheduler
    pub fn new(policy: SchedulerPolicy, memory: MemoryBudget) -> Self {
        let max_deferred = policy.max_deferred;
        let max_retries = policy.max_retries;
        Self {
            policy,
            memory,
            deferred: DeferredQueue::new(max_deferred, max_retries),
            active_requests: 0,
        }
    }

    /// Create a scheduler with default policy
    pub fn with_default_policy(total_memory: u64) -> Self {
        Self::new(SchedulerPolicy::default(), MemoryBudget::new(total_memory))
    }

    /// Make an admission decision for a request.
    ///
    /// # Correctness note
    /// The threshold check uses `effective_used = total - available()` rather than
    /// the raw `current_usage_mb` field. This is intentional: `available()` subtracts
    /// `reserved_mb` from the budget, so `effective_used` correctly represents the
    /// memory that is *not* available for new requests. Using `current_usage_mb`
    /// directly would ignore reservations and allow `decide()` to return `Accept`
    /// for requests that `accept()` will then silently fail to allocate.
    pub fn decide(&self, required_memory: u64) -> AdmissionReason {
        let available = self.memory.available();
        // Use effective used memory so reserved_mb is accounted for, consistent
        // with MemoryBudget::allocate() which gates on available() not current_usage_mb.
        let effective_used = self.memory.total_memory_mb.saturating_sub(available);

        let decision = self
            .policy
            .thresholds
            .check_memory(effective_used, required_memory);

        match decision {
            AdmissionDecision::Accept => {
                AdmissionReason::accept(effective_used, required_memory, available)
            }
            AdmissionDecision::Defer => {
                AdmissionReason::defer(effective_used, required_memory, available)
            }
            AdmissionDecision::Reject => {
                AdmissionReason::reject(effective_used, required_memory, available)
            }
        }
    }

    /// Accept a request and allocate memory
    pub fn accept(&mut self, required_memory: u64) -> bool {
        if self.memory.allocate(required_memory) {
            self.active_requests += 1;
            true
        } else {
            false
        }
    }

    /// Defer a request (add to queue)
    pub fn defer(&mut self, id: String, required_memory: u64, adapter: AdapterDescriptor) -> bool {
        let request = DeferredRequest::new(id, required_memory, adapter);
        self.deferred.enqueue(request)
    }

    /// Try to process deferred requests
    pub fn process_deferred(&mut self) -> Option<DeferredRequest> {
        let available = self.memory.available();
        self.deferred.dequeue(available)
    }

    /// Release memory from a completed request
    pub fn release(&mut self, amount: u64) {
        self.memory.release(amount);
        if self.active_requests > 0 {
            self.active_requests -= 1;
        }
    }

    /// Get current memory usage
    pub fn memory_usage(&self) -> u64 {
        self.memory.current_usage_mb
    }

    /// Get available memory
    pub fn available_memory(&self) -> u64 {
        self.memory.available()
    }

    /// Get memory usage percentage
    pub fn memory_usage_percent(&self) -> f64 {
        self.memory.usage_percent()
    }

    /// Get active request count
    pub fn active_requests(&self) -> usize {
        self.active_requests
    }

    /// Get deferred queue size
    pub fn deferred_count(&self) -> usize {
        self.deferred.size()
    }

    /// Check if stability allows profile switch
    pub fn can_switch_profile(&self) -> bool {
        self.policy.stability.can_switch()
    }

    /// Record a profile switch
    pub fn record_profile_switch(&mut self) {
        self.policy.stability.record_switch();
    }

    /// Update memory reading for hysteresis
    pub fn update_memory_reading(&mut self, memory: u64) {
        self.policy.stability.update_memory(memory);
    }

    /// Get policy reference
    pub fn policy(&self) -> &SchedulerPolicy {
        &self.policy
    }

    /// Get memory budget reference
    pub fn memory_budget(&self) -> &MemoryBudget {
        &self.memory
    }

    /// Get pending deferred requests
    pub fn pending_requests(&self) -> Vec<&DeferredRequest> {
        self.deferred.pending_requests()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{Modality, ModelFormat};

    #[test]
    fn test_memory_thresholds_accept() {
        let thresholds = MemoryThresholds::default();
        let decision = thresholds.check_memory(10 * 1024, 1024); // 10GB used, 1GB required
        assert_eq!(decision, AdmissionDecision::Accept);
    }

    #[test]
    fn test_memory_thresholds_defer() {
        let thresholds = MemoryThresholds::default();
        let decision = thresholds.check_memory(14 * 1024, 512); // 14GB used, 512MB required
        assert_eq!(decision, AdmissionDecision::Defer);
    }

    #[test]
    fn test_memory_thresholds_reject() {
        let thresholds = MemoryThresholds::default();
        let decision = thresholds.check_memory(16 * 1024, 1024); // 16GB used, 1GB required
        assert_eq!(decision, AdmissionDecision::Reject);
    }

    #[test]
    fn test_memory_budget_allocation() {
        let mut budget = MemoryBudget::new(8192); // 8GB
        assert_eq!(budget.available(), 8192);

        assert!(budget.allocate(2048)); // 2GB
        assert_eq!(budget.available(), 6144);
        assert_eq!(budget.current_usage_mb, 2048);

        budget.release(1024);
        assert_eq!(budget.current_usage_mb, 1024);
        assert_eq!(budget.available(), 7168);
    }

    #[test]
    fn test_deferred_queue_enqueue_dequeue() {
        let mut queue = DeferredQueue::new(10, 3);

        let adapter = AdapterDescriptor::builder()
            .id("test")
            .name("Test")
            .supported_modality(Modality::LLM)
            .supported_format(ModelFormat::Safetensors)
            .build()
            .expect("test descriptor should build");

        assert!(queue.enqueue(DeferredRequest::new(
            "req1".to_string(),
            1024,
            adapter.clone()
        )));
        assert_eq!(queue.size(), 1);

        // Can't dequeue - not enough memory
        let result = queue.dequeue(512);
        assert!(result.is_none());

        // Can dequeue with enough memory
        let result = queue.dequeue(2048);
        assert!(result.is_some());
        assert_eq!(queue.size(), 0);
    }

    #[test]
    fn test_deferred_queue_max_retries() {
        let mut queue = DeferredQueue::new(10, 2);

        let adapter = AdapterDescriptor::builder()
            .id("test")
            .name("Test")
            .supported_modality(Modality::LLM)
            .supported_format(ModelFormat::Safetensors)
            .build()
            .expect("test descriptor should build");

        let mut req = DeferredRequest::new("req1".to_string(), 512, adapter);
        req.increment_retry();
        req.increment_retry();

        queue.enqueue(req);
        assert_eq!(queue.size(), 1);

        // Should be expired after max retries
        let expired = queue.get_expired();
        assert_eq!(expired.len(), 1);
        assert_eq!(queue.size(), 0);
    }

    #[test]
    fn test_scheduler_admission() {
        // Create scheduler with memory already at high usage to test defer
        let mut policy = SchedulerPolicy::default();
        // Set defer threshold very low so almost any request will be deferred
        policy.thresholds = MemoryThresholds::new(8192, 512, 256);
        let mut scheduler = Scheduler::new(policy, MemoryBudget::new(8192));

        // Pre-allocate memory to trigger defer
        scheduler.memory.allocate(700);

        // Now should defer because projected (700+512=1212) > 512
        let reason = scheduler.decide(512);
        assert_eq!(reason.decision, AdmissionDecision::Defer);

        // Test accept with fresh scheduler
        let scheduler2 = Scheduler::with_default_policy(8192);
        let reason2 = scheduler2.decide(512);
        assert_eq!(reason2.decision, AdmissionDecision::Accept);
    }

    #[test]
    fn test_scheduler_accept_and_release() {
        let mut scheduler = Scheduler::with_default_policy(8192);

        assert!(scheduler.accept(1024));
        assert_eq!(scheduler.memory_usage(), 1024);
        assert_eq!(scheduler.active_requests(), 1);

        scheduler.release(1024);
        assert_eq!(scheduler.memory_usage(), 0);
        assert_eq!(scheduler.active_requests(), 0);
    }

    #[test]
    fn test_stability_control() {
        let mut stability = StabilityControl::default();

        // Should be able to switch initially
        assert!(stability.can_switch());

        stability.record_switch();

        // Should not be able to switch immediately after
        assert!(!stability.can_switch());
    }

    // ── Regression: decide() must honour reserved_mb ──────────────────────────

    /// Before the fix, `decide()` used raw `current_usage_mb` (ignoring
    /// `reserved_mb`), so it could return `Accept` for a request that
    /// `accept()` would then fail to allocate — a silent decision/allocation
    /// disagreement.
    ///
    /// After the fix, `decide()` uses `total - available()` as the effective
    /// used memory, which correctly includes `reserved_mb`.
    #[test]
    fn test_decide_respects_reserved_memory() {
        // 1000 MB total, 400 MB reserved for system overhead.
        // Effective available = 1000 - 0 - 400 = 600 MB.
        let mut budget = MemoryBudget::new(1000);
        budget.reserve(400);

        // Thresholds: defer > 750 MB, reject > 900 MB (of effective used).
        let thresholds = MemoryThresholds::new(900, 750, 500);
        let policy = SchedulerPolicy::with_thresholds(thresholds);
        let scheduler = Scheduler::new(policy, budget);

        // Request 700 MB.
        // Without fix: current_usage_mb=0, projected=700 ≤ 750 → Accept  ← WRONG
        // With fix:    effective_used = 1000 - 600 = 400, projected=1100 > 900 → Reject ← CORRECT
        //
        // Even using a softer check: effective_used=400, projected=400+700=1100 > 900 → Reject.
        let reason = scheduler.decide(700);
        assert_ne!(
            reason.decision,
            AdmissionDecision::Accept,
            "decide() must not Accept when reserved_mb leaves insufficient headroom"
        );

        // Cross-check: MemoryBudget::allocate() for the same amount also fails.
        let mut budget2 = MemoryBudget::new(1000);
        budget2.reserve(400);
        assert!(
            !budget2.allocate(700),
            "allocate() should fail when available < required"
        );
    }

    /// Verify that decide() and accept() agree when reserved_mb is present
    /// and the request *is* within budget.
    #[test]
    fn test_decide_and_accept_agree_with_reservation() {
        // 1000 MB total, 200 MB reserved.
        // Effective available = 800 MB.
        let mut budget = MemoryBudget::new(1000);
        budget.reserve(200);

        // Generous thresholds — any request ≤ 900 MB projected is accepted.
        let thresholds = MemoryThresholds::new(1200, 1000, 800);
        let policy = SchedulerPolicy::with_thresholds(thresholds);
        let mut scheduler = Scheduler::new(policy, budget);

        // Request 300 MB — well within 800 MB available.
        let reason = scheduler.decide(300);
        assert_eq!(
            reason.decision,
            AdmissionDecision::Accept,
            "decide() should Accept when request fits in available headroom"
        );

        // accept() must also succeed.
        assert!(
            scheduler.accept(300),
            "accept() must succeed when decide() returned Accept"
        );
    }
}
