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

#[cfg(test)]
mod tests {
    use super::{
        AppleRestoreRequest, AppleRestoreResponse, AppleVerifyReceiptRequest,
        AppleVerifyReceiptResponse, StripeConfigResponse, TierResponse, UrlResponse,
    };

    #[test]
    fn billing_responses_serialize_expected_shapes() {
        let url = serde_json::to_value(UrlResponse {
            url: "https://billing.example.com/checkout".to_string(),
        })
        .unwrap();
        let tier = serde_json::to_value(TierResponse {
            tier: "plus".to_string(),
        })
        .unwrap();
        let stripe = serde_json::to_value(StripeConfigResponse {
            publishable_key: "pk_test_123".to_string(),
        })
        .unwrap();
        let verify = serde_json::to_value(AppleVerifyReceiptResponse {
            success: true,
            tier: "plus".to_string(),
        })
        .unwrap();
        let restore = serde_json::to_value(AppleRestoreResponse {
            success: true,
            restored_count: 2,
            tier: "plus".to_string(),
        })
        .unwrap();

        assert_eq!(
            url,
            serde_json::json!({ "url": "https://billing.example.com/checkout" })
        );
        assert_eq!(tier, serde_json::json!({ "tier": "plus" }));
        assert_eq!(
            stripe,
            serde_json::json!({ "publishable_key": "pk_test_123" })
        );
        assert_eq!(
            verify,
            serde_json::json!({ "success": true, "tier": "plus" })
        );
        assert_eq!(
            restore,
            serde_json::json!({ "success": true, "restored_count": 2, "tier": "plus" })
        );
    }

    #[test]
    fn apple_requests_deserialize_expected_fields() {
        let verify: AppleVerifyReceiptRequest = serde_json::from_value(serde_json::json!({
            "signed_transaction": "signed-payload",
            "product_id": "diaryx.plus.yearly"
        }))
        .unwrap();
        let restore: AppleRestoreRequest = serde_json::from_value(serde_json::json!({
            "signed_transactions": ["tx-1", "tx-2"]
        }))
        .unwrap();

        assert_eq!(verify.signed_transaction, "signed-payload");
        assert_eq!(verify.product_id, "diaryx.plus.yearly");
        assert_eq!(restore.signed_transactions, vec!["tx-1", "tx-2"]);
    }
}
