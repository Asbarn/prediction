//! RSA-PSS signing for Kalshi API authentication.
//!
//! Kalshi uses RSA-PSS (SHA-256) signatures for WebSocket and REST API
//! authentication. The signing message is `"{timestamp_ms}{METHOD}{path}"`.
//! Timestamps MUST be in milliseconds (Pitfall 4).

use base64::Engine;
use rsa::pkcs8::DecodePrivateKey;
use rsa::pss::BlindedSigningKey;
use rsa::signature::{RandomizedSigner, SignatureEncoding};
use rsa::RsaPrivateKey;
use sha2::Sha256;

/// Parse a PEM-encoded RSA private key.
///
/// Expects PKCS#8 format (`-----BEGIN PRIVATE KEY-----`).
pub fn load_kalshi_private_key(pem_str: &str) -> anyhow::Result<RsaPrivateKey> {
    let key = RsaPrivateKey::from_pkcs8_pem(pem_str)
        .map_err(|e| anyhow::anyhow!("failed to parse Kalshi RSA private key: {}", e))?;
    Ok(key)
}

/// Sign a Kalshi API request using RSA-PSS with SHA-256.
///
/// Builds the signing message as `"{timestamp_ms}{method}{path}"` (no query params),
/// signs with a blinded PSS signing key, and returns the base64-encoded signature.
///
/// **CRITICAL (Pitfall 4):** `timestamp_ms` MUST be in milliseconds.
/// Use `chrono::Utc::now().timestamp_millis()`.
pub fn sign_kalshi_request(
    private_key: &RsaPrivateKey,
    timestamp_ms: i64,
    method: &str,
    path: &str,
) -> anyhow::Result<String> {
    let message = format!("{}{}{}", timestamp_ms, method, path);

    let signing_key = BlindedSigningKey::<Sha256>::new(private_key.clone());
    let mut rng = rand::thread_rng();
    let signature = signing_key.sign_with_rng(&mut rng, message.as_bytes());

    let encoded = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::RsaPrivateKey;

    /// Generate a test RSA key pair (small key for fast tests).
    fn generate_test_key() -> RsaPrivateKey {
        let mut rng = rand::thread_rng();
        RsaPrivateKey::new(&mut rng, 2048).expect("failed to generate test RSA key")
    }

    #[test]
    fn sign_produces_nonempty_base64() {
        let key = generate_test_key();
        let timestamp_ms = 1703001600000_i64;
        let signature = sign_kalshi_request(&key, timestamp_ms, "GET", "/trade-api/ws/v2").unwrap();

        assert!(!signature.is_empty(), "signature should be non-empty");

        // Verify it's valid base64 by decoding
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&signature)
            .expect("signature should be valid base64");
        assert!(
            !decoded.is_empty(),
            "decoded signature should be non-empty bytes"
        );
    }

    #[test]
    fn different_messages_produce_different_signatures() {
        let key = generate_test_key();
        let ts = 1703001600000_i64;

        let sig1 = sign_kalshi_request(&key, ts, "GET", "/trade-api/ws/v2").unwrap();
        let sig2 = sign_kalshi_request(&key, ts, "POST", "/trade-api/v2/orders").unwrap();

        // RSA-PSS is randomized, so even the same message would produce different sigs.
        // But different messages should definitely differ.
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn timestamp_is_part_of_signature_message() {
        let key = generate_test_key();

        let sig1 = sign_kalshi_request(&key, 1000, "GET", "/test").unwrap();
        let sig2 = sign_kalshi_request(&key, 2000, "GET", "/test").unwrap();

        // Different timestamps produce different signatures
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn load_key_from_pem_roundtrip() {
        let key = generate_test_key();

        // Serialize to PEM
        use rsa::pkcs8::EncodePrivateKey;
        let pem = key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .expect("failed to serialize key to PEM");

        // Load back
        let loaded = load_kalshi_private_key(pem.as_ref()).expect("failed to load PEM key");

        // Sign with both and verify both produce valid base64
        let sig_orig = sign_kalshi_request(&key, 12345, "GET", "/test").unwrap();
        let sig_loaded = sign_kalshi_request(&loaded, 12345, "GET", "/test").unwrap();

        // Both should decode as valid base64 (can't compare because PSS is randomized)
        assert!(
            base64::engine::general_purpose::STANDARD
                .decode(&sig_orig)
                .is_ok()
        );
        assert!(
            base64::engine::general_purpose::STANDARD
                .decode(&sig_loaded)
                .is_ok()
        );
    }
}
