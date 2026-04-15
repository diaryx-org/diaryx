//! Object store handlers — `PUT/GET/DELETE/LIST /namespaces/{id}/objects`.

use crate::auth::RequireAuth;
use crate::tokens::validate_signed_token;
use axum::{
    Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, post, put},
};
use diaryx_server::domain::{ObjectMeta, UsageTotals};
use diaryx_server::ports::{BlobStore, NamespaceStore, ObjectMetaStore, ServerCoreError};
use diaryx_server::use_cases::objects::ObjectService;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Shared state for object handlers.
#[derive(Clone)]
pub struct ObjectState {
    pub namespace_store: Arc<dyn NamespaceStore>,
    pub object_meta_store: Arc<dyn ObjectMetaStore>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

impl From<ObjectMeta> for ObjectMetaResponse {
    fn from(m: ObjectMeta) -> Self {
        Self {
            namespace_id: m.namespace_id,
            key: m.key,
            r2_key: m.blob_key,
            mime_type: m.mime_type,
            size_bytes: m.size_bytes,
            updated_at: m.updated_at,
            audience: m.audience,
            content_hash: m.content_hash,
        }
    }
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

#[derive(Debug, Deserialize)]
struct BatchGetRequest {
    keys: Vec<String>,
}

#[derive(Debug, Serialize)]
struct BatchObjectEntry {
    data: String,
    mime_type: String,
}

#[derive(Debug, Serialize)]
struct BatchGetResponse {
    objects: std::collections::HashMap<String, BatchObjectEntry>,
    #[serde(skip_serializing_if = "std::collections::HashMap::is_empty")]
    errors: std::collections::HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Router (mounted under /namespaces/{ns_id})
// ---------------------------------------------------------------------------

pub fn object_routes(state: ObjectState) -> Router {
    Router::new()
        .route("/objects", get(list_objects))
        .route("/objects/batch", post(batch_get_objects))
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

fn status_for_core_error(err: &ServerCoreError) -> StatusCode {
    match err {
        ServerCoreError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        ServerCoreError::Conflict(_) => StatusCode::CONFLICT,
        ServerCoreError::NotFound(_) => StatusCode::NOT_FOUND,
        ServerCoreError::PermissionDenied(_) => StatusCode::FORBIDDEN,
        ServerCoreError::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
        ServerCoreError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        ServerCoreError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn core_error_response(err: ServerCoreError) -> axum::response::Response {
    let status = status_for_core_error(&err);
    (
        status,
        Json(serde_json::json!({ "error": err.to_string() })),
    )
        .into_response()
}

fn make_service(state: &ObjectState) -> ObjectService<'_> {
    ObjectService::new(
        state.namespace_store.as_ref(),
        state.object_meta_store.as_ref(),
        state.blob_store.as_ref(),
    )
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
    let mime_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");

    let audience = headers.get("x-audience").and_then(|v| v.to_str().ok());

    let service = make_service(&state);

    match service
        .put(&ns_id, &key, mime_type, &body, audience, &auth.user.id)
        .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(serde_json::json!({ "key": result.key, "size_bytes": result.size_bytes })),
        )
            .into_response(),
        Err(e) => core_error_response(e),
    }
}

/// GET /namespaces/{ns_id}/objects/{*key} — retrieve bytes by key.
async fn get_object(
    State(state): State<ObjectState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, key)): Path<(String, String)>,
) -> impl IntoResponse {
    let service = make_service(&state);

    match service.get(&ns_id, &key, &auth.user.id).await {
        Ok(result) => (
            StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                result
                    .mime_type
                    .parse::<axum::http::HeaderValue>()
                    .unwrap_or_else(|_| "application/octet-stream".parse().unwrap()),
            )],
            result.bytes,
        )
            .into_response(),
        Err(e) => core_error_response(e),
    }
}

/// POST /namespaces/{ns_id}/objects/batch — retrieve multiple objects at once.
async fn batch_get_objects(
    State(state): State<ObjectState>,
    RequireAuth(auth): RequireAuth,
    Path(ns_id): Path<String>,
    Json(body): Json<BatchGetRequest>,
) -> impl IntoResponse {
    use base64::Engine as _;

    let service = make_service(&state);

    match service.get_batch(&ns_id, &body.keys, &auth.user.id).await {
        Ok(result) => {
            let objects: std::collections::HashMap<String, BatchObjectEntry> = result
                .objects
                .into_iter()
                .map(|(key, obj)| {
                    let entry = BatchObjectEntry {
                        data: base64::engine::general_purpose::STANDARD.encode(&obj.bytes),
                        mime_type: obj.mime_type,
                    };
                    (key, entry)
                })
                .collect();

            Json(BatchGetResponse {
                objects,
                errors: result.errors,
            })
            .into_response()
        }
        Err(e) => core_error_response(e),
    }
}

/// DELETE /namespaces/{ns_id}/objects/{*key} — delete an object.
async fn delete_object(
    State(state): State<ObjectState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, key)): Path<(String, String)>,
) -> impl IntoResponse {
    let service = make_service(&state);

    match service.delete(&ns_id, &key, &auth.user.id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => core_error_response(e),
    }
}

/// Pagination query parameters.
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
    /// Optional key prefix filter (e.g. `?prefix=files/`).
    #[serde(default)]
    pub prefix: Option<String>,
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
    let service = make_service(&state);

    match service
        .list(&ns_id, pagination.limit, pagination.offset, &auth.user.id)
        .await
    {
        Ok(objects) => {
            let response: Vec<ObjectMetaResponse> = objects
                .into_iter()
                .filter(|o| {
                    pagination
                        .prefix
                        .as_ref()
                        .map(|p| o.key.starts_with(p.as_str()))
                        .unwrap_or(true)
                })
                .map(ObjectMetaResponse::from)
                .collect();
            Json(response).into_response()
        }
        Err(e) => core_error_response(e),
    }
}

/// GET /usage — return aggregated usage totals for the authenticated user.
async fn get_usage(
    State(state): State<ObjectState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    let service = make_service(&state);

    match service.get_usage(&auth.user.id).await {
        Ok(totals) => Json(UsageResponse::from(totals)).into_response(),
        Err(e) => core_error_response(e),
    }
}

/// GET /namespaces/{ns_id}/usage — return usage totals scoped to a namespace.
async fn get_namespace_usage(
    State(state): State<ObjectState>,
    RequireAuth(auth): RequireAuth,
    Path(ns_id): Path<String>,
) -> impl IntoResponse {
    let service = make_service(&state);

    match service.get_namespace_usage(&ns_id, &auth.user.id).await {
        Ok(totals) => Json(UsageResponse::from(totals)).into_response(),
        Err(e) => core_error_response(e),
    }
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
    let service = make_service(&state);

    // Resolve access level via the portable service.
    let access = match service.resolve_public_access(&ns_id, &key).await {
        Ok(a) => a,
        Err(e) => return core_error_response(e),
    };

    // Enforce access control.
    match access.access.as_str() {
        "public" => { /* allowed */ }
        "token" => {
            let token_str = match &params.audience_token {
                Some(t) => t,
                None => return StatusCode::FORBIDDEN.into_response(),
            };
            match validate_signed_token(&state.token_signing_key, token_str) {
                Some(claims) if claims.slug == ns_id && claims.audience == access.audience_name => {
                    /* valid */
                }
                _ => return StatusCode::FORBIDDEN.into_response(),
            }
        }
        _ => return StatusCode::FORBIDDEN.into_response(), // "private" or unknown
    }

    // Fetch and serve the blob.
    match service
        .fetch_blob(&ns_id, &key, access.meta.blob_key.as_deref())
        .await
    {
        Ok(result) => (
            StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                result
                    .mime_type
                    .parse::<axum::http::HeaderValue>()
                    .unwrap_or_else(|_| "application/octet-stream".parse().unwrap()),
            )],
            result.bytes,
        )
            .into_response(),
        Err(e) => core_error_response(e),
    }
}
