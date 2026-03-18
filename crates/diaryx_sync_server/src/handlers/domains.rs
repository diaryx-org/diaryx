//! Custom domain handlers — manage domain→namespace+audience mappings and
//! serve as a Caddy `forward_auth` endpoint.
//!
//! Domain registrations are also synced to Cloudflare KV (best-effort) so the
//! site-proxy worker can resolve custom domains at the edge without hitting
//! this server.

use super::require_namespace_owner;
use crate::auth::RequireAuth;
use crate::blob_store::BlobStore;
use crate::db::NamespaceRepo;
use crate::publish::validate_signed_token;
use axum::{
    Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::warn;

/// Shared state for domain handlers.
#[derive(Clone)]
pub struct DomainState {
    pub ns_repo: Arc<NamespaceRepo>,
    pub blob_store: Arc<dyn BlobStore>,
    pub token_signing_key: Vec<u8>,
    /// HTTP client for Cloudflare KV REST API calls.
    pub http_client: reqwest::Client,
    /// Cloudflare account ID (reused from R2 config).
    pub cf_account_id: String,
    /// Cloudflare KV API token.
    pub kv_api_token: Option<String>,
    /// Cloudflare KV namespace ID for domain mappings.
    pub kv_namespace_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RegisterDomainRequest {
    pub audience_name: String,
}

#[derive(Debug, Serialize)]
pub struct DomainResponse {
    pub domain: String,
    pub namespace_id: String,
    pub audience_name: String,
    pub created_at: i64,
    pub verified: bool,
}

// ---------------------------------------------------------------------------
// KV sync helpers
// ---------------------------------------------------------------------------

/// Write a domain→namespace mapping to Cloudflare KV (best-effort).
async fn kv_put_domain(
    state: &DomainState,
    hostname: &str,
    namespace_id: &str,
    audience_name: &str,
) {
    let (Some(token), Some(kv_id)) = (&state.kv_api_token, &state.kv_namespace_id) else {
        return;
    };
    if state.cf_account_id.is_empty() {
        return;
    }

    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/storage/kv/namespaces/{}/values/domain:{}",
        state.cf_account_id, kv_id, hostname
    );
    let body = serde_json::json!({
        "namespace_id": namespace_id,
        "audience_name": audience_name,
    });

    if let Err(e) = state
        .http_client
        .put(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
    {
        warn!("Failed to write domain '{}' to KV: {}", hostname, e);
    }
}

/// Delete a domain mapping from Cloudflare KV (best-effort).
async fn kv_delete_domain(state: &DomainState, hostname: &str) {
    let (Some(token), Some(kv_id)) = (&state.kv_api_token, &state.kv_namespace_id) else {
        return;
    };
    if state.cf_account_id.is_empty() {
        return;
    }

    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/storage/kv/namespaces/{}/values/domain:{}",
        state.cf_account_id, kv_id, hostname
    );

    if let Err(e) = state
        .http_client
        .delete(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
    {
        warn!("Failed to delete domain '{}' from KV: {}", hostname, e);
    }
}

// ---------------------------------------------------------------------------
// Owner management routes (mounted under /namespaces/{ns_id})
// ---------------------------------------------------------------------------

pub fn domain_routes(state: DomainState) -> Router {
    Router::new()
        .route("/domains", get(list_domains))
        .route(
            "/domains/{domain}",
            put(register_domain).delete(remove_domain),
        )
        .with_state(state)
}

/// PUT /namespaces/{ns_id}/domains/{domain} — register a custom domain.
async fn register_domain(
    State(state): State<DomainState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, domain)): Path<(String, String)>,
    Json(req): Json<RegisterDomainRequest>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    // Validate audience exists.
    if state
        .ns_repo
        .get_audience(&ns_id, &req.audience_name)
        .is_none()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("audience '{}' does not exist", req.audience_name) })),
        )
            .into_response();
    }

    match state
        .ns_repo
        .upsert_custom_domain(&domain, &ns_id, &req.audience_name)
    {
        Ok(()) => {
            let info = state
                .ns_repo
                .get_custom_domain(&domain)
                .expect("just upserted");

            // Sync to Cloudflare KV (best-effort).
            kv_put_domain(&state, &domain, &ns_id, &req.audience_name).await;

            Json(DomainResponse {
                domain: info.domain,
                namespace_id: info.namespace_id,
                audience_name: info.audience_name,
                created_at: info.created_at,
                verified: info.verified,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// GET /namespaces/{ns_id}/domains — list custom domains for a namespace.
async fn list_domains(
    State(state): State<DomainState>,
    RequireAuth(auth): RequireAuth,
    Path(ns_id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    let domains: Vec<DomainResponse> = state
        .ns_repo
        .list_custom_domains(&ns_id)
        .into_iter()
        .map(|d| DomainResponse {
            domain: d.domain,
            namespace_id: d.namespace_id,
            audience_name: d.audience_name,
            created_at: d.created_at,
            verified: d.verified,
        })
        .collect();

    Json(domains).into_response()
}

/// DELETE /namespaces/{ns_id}/domains/{domain} — remove a custom domain.
async fn remove_domain(
    State(state): State<DomainState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, domain)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    match state.ns_repo.get_custom_domain(&domain) {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(d) if d.namespace_id != ns_id => StatusCode::NOT_FOUND.into_response(),
        Some(_) => match state.ns_repo.delete_custom_domain(&domain) {
            Ok(_) => {
                // Remove from Cloudflare KV (best-effort).
                kv_delete_domain(&state, &domain).await;
                StatusCode::NO_CONTENT.into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response(),
        },
    }
}

// ---------------------------------------------------------------------------
// Caddy forward_auth endpoint (unauthenticated)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DomainAuthParams {
    pub audience_token: Option<String>,
}

pub fn domain_auth_route(state: DomainState) -> Router {
    Router::new()
        .route("/domain-auth", get(domain_auth))
        .with_state(state)
}

/// GET /domain-auth — Caddy `forward_auth` endpoint.
///
/// Reads `X-Forwarded-Host` and `X-Forwarded-Uri` headers to resolve the
/// custom domain → namespace + audience, then checks access control.
///
/// Returns 200 with the object bytes on success, or 403/404 on failure.
async fn domain_auth(
    State(state): State<DomainState>,
    headers: HeaderMap,
    Query(params): Query<DomainAuthParams>,
) -> impl IntoResponse {
    let host = match headers
        .get("x-forwarded-host")
        .and_then(|v| v.to_str().ok())
    {
        Some(h) => h.to_string(),
        None => return StatusCode::BAD_REQUEST.into_response(),
    };

    let uri = headers
        .get("x-forwarded-uri")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("/");

    // Strip leading slash for the object key.
    let key = uri.trim_start_matches('/');

    // Look up the custom domain.
    let domain_info = match state.ns_repo.get_custom_domain(&host) {
        Some(d) => d,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let ns_id = &domain_info.namespace_id;
    let audience_name = &domain_info.audience_name;

    // Look up the object.
    let meta = match state.ns_repo.get_object_meta(ns_id, key) {
        Some(m) => m,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    // Object must belong to the domain's audience.
    match &meta.audience {
        Some(a) if a == audience_name => {}
        _ => return StatusCode::NOT_FOUND.into_response(),
    }

    // Check audience access level.
    let audience = match state.ns_repo.get_audience(ns_id, audience_name) {
        Some(a) => a,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    match audience.access.as_str() {
        "public" => { /* allowed */ }
        "token" => {
            let token_str = match &params.audience_token {
                Some(t) => t,
                None => return StatusCode::FORBIDDEN.into_response(),
            };
            match validate_signed_token(&state.token_signing_key, token_str) {
                Some(claims) if claims.slug == *ns_id && claims.audience == *audience_name => {}
                _ => return StatusCode::FORBIDDEN.into_response(),
            }
        }
        _ => return StatusCode::FORBIDDEN.into_response(),
    }

    // Serve the object bytes directly.
    let rkey = meta
        .r2_key
        .unwrap_or_else(|| format!("ns/{}/{}", ns_id, key));
    match state.blob_store.get(&rkey).await {
        Ok(Some(bytes)) => (
            StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                meta.mime_type
                    .parse::<axum::http::HeaderValue>()
                    .unwrap_or_else(|_| "application/octet-stream".parse().unwrap()),
            )],
            bytes,
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}
