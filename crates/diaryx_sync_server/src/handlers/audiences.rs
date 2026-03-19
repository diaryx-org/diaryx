//! Audience visibility handlers — `PUT/GET/DELETE /namespaces/{id}/audiences/{name}`.

use super::require_namespace_owner;
use crate::auth::RequireAuth;
use crate::blob_store::BlobStore;
use crate::db::{AudienceInfo, NamespaceRepo};
use crate::tokens::create_signed_token;
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

/// Shared state for audience handlers.
#[derive(Clone)]
pub struct AudienceState {
    pub ns_repo: Arc<NamespaceRepo>,
    /// HMAC-SHA256 key used to sign audience access tokens.
    pub token_signing_key: Vec<u8>,
    /// Blob store for writing `_audiences.json` metadata to R2.
    pub blob_store: Arc<dyn BlobStore>,
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SetAudienceRequest {
    /// "public" | "token" | "private"
    pub access: String,
}

#[derive(Debug, Serialize)]
pub struct AudienceResponse {
    pub namespace_id: String,
    pub name: String,
    pub access: String,
}

impl From<AudienceInfo> for AudienceResponse {
    fn from(a: AudienceInfo) -> Self {
        Self {
            namespace_id: a.namespace_id,
            name: a.audience_name,
            access: a.access,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub token: String,
}

// ---------------------------------------------------------------------------
// Router (mounted under /namespaces/{ns_id})
// ---------------------------------------------------------------------------

pub fn audience_routes(state: AudienceState) -> Router {
    Router::new()
        .route("/audiences", get(list_audiences))
        .route(
            "/audiences/{name}",
            put(set_audience).delete(delete_audience),
        )
        .route("/audiences/{name}/token", get(get_audience_token))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write `ns/{ns_id}/_audiences.json` to R2 with the current audience map.
/// Best-effort — errors are logged but do not fail the request.
async fn write_audiences_meta(ns_repo: &NamespaceRepo, blob_store: &dyn BlobStore, ns_id: &str) {
    let audiences = ns_repo.list_audiences(ns_id);
    let map: serde_json::Map<String, serde_json::Value> = audiences
        .into_iter()
        .map(|a| (a.audience_name, serde_json::Value::String(a.access)))
        .collect();
    let json = serde_json::to_vec(&map).unwrap_or_default();
    let key = format!("ns/{}/_audiences.json", ns_id);
    if let Err(e) = blob_store.put(&key, &json, "application/json", None).await {
        warn!(
            "Failed to write audiences metadata to R2 for {}: {}",
            ns_id, e
        );
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// PUT /namespaces/{ns_id}/audiences/{name} — set access level for an audience.
async fn set_audience(
    State(state): State<AudienceState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, name)): Path<(String, String)>,
    Json(req): Json<SetAudienceRequest>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    let access = req.access.as_str();
    if !matches!(access, "public" | "token" | "private") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "access must be 'public', 'token', or 'private'" })),
        )
            .into_response();
    }

    match state.ns_repo.upsert_audience(&ns_id, &name, access) {
        Ok(()) => {
            let info = state
                .ns_repo
                .get_audience(&ns_id, &name)
                .expect("just upserted");
            write_audiences_meta(&state.ns_repo, &*state.blob_store, &ns_id).await;
            Json(AudienceResponse::from(info)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /namespaces/{ns_id}/audiences — list all audiences for a namespace.
async fn list_audiences(
    State(state): State<AudienceState>,
    RequireAuth(auth): RequireAuth,
    Path(ns_id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    let audiences: Vec<AudienceResponse> = state
        .ns_repo
        .list_audiences(&ns_id)
        .into_iter()
        .map(AudienceResponse::from)
        .collect();

    Json(audiences).into_response()
}

/// GET /namespaces/{ns_id}/audiences/{name}/token — generate a signed access token.
async fn get_audience_token(
    State(state): State<AudienceState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, name)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    match state.ns_repo.get_audience(&ns_id, &name) {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(audience) if audience.access == "public" => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "audience is public; no token needed" })),
        )
            .into_response(),
        Some(_) => {
            let token_id = Uuid::new_v4().to_string();
            match create_signed_token(
                &state.token_signing_key,
                &ns_id,
                &name,
                &token_id,
                None, // no expiry
            ) {
                Ok(token) => Json(TokenResponse { token }).into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
    }
}

/// DELETE /namespaces/{ns_id}/audiences/{name} — remove an audience record.
async fn delete_audience(
    State(state): State<AudienceState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, name)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    match state.ns_repo.get_audience(&ns_id, &name) {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(_) => {
            // NULL out audience on objects that reference this audience.
            let _ = state.ns_repo.clear_objects_audience(&ns_id, &name);
            match state.ns_repo.delete_audience(&ns_id, &name) {
                Ok(()) => {
                    write_audiences_meta(&state.ns_repo, &*state.blob_store, &ns_id).await;
                    StatusCode::NO_CONTENT.into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
    }
}
