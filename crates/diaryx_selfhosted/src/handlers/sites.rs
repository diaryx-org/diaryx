//! Site serving handler — serves published namespace content directly.
//!
//! Route: `GET /sites/{ns_id}/{*path}` (mounted outside `/api`, no auth required)
//!
//! Path format: `/sites/{ns_id}/{audience}/{file_path}`
//! The first path segment after the namespace ID is the audience name.

use super::objects::ObjectState;
use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use diaryx_server::audience_token::{GateKind, validate_audience_token};
use diaryx_server::domain::GateRecord;
use diaryx_server::ports::ServerCoreError;
use diaryx_server::use_cases::objects::ObjectService;
use serde::Deserialize;

fn gate_check_passes(
    gates: &[GateRecord],
    audience_name: &str,
    slug: &str,
    signing_key: &[u8],
    supplied_token: Option<&str>,
) -> bool {
    if gates.is_empty() {
        return true;
    }
    let claims = supplied_token.and_then(|t| validate_audience_token(signing_key, t));
    for gate in gates {
        match gate {
            GateRecord::Link => {
                if let Some(ref c) = claims
                    && matches!(c.gate, GateKind::Link)
                    && c.slug == slug
                    && c.audience == audience_name
                {
                    return true;
                }
            }
            GateRecord::Password { version, .. } => {
                if let Some(ref c) = claims
                    && matches!(c.gate, GateKind::Unlock)
                    && c.slug == slug
                    && c.audience == audience_name
                    && c.password_version == Some(*version)
                {
                    return true;
                }
            }
        }
    }
    false
}

#[derive(Deserialize)]
struct SiteParams {
    audience_token: Option<String>,
}

pub fn site_routes(state: ObjectState) -> Router {
    Router::new()
        .route("/sites/{ns_id}/{*path}", get(serve_site))
        .with_state(state)
}

fn make_service(state: &ObjectState) -> ObjectService<'_> {
    ObjectService::new(
        state.namespace_store.as_ref(),
        state.object_meta_store.as_ref(),
        state.blob_store.as_ref(),
    )
}

async fn serve_site(
    State(state): State<ObjectState>,
    Path((ns_id, path)): Path<(String, String)>,
    Query(params): Query<SiteParams>,
) -> impl IntoResponse {
    // Normalize path: empty or "/" → "index.html"
    let path = if path.is_empty() || path == "/" {
        "index.html".to_string()
    } else {
        path.trim_start_matches('/').to_string()
    };

    let service = make_service(&state);

    // Try the path as-is first
    let (object_key, access) = match service.resolve_public_access(&ns_id, &path).await {
        Ok(a) => (path.clone(), a),
        Err(_) => {
            // If no file extension, try appending /index.html
            if !path.contains('.') || path.ends_with('/') {
                let index_path = format!("{}/index.html", path.trim_end_matches('/'));
                match service.resolve_public_access(&ns_id, &index_path).await {
                    Ok(a) => (index_path, a),
                    Err(_) => return StatusCode::NOT_FOUND.into_response(),
                }
            } else {
                return StatusCode::NOT_FOUND.into_response();
            }
        }
    };

    // Enforce the audience's gate stack. Short-circuit OR: any satisfied gate grants access.
    if !gate_check_passes(
        &access.gates,
        &access.audience_name,
        &ns_id,
        &state.token_signing_key,
        params.audience_token.as_deref(),
    ) {
        return StatusCode::FORBIDDEN.into_response();
    }

    // Fetch and serve the blob
    match service
        .fetch_blob(&ns_id, &object_key, access.meta.blob_key.as_deref())
        .await
    {
        Ok(result) => {
            let content_type = result
                .mime_type
                .parse::<axum::http::HeaderValue>()
                .unwrap_or_else(|_| "application/octet-stream".parse().unwrap());
            (
                StatusCode::OK,
                [
                    (axum::http::header::CONTENT_TYPE, content_type),
                    (
                        axum::http::header::CACHE_CONTROL,
                        "public, max-age=60".parse().unwrap(),
                    ),
                ],
                result.bytes,
            )
                .into_response()
        }
        Err(e) => match e {
            ServerCoreError::NotFound(_) => StatusCode::NOT_FOUND.into_response(),
            _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
    }
}
