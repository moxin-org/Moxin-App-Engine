//! Authentication trait boundary for the gateway pipeline.
//!
//! Defines three things:
//! - [`AuthClaims`] — the verified identity produced by any auth backend
//! - [`AuthProvider`] — async trait gateway middleware calls to verify a request
//! - [`ApiKeyStore`] — persistence trait for issuing, looking up, and revoking API keys
//! - [`AuthError`] — typed error enum covering every auth failure mode
//!
//! Concrete implementations (in-memory store, JWT verifier, etc.) live in
//! `mofa-foundation` so the kernel stays free of crypto and HTTP dependencies.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// AuthError
// ─────────────────────────────────────────────────────────────────────────────

/// Every way authentication can fail in the gateway pipeline.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AuthError {
    /// No credential was present in the request (missing Authorization header,
    /// missing API key header, etc.).
    #[error("missing credentials")]
    MissingCredentials,

    /// A credential was present but could not be verified — wrong key, bad
    /// signature, malformed token, etc.
    #[error("invalid credentials")]
    InvalidCredentials,

    /// The credential was valid at some point but has since expired.
    #[error("credentials have expired")]
    ExpiredCredentials,

    /// The credential was explicitly revoked.
    #[error("credentials have been revoked")]
    RevokedCredentials,

    /// The caller's verified claims do not include the scope required to access
    /// this route.
    #[error("insufficient scope: required '{required}'")]
    InsufficientScope {
        /// The scope string that was required but not present in the claims.
        required: String,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// AuthClaims
// ─────────────────────────────────────────────────────────────────────────────

/// The verified identity produced by a successful authentication check.
///
/// All auth backends — API key, JWT, OAuth 2.0, mTLS — produce `AuthClaims`
/// so downstream middleware and routing logic have a single type to reason
/// about regardless of the authentication strategy in use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthClaims {
    /// Stable identifier of the authenticated agent or user
    /// (e.g. `"agent:summarizer"`, `"user:alice"`).
    pub subject: String,
    /// Permitted operations for this identity
    /// (e.g. `["agents:invoke", "agents:read"]`).
    pub scopes: Vec<String>,
    /// Optional expiry as a UNIX timestamp in milliseconds.
    /// `None` means the credential does not expire.
    pub expires_at_ms: Option<u64>,
    /// Arbitrary extra attributes forwarded from the auth backend
    /// (e.g. tenant ID, rate-limit tier).
    pub attributes: HashMap<String, String>,
}

impl AuthClaims {
    /// Create claims with no expiry and no extra attributes.
    pub fn new(subject: impl Into<String>, scopes: Vec<String>) -> Self {
        Self {
            subject: subject.into(),
            scopes,
            expires_at_ms: None,
            attributes: HashMap::new(),
        }
    }

    /// Set an optional expiry timestamp (milliseconds since UNIX epoch).
    pub fn with_expiry(mut self, expires_at_ms: u64) -> Self {
        self.expires_at_ms = Some(expires_at_ms);
        self
    }

    /// Attach an extra attribute.
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Returns `true` if the claims include `scope`.
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    /// Returns `true` if an expiry is set and has already passed.
    ///
    /// `now_ms` should be the current time as milliseconds since UNIX epoch.
    /// Injecting it as a parameter makes this method unit-testable without
    /// mocking the system clock.
    pub fn is_expired(&self, now_ms: u64) -> bool {
        self.expires_at_ms.map(|exp| now_ms > exp).unwrap_or(false)
    }

    /// Convenience: assert a required scope is present, returning
    /// [`AuthError::InsufficientScope`] if it is not.
    pub fn require_scope(&self, scope: &str) -> Result<(), AuthError> {
        if self.has_scope(scope) {
            Ok(())
        } else {
            Err(AuthError::InsufficientScope {
                required: scope.to_string(),
            })
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AuthProvider
// ─────────────────────────────────────────────────────────────────────────────

/// Kernel contract for authenticating an inbound request.
///
/// Gateway middleware calls `authenticate` with the request headers extracted
/// into a plain `HashMap` so the trait carries no dependency on axum or any
/// HTTP framework.  The gateway layer is responsible for extracting headers
/// from the raw request before calling this trait.
///
/// Implementations must be `Send + Sync` so they can be held behind an `Arc`
/// and shared across Tokio tasks.
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// Verify the request headers and return verified claims on success.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError`] when credentials are missing, invalid, expired,
    /// or lack a required scope.
    async fn authenticate(
        &self,
        headers: &HashMap<String, String>,
    ) -> Result<AuthClaims, AuthError>;
}

// ─────────────────────────────────────────────────────────────────────────────
// ApiKeyStore
// ─────────────────────────────────────────────────────────────────────────────

/// Kernel contract for API key lifecycle management.
///
/// The store is the persistence layer behind an API-key-based
/// [`AuthProvider`].  In-memory and file-backed implementations live in
/// `mofa-foundation`.
pub trait ApiKeyStore: Send + Sync {
    /// Look up the claims associated with `key`.
    ///
    /// Returns `Ok(AuthClaims)` if valid, `Err(AuthError::RevokedCredentials)` if revoked,
    /// or `Err(AuthError::InvalidCredentials)` if not found.
    fn lookup(&self, key: &str) -> Result<AuthClaims, AuthError>;

    /// Issue a new API key for `subject` with the given `scopes`.
    ///
    /// Returns the generated key string.  The key must be stored internally
    /// so that a subsequent `lookup` call returns the associated claims.
    fn issue(&mut self, subject: impl Into<String>, scopes: Vec<String>) -> String;

    /// Revoke an existing key.
    ///
    /// Returns `true` if the key existed and was removed, `false` if it was
    /// not found (already revoked or never issued).
    fn revoke(&mut self, key: &str) -> bool;
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    // ── AuthClaims ───────────────────────────────────────────────────────────

    #[test]
    fn claims_has_scope_true() {
        let claims = AuthClaims::new("agent:a", vec!["agents:invoke".to_string()]);
        assert!(claims.has_scope("agents:invoke"));
    }

    #[test]
    fn claims_has_scope_false() {
        let claims = AuthClaims::new("agent:a", vec!["agents:read".to_string()]);
        assert!(!claims.has_scope("agents:invoke"));
    }

    #[test]
    fn claims_require_scope_ok() {
        let claims = AuthClaims::new("agent:a", vec!["agents:invoke".to_string()]);
        assert!(claims.require_scope("agents:invoke").is_ok());
    }

    #[test]
    fn claims_require_scope_insufficient() {
        let claims = AuthClaims::new("agent:a", vec!["agents:read".to_string()]);
        let err = claims.require_scope("agents:invoke").unwrap_err();
        assert!(matches!(
            err,
            AuthError::InsufficientScope { required } if required == "agents:invoke"
        ));
    }

    #[test]
    fn claims_not_expired_when_no_expiry() {
        let claims = AuthClaims::new("agent:a", vec![]);
        assert!(!claims.is_expired(9_999_999_999_999));
    }

    #[test]
    fn claims_not_expired_before_expiry() {
        let claims = AuthClaims::new("agent:a", vec![]).with_expiry(1_000_000);
        assert!(!claims.is_expired(999_999));
    }

    #[test]
    fn claims_expired_after_expiry() {
        let claims = AuthClaims::new("agent:a", vec![]).with_expiry(1_000_000);
        assert!(claims.is_expired(1_000_001));
    }

    #[test]
    fn claims_attributes_builder() {
        let claims = AuthClaims::new("agent:a", vec![])
            .with_attribute("tenant", "acme")
            .with_attribute("tier", "pro");
        assert_eq!(claims.attributes.get("tenant"), Some(&"acme".to_string()));
        assert_eq!(claims.attributes.get("tier"), Some(&"pro".to_string()));
    }

    // ── AuthProvider ─────────────────────────────────────────────────────────

    /// Minimal in-test AuthProvider that accepts a single hardcoded key.
    struct HardcodedKeyProvider {
        valid_key: String,
        scopes: Vec<String>,
    }

    #[async_trait]
    impl AuthProvider for HardcodedKeyProvider {
        async fn authenticate(
            &self,
            headers: &HashMap<String, String>,
        ) -> Result<AuthClaims, AuthError> {
            match headers.get("x-api-key").map(|s| s.as_str()) {
                None => Err(AuthError::MissingCredentials),
                Some(k) if k == self.valid_key => {
                    Ok(AuthClaims::new("agent:test", self.scopes.clone()))
                }
                Some(_) => Err(AuthError::InvalidCredentials),
            }
        }
    }

    fn provider(scopes: Vec<&str>) -> HardcodedKeyProvider {
        HardcodedKeyProvider {
            valid_key: "secret".to_string(),
            scopes: scopes.into_iter().map(String::from).collect(),
        }
    }

    #[tokio::test]
    async fn auth_provider_missing_header() {
        let p = provider(vec![]);
        let err = p.authenticate(&HashMap::new()).await.unwrap_err();
        assert_eq!(err, AuthError::MissingCredentials);
    }

    #[tokio::test]
    async fn auth_provider_invalid_key() {
        let p = provider(vec![]);
        let headers = HashMap::from([("x-api-key".to_string(), "wrong".to_string())]);
        let err = p.authenticate(&headers).await.unwrap_err();
        assert_eq!(err, AuthError::InvalidCredentials);
    }

    #[tokio::test]
    async fn auth_provider_valid_key_returns_claims() {
        let p = provider(vec!["agents:invoke"]);
        let headers = HashMap::from([("x-api-key".to_string(), "secret".to_string())]);
        let claims = p.authenticate(&headers).await.unwrap();
        assert_eq!(claims.subject, "agent:test");
        assert!(claims.has_scope("agents:invoke"));
    }

    #[tokio::test]
    async fn auth_provider_insufficient_scope_after_auth() {
        let p = provider(vec!["agents:read"]);
        let headers = HashMap::from([("x-api-key".to_string(), "secret".to_string())]);
        let claims = p.authenticate(&headers).await.unwrap();
        let err = claims.require_scope("agents:invoke").unwrap_err();
        assert!(matches!(
            err,
            AuthError::InsufficientScope { required } if required == "agents:invoke"
        ));
    }

    // ── ApiKeyStore ───────────────────────────────────────────────────────────

    /// Minimal in-test ApiKeyStore.
    struct InMemoryApiKeyStore {
        keys: HashMap<String, AuthClaims>,
        counter: u32,
    }

    impl InMemoryApiKeyStore {
        fn new() -> Self {
            Self {
                keys: HashMap::new(),
                counter: 0,
            }
        }
    }

    impl ApiKeyStore for InMemoryApiKeyStore {
        fn lookup(&self, key: &str) -> Result<AuthClaims, AuthError> {
            self.keys.get(key).cloned().ok_or(AuthError::InvalidCredentials)
        }

        fn issue(&mut self, subject: impl Into<String>, scopes: Vec<String>) -> String {
            self.counter += 1;
            let key = format!("key-{}", self.counter);
            self.keys
                .insert(key.clone(), AuthClaims::new(subject, scopes));
            key
        }

        fn revoke(&mut self, key: &str) -> bool {
            self.keys.remove(key).is_some()
        }
    }

    #[test]
    fn api_key_store_issue_and_lookup() {
        let mut store = InMemoryApiKeyStore::new();
        let key = store.issue("agent:a", vec!["agents:invoke".to_string()]);
        let claims = store.lookup(&key).unwrap();
        assert_eq!(claims.subject, "agent:a");
        assert!(claims.has_scope("agents:invoke"));
    }

    #[test]
    fn api_key_store_lookup_missing_returns_error() {
        let store = InMemoryApiKeyStore::new();
        assert!(matches!(store.lookup("nonexistent"), Err(AuthError::InvalidCredentials)));
    }

    #[test]
    fn api_key_store_revoke_returns_true() {
        let mut store = InMemoryApiKeyStore::new();
        let key = store.issue("agent:a", vec![]);
        assert!(store.revoke(&key));
        assert!(matches!(store.lookup(&key), Err(AuthError::InvalidCredentials)));
    }

    #[test]
    fn api_key_store_revoke_missing_returns_false() {
        let mut store = InMemoryApiKeyStore::new();
        assert!(!store.revoke("ghost"));
    }

    #[test]
    fn api_key_store_each_issue_unique_key() {
        let mut store = InMemoryApiKeyStore::new();
        let k1 = store.issue("agent:a", vec![]);
        let k2 = store.issue("agent:b", vec![]);
        assert_ne!(k1, k2);
    }
}
