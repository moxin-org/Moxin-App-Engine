//! Gateway level caching mechanisms.
//!
//! Provides in-memory (L1) caching capabilities for gateway responses.

pub mod l1;

pub use l1::{CacheStats, L1Cache};
