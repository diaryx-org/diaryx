//! Generic proxy types and utilities.
//!
//! The proxy system lets plugins call external APIs through the server,
//! with credential management, tier gating, rate limiting, and usage metering.
//!
//! Three credential tiers:
//! - **Platform**: Server-managed key (env var), gated by billing tier
//! - **User**: User-provided key (per-user secret store)
//! - **Developer**: HMAC-signed requests, developer holds the real API key

use crate::domain::UserTier;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// How the proxy authenticates with the upstream API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProxyAuthMethod {
    /// Key from server env/secrets. Gated by billing tier.
    PlatformSecret {
        env_key: String,
        auth_header: String,
        auth_prefix: String,
        required_tier: UserTier,
    },
    /// Key from user's per-user secret store.
    UserSecret {
        secret_key: String,
        auth_header: String,
        auth_prefix: String,
    },
    /// Server signs request with shared HMAC secret; developer's proxy validates.
    HmacSigned { hmac_secret_env: String },
}

/// Validation rules applied to the request body before forwarding.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyValidation {
    /// Allowed values for specific JSON body fields (e.g., `{"model": ["gpt-4", "claude-3"]}`).
    #[serde(default)]
    pub allowed_values: HashMap<String, Vec<String>>,
    /// Maximum request body size in bytes.
    pub max_body_bytes: Option<usize>,
}

/// Configuration for a registered proxy endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub proxy_id: String,
    pub upstream: String,
    pub auth_method: ProxyAuthMethod,
    #[serde(default)]
    pub allowed_paths: Option<Vec<String>>,
    pub rate_limit_per_minute: Option<u32>,
    pub monthly_quota: Option<u64>,
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub validation: Option<ProxyValidation>,
}

/// A resolved proxy request ready for the transport layer to execute.
pub struct ProxyForward {
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub streaming: bool,
}

/// Result of proxy resolution.
pub enum ProxyResult {
    Forward(ProxyForward),
    Rejected {
        status: u16,
        code: String,
        message: String,
    },
}

/// HMAC-sign a proxy request for the developer tier.
///
/// The signature covers `"{timestamp}\n{user_id}\n{body_sha256}"`.
pub fn sign_proxy_request(
    hmac_secret: &[u8],
    timestamp: u64,
    user_id: &str,
    body: &[u8],
) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let body_hash = {
        use sha2::Digest;
        let mut hasher = Sha256::new();
        hasher.update(body);
        hex::encode(hasher.finalize())
    };

    let message = format!("{}\n{}\n{}", timestamp, user_id, body_hash);

    let mut mac =
        Hmac::<Sha256>::new_from_slice(hmac_secret).expect("HMAC can take key of any size");
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Verify a proxy request signature.
pub fn verify_proxy_signature(
    hmac_secret: &[u8],
    timestamp: u64,
    user_id: &str,
    body: &[u8],
    signature: &str,
) -> bool {
    let expected = sign_proxy_request(hmac_secret, timestamp, user_id, body);
    // Constant-time comparison
    expected.len() == signature.len()
        && expected
            .bytes()
            .zip(signature.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_sign_verify_roundtrip() {
        let secret = b"test-secret-key";
        let timestamp = 1711000000u64;
        let user_id = "user-123";
        let body = b"hello world";

        let sig = sign_proxy_request(secret, timestamp, user_id, body);
        assert!(verify_proxy_signature(
            secret, timestamp, user_id, body, &sig
        ));
    }

    #[test]
    fn hmac_verify_rejects_wrong_secret() {
        let secret = b"test-secret-key";
        let wrong_secret = b"wrong-secret";
        let sig = sign_proxy_request(secret, 1711000000, "user-123", b"body");
        assert!(!verify_proxy_signature(
            wrong_secret,
            1711000000,
            "user-123",
            b"body",
            &sig
        ));
    }

    #[test]
    fn hmac_verify_rejects_tampered_body() {
        let secret = b"test-secret-key";
        let sig = sign_proxy_request(secret, 1711000000, "user-123", b"original");
        assert!(!verify_proxy_signature(
            secret,
            1711000000,
            "user-123",
            b"tampered",
            &sig
        ));
    }
}
