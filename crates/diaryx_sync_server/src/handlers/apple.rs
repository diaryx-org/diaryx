use crate::adapters::NativeBillingStore;
use crate::auth::RequireAuth;
use crate::config::AppleIapConfig;
use crate::db::AuthRepo;
use axum::{
    Json, Router, body::Bytes, extract::State, http::StatusCode, response::IntoResponse,
    routing::post,
};
use base64::Engine;
use diaryx_server::UserTier;
use diaryx_server::api::billing::{
    AppleRestoreRequest, AppleRestoreResponse, AppleVerifyReceiptRequest,
    AppleVerifyReceiptResponse,
};
use diaryx_server::ports::UserStore;
use diaryx_server::use_cases::billing::BillingService;
use serde::Deserialize;
use std::sync::Arc;
use tracing::{info, warn};

// Apple Root CA - G3 certificate (DER, embedded)
const APPLE_ROOT_CA_G3_DER: &[u8] = include_bytes!("apple_root_ca_g3.der");

#[derive(Clone)]
pub struct AppleIapState {
    pub repo: Arc<AuthRepo>,
    pub user_store: Arc<dyn UserStore>,
    pub config: AppleIapConfig,
}

pub fn apple_iap_routes(state: AppleIapState) -> Router {
    Router::new()
        .route("/apple/verify-receipt", post(verify_receipt))
        .route("/apple/restore", post(restore_purchases))
        .route("/apple/webhook", post(handle_webhook))
        .with_state(state)
}

/// Decoded payload from an Apple StoreKit 2 signed transaction (JWS).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransactionPayload {
    original_transaction_id: String,
    product_id: String,
    /// UUID linking the purchase to a Diaryx user account.
    app_account_token: Option<String>,
    /// Subscription expiry (ms since epoch).
    expires_date: Option<u64>,
    /// If set, the transaction was revoked.
    revocation_date: Option<u64>,
    bundle_id: String,
    /// "Sandbox" or "Production"
    #[serde(default)]
    environment: String,
}

// ============================================================================
// Handlers
// ============================================================================

/// POST /api/apple/verify-receipt — Verify a StoreKit 2 JWS signed transaction
/// and upgrade the user to Plus.
async fn verify_receipt(
    State(state): State<AppleIapState>,
    RequireAuth(auth): RequireAuth,
    Json(body): Json<AppleVerifyReceiptRequest>,
) -> impl IntoResponse {
    let user_id = &auth.user.id;

    // Verify and decode the signed transaction
    let payload = match verify_and_decode_transaction(&body.signed_transaction, &state.config) {
        Ok(p) => p,
        Err(msg) => {
            warn!(
                "Apple JWS verification failed for user {}: {}",
                user_id, msg
            );
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    };

    // Validate product ID matches
    if payload.product_id != body.product_id {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Product ID mismatch" })),
        )
            .into_response();
    }

    // Validate bundle ID
    if payload.bundle_id != state.config.bundle_id {
        warn!(
            "Bundle ID mismatch: expected {}, got {}",
            state.config.bundle_id, payload.bundle_id
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Bundle ID mismatch" })),
        )
            .into_response();
    }

    // Validate appAccountToken matches authenticated user (if present)
    if let Some(ref token) = payload.app_account_token
        && token != user_id
    {
        warn!(
            "appAccountToken mismatch: expected {}, got {}",
            user_id, token
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Account token mismatch" })),
        )
            .into_response();
    }

    // Check subscription is active
    if payload.revocation_date.is_some() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Transaction has been revoked" })),
        )
            .into_response();
    }

    if let Some(expires) = payload.expires_date {
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        if expires < now_ms {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Subscription has expired" })),
            )
                .into_response();
        }
    }

    // Use BillingService to store transaction and upgrade tier
    let billing_store = NativeBillingStore::new(state.repo.clone());
    let billing = BillingService::new(&billing_store, state.user_store.as_ref());

    match billing
        .activate_apple_transaction(user_id, &payload.original_transaction_id)
        .await
    {
        Ok(()) => Json(AppleVerifyReceiptResponse {
            success: true,
            tier: "plus".to_string(),
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/apple/restore — Verify multiple transactions from a restore flow.
async fn restore_purchases(
    State(state): State<AppleIapState>,
    RequireAuth(auth): RequireAuth,
    Json(body): Json<AppleRestoreRequest>,
) -> impl IntoResponse {
    let user_id = &auth.user.id;
    let mut restored_count = 0usize;
    let mut best_tier = UserTier::Free;

    let billing_store = NativeBillingStore::new(state.repo.clone());
    let billing = BillingService::new(&billing_store, state.user_store.as_ref());

    for signed_tx in &body.signed_transactions {
        let payload = match verify_and_decode_transaction(signed_tx, &state.config) {
            Ok(p) => p,
            Err(msg) => {
                warn!(
                    "Apple restore: skipping invalid transaction for user {}: {}",
                    user_id, msg
                );
                continue;
            }
        };

        // Skip revoked
        if payload.revocation_date.is_some() {
            continue;
        }

        // Skip expired
        if let Some(expires) = payload.expires_date {
            let now_ms = chrono::Utc::now().timestamp_millis() as u64;
            if expires < now_ms {
                continue;
            }
        }

        // Validate bundle ID
        if payload.bundle_id != state.config.bundle_id {
            continue;
        }

        // Store transaction via BillingService
        if let Err(e) = billing
            .activate_apple_transaction(user_id, &payload.original_transaction_id)
            .await
        {
            warn!("Failed to activate Apple transaction during restore: {}", e);
            continue;
        }

        best_tier = UserTier::Plus;
        restored_count += 1;
    }

    if best_tier == UserTier::Plus {
        info!(
            "User {} restored {} Apple IAP transaction(s), upgraded to Plus",
            user_id, restored_count
        );
    }

    Json(AppleRestoreResponse {
        success: true,
        restored_count,
        tier: best_tier.as_str().to_string(),
    })
    .into_response()
}

/// POST /api/apple/webhook — App Store Server Notifications V2.
async fn handle_webhook(body: Bytes) -> impl IntoResponse {
    let payload_str = std::str::from_utf8(&body).unwrap_or("<binary>");
    info!(
        "Received Apple App Store Server Notification ({} bytes)",
        body.len()
    );
    let _ = payload_str;
    StatusCode::OK
}

// ============================================================================
// JWS Verification
// ============================================================================

/// Verify an Apple StoreKit 2 JWS signed transaction and decode its payload.
fn verify_and_decode_transaction(
    jws: &str,
    config: &AppleIapConfig,
) -> Result<TransactionPayload, String> {
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let parts: Vec<&str> = jws.split('.').collect();
    if parts.len() != 3 {
        return Err("Invalid JWS format: expected 3 parts".to_string());
    }

    if config.skip_signature_verify {
        warn!("APPLE_IAP_SKIP_SIGNATURE_VERIFY is enabled — skipping JWS verification");

        let payload_bytes = b64
            .decode(parts[1])
            .map_err(|e| format!("Failed to decode JWS payload: {}", e))?;
        let payload: TransactionPayload = serde_json::from_slice(&payload_bytes)
            .map_err(|e| format!("Failed to parse JWS payload: {}", e))?;
        return Ok(payload);
    }

    // Decode header to get x5c chain
    let header_bytes = b64
        .decode(parts[0])
        .map_err(|e| format!("Failed to decode JWS header: {}", e))?;
    let header: serde_json::Value = serde_json::from_slice(&header_bytes)
        .map_err(|e| format!("Failed to parse JWS header: {}", e))?;

    let alg = header["alg"].as_str().unwrap_or("");
    if alg != "ES256" {
        return Err(format!("Unsupported JWS algorithm: {}", alg));
    }

    let x5c = header["x5c"]
        .as_array()
        .ok_or_else(|| "Missing x5c in JWS header".to_string())?;

    if x5c.is_empty() {
        return Err("Empty x5c certificate chain".to_string());
    }

    // Decode certificates from x5c (base64-encoded DER)
    let std_b64 = base64::engine::general_purpose::STANDARD;
    let mut certs_der = Vec::new();
    for cert_b64 in x5c {
        let cert_str = cert_b64
            .as_str()
            .ok_or_else(|| "x5c entry is not a string".to_string())?;
        let cert_bytes = std_b64
            .decode(cert_str)
            .map_err(|e| format!("Failed to decode x5c certificate: {}", e))?;
        certs_der.push(cert_bytes);
    }

    // Verify the certificate chain terminates at Apple Root CA-G3
    verify_certificate_chain(&certs_der)?;

    // Extract the leaf certificate's public key for JWS signature verification
    let leaf_cert = x509_parser::parse_x509_certificate(&certs_der[0])
        .map_err(|e| format!("Failed to parse leaf certificate: {}", e))?
        .1;

    let public_key_der = leaf_cert.public_key().subject_public_key.data.clone();

    // Verify the JWS signature using jsonwebtoken
    let decoding_key = jsonwebtoken::DecodingKey::from_ec_der(&public_key_der);
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::ES256);
    validation.validate_exp = false;
    validation.required_spec_claims.clear();

    let token_data = jsonwebtoken::decode::<TransactionPayload>(jws, &decoding_key, &validation)
        .map_err(|e| format!("JWS signature verification failed: {}", e))?;

    Ok(token_data.claims)
}

/// Verify that a certificate chain (leaf → intermediate → ... → root) is valid
/// and terminates at the embedded Apple Root CA-G3 certificate.
fn verify_certificate_chain(certs_der: &[Vec<u8>]) -> Result<(), String> {
    if certs_der.is_empty() {
        return Err("Empty certificate chain".to_string());
    }

    // Parse the embedded Apple Root CA
    let (_, apple_root) = x509_parser::parse_x509_certificate(APPLE_ROOT_CA_G3_DER)
        .map_err(|e| format!("Failed to parse embedded Apple Root CA: {}", e))?;

    let last_cert_der = &certs_der[certs_der.len() - 1];
    let (_, last_cert) = x509_parser::parse_x509_certificate(last_cert_der)
        .map_err(|e| format!("Failed to parse last certificate in chain: {}", e))?;

    if last_cert.issuer() != apple_root.subject() {
        return Err("Certificate chain does not terminate at Apple Root CA-G3".to_string());
    }

    // Verify each cert in the chain is signed by the next
    for i in 0..certs_der.len().saturating_sub(1) {
        let (_, cert) = x509_parser::parse_x509_certificate(&certs_der[i])
            .map_err(|e| format!("Failed to parse certificate {}: {}", i, e))?;
        let (_, issuer_cert) = x509_parser::parse_x509_certificate(&certs_der[i + 1])
            .map_err(|e| format!("Failed to parse certificate {}: {}", i + 1, e))?;

        cert.verify_signature(Some(issuer_cert.public_key()))
            .map_err(|e| format!("Certificate {} signature verification failed: {}", i, e))?;
    }

    // Verify the last cert is signed by the Apple Root CA
    last_cert
        .verify_signature(Some(apple_root.public_key()))
        .map_err(|e| format!("Root CA signature verification failed: {}", e))?;

    Ok(())
}
