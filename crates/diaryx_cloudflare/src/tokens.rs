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

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::{AudienceTokenClaims, validate_audience_token};
    use base64::Engine;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use wasm_bindgen_test::wasm_bindgen_test;

    fn signed_token(claims: &AudienceTokenClaims, signing_key: &[u8]) -> String {
        let payload = serde_json::to_vec(claims).expect("serialize claims");
        let mut mac = Hmac::<Sha256>::new_from_slice(signing_key).expect("hmac key");
        mac.update(&payload);
        let signature = mac.finalize().into_bytes();

        let payload_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
        let signature_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature);
        format!("{payload_b64}.{signature_b64}")
    }

    #[wasm_bindgen_test]
    fn validates_non_expired_tokens() {
        let key = b"cloudflare-audience-key";
        let token = signed_token(
            &AudienceTokenClaims {
                slug: "workspace:alpha".to_string(),
                audience: "members".to_string(),
                token_id: "tok-1".to_string(),
                expires_at: Some(chrono::Utc::now().timestamp() + 60),
            },
            key,
        );

        let claims = validate_audience_token(key, &token).expect("valid token");
        assert_eq!(claims.slug, "workspace:alpha");
        assert_eq!(claims.audience, "members");
        assert_eq!(claims.token_id, "tok-1");
    }

    #[wasm_bindgen_test]
    fn rejects_tampered_or_expired_tokens() {
        let key = b"cloudflare-audience-key";
        let expired = signed_token(
            &AudienceTokenClaims {
                slug: "workspace:alpha".to_string(),
                audience: "members".to_string(),
                token_id: "tok-2".to_string(),
                expires_at: Some(chrono::Utc::now().timestamp() - 1),
            },
            key,
        );
        assert!(validate_audience_token(key, &expired).is_none());

        let valid = signed_token(
            &AudienceTokenClaims {
                slug: "workspace:alpha".to_string(),
                audience: "members".to_string(),
                token_id: "tok-3".to_string(),
                expires_at: None,
            },
            key,
        );
        let tampered = format!("{valid}x");
        assert!(validate_audience_token(key, &tampered).is_none());
    }

    #[wasm_bindgen_test]
    fn rejects_blank_claims() {
        let key = b"cloudflare-audience-key";
        let token = signed_token(
            &AudienceTokenClaims {
                slug: "   ".to_string(),
                audience: "members".to_string(),
                token_id: "tok-4".to_string(),
                expires_at: None,
            },
            key,
        );

        assert!(validate_audience_token(key, &token).is_none());
    }
}
