//! Gateway layer implementations for `mofa-foundation`.
//!
//! Provides concrete types implementing kernel gateway traits:
//! - [`auth`] — [`InMemoryApiKeyStore`](auth::InMemoryApiKeyStore), [`ApiKeyAuthProvider`](auth::ApiKeyAuthProvider)

pub mod auth;

pub use auth::{ApiKeyAuthProvider, InMemoryApiKeyStore};
/// Foundation-layer gateway implementations.
///
/// This module contains concrete implementations of the kernel-level gateway
/// traits. Kernel traits live in `mofa-kernel::gateway`; implementations live
/// here so the kernel stays free of runtime dependencies.

pub mod rate_limiter;

pub use rate_limiter::TokenBucketRateLimiter;
