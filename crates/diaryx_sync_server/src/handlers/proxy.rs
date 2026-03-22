//! Generic proxy handler.
//!
//! `POST /proxy/{proxy_id}/{*path}` — resolves credentials, validates,
//! and forwards requests to upstream APIs. Supports streaming (SSE passthrough).

use crate::auth::RequireAuth;
use crate::rate_limit::RateLimiter;
use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use diaryx_server::proxy::{ProxyAuthMethod, sign_proxy_request};
use diaryx_server::{ProxyConfigStore, ProxySecretResolver, ProxyUsageStore, UserTier};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, warn};

#[derive(Clone)]
pub struct ProxyState {
    pub config_store: Arc<dyn ProxyConfigStore>,
    pub secret_resolver: Arc<dyn ProxySecretResolver>,
    pub usage_store: Arc<dyn ProxyUsageStore>,
    pub rate_limiter: Arc<RateLimiter>,
    pub http_client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct ProxyErrorResponse {
    error: String,
    message: String,
}

fn proxy_error(status: StatusCode, error_code: &str, message: &str) -> Response {
    (
        status,
        axum::Json(ProxyErrorResponse {
            error: error_code.to_string(),
            message: message.to_string(),
        }),
    )
        .into_response()
}

pub fn proxy_routes(state: ProxyState) -> Router {
    Router::new()
        .route("/proxy/{proxy_id}", post(proxy_handler))
        .route("/proxy/{proxy_id}/{*path}", post(proxy_handler))
        // Backward compat: old AI endpoint redirects to diaryx.ai proxy
        .route("/ai/{*path}", post(ai_compat_handler))
        .with_state(state)
}

/// Backward-compatible handler for `POST /api/ai/{*path}`.
/// Rewrites the request to use the `diaryx.ai` proxy.
async fn ai_compat_handler(
    state: State<ProxyState>,
    auth: RequireAuth,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    proxy_handler(
        state,
        auth,
        Path(("diaryx.ai".to_string(), Some(path))),
        headers,
        body,
    )
    .await
}

async fn proxy_handler(
    State(state): State<ProxyState>,
    RequireAuth(auth): RequireAuth,
    Path(params): Path<(String, Option<String>)>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let (proxy_id, path) = params;
    let path = path.unwrap_or_default();

    // 1. Look up proxy config
    let config = match state.config_store.get_proxy(&proxy_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return proxy_error(
                StatusCode::NOT_FOUND,
                "proxy_not_found",
                &format!("Proxy '{}' not found", proxy_id),
            );
        }
        Err(e) => {
            error!("Proxy config lookup failed: {}", e);
            return proxy_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "Proxy service unavailable",
            );
        }
    };

    // 2. Check tier
    if let ProxyAuthMethod::PlatformSecret { required_tier, .. } = &config.auth_method {
        if *required_tier == UserTier::Plus && auth.user.tier != UserTier::Plus {
            return proxy_error(
                StatusCode::FORBIDDEN,
                "plus_required",
                "Diaryx Plus is required for this proxy",
            );
        }
    }

    // 3. Check path allowlist
    if let Some(ref allowed) = config.allowed_paths {
        if !allowed.iter().any(|p| path.starts_with(p.as_str())) {
            return proxy_error(
                StatusCode::FORBIDDEN,
                "path_not_allowed",
                &format!("Path '{}' not allowed", path),
            );
        }
    }

    // 4. Validate body
    if let Some(ref validation) = config.validation {
        if let Some(max_bytes) = validation.max_body_bytes {
            if body.len() > max_bytes {
                return proxy_error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "body_too_large",
                    &format!("Body exceeds {} bytes", max_bytes),
                );
            }
        }
        if !validation.allowed_values.is_empty() {
            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body) {
                for (field, allowed) in &validation.allowed_values {
                    if let Some(value) = json.get(field).and_then(|v| v.as_str()) {
                        if !allowed.iter().any(|a| a == value) {
                            return proxy_error(
                                StatusCode::BAD_REQUEST,
                                "value_not_allowed",
                                &format!("'{}' is not allowed for '{}'", value, field),
                            );
                        }
                    }
                }
            }
        }
    }

    // 5. Rate limit
    if let Some(limit) = config.rate_limit_per_minute {
        let key = format!("{}:{}", auth.user.id, proxy_id);
        if let Err(retry_after) =
            state
                .rate_limiter
                .check(&key, "proxy", limit as usize, Duration::from_secs(60))
        {
            let mut resp_headers = HeaderMap::new();
            if let Ok(val) = retry_after.to_string().parse() {
                resp_headers.insert(header::RETRY_AFTER, val);
            }
            return (
                StatusCode::TOO_MANY_REQUESTS,
                resp_headers,
                axum::Json(ProxyErrorResponse {
                    error: "rate_limited".to_string(),
                    message: "Rate limit exceeded".to_string(),
                }),
            )
                .into_response();
        }
    }

    // 6. Monthly quota
    if let Some(quota) = config.monthly_quota {
        let period = chrono::Utc::now().format("%Y-%m").to_string();
        match state
            .usage_store
            .get_monthly_count(&auth.user.id, &proxy_id, &period)
            .await
        {
            Ok(count) if count >= quota => {
                return proxy_error(
                    StatusCode::TOO_MANY_REQUESTS,
                    "quota_exceeded",
                    "Monthly quota exceeded",
                );
            }
            Err(e) => {
                error!("Proxy usage lookup failed: {}", e);
                return proxy_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "Usage tracking unavailable",
                );
            }
            _ => {}
        }
    }

    // 7. Resolve credentials and build upstream request
    let url = format!(
        "{}/{}",
        config.upstream.trim_end_matches('/'),
        path.trim_start_matches('/')
    );

    let mut upstream_req = state
        .http_client
        .post(&url)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.to_vec());

    match &config.auth_method {
        ProxyAuthMethod::PlatformSecret {
            env_key,
            auth_header,
            auth_prefix,
            ..
        } => {
            let secret = match state.secret_resolver.resolve_platform_secret(env_key) {
                Some(s) => s,
                None => {
                    return proxy_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "provider_unavailable",
                        "Proxy not configured",
                    );
                }
            };
            upstream_req =
                upstream_req.header(auth_header.as_str(), format!("{}{}", auth_prefix, secret));
        }
        ProxyAuthMethod::UserSecret {
            secret_key,
            auth_header,
            auth_prefix,
        } => {
            let secret = match state
                .secret_resolver
                .resolve_user_secret(&auth.user.id, secret_key)
                .await
            {
                Some(s) => s,
                None => {
                    return proxy_error(
                        StatusCode::BAD_REQUEST,
                        "secret_not_set",
                        &format!("Configure your '{}' API key first", secret_key),
                    );
                }
            };
            upstream_req =
                upstream_req.header(auth_header.as_str(), format!("{}{}", auth_prefix, secret));
        }
        ProxyAuthMethod::HmacSigned { hmac_secret_env } => {
            let hmac_secret = match state
                .secret_resolver
                .resolve_platform_secret(hmac_secret_env)
            {
                Some(s) => s,
                None => {
                    return proxy_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "provider_unavailable",
                        "HMAC secret not configured",
                    );
                }
            };
            let timestamp = chrono::Utc::now().timestamp() as u64;
            let signature =
                sign_proxy_request(hmac_secret.as_bytes(), timestamp, &auth.user.id, &body);
            upstream_req = upstream_req
                .header("X-Diaryx-Timestamp", timestamp.to_string())
                .header("X-Diaryx-User", &auth.user.id)
                .header("X-Diaryx-Signature", signature);
        }
    }

    // Forward headers from client (skip hop-by-hop)
    for (name, value) in headers.iter() {
        let name_str = name.as_str();
        if matches!(
            name_str,
            "host" | "connection" | "authorization" | "content-length" | "transfer-encoding"
        ) {
            continue;
        }
        if let Ok(v) = value.to_str() {
            upstream_req = upstream_req.header(name_str, v);
        }
    }

    // 8. Execute upstream request
    let upstream = match upstream_req.send().await {
        Ok(resp) => resp,
        Err(e) => {
            warn!("Proxy upstream request failed: {}", e);
            return proxy_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_unavailable",
                "Upstream request failed",
            );
        }
    };

    let upstream_status = upstream.status();

    // 9. Record usage on success
    if upstream_status.is_success() {
        let period = chrono::Utc::now().format("%Y-%m").to_string();
        if let Err(e) = state
            .usage_store
            .increment_monthly_count(&auth.user.id, &proxy_id, &period)
            .await
        {
            error!("Proxy usage increment failed: {}", e);
        }
    }

    // 10. Stream or buffer response
    if config.streaming {
        let status =
            StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let mut resp_headers = HeaderMap::new();
        for (name, value) in upstream.headers() {
            if !matches!(name.as_str(), "transfer-encoding" | "connection") {
                resp_headers.insert(name.clone(), value.clone());
            }
        }
        let stream = upstream.bytes_stream();
        Response::builder()
            .status(status)
            .body(Body::from_stream(stream))
            .unwrap_or_else(|_| {
                proxy_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "Response build failed",
                )
            })
    } else {
        let status =
            StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        match upstream.bytes().await {
            Ok(bytes) => Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(bytes))
                .unwrap_or_else(|_| {
                    proxy_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal",
                        "Response build failed",
                    )
                }),
            Err(e) => {
                warn!("Failed to read upstream response: {}", e);
                proxy_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "provider_unavailable",
                    "Failed to read upstream response",
                )
            }
        }
    }
}
