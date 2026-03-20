use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenClaims {
    #[serde(rename = "s")]
    pub slug: String,
    #[serde(rename = "a")]
    pub audience: String,
    #[serde(rename = "t")]
    pub token_id: String,
    #[serde(rename = "e")]
    pub expires_at: Option<i64>,
}

pub fn create_signed_token(
    signing_key: &[u8],
    slug: &str,
    audience: &str,
    token_id: &str,
    expires_at: Option<i64>,
) -> Result<String, String> {
    let claims = TokenClaims {
        slug: slug.to_string(),
        audience: audience.to_string(),
        token_id: token_id.to_string(),
        expires_at,
    };
    let payload = serde_json::to_vec(&claims)
        .map_err(|e| format!("failed to serialize token payload: {}", e))?;

    let mut mac =
        HmacSha256::new_from_slice(signing_key).map_err(|e| format!("invalid key: {}", e))?;
    mac.update(&payload);
    let signature = mac.finalize().into_bytes();

    let payload_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
    let signature_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature);
    Ok(format!("{}.{}", payload_b64, signature_b64))
}

pub fn validate_signed_token(signing_key: &[u8], token_string: &str) -> Option<TokenClaims> {
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

    let claims: TokenClaims = serde_json::from_slice(&payload).ok()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trip_and_tamper_detection() {
        let key = [7u8; 32];
        let token = create_signed_token(&key, "demo", "family", "tok-1", None).unwrap();
        let claims = validate_signed_token(&key, &token).expect("claims");
        assert_eq!(claims.slug, "demo");
        assert_eq!(claims.audience, "family");

        let tampered = format!("{}x", token);
        assert!(validate_signed_token(&key, &tampered).is_none());
    }
}
