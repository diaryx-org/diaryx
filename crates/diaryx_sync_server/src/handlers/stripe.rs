use crate::adapters::NativeBillingStore;
use crate::auth::RequireAuth;
use crate::config::StripeConfig;
use crate::db::AuthRepo;
use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use diaryx_server::api::billing::{StripeConfigResponse, UrlResponse};
use diaryx_server::ports::UserStore;
use diaryx_server::use_cases::billing::BillingService;
use hmac::{Hmac, Mac};
use serde_json;
use sha2::Sha256;
use std::sync::Arc;
use stripe::{
    BillingPortalSession, CheckoutSession, CheckoutSessionMode, Client, CreateBillingPortalSession,
    CreateCheckoutSession, CreateCheckoutSessionLineItems, CreateCustomer, Customer,
};
use tracing::{error, warn};

#[derive(Clone)]
pub struct StripeState {
    pub repo: Arc<AuthRepo>,
    pub user_store: Arc<dyn UserStore>,
    pub config: StripeConfig,
    pub app_base_url: String,
}

pub fn stripe_routes(state: StripeState) -> Router {
    Router::new()
        .route("/stripe/checkout", post(create_checkout_session))
        .route("/stripe/portal", post(create_portal_session))
        .route("/stripe/webhook", post(handle_webhook))
        .route("/stripe/config", get(get_stripe_config))
        .with_state(state)
}

// ============================================================================
// Handlers
// ============================================================================

/// POST /api/stripe/checkout - Create a Stripe Checkout Session for upgrading to Plus.
async fn create_checkout_session(
    State(state): State<StripeState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    let client = Client::new(&state.config.secret_key);
    let user_id = &auth.user.id;
    let user_email = &auth.user.email;

    let billing_store = NativeBillingStore::new(state.repo.clone());
    let billing = BillingService::new(&billing_store, state.user_store.as_ref());

    // Look up or create Stripe customer
    let customer_id = match billing.get_stripe_customer_id(user_id).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            // Create a new Stripe customer
            let mut params = CreateCustomer::new();
            params.email = Some(user_email);

            match Customer::create(&client, params).await {
                Ok(customer) => {
                    let cid = customer.id.as_str().to_string();
                    if let Err(e) = billing.set_stripe_customer_id(user_id, &cid).await {
                        error!("Failed to save Stripe customer ID: {}", e);
                        return (StatusCode::INTERNAL_SERVER_ERROR, "Database error")
                            .into_response();
                    }
                    cid
                }
                Err(e) => {
                    error!("Failed to create Stripe customer: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to create customer",
                    )
                        .into_response();
                }
            }
        }
        Err(e) => {
            error!("Database error looking up Stripe customer: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    // Create Checkout Session
    let customer_id_parsed = match customer_id.parse() {
        Ok(id) => id,
        Err(_) => {
            error!("Invalid stored Stripe customer ID: {}", customer_id);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Invalid customer ID").into_response();
        }
    };

    let success_url = format!("{}?checkout=success", state.app_base_url);
    let cancel_url = format!("{}?checkout=cancelled", state.app_base_url);

    let mut params = CreateCheckoutSession::new();
    params.customer = Some(customer_id_parsed);
    params.mode = Some(CheckoutSessionMode::Subscription);
    params.success_url = Some(&success_url);
    params.cancel_url = Some(&cancel_url);
    params.line_items = Some(vec![CreateCheckoutSessionLineItems {
        price: Some(state.config.price_id.clone()),
        quantity: Some(1),
        ..Default::default()
    }]);

    match CheckoutSession::create(&client, params).await {
        Ok(session) => match session.url {
            Some(url) => Json(UrlResponse { url }).into_response(),
            None => {
                error!("Stripe checkout session created without URL");
                (StatusCode::INTERNAL_SERVER_ERROR, "No checkout URL").into_response()
            }
        },
        Err(e) => {
            error!("Failed to create Stripe checkout session: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create checkout session",
            )
                .into_response()
        }
    }
}

/// POST /api/stripe/portal - Create a Stripe Customer Portal session.
async fn create_portal_session(
    State(state): State<StripeState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    let client = Client::new(&state.config.secret_key);
    let user_id = &auth.user.id;

    let billing_store = NativeBillingStore::new(state.repo.clone());
    let billing = BillingService::new(&billing_store, state.user_store.as_ref());

    let customer_id = match billing.get_stripe_customer_id(user_id).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            return (StatusCode::BAD_REQUEST, "No billing account found").into_response();
        }
        Err(e) => {
            error!("Database error looking up Stripe customer: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    let customer_id_parsed = match customer_id.parse() {
        Ok(id) => id,
        Err(_) => {
            error!("Invalid stored Stripe customer ID: {}", customer_id);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Invalid customer ID").into_response();
        }
    };

    let mut params = CreateBillingPortalSession::new(customer_id_parsed);
    params.return_url = Some(&state.app_base_url);

    match BillingPortalSession::create(&client, params).await {
        Ok(session) => Json(UrlResponse { url: session.url }).into_response(),
        Err(e) => {
            error!("Failed to create Stripe portal session: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create portal session",
            )
                .into_response()
        }
    }
}

/// POST /api/stripe/webhook - Handle Stripe webhook events.
async fn handle_webhook(
    State(state): State<StripeState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let signature_header = match headers.get("Stripe-Signature") {
        Some(sig) => match sig.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return StatusCode::BAD_REQUEST,
        },
        None => return StatusCode::BAD_REQUEST,
    };

    let payload = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(_) => return StatusCode::BAD_REQUEST,
    };

    // Verify webhook signature
    if let Err(msg) =
        verify_stripe_signature(payload, &signature_header, &state.config.webhook_secret)
    {
        warn!("Stripe webhook signature verification failed: {}", msg);
        return StatusCode::BAD_REQUEST;
    }

    // Parse event JSON
    let event: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to parse Stripe webhook payload: {}", e);
            return StatusCode::BAD_REQUEST;
        }
    };

    let event_type = event["type"].as_str().unwrap_or("");
    let data_object = &event["data"]["object"];

    let billing_store = NativeBillingStore::new(state.repo.clone());
    let billing = BillingService::new(&billing_store, state.user_store.as_ref());

    match event_type {
        "checkout.session.completed" => {
            let customer_id = data_object["customer"].as_str().unwrap_or("");
            let subscription_id = data_object["subscription"].as_str();
            let _ = billing
                .handle_checkout_completed(customer_id, subscription_id)
                .await;
        }
        "customer.subscription.updated" => {
            let customer_id = data_object["customer"].as_str().unwrap_or("");
            let status = data_object["status"].as_str().unwrap_or("");
            let _ = billing
                .handle_subscription_updated(customer_id, status)
                .await;
        }
        "customer.subscription.deleted" => {
            let customer_id = data_object["customer"].as_str().unwrap_or("");
            let _ = billing.handle_subscription_deleted(customer_id).await;
        }
        _ => {}
    }

    StatusCode::OK
}

/// GET /api/stripe/config - Return Stripe publishable key.
async fn get_stripe_config(State(state): State<StripeState>) -> impl IntoResponse {
    Json(StripeConfigResponse {
        publishable_key: state.config.publishable_key.clone(),
    })
}

// ============================================================================
// Webhook signature verification
// ============================================================================

/// Verify Stripe webhook signature using HMAC-SHA256.
fn verify_stripe_signature(
    payload: &str,
    signature_header: &str,
    secret: &str,
) -> Result<(), String> {
    let mut timestamp = None;
    let mut signatures = Vec::new();

    for part in signature_header.split(',') {
        let part = part.trim();
        if let Some(ts) = part.strip_prefix("t=") {
            timestamp = Some(ts.to_string());
        } else if let Some(sig) = part.strip_prefix("v1=") {
            signatures.push(sig.to_string());
        }
    }

    let timestamp = timestamp.ok_or_else(|| "Missing timestamp in signature".to_string())?;
    if signatures.is_empty() {
        return Err("Missing v1 signature".to_string());
    }

    // Check timestamp tolerance (5 minutes)
    if let Ok(ts) = timestamp.parse::<i64>() {
        let now = chrono::Utc::now().timestamp();
        if (now - ts).unsigned_abs() > 300 {
            return Err("Signature timestamp too old".to_string());
        }
    }

    // Compute expected signature
    let signed_payload = format!("{}.{}", timestamp, payload);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|e| format!("HMAC key error: {}", e))?;
    mac.update(signed_payload.as_bytes());
    let result = mac.finalize().into_bytes();
    let expected: String = result.iter().map(|b| format!("{:02x}", b)).collect();

    // Constant-time comparison
    if signatures
        .iter()
        .any(|sig| constant_time_eq(sig, &expected))
    {
        Ok(())
    } else {
        Err("Signature mismatch".to_string())
    }
}

/// Constant-time string comparison to prevent timing attacks.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}
