//! Fairness-aware deferred request queue.
//!
//! When memory pressure causes requests to be deferred, we need a fair
//! queuing policy that:
//! - Prevents starvation of small requests behind large ones
//! - Respects a maximum retry count (eventually reject)
//! - Dequeues oldest-first among requests that fit in available memory

use std::collections::VecDeque;
use std::time::Instant;

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
}

impl DeferredRequest {
    /// Create a new deferred request.
    pub fn new(id: String, required_mb: u64) -> Self {
        Self {
            id,
            required_mb,
            enqueued_at: Instant::now(),
            retry_count: 0,
        }
    }

    /// How long this request has been waiting (milliseconds).
    pub fn wait_time_ms(&self) -> u64 {
        self.enqueued_at.elapsed().as_millis() as u64
    }

    /// Increment the retry counter.
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }
}

/// Age-aware deferred queue with capacity limits.
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

    /// Dequeue the **oldest** request that fits in `available_mb`.
    ///
    /// This is the fairness policy: we scan from oldest to newest and
    /// take the first request whose memory requirement is satisfied,
    /// preventing starvation of small requests behind large ones.
    pub fn dequeue_oldest_fitting(&mut self, available_mb: u64) -> Option<DeferredRequest> {
        let mut best_idx: Option<usize> = None;

        for (i, req) in self.queue.iter().enumerate() {
            if req.required_mb <= available_mb && req.retry_count < self.max_retries {
                best_idx = Some(i);
                break; // oldest first
            }

            #[test]
            fn test_enqueue_full_returns_false() {
                let mut q = DeferredQueue::new(2, 3);
                assert!(q.enqueue(DeferredRequest::new("r1".into(), 100)));
                assert!(q.enqueue(DeferredRequest::new("r2".into(), 100)));
                assert!(!q.enqueue(DeferredRequest::new("r3".into(), 100)));
                assert_eq!(q.len(), 2);
            }

            #[test]
            fn test_dequeue_empty_returns_none() {
                let mut q = DeferredQueue::new(10, 3);
                let result = q.dequeue_oldest_fitting(1000);
                assert!(result.is_none());
            }

            #[test]
            fn test_drain_expired_multiple() {
                let mut q = DeferredQueue::new(10, 2);
                let mut req1 = DeferredRequest::new("r1".into(), 100);
                let mut req2 = DeferredRequest::new("r2".into(), 100);
                let mut req3 = DeferredRequest::new("r3".into(), 100);
                req1.increment_retry();
                req1.increment_retry(); // expired
                req2.increment_retry(); // not expired (1)
                req3.increment_retry();
                req3.increment_retry(); // expired
                q.enqueue(req1);
                q.enqueue(req2);
                q.enqueue(req3);

                let expired = q.drain_expired();
                assert_eq!(expired.len(), 2);
                let ids: Vec<_> = expired.iter().map(|r| r.id.as_str()).collect();
                assert!(ids.contains(&"r1"));
                assert!(ids.contains(&"r3"));
                assert_eq!(q.len(), 1);
                assert_eq!(q.dequeue_oldest_fitting(1000).unwrap().id, "r2");
            }

            #[test]
            fn test_dequeue_skips_all_max_retries_and_fits() {
                // All requests are over max_retries, none should be dequeued
                let mut q = DeferredQueue::new(10, 1);
                let mut req = DeferredRequest::new("r1".into(), 100);
                req.increment_retry();
                q.enqueue(req);
                assert!(q.dequeue_oldest_fitting(1000).is_none());
            }

            #[test]
            fn test_wait_time_ms_increases() {
                let req = DeferredRequest::new("r1".into(), 100);
                let time1 = req.wait_time_ms();
                // Sleep a tiny bit to ensure time passes
                std::thread::sleep(std::time::Duration::from_millis(10));
                let time2 = req.wait_time_ms();
                assert!(time2 >= time1);
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
}
