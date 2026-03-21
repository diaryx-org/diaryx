//! Billing API request/response types (Stripe + Apple IAP).

use serde::{Deserialize, Serialize};

// ============================================================================
// Shared
// ============================================================================

/// Generic `{ "url": "..." }` response used by checkout/portal endpoints.
#[derive(Debug, Serialize, Deserialize)]
pub struct UrlResponse {
    pub url: String,
}

/// Generic `{ "tier": "..." }` info included in billing responses.
#[derive(Debug, Serialize, Deserialize)]
pub struct TierResponse {
    pub tier: String,
}

// ============================================================================
// Stripe
// ============================================================================

/// Response from GET /api/stripe/config.
#[derive(Debug, Serialize, Deserialize)]
pub struct StripeConfigResponse {
    pub publishable_key: String,
}

// ============================================================================
// Apple IAP
// ============================================================================

/// POST /api/apple/verify-receipt
#[derive(Debug, Deserialize)]
pub struct AppleVerifyReceiptRequest {
    pub signed_transaction: String,
    pub product_id: String,
}

/// Response from POST /api/apple/verify-receipt.
#[derive(Debug, Serialize, Deserialize)]
pub struct AppleVerifyReceiptResponse {
    pub success: bool,
    pub tier: String,
}

/// POST /api/apple/restore
#[derive(Debug, Deserialize)]
pub struct AppleRestoreRequest {
    pub signed_transactions: Vec<String>,
}

/// Response from POST /api/apple/restore.
#[derive(Debug, Serialize, Deserialize)]
pub struct AppleRestoreResponse {
    pub success: bool,
    pub restored_count: usize,
    pub tier: String,
}
