//! WebSocket authentication and authorization.

use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::RwLock;

/// Authenticated client metadata.
#[derive(Debug, Clone)]
pub struct AuthInfo {
    pub client_id: String,
    pub permissions: Vec<String>,
}

/// WebSocket authentication provider.
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// Whether authentication is enabled.
    fn is_enabled(&self) -> bool;

    /// Validate a token. Returns [`AuthInfo`] on success.
    async fn validate(&self, token: &str) -> Result<AuthInfo, String>;
}

/// Allows all connections unconditionally (development-only).
pub struct NoopAuthProvider;

#[async_trait]
impl AuthProvider for NoopAuthProvider {
    fn is_enabled(&self) -> bool {
        false
    }

    async fn validate(&self, _token: &str) -> Result<AuthInfo, String> {
        Ok(AuthInfo {
            client_id: "anonymous".to_string(),
            permissions: vec!["read:metrics".to_string()],
        })
    }
}

/// Validates bearer tokens derived from a shared secret.
pub struct TokenAuthProvider {
    secret: String,
    valid_tokens: RwLock<HashSet<String>>,
}

impl TokenAuthProvider {
    pub fn new(secret: &str) -> Self {
        Self {
            secret: secret.to_string(),
            valid_tokens: RwLock::new(HashSet::new()),
        }
    }

    /// Generate and register a token for `client_id`.
    pub fn generate_token(&self, client_id: &str) -> String {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(self.secret.as_bytes());
        hasher.update(b":");
        hasher.update(client_id.as_bytes());
        let hash = hasher.finalize();
        let token = hex::encode(hash);

        if let Ok(mut tokens) = self.valid_tokens.write() {
            tokens.insert(token.clone());
        }

        token
    }

    /// Register an externally-created token
    pub fn add_token(&self, token: &str) {
        if let Ok(mut tokens) = self.valid_tokens.write() {
            tokens.insert(token.to_string());
        }
    }

    /// Remove a token so it can no longer authenticate
    pub fn revoke_token(&self, token: &str) {
        if let Ok(mut tokens) = self.valid_tokens.write() {
            tokens.remove(token);
        }
    }

    /// Returns `true` if the token is currently valid
    pub fn is_valid_token(&self, token: &str) -> bool {
        self.valid_tokens
            .read()
            .map(|tokens| tokens.contains(token))
            .unwrap_or(false)
    }
}

#[async_trait]
impl AuthProvider for TokenAuthProvider {
    fn is_enabled(&self) -> bool {
        true
    }

    async fn validate(&self, token: &str) -> Result<AuthInfo, String> {
        if token.is_empty() {
            return Err("empty token".to_string());
        }

        if self.is_valid_token(token) {
            Ok(AuthInfo {
                client_id: format!("token-{}", &token[..8.min(token.len())]),
                permissions: vec!["*".to_string()],
            })
        } else {
            Err("invalid token".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_noop_auth_always_succeeds() {
        let provider = NoopAuthProvider;
        assert!(!provider.is_enabled());

        let result = provider.validate("anything").await;
        assert!(result.is_ok());
        let auth_info = result.unwrap();
        assert_eq!(auth_info.client_id, "anonymous");
        assert_eq!(auth_info.permissions, vec!["read:metrics".to_string()]);
    }

    #[tokio::test]
    async fn test_token_auth_valid() {
        let provider = TokenAuthProvider::new("test-secret");
        let token = provider.generate_token("client-1");

        assert!(provider.is_enabled());
        assert!(provider.is_valid_token(&token));

        let result = provider.validate(&token).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_token_auth_invalid() {
        let provider = TokenAuthProvider::new("test-secret");
        let _token = provider.generate_token("client-1");

        let result = provider.validate("bad-token").await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "invalid token");
    }

    #[tokio::test]
    async fn test_token_auth_empty() {
        let provider = TokenAuthProvider::new("test-secret");

        let result = provider.validate("").await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "empty token");
    }

    #[tokio::test]
    async fn test_token_revocation() {
        let provider = TokenAuthProvider::new("test-secret");
        let token = provider.generate_token("client-1");
        assert!(provider.is_valid_token(&token));

        provider.revoke_token(&token);
        assert!(!provider.is_valid_token(&token));

        let result = provider.validate(&token).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_add_external_token() {
        let provider = TokenAuthProvider::new("test-secret");
        provider.add_token("external-token-123");
        assert!(provider.is_valid_token("external-token-123"));
    }
}
