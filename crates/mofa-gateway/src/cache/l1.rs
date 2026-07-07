//! L1 DashMap Cache for the MoFA Gateway.
//!
//! Provides a high-performance, concurrent, in-memory cache for gateway responses.

use dashmap::DashMap;
use mofa_kernel::gateway::{GatewayRequest, GatewayResponse};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
/// Statistics related to the L1 Cache.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of successful cache hits.
    pub hits: usize,
    /// Number of cache misses.
    pub misses: usize,
    /// Current number of entries in the cache.
    pub size: usize,
}

/// A single entry in the L1 Cache.
#[derive(Debug)]
struct CacheEntry {
    response: GatewayResponse,
    expires_at: Instant,
    path: String,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct CacheKey {
    path: String,
    method: mofa_kernel::gateway::route::HttpMethod,
}

/// L1 in-memory cache keyed on deterministic request hash.
///
/// Features per-entry TTL and thread-safe concurrent access via `DashMap`.
pub struct L1Cache {
    entries: DashMap<CacheKey, CacheEntry>,
    default_ttl: Duration,
    max_entries: usize,
    hits: AtomicUsize,
    misses: AtomicUsize,
}

impl L1Cache {
    /// Create a new L1 Cache with the given default TTL and maximum capacity.
    ///
    /// # Panics
    /// Panics if `max_entries` is 0.
    pub fn new(default_ttl: Duration, max_entries: usize) -> Self {
        assert!(max_entries > 0, "max_entries must be strictly greater than 0 to enable caching");
        Self {
            entries: DashMap::new(),
            default_ttl,
            max_entries,
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
        }
    }

    /// Retrieve a cached response for the given request.
    ///
    /// If the response exists but has expired, it is removed and `None` is returned.
    pub fn get(&self, req: &GatewayRequest) -> Option<GatewayResponse> {
        let key = CacheKey {
            path: req.path.clone(),
            method: req.method.clone(),
        };

        if let Some(entry) = self.entries.get(&key) {
            if entry.expires_at > Instant::now() {
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Some(entry.response.clone());
            }
        }

        // Atomically remove only if still expired
        self.entries.remove_if(&key, |_, entry| entry.expires_at <= Instant::now());
        self.misses.fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Insert a response into the cache.
    ///
    /// If `ttl` is `None`, the default TTL is used.
    /// If the cache exceeds `max_entries`, it currently evicts a random (first available) entry.
    /// Note: Capacity enforcement is best-effort under high concurrency.
    pub fn insert(&self, req: &GatewayRequest, resp: GatewayResponse, ttl: Option<Duration>) {
        if self.entries.len() >= self.max_entries {
            // Simple eviction: remove the first entry we can grab
            // For a production system this might use an LRU or random eviction strategy
            let eviction_key = self.entries.iter().next().map(|r| r.key().clone());
            if let Some(k) = eviction_key {
                self.entries.remove(&k);
            }
        }

        let key = CacheKey {
            path: req.path.clone(),
            method: req.method.clone(),
        };
        let expires_at = Instant::now() + ttl.unwrap_or(self.default_ttl);

        self.entries.insert(
            key,
            CacheEntry {
                response: resp,
                expires_at,
                path: req.path.clone(),
            },
        );
    }

    /// Explicitly invalidate a specific request's cache entry.
    pub fn invalidate(&self, req: &GatewayRequest) -> bool {
        let key = CacheKey {
            path: req.path.clone(),
            method: req.method.clone(),
        };
        self.entries.remove(&key).is_some()
    }

    /// Invalidate all cache entries whose path starts with the given prefix.
    pub fn invalidate_prefix(&self, path_prefix: &str) -> usize {
        let mut to_remove = Vec::new();
        for entry in self.entries.iter() {
            if entry.path.starts_with(path_prefix) {
                to_remove.push(entry.key().clone());
            }
        }

        let count = to_remove.len();
        for key in to_remove {
            self.entries.remove(&key);
        }
        count
    }

    /// Return current cache statistics.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            size: self.entries.len(),
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use mofa_kernel::gateway::route::HttpMethod;

    fn make_req(path: &str) -> GatewayRequest {
        GatewayRequest::new("id1", path, HttpMethod::Get)
    }

    fn make_resp() -> GatewayResponse {
        GatewayResponse::new(200, "backend")
    }

    #[test]
    fn cache_miss_on_empty() {
        let cache = L1Cache::new(Duration::from_secs(60), 100);
        let req = make_req("/test");
        assert!(cache.get(&req).is_none());
        assert_eq!(cache.stats().misses, 1);
        assert_eq!(cache.stats().hits, 0);
    }

    #[test]
    fn cache_hit_after_insert() {
        let cache = L1Cache::new(Duration::from_secs(60), 100);
        let req = make_req("/test");
        cache.insert(&req, make_resp(), None);

        let resp = cache.get(&req);
        assert!(resp.is_some());
        assert_eq!(resp.unwrap().status, 200);
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn expired_entry_returns_miss() {
        let cache = L1Cache::new(Duration::from_millis(1), 100);
        let req = make_req("/quick");
        cache.insert(&req, make_resp(), None);

        std::thread::sleep(Duration::from_millis(5)); // wait for expiry

        assert!(cache.get(&req).is_none());
        assert_eq!(cache.stats().misses, 1);
        assert_eq!(cache.stats().size, 0); // Should have been removed
    }

    #[test]
    fn invalidate_removes_entry() {
        let cache = L1Cache::new(Duration::from_secs(60), 100);
        let req = make_req("/invalidate-me");
        cache.insert(&req, make_resp(), None);

        assert!(cache.invalidate(&req));
        assert!(cache.get(&req).is_none());
        assert_eq!(cache.stats().size, 0);
    }

    #[test]
    fn invalidate_prefix_removes_matching() {
        let cache = L1Cache::new(Duration::from_secs(60), 100);
        cache.insert(&make_req("/api/v1/a"), make_resp(), None);
        cache.insert(&make_req("/api/v1/b"), make_resp(), None);
        cache.insert(&make_req("/api/v2/c"), make_resp(), None);

        let removed = cache.invalidate_prefix("/api/v1");
        assert_eq!(removed, 2);
        assert_eq!(cache.stats().size, 1); // Only v2 remains
    }

    #[test]
    fn stats_track_hits_and_misses() {
        let cache = L1Cache::new(Duration::from_secs(60), 100);
        let req = make_req("/test");

        cache.get(&req); // miss
        cache.insert(&req, make_resp(), None);
        cache.get(&req); // hit
        cache.get(&req); // hit

        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 2);
    }

    #[test]
    fn max_entries_limits_size() {
        let cache = L1Cache::new(Duration::from_secs(60), 2);
        
        cache.insert(&make_req("/1"), make_resp(), None);
        cache.insert(&make_req("/2"), make_resp(), None);
        // This will evict one of the previous two to enforce the max size
        cache.insert(&make_req("/3"), make_resp(), None);

        assert_eq!(cache.stats().size, 2);
    }
}
