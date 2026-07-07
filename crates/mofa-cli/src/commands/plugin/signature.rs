//! Ed25519 signature verification for plugin integrity.
//!
//! Publishers sign a canonical byte payload with their Ed25519 private key.
//! The CLI verifies the signature using the publisher's base64-encoded public key
//! stored in the plugin catalog or manifest.
//!
//! ## Signing payload
//!
//! For registry plugins the payload is:
//!   `<plugin_id>:<kind>:<sha256_of_config_json>`
//!
//! For downloaded plugins the payload is:
//!   `<plugin_id>:<sha256_hex_of_content>`
//!
//! This ensures the signature covers both plugin identity and content integrity.
//!
//! ```text
//! Publisher                          CLI (install --verify-signature)
//!    │                                    │
//!    │  ed25519_sign(private_key, payload) │
//!    │ ──────────────────────────────────>│
//!    │                                    │ ed25519_verify(public_key, payload, sig)
//!    │                                    │ ✓ or ✗
//! ```

use crate::CliError;
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

/// Verify an Ed25519 signature over `message`.
///
/// - `public_key_b64`: standard base64-encoded 32-byte Ed25519 public key
/// - `message`: raw bytes that were signed
/// - `signature_b64`: standard base64-encoded 64-byte Ed25519 signature
pub fn verify(public_key_b64: &str, message: &[u8], signature_b64: &str) -> Result<(), CliError> {
    let key_bytes = B64.decode(public_key_b64).map_err(|e| {
        CliError::PluginError(format!("invalid public key encoding: {e}"))
    })?;

    let sig_bytes = B64.decode(signature_b64).map_err(|e| {
        CliError::PluginError(format!("invalid signature encoding: {e}"))
    })?;

    let key_array: [u8; 32] = key_bytes.try_into().map_err(|_| {
        CliError::PluginError("public key must be exactly 32 bytes".into())
    })?;

    let sig_array: [u8; 64] = sig_bytes.try_into().map_err(|_| {
        CliError::PluginError("signature must be exactly 64 bytes".into())
    })?;

    let verifying_key = VerifyingKey::from_bytes(&key_array).map_err(|e| {
        CliError::PluginError(format!("malformed public key: {e}"))
    })?;

    let signature = Signature::from_bytes(&sig_array);

    verifying_key.verify(message, &signature).map_err(|_| {
        CliError::PluginError(
            "signature verification failed — plugin may be tampered with or signed by an untrusted key".into(),
        )
    })
}

/// Build the canonical signing payload for a registry plugin entry.
///
/// Payload: `<id>:<kind>:<sha256_of_config_json>`
pub fn registry_payload(id: &str, kind: &str, config_json: &str) -> Vec<u8> {
    let config_hash = hex::encode(Sha256::digest(config_json.as_bytes()));
    format!("{id}:{kind}:{config_hash}").into_bytes()
}

/// Build the canonical signing payload for a downloaded plugin.
///
/// Payload: `<id>:<sha256_hex_of_content>`
pub fn download_payload(id: &str, sha256_hex: &str) -> Vec<u8> {
    format!("{id}:{sha256_hex}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    use ed25519_dalek::Signer;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn gen_keypair() -> (SigningKey, String, String) {
        let signing_key = SigningKey::generate(&mut OsRng);
        let pub_b64 = B64.encode(signing_key.verifying_key().as_bytes());
        (signing_key, pub_b64, String::new())
    }

    #[test]
    fn verify_valid_registry_signature() {
        let (sk, pub_b64, _) = gen_keypair();
        let payload = registry_payload("my-plugin", "builtin:http", r#"{"url":"https://x.com"}"#);
        let sig = sk.sign(&payload);
        let sig_b64 = B64.encode(sig.to_bytes());

        assert!(verify(&pub_b64, &payload, &sig_b64).is_ok());
    }

    #[test]
    fn verify_valid_download_signature() {
        let (sk, pub_b64, _) = gen_keypair();
        let payload = download_payload("my-plugin", "abc123deadbeef");
        let sig = sk.sign(&payload);
        let sig_b64 = B64.encode(sig.to_bytes());

        assert!(verify(&pub_b64, &payload, &sig_b64).is_ok());
    }

    #[test]
    fn reject_tampered_content() {
        let (sk, pub_b64, _) = gen_keypair();
        let original = registry_payload("plugin", "builtin:http", r#"{"url":"https://good.com"}"#);
        let sig = sk.sign(&original);
        let sig_b64 = B64.encode(sig.to_bytes());

        // tampered config
        let tampered = registry_payload("plugin", "builtin:http", r#"{"url":"https://evil.com"}"#);
        assert!(verify(&pub_b64, &tampered, &sig_b64).is_err());
    }

    #[test]
    fn reject_wrong_key() {
        let (sk, _, _) = gen_keypair();
        let (_, wrong_pub, _) = gen_keypair();

        let payload = registry_payload("plugin", "builtin:http", "{}");
        let sig = sk.sign(&payload);
        let sig_b64 = B64.encode(sig.to_bytes());

        assert!(verify(&wrong_pub, &payload, &sig_b64).is_err());
    }

    #[test]
    fn reject_invalid_key_length() {
        // 31 bytes instead of 32
        let short_key = B64.encode([0u8; 31]);
        let sig = B64.encode([0u8; 64]);
        assert!(verify(&short_key, b"data", &sig).is_err());
    }

    #[test]
    fn reject_invalid_sig_length() {
        let (_, pub_b64, _) = gen_keypair();
        let short_sig = B64.encode([0u8; 63]);
        assert!(verify(&pub_b64, b"data", &short_sig).is_err());
    }

    #[test]
    fn reject_malformed_base64() {
        assert!(verify("not-valid-base64!!!", b"data", "also-invalid").is_err());
    }

    #[test]
    fn registry_payload_is_deterministic() {
        let p1 = registry_payload("a", "b", "c");
        let p2 = registry_payload("a", "b", "c");
        assert_eq!(p1, p2);
    }

    #[test]
    fn download_payload_is_deterministic() {
        let p1 = download_payload("a", "deadbeef");
        let p2 = download_payload("a", "deadbeef");
        assert_eq!(p1, p2);
    }
}
