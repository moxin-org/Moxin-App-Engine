//! Cryptographic signature verification for Skills.
//!
//! Provides a `TrustStore` that holds a set of trusted Ed25519 public keys
//! and verifies that skill payloads are signed by one of those keys.
//!
//! **Security policy:** when the trust store contains no keys, *all* signed
//! skills are **rejected**. This prevents an attacker from generating their
//! own keypair, self-signing a malicious SKILL.md, and bypassing verification
//! simply because the operator has not yet configured any trusted keys.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use mofa_kernel::plugin::{PluginError, PluginResult};
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

/// A store of trusted Ed25519 public keys (base64-encoded).
///
/// Only skills signed by a key present in this store will pass verification.
/// An empty store rejects **all** signed skills by design.
#[derive(Debug, Clone)]
pub struct TrustStore {
    trusted_keys: Arc<RwLock<HashSet<String>>>,
}

impl TrustStore {
    /// Create a new, empty trust store.
    ///
    /// **Important:** an empty trust store rejects all signatures. You must
    /// call [`add_key`] with at least one trusted public key before any
    /// signed skill can be loaded.
    pub fn new() -> Self {
        Self {
            trusted_keys: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Create a trust store pre-populated with the given keys.
    pub fn with_keys(keys: impl IntoIterator<Item = String>) -> Self {
        Self {
            trusted_keys: Arc::new(RwLock::new(keys.into_iter().collect())),
        }
    }

    /// Add a trusted public key (base64-encoded).
    pub fn add_key(&self, key_b64: String) {
        if let Ok(mut keys) = self.trusted_keys.write() {
            keys.insert(key_b64);
        }
    }

    /// Returns `true` if no trusted keys have been registered.
    pub fn is_empty(&self) -> bool {
        self.trusted_keys
            .read()
            .map(|keys| keys.is_empty())
            .unwrap_or(true)
    }

    /// Verify a skill payload against an Ed25519 signature.
    ///
    /// # Arguments
    ///
    /// * `content`        – the raw markdown body that was signed
    /// * `signature_b64`  – base64-encoded Ed25519 signature
    /// * `signer_key_b64` – base64-encoded Ed25519 public key of the signer
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - the trust store is empty (no trusted keys configured)
    /// - the signer key is not in the trust store
    /// - the base64 decoding fails
    /// - the cryptographic verification fails
    pub fn verify(
        &self,
        content: &str,
        signature_b64: &str,
        signer_key_b64: &str,
    ) -> PluginResult<()> {
        // ── 1. Reject if no trusted keys are configured ──────────────
        // This is the critical security check: an empty trust store must
        // NOT accept arbitrary self-signed skills.
        let keys = self
            .trusted_keys
            .read()
            .map_err(|e| PluginError::ExecutionFailed(format!("TrustStore lock poisoned: {}", e)))?;

        if keys.is_empty() {
            return Err(PluginError::ExecutionFailed(
                "TrustStore has no trusted keys configured; all signed skills are rejected. \
                 Add at least one trusted public key via TrustStore::add_key()."
                    .to_string(),
            ));
        }

        // ── 2. Verify the signer key is trusted ─────────────────────
        if !keys.contains(signer_key_b64) {
            return Err(PluginError::ExecutionFailed(format!(
                "Signer key is not in the trust store: {}",
                signer_key_b64
            )));
        }

        // Drop the lock before doing expensive crypto work.
        drop(keys);

        // ── 3. Decode base64 values ─────────────────────────────────
        use base64::Engine;
        use base64::engine::general_purpose::STANDARD;

        let sig_bytes = STANDARD.decode(signature_b64).map_err(|e| {
            PluginError::ExecutionFailed(format!("Invalid base64 signature: {}", e))
        })?;

        let key_bytes = STANDARD.decode(signer_key_b64).map_err(|e| {
            PluginError::ExecutionFailed(format!("Invalid base64 public key: {}", e))
        })?;

        // ── 4. Reconstruct cryptographic types ──────────────────────
        let key_array: [u8; 32] = key_bytes.as_slice().try_into().map_err(|_| {
            PluginError::ExecutionFailed(format!(
                "Public key must be exactly 32 bytes, got {}",
                key_bytes.len()
            ))
        })?;

        let verifying_key = VerifyingKey::from_bytes(&key_array).map_err(|e| {
            PluginError::ExecutionFailed(format!("Invalid Ed25519 public key: {}", e))
        })?;

        let signature = Signature::from_slice(&sig_bytes).map_err(|e| {
            PluginError::ExecutionFailed(format!("Invalid Ed25519 signature: {}", e))
        })?;

        // ── 5. Verify ───────────────────────────────────────────────
        verifying_key
            .verify(content.as_bytes(), &signature)
            .map_err(|e| {
                PluginError::ExecutionFailed(format!("Signature verification failed: {}", e))
            })?;

        Ok(())
    }
}

impl Default for TrustStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    /// Helper: generate a keypair, sign content, return (signing_key, sig_b64, key_b64).
    fn sign_content(content: &str) -> (SigningKey, String, String) {
        use base64::Engine;
        use base64::engine::general_purpose::STANDARD;
        use ed25519_dalek::Signer;

        // Generate 32 random bytes for the secret key using rand 0.8
        let mut secret = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut secret);
        let signing_key = SigningKey::from_bytes(&secret);

        let signature = signing_key.sign(content.as_bytes());
        let sig_b64 = STANDARD.encode(signature.to_bytes());
        let key_b64 = STANDARD.encode(signing_key.verifying_key().to_bytes());
        (signing_key, sig_b64, key_b64)
    }

    #[test]
    fn empty_trust_store_rejects_all_signatures() {
        let store = TrustStore::new();
        let content = "# Test Skill";
        let (_sk, sig, key) = sign_content(content);

        let result = store.verify(content, &sig, &key);
        assert!(result.is_err(), "Empty trust store must reject all signatures");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("no trusted keys configured"),
            "Error should mention missing keys: {err_msg}"
        );
    }

    #[test]
    fn valid_signature_with_trusted_key_passes() {
        let store = TrustStore::new();
        let content = "# Test Skill\nSome instructions here.";
        let (_sk, sig, key) = sign_content(content);

        store.add_key(key.clone());
        let result = store.verify(content, &sig, &key);
        assert!(result.is_ok(), "Valid signature with trusted key should pass");
    }

    #[test]
    fn untrusted_key_is_rejected() {
        let store = TrustStore::new();
        let content = "# Test Skill";
        let (_sk, sig, key) = sign_content(content);

        // Add a *different* key to the store
        store.add_key("SomeOtherKeyThatIsNotTheSignersKey".to_string());

        let result = store.verify(content, &sig, &key);
        assert!(result.is_err(), "Untrusted signer key should be rejected");
    }

    #[test]
    fn tampered_content_is_rejected() {
        let store = TrustStore::new();
        let content = "# Test Skill\nOriginal content.";
        let (_sk, sig, key) = sign_content(content);

        store.add_key(key.clone());

        let tampered = "# Test Skill\nMalicious content injected!";
        let result = store.verify(tampered, &sig, &key);
        assert!(
            result.is_err(),
            "Tampered content must fail signature verification"
        );
    }
}
