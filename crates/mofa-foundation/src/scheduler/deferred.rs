//! Fairness-aware deferred request queue.
//!
//! When memory pressure causes requests to be deferred, we need a fair
//! queuing policy that:
//! - Prevents starvation of small requests behind large ones
//! - Respects a maximum retry count (eventually reject)
//! - Dequeues oldest-first among requests that fit in available memory

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// A request that was deferred due to memory pressure.
#[derive(Debug, Clone)]
pub struct DeferredRequest {
    /// Unique request identifier.
    pub id: String,
    /// Memory required by this request (MB).
    pub required_mb: u64,
    /// When the request was first deferred.
    pub enqueued_at: Instant,
    /// Number of retry attempts so far.
    pub retry_count: u32,
    /// Time-to-live: request expires after this duration if not processed.
    /// Prevents "ghost" requests from filling the queue when callers disconnect.
    pub ttl: Duration,
}

impl DeferredRequest {
    /// Create a new deferred request with default TTL (5 minutes).
    pub fn new(id: String, required_mb: u64) -> Self {
        Self::with_ttl(id, required_mb, Duration::from_secs(300))
    }

    /// Create a new deferred request with custom TTL.
    pub fn with_ttl(id: String, required_mb: u64, ttl: Duration) -> Self {
        Self {
            id,
            required_mb,
            enqueued_at: Instant::now(),
            retry_count: 0,
            ttl,
        }
    }

    /// How long this request has been waiting (milliseconds).
    pub fn wait_time_ms(&self) -> u64 {
        self.enqueued_at.elapsed().as_millis() as u64
    }

    /// Check if this request has exceeded its TTL.
    pub fn is_expired(&self) -> bool {
        self.enqueued_at.elapsed() > self.ttl
    }

    /// Increment the retry counter.
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }
}

/// Age-aware deferred queue with capacity limits and TTL-based expiry.
///
/// Prevents "ghost" requests from filling the queue when callers disconnect:
/// - Requests can be explicitly canceled with `cancel_request(id)`
/// - Requests automatically expire after their TTL (default 5 minutes)
/// - Both mechanisms prevent DoS via queue saturation
#[derive(Debug)]
pub struct DeferredQueue {
    queue: VecDeque<DeferredRequest>,
    max_size: usize,
    max_retries: u32,
}

impl DeferredQueue {
    /// Create a new queue with capacity and retry limits.
    pub fn new(max_size: usize, max_retries: u32) -> Self {
        Self {
            queue: VecDeque::new(),
            max_size,
            max_retries,
        }
    }

    /// Add a request to the queue. Returns `false` if the queue is full.
    pub fn enqueue(&mut self, request: DeferredRequest) -> bool {
        if self.queue.len() >= self.max_size {
            return false;
        }
        self.queue.push_back(request);
        true
    }

    /// Cancel a specific request by ID and remove it from the queue.
    ///
    /// Prevents "ghost" requests from occupied callers (e.g., disconnected clients)
    /// from blocking the queue.
    ///
    /// Returns `true` if a request was removed, `false` if not found.
    pub fn cancel_request(&mut self, request_id: &str) -> bool {
        if let Some(pos) = self.queue.iter().position(|r| r.id == request_id) {
            self.queue.remove(pos).is_some()
        } else {
            false
        }
    }

    /// Dequeue the **oldest** request that fits in `available_mb`.
    ///
    /// This is the fairness policy: we scan from oldest to newest and
    /// take the first request whose memory requirement is satisfied,
    /// preventing starvation of small requests behind large ones.
    ///
    /// Skips requests that have exceeded their TTL or max retries.
    pub fn dequeue_oldest_fitting(&mut self, available_mb: u64) -> Option<DeferredRequest> {
        let mut best_idx: Option<usize> = None;

        for (i, req) in self.queue.iter().enumerate() {
            // Skip expired requests (either by TTL or max retries)
            if req.is_expired() || req.retry_count >= self.max_retries {
                continue;
            }

            if req.required_mb <= available_mb {
                best_idx = Some(i);
                break; // oldest first
            }
        }

        best_idx.and_then(|i| self.queue.remove(i))
    }

    /// Remove and return all requests that have exceeded max retries.
    pub fn drain_expired(&mut self) -> Vec<DeferredRequest> {
        let expired: Vec<DeferredRequest> = self
            .queue
            .iter()
            .filter(|r| r.retry_count >= self.max_retries)
            .cloned()
            .collect();

        self.queue.retain(|r| r.retry_count < self.max_retries);
        expired
    }

    /// Remove and return all requests that have exceeded their TTL.
    ///
    /// Automatically called periodically by the scheduler to prevent
    /// "ghost" requests from filling the queue when callers disconnect.
    pub fn drain_ttl_expired(&mut self) -> Vec<DeferredRequest> {
        let expired: Vec<DeferredRequest> = self
            .queue
            .iter()
            .filter(|r| r.is_expired())
            .cloned()
            .collect();

        self.queue.retain(|r| !r.is_expired());
        expired
    }

    /// Current number of requests in the queue.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Is the queue empty?
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enqueue_dequeue_basic() {
        let mut q = DeferredQueue::new(10, 3);
        assert!(q.enqueue(DeferredRequest::new("r1".into(), 1024)));
        assert!(q.enqueue(DeferredRequest::new("r2".into(), 512)));
        assert_eq!(q.len(), 2);

        // Not enough memory for r1 (1024), but enough for r2 (512)
        // However, r1 is older so fairness scans r1 first — skips it, takes r2
        let req = q.dequeue_oldest_fitting(600);
        assert!(req.is_some());
        assert_eq!(req.unwrap().id, "r2"); // r2 fits, r1 doesn't
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn test_dequeue_oldest_first() {
        let mut q = DeferredQueue::new(10, 3);
        q.enqueue(DeferredRequest::new("r1".into(), 512));
        q.enqueue(DeferredRequest::new("r2".into(), 256));

        // Both fit — oldest (r1) should be dequeued first
        let req = q.dequeue_oldest_fitting(1024);
        assert_eq!(req.unwrap().id, "r1");
    }

    #[test]
    fn test_queue_capacity_limit() {
        let mut q = DeferredQueue::new(2, 3);
        assert!(q.enqueue(DeferredRequest::new("r1".into(), 100)));
        assert!(q.enqueue(DeferredRequest::new("r2".into(), 100)));
        assert!(!q.enqueue(DeferredRequest::new("r3".into(), 100))); // full
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn test_drain_expired() {
        let mut q = DeferredQueue::new(10, 2);
        let mut req = DeferredRequest::new("r1".into(), 100);
        req.increment_retry();
        req.increment_retry(); // now at max_retries = 2
        q.enqueue(req);
        q.enqueue(DeferredRequest::new("r2".into(), 100)); // still valid

        let expired = q.drain_expired();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].id, "r1");
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn test_dequeue_respects_max_retries() {
        let mut q = DeferredQueue::new(10, 1);
        let mut req = DeferredRequest::new("r1".into(), 100);
        req.increment_retry(); // at max_retries
        q.enqueue(req);

        // r1 has exceeded max retries — should not be dequeued
        let result = q.dequeue_oldest_fitting(10_000);
        assert!(result.is_none());
    }

    #[test]
    fn test_cancel_request() {
        let mut q = DeferredQueue::new(10, 3);
        q.enqueue(DeferredRequest::new("r1".into(), 100));
        q.enqueue(DeferredRequest::new("r2".into(), 100));
        assert_eq!(q.len(), 2);

        // Cancel a request that exists
        assert!(q.cancel_request("r1"));
        assert_eq!(q.len(), 1);

        // Try to cancel non-existent request
        assert!(!q.cancel_request("r3"));
        assert_eq!(q.len(), 1);

        // Verify r2 is still there
        let remaining = q.dequeue_oldest_fitting(10_000);
        assert_eq!(remaining.unwrap().id, "r2");
    }

    #[test]
    fn test_ttl_expiry() {
        let mut q = DeferredQueue::new(10, 3);

        // Create request with very short TTL (1ms)
        let req = DeferredRequest::with_ttl("r1".into(), 100, Duration::from_millis(1));
        q.enqueue(req);

        // Immediately after enqueue, should not be expired
        assert!(!q.dequeue_oldest_fitting(10_000).is_none());

        // Create new request with short TTL and wait
        let req2 = DeferredRequest::with_ttl("r2".into(), 100, Duration::from_millis(10));
        q.enqueue(req2);

        // Wait for TTL to expire
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Request should now be expired and not dequeued
        assert!(q.dequeue_oldest_fitting(10_000).is_none());

        // But drain_ttl_expired should find it
        let expired = q.drain_ttl_expired();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].id, "r2");
    }

    #[test]
    fn test_drain_ttl_expired() {
        let mut q = DeferredQueue::new(10, 3);

        // Add request with long TTL
        q.enqueue(DeferredRequest::with_ttl(
            "r1".into(),
            100,
            Duration::from_secs(300),
        ));

        // Add request with short TTL
        q.enqueue(DeferredRequest::with_ttl(
            "r2".into(),
            100,
            Duration::from_millis(1),
        ));

        // Wait for short TTL to expire
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Only r2 should be expired
        let expired = q.drain_ttl_expired();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].id, "r2");
        assert_eq!(q.len(), 1); // r1 still in queue
    }

    #[test]
    fn test_ghost_request_prevention() {
        // Simulate the fix #1569 scenario:
        // 1. Submit requests that exceed defer_threshold
        // 2. Callers disconnect (simulate with cancel_request)
        // 3. New legitimate requests should still enqueue
        let mut q = DeferredQueue::new(100, 10);

        // Fill with "ghost" requests from disconnected callers
        for i in 0..50 {
            q.enqueue(DeferredRequest::new(format!("ghost-{}", i), 100));
        }
        assert_eq!(q.len(), 50);

        // Callers disconnect - we cancel these "ghost" requests
        for i in 0..50 {
            assert!(q.cancel_request(&format!("ghost-{}", i)));
        }

        // Queue should now be empty, allowing new legitimate requests
        assert!(q.is_empty());
        assert!(q.enqueue(DeferredRequest::new("legitimate".into(), 100)));
    }
}
