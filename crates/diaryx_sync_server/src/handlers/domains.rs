//! Custom domain handlers — manage domain→namespace+audience mappings and
//! serve as a Caddy `forward_auth` endpoint.
//!
//! Domain registrations are also synced to Cloudflare KV (best-effort) so the
//! site-proxy worker can resolve custom domains at the edge without hitting
//! this server.

use super::require_namespace_owner;
use crate::auth::RequireAuth;
use crate::db::NamespaceRepo;
use crate::tokens::validate_signed_token;
use axum::{
    Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, put},
};
use diaryx_server::domain::CustomDomainInfo as CoreCustomDomainInfo;
use diaryx_server::ports::{BlobStore, DomainMappingCache, NamespaceStore, ServerCoreError};
use diaryx_server::use_cases::domains::DomainService;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::warn;

/// Shared state for domain handlers.
#[derive(Clone)]
pub struct DomainState {
    pub ns_repo: Arc<NamespaceRepo>,
    pub namespace_store: Arc<dyn NamespaceStore>,
    pub domain_mapping_cache: Arc<dyn DomainMappingCache>,
    pub blob_store: Arc<dyn BlobStore>,
    pub token_signing_key: Vec<u8>,
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

#[derive(Debug, Deserialize)]
pub struct ClaimSubdomainRequest {
    pub subdomain: String,
    #[serde(default)]
    pub default_audience: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SubdomainResponse {
    pub subdomain: String,
    pub namespace_id: String,
    pub url: String,
}

impl From<CoreCustomDomainInfo> for DomainResponse {
    fn from(value: CoreCustomDomainInfo) -> Self {
        Self {
            domain: value.domain,
            namespace_id: value.namespace_id,
            audience_name: value.audience_name,
            created_at: value.created_at,
            verified: value.verified,
        }
    }
}

fn status_for_core_error(error: &ServerCoreError) -> StatusCode {
    match error {
        ServerCoreError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        ServerCoreError::NotFound(_) => StatusCode::NOT_FOUND,
        ServerCoreError::PermissionDenied(_) => StatusCode::FORBIDDEN,
        ServerCoreError::Conflict(_) => StatusCode::CONFLICT,
        ServerCoreError::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
        ServerCoreError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        ServerCoreError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn core_error_response(error: ServerCoreError) -> Response {
    (
        status_for_core_error(&error),
        Json(serde_json::json!({ "error": error.to_string() })),
    )
        .into_response()
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
        .route("/subdomain", put(claim_subdomain).delete(release_subdomain))
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

    let service = DomainService::new(
        state.namespace_store.as_ref(),
        state.domain_mapping_cache.as_ref(),
    );
    match service
        .register_domain(&ns_id, &domain, &req.audience_name)
        .await
    {
        Ok(info) => Json(DomainResponse::from(info)).into_response(),
        Err(error) => core_error_response(error),
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

    match state.namespace_store.list_custom_domains(&ns_id).await {
        Ok(domains) => Json(
            domains
                .into_iter()
                .map(DomainResponse::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(error) => core_error_response(error),
    }
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

    let service = DomainService::new(
        state.namespace_store.as_ref(),
        state.domain_mapping_cache.as_ref(),
    );
    match service.remove_domain(&ns_id, &domain).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => core_error_response(error),
    }
}

/// PUT /namespaces/{ns_id}/subdomain — claim a subdomain for this namespace.
async fn claim_subdomain(
    State(state): State<DomainState>,
    RequireAuth(auth): RequireAuth,
    Path(ns_id): Path<String>,
    Json(req): Json<ClaimSubdomainRequest>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    let service = DomainService::new(
        state.namespace_store.as_ref(),
        state.domain_mapping_cache.as_ref(),
    );
    match service
        .claim_subdomain(&ns_id, &req.subdomain, req.default_audience.as_deref())
        .await
    {
        Ok(claimed) => Json(SubdomainResponse {
            subdomain: claimed.subdomain.clone(),
            namespace_id: claimed.namespace_id,
            url: format!("https://{}.diaryx.org", claimed.subdomain),
        })
        .into_response(),
        Err(error) => core_error_response(error),
    }
}

/// DELETE /namespaces/{ns_id}/subdomain — release the subdomain for this namespace.
async fn release_subdomain(
    State(state): State<DomainState>,
    RequireAuth(auth): RequireAuth,
    Path(ns_id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    let service = DomainService::new(
        state.namespace_store.as_ref(),
        state.domain_mapping_cache.as_ref(),
    );
    match service.release_subdomain(&ns_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => core_error_response(error),
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
        .route("/domain-check", get(domain_check))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Caddy on-demand TLS `ask` endpoint (unauthenticated)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DomainCheckParams {
    pub domain: Option<String>,
}

/// GET /domain-check?domain=example.com — Caddy on-demand TLS validation.
///
/// Returns 200 if the domain is registered in the `custom_domains` table,
/// 404 otherwise. Caddy uses this to decide whether to provision a TLS
/// certificate for an incoming hostname.
async fn domain_check(
    State(state): State<DomainState>,
    Query(params): Query<DomainCheckParams>,
) -> impl IntoResponse {
    let domain = match params.domain {
        Some(d) if !d.is_empty() => d,
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };

    match state.namespace_store.get_custom_domain(&domain).await {
        Ok(Some(_)) => StatusCode::OK.into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            warn!("Failed to resolve domain '{}': {}", domain, error);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
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
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
