//Message bus metrics

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// Point-in-time metrics snapshot
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageBusMetrics {
    pub queue_depth: i64,
    pub total_published: u64,
    pub total_delivered: u64,
    pub total_acked: u64,
    pub total_nacked: u64,
    pub total_dead_lettered: u64,
    pub total_dropped: u64,
    pub total_retries: u64,
    pub avg_delivery_latency_us: u64,
    /// P99 over a recent sliding window
    pub p99_delivery_latency_us: u64,
}

// Lock-free atomic counters
#[derive(Debug)]
pub struct MessageBusCounters {
    pub queue_depth: AtomicI64,
    pub total_published: AtomicU64,
    pub total_delivered: AtomicU64,
    pub total_acked: AtomicU64,
    pub total_nacked: AtomicU64,
    pub total_dead_lettered: AtomicU64,
    pub total_dropped: AtomicU64,
    pub total_retries: AtomicU64,
    pub avg_delivery_latency_us: AtomicU64,
    pub p99_delivery_latency_us: AtomicU64,
    latency_sum_us: AtomicU64,
    latency_count: AtomicU64,
    latency_window: Mutex<VecDeque<u64>>,
    latency_window_capacity: usize,
}

impl Default for MessageBusCounters {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageBusCounters {
    pub fn new() -> Self {
        Self {
            queue_depth: AtomicI64::new(0),
            total_published: AtomicU64::new(0),
            total_delivered: AtomicU64::new(0),
            total_acked: AtomicU64::new(0),
            total_nacked: AtomicU64::new(0),
            total_dead_lettered: AtomicU64::new(0),
            total_dropped: AtomicU64::new(0),
            total_retries: AtomicU64::new(0),
            avg_delivery_latency_us: AtomicU64::new(0),
            p99_delivery_latency_us: AtomicU64::new(0),
            latency_sum_us: AtomicU64::new(0),
            latency_count: AtomicU64::new(0),
            latency_window: Mutex::new(VecDeque::new()),
            latency_window_capacity: 128,
        }
    }

    pub fn inc_published(&self) {
        self.total_published.fetch_add(1, Ordering::Relaxed);
        self.queue_depth.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_delivered(&self) {
        self.total_delivered.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_acked(&self) {
        self.total_acked.fetch_add(1, Ordering::Relaxed);
        self.dec_queue_depth();
    }

    pub fn inc_nacked(&self) {
        self.total_nacked.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_dead_lettered(&self) {
        self.total_dead_lettered.fetch_add(1, Ordering::Relaxed);
        self.dec_queue_depth();
    }

    pub fn inc_dropped(&self) {
        self.total_dropped.fetch_add(1, Ordering::Relaxed);
        self.dec_queue_depth();
    }

    pub fn inc_retries(&self) {
        self.total_retries.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_delivery_latency_us(&self, latency_us: u64) {
        self.latency_sum_us.fetch_add(latency_us, Ordering::Relaxed);
        let count = self.latency_count.fetch_add(1, Ordering::Relaxed) + 1;
        let avg = self
            .latency_sum_us
            .load(Ordering::Relaxed)
            .saturating_div(count);
        self.avg_delivery_latency_us.store(avg, Ordering::Relaxed);

        if let Ok(mut window) = self.latency_window.lock() {
            window.push_back(latency_us);
            if window.len() > self.latency_window_capacity {
                window.pop_front();
            }
        }
    }

    pub fn snapshot(&self) -> MessageBusMetrics {
        let p99 = if let Ok(window) = self.latency_window.lock() {
            if window.is_empty() {
                0
            } else {
                let mut samples: Vec<u64> = window.iter().copied().collect();
                samples.sort_unstable();
                let idx = ((samples.len() as f64) * 0.99).ceil() as usize;
                let idx = idx.saturating_sub(1).min(samples.len() - 1);
                samples[idx]
            }
        } else {
            0
        };
        self.p99_delivery_latency_us.store(p99, Ordering::Relaxed);

        MessageBusMetrics {
            queue_depth: self.queue_depth.load(Ordering::Relaxed),
            total_published: self.total_published.load(Ordering::Relaxed),
            total_delivered: self.total_delivered.load(Ordering::Relaxed),
            total_acked: self.total_acked.load(Ordering::Relaxed),
            total_nacked: self.total_nacked.load(Ordering::Relaxed),
            total_dead_lettered: self.total_dead_lettered.load(Ordering::Relaxed),
            total_dropped: self.total_dropped.load(Ordering::Relaxed),
            total_retries: self.total_retries.load(Ordering::Relaxed),
            avg_delivery_latency_us: self.avg_delivery_latency_us.load(Ordering::Relaxed),
            p99_delivery_latency_us: p99,
        }
    }

    fn dec_queue_depth(&self) {
        let _ = self.queue_depth.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
            Some(v.saturating_sub(1))
        });
    }
}

pub trait MessageBusObserver: Send + Sync {
    fn counters(&self) -> &MessageBusCounters;

    fn metrics_snapshot(&self) -> MessageBusMetrics {
        self.counters().snapshot()
    }
}

pub type SharedCounters = Arc<MessageBusCounters>;

pub fn new_shared_counters() -> SharedCounters {
    Arc::new(MessageBusCounters::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counters() {
        let c = MessageBusCounters::new();
        c.inc_published();
        c.inc_published();
        c.inc_delivered();
        c.inc_acked();
        c.inc_dropped();
        c.record_delivery_latency_us(100);
        c.record_delivery_latency_us(200);

        let snap = c.snapshot();
        assert_eq!(snap.total_published, 2);
        assert_eq!(snap.total_delivered, 1);
        assert_eq!(snap.total_acked, 1);
        assert_eq!(snap.total_dropped, 1);
        assert_eq!(snap.queue_depth, 0);
        assert!(snap.avg_delivery_latency_us > 0);
    }
}
