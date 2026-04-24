//! Shared audience-access token format (HMAC-SHA256 + base64url).
//!
//! Tokens are consumed by the site-proxy worker to decide whether a reader
//! may see a given audience's content. Both native sync-server adapters and
//! the Cloudflare worker use the same format so tokens minted in one place
//! validate in the other.
//!
//! Claim keys are single-character to keep tokens compact. A token carries:
//! - `s` — namespace slug.
//! - `a` — audience name.
//! - `t` — token id (UUID, mostly for observability).
//! - `g` — gate kind the token was issued for: `"link"` | `"unlock"`.
//! - `pv` — password-gate version (present only when `g == "unlock"`).
//! - `e` — optional unix-seconds expiry.

use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Gate kind this token was minted to satisfy. The worker uses this to look
/// up the matching gate on the audience's current gate set and decide whether
/// to grant access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    /// Magic-link bypass. Valid as long as the audience still has a `link`
    /// gate; no per-gate version check.
    Link,
    /// Password unlock. Carries a `pv` that must equal the password gate's
    /// current version; mismatch invalidates the token (supports rotation).
    Unlock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudienceTokenClaims {
    #[serde(rename = "s")]
    pub slug: String,
    #[serde(rename = "a")]
    pub audience: String,
    #[serde(rename = "t")]
    pub token_id: String,
    #[serde(rename = "g")]
    pub gate: GateKind,
    #[serde(rename = "pv", default, skip_serializing_if = "Option::is_none")]
    pub password_version: Option<u32>,
    #[serde(rename = "e", default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

/// Sign a set of claims into a `payload.signature` token string.
pub fn create_audience_token(
    signing_key: &[u8],
    claims: &AudienceTokenClaims,
) -> Result<String, String> {
    let payload = serde_json::to_vec(claims)
        .map_err(|e| format!("failed to serialize token payload: {}", e))?;

    let mut mac =
        HmacSha256::new_from_slice(signing_key).map_err(|e| format!("invalid key: {}", e))?;
    mac.update(&payload);
    let signature = mac.finalize().into_bytes();

    let payload_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
    let signature_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature);
    Ok(format!("{}.{}", payload_b64, signature_b64))
}

/// Validate a token string: checks signature, well-formedness, and expiry
/// (if present). The caller is still responsible for matching `slug` +
/// `audience` + per-gate version against the current audience record.
pub fn validate_audience_token(
    signing_key: &[u8],
    token_string: &str,
) -> Option<AudienceTokenClaims> {
    let (payload_b64, signature_b64) = token_string.split_once('.')?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64.as_bytes())
        .ok()?;
    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(signature_b64.as_bytes())
        .ok()?;

    let mut mac = HmacSha256::new_from_slice(signing_key).ok()?;
    mac.update(&payload);
    mac.verify_slice(&signature).ok()?;

    let claims: AudienceTokenClaims = serde_json::from_slice(&payload).ok()?;
    if claims.slug.trim().is_empty()
        || claims.audience.trim().is_empty()
        || claims.token_id.trim().is_empty()
    {
        return None;
    }

    // Unlock tokens must declare the password version they were minted under.
    if matches!(claims.gate, GateKind::Unlock) && claims.password_version.is_none() {
        return None;
    }

    if let Some(expires_at) = claims.expires_at
        && expires_at < chrono::Utc::now().timestamp()
    {
        return None;
    }

    Some(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_token_round_trip() {
        let key = [7u8; 32];
        let claims = AudienceTokenClaims {
            slug: "demo".to_string(),
            audience: "family".to_string(),
            token_id: "tok-1".to_string(),
            gate: GateKind::Link,
            password_version: None,
            expires_at: None,
        };
        let token = create_audience_token(&key, &claims).unwrap();
        let decoded = validate_audience_token(&key, &token).expect("claims");
        assert_eq!(decoded.slug, "demo");
        assert_eq!(decoded.audience, "family");
        assert!(matches!(decoded.gate, GateKind::Link));
        assert!(decoded.password_version.is_none());
    }

    #[test]
    fn unlock_token_carries_password_version() {
        let key = [7u8; 32];
        let claims = AudienceTokenClaims {
            slug: "demo".to_string(),
            audience: "inner".to_string(),
            token_id: "tok-2".to_string(),
            gate: GateKind::Unlock,
            password_version: Some(3),
            expires_at: None,
        };
        let token = create_audience_token(&key, &claims).unwrap();
        let decoded = validate_audience_token(&key, &token).expect("claims");
        assert!(matches!(decoded.gate, GateKind::Unlock));
        assert_eq!(decoded.password_version, Some(3));
    }

    #[test]
    fn unlock_token_without_version_is_rejected() {
        // Build a malformed token by hand: Unlock claims but no pv.
        let key = [7u8; 32];
        let malformed = AudienceTokenClaims {
            slug: "demo".to_string(),
            audience: "inner".to_string(),
            token_id: "tok-3".to_string(),
            gate: GateKind::Unlock,
            password_version: None,
            expires_at: None,
        };
        let token = create_audience_token(&key, &malformed).unwrap();
        // Token is signature-valid, but validator rejects Unlock with no pv.
        assert!(validate_audience_token(&key, &token).is_none());
    }

    #[test]
    fn tampered_token_fails_signature_check() {
        let key = [7u8; 32];
        let claims = AudienceTokenClaims {
            slug: "demo".to_string(),
            audience: "family".to_string(),
            token_id: "tok-1".to_string(),
            gate: GateKind::Link,
            password_version: None,
            expires_at: None,
        };
        let token = create_audience_token(&key, &claims).unwrap();
        let tampered = format!("{}x", token);
        assert!(validate_audience_token(&key, &tampered).is_none());
    }

    #[test]
    fn expired_token_rejected() {
        let key = [7u8; 32];
        let claims = AudienceTokenClaims {
            slug: "demo".to_string(),
            audience: "family".to_string(),
            token_id: "tok-1".to_string(),
            gate: GateKind::Link,
            password_version: None,
            expires_at: Some(0), // epoch
        };
        let token = create_audience_token(&key, &claims).unwrap();
        assert!(validate_audience_token(&key, &token).is_none());
    }

    #[test]
    fn blank_claims_rejected() {
        let key = [7u8; 32];
        let claims = AudienceTokenClaims {
            slug: "   ".to_string(),
            audience: "family".to_string(),
            token_id: "tok-1".to_string(),
            gate: GateKind::Link,
            password_version: None,
            expires_at: None,
        };
        let token = create_audience_token(&key, &claims).unwrap();
        assert!(validate_audience_token(&key, &token).is_none());
    }
}
