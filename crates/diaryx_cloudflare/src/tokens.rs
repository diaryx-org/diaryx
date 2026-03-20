//! HMAC-SHA256 audience token validation.
//!
//! Same format as the native server's `tokens.rs` — payload.signature,
//! both URL-safe base64 encoded.

use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudienceTokenClaims {
    #[serde(rename = "s")]
    pub slug: String,
    #[serde(rename = "a")]
    pub audience: String,
    #[serde(rename = "t")]
    pub token_id: String,
    #[serde(rename = "e")]
    pub expires_at: Option<i64>,
}

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

    if let Some(expires_at) = claims.expires_at
        && expires_at < chrono::Utc::now().timestamp()
    {
        return None;
    }

    Some(claims)
}
