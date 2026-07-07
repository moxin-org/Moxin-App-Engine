//! Concrete `AuthProvider` and `ApiKeyStore` implementations for the gateway.
//!
//! | Type | Description |
//! |------|-------------|
//! | [`InMemoryApiKeyStore`] | In-memory `ApiKeyStore` with UUIDv4 key generation |
//! | [`ApiKeyAuthProvider`] | `AuthProvider` that validates via `x-api-key` header |
//!
//! # Usage
//!
//! ```rust
//! use mofa_foundation::gateway::auth::{ApiKeyAuthProvider, InMemoryApiKeyStore};
//! use mofa_kernel::gateway::{ApiKeyStore, AuthProvider};
//! use std::sync::Arc;
//!
//! # #[tokio::main]
//! # async fn main() {
//! let mut store = InMemoryApiKeyStore::new();
//! let key = store.issue("agent:summarizer", vec!["agents:invoke".to_string()]);
//!
//! let provider = ApiKeyAuthProvider::new(Arc::new(store));
//!
//! let headers = std::collections::HashMap::from([
//!     ("x-api-key".to_string(), key.clone()),
//! ]);
//! let claims = provider.authenticate(&headers).await.unwrap();
//! assert_eq!(claims.subject, "agent:summarizer");
//! assert!(claims.has_scope("agents:invoke"));
//! # }
//! ```

use async_trait::async_trait;
use mofa_kernel::gateway::{ApiKeyStore, AuthClaims, AuthError, AuthProvider};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

// ─────────────────────────────────────────────────────────────────────────────
// InMemoryApiKeyStore
// ─────────────────────────────────────────────────────────────────────────────

/// In-memory [`ApiKeyStore`] backed by a plain `HashMap`.
///
/// Keys are generated as random UUIDv4 strings.  This is intentionally simple — production
/// deployments should replace this with a store backed by a persistent
/// deployments should replace this with a store backed by a persistent
/// database (Redis, PostgreSQL, etc.).
pub struct InMemoryApiKeyStore {
    keys: HashMap<String, Option<AuthClaims>>,
    counter: u64,
}

impl Default for InMemoryApiKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryApiKeyStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self {
            keys: HashMap::new(),
            counter: 0,
        }
    }

    /// Return the number of active (non-revoked) keys.
    pub fn key_count(&self) -> usize {
        self.keys.values().filter(|v| v.is_some()).count()
    }

    /// Issue a key that expires at a specific UNIX timestamp (milliseconds).
    pub fn issue_with_expiry(
        &mut self,
        subject: impl Into<String>,
        scopes: Vec<String>,
        expires_at_ms: u64,
    ) -> String {
        let key = self.generate_key();
        let claims = AuthClaims::new(subject, scopes).with_expiry(expires_at_ms);
        self.keys.insert(key.clone(), Some(claims));
        key
    }

    fn generate_key(&mut self) -> String {
        self.counter += 1;
        format!("mofa_{}", uuid::Uuid::new_v4().simple())
    }
}

impl ApiKeyStore for InMemoryApiKeyStore {
    fn lookup(&self, key: &str) -> Result<AuthClaims, AuthError> {
        match self.keys.get(key) {
            Some(Some(claims)) => Ok(claims.clone()),
            Some(None) => Err(AuthError::RevokedCredentials),
            None => Err(AuthError::InvalidCredentials),
        }
    }

    fn issue(&mut self, subject: impl Into<String>, scopes: Vec<String>) -> String {
        let key = self.generate_key();
        self.keys
            .insert(key.clone(), Some(AuthClaims::new(subject, scopes)));
        key
    }

    fn revoke(&mut self, key: &str) -> bool {
        if let Some(val) = self.keys.get_mut(key) {
            if val.is_some() {
                *val = None;
                return true;
            }
        }
        false
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ApiKeyAuthProvider
// ─────────────────────────────────────────────────────────────────────────────

/// [`AuthProvider`] that validates requests via a header-based API key.
///
/// By default reads the `x-api-key` header.  The header name is configurable
/// at construction time.
///
/// Validation logic:
/// 1. If the header is absent → [`AuthError::MissingCredentials`].
/// 2. If the key is not in the store → [`AuthError::InvalidCredentials`].
/// 3. If the key's claims have expired → [`AuthError::ExpiredCredentials`].
/// 4. Otherwise → returns the associated [`AuthClaims`].
pub struct ApiKeyAuthProvider<S: ApiKeyStore> {
    store: Arc<S>,
    header_name: String,
}

impl<S: ApiKeyStore> ApiKeyAuthProvider<S> {
    /// Create a provider using the default `x-api-key` header.
    pub fn new(store: Arc<S>) -> Self {
        Self {
            store,
            header_name: "x-api-key".to_string(),
        }
    }

    /// Create a provider that reads from a custom header name.
    ///
    /// The header name is lowercased automatically. Returns an error if the
    /// provided name contains invalid characters (non-ascii alphanumeric or hyphen).
    pub fn with_header(store: Arc<S>, header_name: impl Into<String>) -> Result<Self, String> {
        let name = header_name.into().to_ascii_lowercase();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err("Invalid header name".to_string());
        }
        Ok(Self {
            store,
            header_name: name,
        })
    }
}

#[async_trait]
impl<S: ApiKeyStore> AuthProvider for ApiKeyAuthProvider<S> {
    async fn authenticate(
        &self,
        headers: &HashMap<String, String>,
    ) -> Result<AuthClaims, AuthError> {
        // 1. Extract the key from the configured header.
        let key = headers
            .get(&self.header_name)
            .ok_or(AuthError::MissingCredentials)?;

        // 2. Look up the key in the store.
        let claims = self.store.lookup(key)?;

        // 3. Check expiry.
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| {
                let ms = d.as_millis();
                u64::try_from(ms).unwrap_or(u64::MAX)
            })
            .unwrap_or(0);

        if claims.is_expired(now_ms) {
            return Err(AuthError::ExpiredCredentials);
        }

        Ok(claims)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests (Aligned with PR Description)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // ── Helper ───────────────────────────────────────────────────────────────

    fn make_provider_with_keys(keys: Vec<(&str, Vec<String>, Option<u64>)>) -> (ApiKeyAuthProvider<InMemoryApiKeyStore>, Vec<String>) {
        let mut store = InMemoryApiKeyStore::new();
        let mut generated_keys = Vec::new();
        for (subject, scopes, expiry) in keys {
            let key = match expiry {
                Some(exp) => store.issue_with_expiry(subject, scopes, exp),
                None => store.issue(subject, scopes),
            };
            generated_keys.push(key);
        }
        (ApiKeyAuthProvider::new(Arc::new(store)), generated_keys)
    }

    // ── PR Body Test Suite ───────────────────────────────────────────────────

    #[tokio::test]
    async fn valid_key_populates_context() {
        let (provider, keys) = make_provider_with_keys(vec![
            ("agent-prime", vec!["chat:write".into()], None)
        ]);
        let headers = HashMap::from([("x-api-key".to_string(), keys[0].clone())]);
        let claims = provider.authenticate(&headers).await.expect("Auth should succeed");
        assert_eq!(claims.subject, "agent-prime");
        assert!(claims.has_scope("chat:write"));
    }

    #[tokio::test]
    async fn missing_header_returns_400() {
        let (provider, _) = make_provider_with_keys(vec![]);
        let headers = HashMap::new(); // No x-api-key
        let err = provider.authenticate(&headers).await.unwrap_err();
        assert_eq!(err, AuthError::MissingCredentials);
    }

    #[tokio::test]
    async fn unknown_key_returns_401() {
        let (provider, _) = make_provider_with_keys(vec![]);
        let headers = HashMap::from([("x-api-key".to_string(), "ghost-key".to_string())]);
        let err = provider.authenticate(&headers).await.unwrap_err();
        assert_eq!(err, AuthError::InvalidCredentials);
    }

    #[tokio::test]
    async fn revoked_key_returns_401() {
        let mut store = InMemoryApiKeyStore::new();
        let key = store.issue("subject", vec![]);
        store.revoke(&key); // Revoke it

        let provider = ApiKeyAuthProvider::new(Arc::new(store));
        let headers = HashMap::from([("x-api-key".to_string(), key)]);
        let err = provider.authenticate(&headers).await.unwrap_err();
        assert_eq!(err, AuthError::RevokedCredentials);
    }

    #[tokio::test]
    async fn expired_key_returns_401() {
        let (provider, keys) = make_provider_with_keys(vec![
            ("subject", vec![], Some(1)) // Expired 1ms after epoch
        ]);
        let headers = HashMap::from([("x-api-key".to_string(), keys[0].clone())]);
        let err = provider.authenticate(&headers).await.unwrap_err();
        assert_eq!(err, AuthError::ExpiredCredentials);
    }

    #[tokio::test]
    async fn non_expired_key_passes() {
        let future = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64 + 100_000;
        let (provider, keys) = make_provider_with_keys(vec![
            ("subject", vec![], Some(future))
        ]);
        let headers = HashMap::from([("x-api-key".to_string(), keys[0].clone())]);
        assert!(provider.authenticate(&headers).await.is_ok());
    }

    #[tokio::test]
    async fn key_with_no_expiry_never_expires() {
        let (provider, keys) = make_provider_with_keys(vec![
            ("subject", vec![], None)
        ]);
        let headers = HashMap::from([("x-api-key".to_string(), keys[0].clone())]);
        assert!(provider.authenticate(&headers).await.is_ok());
    }

    #[tokio::test]
    async fn custom_header_name_is_respected() {
        let mut store = InMemoryApiKeyStore::new();
        store.keys.insert("secret".to_string(), Some(AuthClaims::new("subject", vec![])));
        let provider = ApiKeyAuthProvider::with_header(Arc::new(store), "x-internal-token").unwrap();

        let headers = HashMap::from([("x-internal-token".to_string(), "secret".to_string())]);
        assert!(provider.authenticate(&headers).await.is_ok());
    }

    #[test]
    fn revoke_unknown_key_returns_error() {
        let mut store = InMemoryApiKeyStore::new();
        // Since revert returns bool, not Result (following the trait),
        // we assert it returns false for nonexistent.
        assert!(!store.revoke("missing-key"));
    }

    #[tokio::test]
    async fn scopes_from_claims_appear_in_context() {
        let scopes = vec!["read".to_string(), "write".to_string()];
        let (provider, keys) = make_provider_with_keys(vec![
            ("subject", scopes.clone(), None)
        ]);
        let headers = HashMap::from([("x-api-key".to_string(), keys[0].clone())]);
        let claims = provider.authenticate(&headers).await.unwrap();
        assert_eq!(claims.scopes, scopes);
    }

    #[tokio::test]
    async fn concurrent_issue_and_lookup_are_safe() {
        // We use a Mutex to allow multiple threads to 'issue' to the same store.
        // In reality, InMemoryApiKeyStore is not Sync for mutation, so we wrap it.
        let shared_store = Arc::new(Mutex::new(InMemoryApiKeyStore::new()));
        let mut handles = vec![];

        for i in 0..32 {
            let store = shared_store.clone();
            handles.push(tokio::spawn(async move {
                let subject = format!("agent-{}", i);
                let key = {
                    let mut s = store.lock().unwrap();
                    s.issue(subject, vec![])
                };
                // Verify we can find it immediately
                let s = store.lock().unwrap();
                let claims = s.lookup(&key).expect("Key must be findable immediately");
                assert_eq!(claims.subject, format!("agent-{}", i));
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        let final_count = shared_store.lock().unwrap().key_count();
        assert_eq!(final_count, 32);
    }
}
