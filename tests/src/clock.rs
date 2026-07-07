//! Deterministic clock for testing time-dependent agent code.

use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

fn duration_to_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Trait for injectable time sources.
pub trait Clock: Send + Sync {
    /// Returns the current time in milliseconds since an arbitrary epoch.
    fn now_millis(&self) -> u64;
}

/// A mock clock with manually controlled time.
///
/// Time starts at zero and only advances when you call [`advance`](Self::advance),
/// [`set`](Self::set), or enable [`set_auto_advance`](Self::set_auto_advance).
pub struct MockClock {
    current_ms: AtomicU64,
    auto_advance_ms: RwLock<Option<u64>>,
}

impl Default for MockClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MockClock {
    /// Create a clock starting at time zero.
    pub fn new() -> Self {
        Self {
            current_ms: AtomicU64::new(0),
            auto_advance_ms: RwLock::new(None),
        }
    }

    /// Create a clock starting at the given duration from epoch.
    pub fn starting_at(start: Duration) -> Self {
        Self {
            current_ms: AtomicU64::new(duration_to_millis(start)),
            auto_advance_ms: RwLock::new(None),
        }
    }

    /// Advance the clock by the given duration.
    pub fn advance(&self, duration: Duration) {
        self.current_ms
            .fetch_add(duration_to_millis(duration), Ordering::Relaxed);
    }

    /// Set the clock to an exact time.
    pub fn set(&self, duration: Duration) {
        self.current_ms
            .store(duration_to_millis(duration), Ordering::Relaxed);
    }

    /// Enable auto-advance: each call to [`now_millis`](Clock::now_millis)
    /// automatically moves time forward by the given step.
    pub fn set_auto_advance(&self, step: Duration) {
        *self.auto_advance_ms.write().expect("lock poisoned") = Some(duration_to_millis(step));
    }

    /// Disable auto-advance.
    pub fn clear_auto_advance(&self) {
        *self.auto_advance_ms.write().expect("lock poisoned") = None;
    }

    /// Return the current time without applying auto-advance.
    pub fn peek_millis(&self) -> u64 {
        self.current_ms.load(Ordering::Relaxed)
    }

    /// Compute an absolute deadline relative to the current time.
    pub fn deadline_after(&self, timeout: Duration) -> u64 {
        self.peek_millis().saturating_add(duration_to_millis(timeout))
    }

    /// Check whether the current time has reached the given deadline.
    pub fn has_reached_deadline(&self, deadline_ms: u64) -> bool {
        self.peek_millis() >= deadline_ms
    }

    /// Return the remaining duration until the deadline, floored at zero.
    pub fn remaining_until(&self, deadline_ms: u64) -> Duration {
        Duration::from_millis(deadline_ms.saturating_sub(self.peek_millis()))
    }
}

impl Clock for MockClock {
    fn now_millis(&self) -> u64 {
        let current = self.current_ms.load(Ordering::Relaxed);
        if let Some(step) = *self.auto_advance_ms.read().expect("lock poisoned") {
            self.current_ms.fetch_add(step, Ordering::Relaxed);
        }
        current
    }
}

/// A real-time clock backed by [`std::time::SystemTime`].
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}
