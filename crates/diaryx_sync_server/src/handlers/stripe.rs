use crate::auth::RequireAuth;
use crate::config::StripeConfig;
use crate::db::{AuthRepo, UserTier};
use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;
use std::sync::Arc;
use stripe::{
    BillingPortalSession, CheckoutSession, CheckoutSessionMode, Client, CreateBillingPortalSession,
    CreateCheckoutSession, CreateCheckoutSessionLineItems, CreateCustomer, Customer,
};
use tracing::{error, info, warn};

#[derive(Clone)]
pub struct StripeState {
    pub repo: Arc<AuthRepo>,
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
// Types
// ============================================================================

#[derive(Serialize)]
struct UrlResponse {
    url: String,
}

#[derive(Serialize)]
struct StripeConfigResponse {
    publishable_key: String,
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

    // Look up or create Stripe customer
    let customer_id = match state.repo.get_stripe_customer_id(user_id) {
        Ok(Some(id)) => id,
        Ok(None) => {
            // Create a new Stripe customer
            let mut params = CreateCustomer::new();
            params.email = Some(user_email);

            match Customer::create(&client, params).await {
                Ok(customer) => {
                    let cid = customer.id.as_str().to_string();
                    if let Err(e) = state.repo.set_stripe_customer_id(user_id, &cid) {
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

    let customer_id = match state.repo.get_stripe_customer_id(user_id) {
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
///
/// Verifies the webhook signature using HMAC-SHA256, then parses the event
/// JSON to handle subscription lifecycle events.
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

    match event_type {
        "checkout.session.completed" => {
            handle_checkout_completed(&state, data_object);
        }
        "customer.subscription.updated" => {
            handle_subscription_updated(&state, data_object);
        }
        "customer.subscription.deleted" => {
            handle_subscription_deleted(&state, data_object);
        }
        other => {
            info!("Unhandled Stripe event: {}", other);
        }
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
///
/// The Stripe-Signature header format is:
/// `t=<timestamp>,v1=<signature>[,v0=<legacy_signature>]`
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

// ============================================================================
// Webhook event handlers
// ============================================================================

fn handle_checkout_completed(state: &StripeState, data: &serde_json::Value) {
    let customer_id = match data["customer"].as_str() {
        Some(id) => id,
        None => {
            warn!("checkout.session.completed without customer ID");
            return;
        }
    };

    let user_id = match state.repo.get_user_id_by_stripe_customer_id(customer_id) {
        Ok(Some(id)) => id,
        Ok(None) => {
            warn!(
                "checkout.session.completed for unknown customer: {}",
                customer_id
            );
            return;
        }
        Err(e) => {
            error!("Database error in checkout webhook: {}", e);
            return;
        }
    };

    // Save subscription ID
    if let Some(sub_id) = data["subscription"].as_str() {
        if let Err(e) = state
            .repo
            .set_stripe_subscription_id(&user_id, Some(sub_id))
        {
            error!("Failed to save subscription ID: {}", e);
        }
    }

    // Upgrade to Plus
    match state.repo.set_user_tier(&user_id, UserTier::Plus) {
        Ok(_) => info!("User {} upgraded to Plus via checkout", user_id),
        Err(e) => error!("Failed to upgrade user {} to Plus: {}", user_id, e),
    }
}

fn handle_subscription_updated(state: &StripeState, data: &serde_json::Value) {
    let customer_id = match data["customer"].as_str() {
        Some(id) => id,
        None => {
            warn!("customer.subscription.updated without customer ID");
            return;
        }
    };

    let user_id = match state.repo.get_user_id_by_stripe_customer_id(customer_id) {
        Ok(Some(id)) => id,
        Ok(None) => {
            warn!(
                "customer.subscription.updated for unknown customer: {}",
                customer_id
            );
            return;
        }
        Err(e) => {
            error!("Database error in subscription webhook: {}", e);
            return;
        }
    };

    let status = data["status"].as_str().unwrap_or("");
    let tier = match status {
        "active" | "trialing" => UserTier::Plus,
        _ => UserTier::Free,
    };

    match state.repo.set_user_tier(&user_id, tier) {
        Ok(_) => info!(
            "User {} tier set to {} (subscription status: {})",
            user_id,
            tier.as_str(),
            status
        ),
        Err(e) => error!("Failed to update user {} tier: {}", user_id, e),
    }
}

fn handle_subscription_deleted(state: &StripeState, data: &serde_json::Value) {
    let customer_id = match data["customer"].as_str() {
        Some(id) => id,
        None => {
            warn!("customer.subscription.deleted without customer ID");
            return;
        }
    };

    let user_id = match state.repo.get_user_id_by_stripe_customer_id(customer_id) {
        Ok(Some(id)) => id,
        Ok(None) => {
            warn!(
                "customer.subscription.deleted for unknown customer: {}",
                customer_id
            );
            return;
        }
        Err(e) => {
            error!("Database error in subscription webhook: {}", e);
            return;
        }
    };

    // Downgrade to Free
    match state.repo.set_user_tier(&user_id, UserTier::Free) {
        Ok(_) => info!("User {} downgraded to Free (subscription deleted)", user_id),
        Err(e) => error!("Failed to downgrade user {}: {}", user_id, e),
    }

    // Clear subscription ID
    if let Err(e) = state.repo.set_stripe_subscription_id(&user_id, None) {
        error!(
            "Failed to clear subscription ID for user {}: {}",
            user_id, e
        );
    }
}
