//! Object store handlers — `PUT/GET/DELETE/LIST /namespaces/{id}/objects`.

use super::require_namespace_owner;
use crate::auth::RequireAuth;
use crate::blob_store::BlobStore;
use crate::db::{NamespaceRepo, UsageTotals};
use crate::publish::validate_signed_token;
use axum::{
    Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Shared state for object handlers.
#[derive(Clone)]
pub struct ObjectState {
    pub ns_repo: Arc<NamespaceRepo>,
    /// Single R2 bucket for all namespace objects.
    pub blob_store: Arc<dyn BlobStore>,
    /// HMAC-SHA256 key for validating audience access tokens.
    pub token_signing_key: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ObjectMetaResponse {
    pub namespace_id: String,
    pub key: String,
    pub r2_key: Option<String>,
    pub mime_type: String,
    pub size_bytes: u64,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub relay_seconds: u64,
}

impl From<UsageTotals> for UsageResponse {
    fn from(t: UsageTotals) -> Self {
        Self {
            bytes_in: t.bytes_in,
            bytes_out: t.bytes_out,
            relay_seconds: t.relay_seconds,
        }
    }
}

// ---------------------------------------------------------------------------
// Router (mounted under /namespaces/{ns_id})
// ---------------------------------------------------------------------------

pub fn object_routes(state: ObjectState) -> Router {
    Router::new()
        .route("/objects", get(list_objects))
        .route(
            "/objects/{*key}",
            put(put_object).get(get_object).delete(delete_object),
        )
        .route("/usage", get(get_namespace_usage))
        .with_state(state)
}

pub fn usage_routes(state: ObjectState) -> Router {
    Router::new().route("/", get(get_usage)).with_state(state)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Derive the R2 key for a namespace object.
fn r2_key(namespace_id: &str, key: &str) -> String {
    format!("ns/{}/{}", namespace_id, key)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// PUT /namespaces/{ns_id}/objects/{*key} — store bytes under the given key.
async fn put_object(
    State(state): State<ObjectState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, key)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    let mime_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    // Optional audience tag via X-Audience header.
    let audience = headers
        .get("x-audience")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Validate audience exists if specified.
    if let Some(ref aud) = audience {
        if state.ns_repo.get_audience(&ns_id, aud).is_none() {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("audience '{}' does not exist", aud) })),
            )
                .into_response();
        }
    }

    let rkey = r2_key(&ns_id, &key);
    let size = body.len() as u64;

    let r2_metadata = audience.as_ref().map(|aud| {
        let mut m = HashMap::new();
        m.insert("audience".to_string(), aud.clone());
        m
    });

    if let Err(e) = state
        .blob_store
        .put(&rkey, &body, &mime_type, r2_metadata.as_ref())
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    if let Err(e) =
        state
            .ns_repo
            .upsert_object(&ns_id, &key, &rkey, &mime_type, size, audience.as_deref())
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    // Record bytes_in usage (fire-and-forget; errors are non-fatal).
    let _ = state
        .ns_repo
        .record_usage(&auth.user.id, "bytes_in", size, Some(&ns_id));

    (
        StatusCode::OK,
        Json(serde_json::json!({ "key": key, "size_bytes": size })),
    )
        .into_response()
}

/// GET /namespaces/{ns_id}/objects/{*key} — retrieve bytes by key.
async fn get_object(
    State(state): State<ObjectState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, key)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    let meta = match state.ns_repo.get_object_meta(&ns_id, &key) {
        Some(m) => m,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let rkey = meta.r2_key.unwrap_or_else(|| r2_key(&ns_id, &key));
    match state.blob_store.get(&rkey).await {
        Ok(Some(bytes)) => {
            let size = bytes.len() as u64;
            let _ = state
                .ns_repo
                .record_usage(&auth.user.id, "bytes_out", size, Some(&ns_id));
            (
                StatusCode::OK,
                [(
                    axum::http::header::CONTENT_TYPE,
                    meta.mime_type
                        .parse::<axum::http::HeaderValue>()
                        .unwrap_or_else(|_| "application/octet-stream".parse().unwrap()),
                )],
                bytes,
            )
                .into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// DELETE /namespaces/{ns_id}/objects/{*key} — delete an object.
async fn delete_object(
    State(state): State<ObjectState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, key)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    let meta = match state.ns_repo.get_object_meta(&ns_id, &key) {
        Some(m) => m,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let rkey = meta.r2_key.unwrap_or_else(|| r2_key(&ns_id, &key));
    if let Err(e) = state.blob_store.delete(&rkey).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    let _ = state.ns_repo.delete_object(&ns_id, &key);
    StatusCode::NO_CONTENT.into_response()
}

/// Pagination query parameters.
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 {
    100
}

/// GET /namespaces/{ns_id}/objects — list object metadata.
async fn list_objects(
    State(state): State<ObjectState>,
    RequireAuth(auth): RequireAuth,
    Path(ns_id): Path<String>,
    Query(pagination): Query<PaginationParams>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    let limit = pagination.limit.min(500);
    let objects: Vec<ObjectMetaResponse> = state
        .ns_repo
        .list_objects(&ns_id, limit, pagination.offset)
        .into_iter()
        .map(|m| ObjectMetaResponse {
            namespace_id: m.namespace_id,
            key: m.key,
            r2_key: m.r2_key,
            mime_type: m.mime_type,
            size_bytes: m.size_bytes,
            updated_at: m.updated_at,
            audience: m.audience,
        })
        .collect();

    Json(objects).into_response()
}

/// GET /usage — return aggregated usage totals for the authenticated user.
async fn get_usage(
    State(state): State<ObjectState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    let totals = state.ns_repo.get_usage_totals(&auth.user.id);
    Json(UsageResponse::from(totals))
}

/// GET /namespaces/{ns_id}/usage — return usage totals scoped to a namespace.
async fn get_namespace_usage(
    State(state): State<ObjectState>,
    RequireAuth(auth): RequireAuth,
    Path(ns_id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    let totals = state
        .ns_repo
        .get_namespace_usage_totals(&auth.user.id, &ns_id);
    Json(UsageResponse::from(totals)).into_response()
}

// ---------------------------------------------------------------------------
// Public (unauthenticated) object access
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PublicObjectParams {
    pub audience_token: Option<String>,
}

/// Routes for unauthenticated public object access.
pub fn public_object_routes(state: ObjectState) -> Router {
    Router::new()
        .route("/public/{ns_id}/objects/{*key}", get(get_public_object))
        .with_state(state)
}

/// GET /public/{ns_id}/objects/{*key} — retrieve an object via audience access control.
async fn get_public_object(
    State(state): State<ObjectState>,
    Path((ns_id, key)): Path<(String, String)>,
    Query(params): Query<PublicObjectParams>,
) -> impl IntoResponse {
    let meta = match state.ns_repo.get_object_meta(&ns_id, &key) {
        Some(m) => m,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    // Private objects (no audience) are not served publicly.
    let audience_name = match &meta.audience {
        Some(a) => a.clone(),
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    // Look up the audience record.
    let audience = match state.ns_repo.get_audience(&ns_id, &audience_name) {
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
                Some(claims) if claims.slug == ns_id && claims.audience == audience_name => { /* valid */
                }
                _ => return StatusCode::FORBIDDEN.into_response(),
            }
        }
        _ => return StatusCode::FORBIDDEN.into_response(), // "private" or unknown
    }

    // Serve the bytes.
    let rkey = meta.r2_key.unwrap_or_else(|| r2_key(&ns_id, &key));
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
