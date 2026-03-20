//! Thin HTTP handlers that wire CF Worker requests to portable services.

use crate::adapters::d1::*;
use crate::adapters::r2::R2BlobStore;
use crate::adapters::resend::ResendMailer;
use crate::config;
use crate::tokens::validate_audience_token;
use diaryx_server::ports::{Mailer, ServerCoreError};
use diaryx_server::use_cases::auth::{
    AuthConfig, AuthenticationService, SessionValidationService, extract_token,
};
use diaryx_server::use_cases::{
    audiences::AudienceService, namespaces::NamespaceService, objects::ObjectService,
    sessions::SessionService,
};
use serde::Deserialize;
use worker::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn error_response(err: ServerCoreError) -> Result<Response> {
    let (status, msg) = match &err {
        ServerCoreError::NotFound(m) => (404, m.as_str()),
        ServerCoreError::PermissionDenied(m) => (403, m.as_str()),
        ServerCoreError::InvalidInput(m) => (400, m.as_str()),
        ServerCoreError::Conflict(m) => (409, m.as_str()),
        ServerCoreError::RateLimited(m) => (429, m.as_str()),
        ServerCoreError::Unavailable(m) => (503, m.as_str()),
        ServerCoreError::Internal(m) => (500, m.as_str()),
    };
    Response::from_json(&serde_json::json!({ "error": msg })).map(|r| r.with_status(status))
}

fn db(ctx: &RouteContext<()>) -> Result<D1Database> {
    ctx.env.d1("DB")
}

fn bucket(ctx: &RouteContext<()>) -> Result<Bucket> {
    ctx.env.bucket("OBJECTS")
}

fn auth_cfg(ctx: &RouteContext<()>) -> AuthConfig {
    config::auth_config(&ctx.env)
}

fn signing_key(ctx: &RouteContext<()>) -> Vec<u8> {
    config::token_signing_key(&ctx.env)
}

/// Build a Set-Cookie header value for a session token.
fn set_session_cookie(token: &str, env: &Env) -> String {
    let max_age_secs = config::session_expiry_days(env) * 86400;
    if config::secure_cookies(env) {
        format!(
            "diaryx_session={token}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age={max_age_secs}"
        )
    } else {
        format!("diaryx_session={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age_secs}")
    }
}

/// Authenticate the request, returning the user ID on success.
async fn authenticate(req: &Request, ctx: &RouteContext<()>) -> Result<String> {
    let auth_store = D1AuthStore::new(db(ctx)?);
    let session_store = D1AuthSessionStore::new(db(ctx)?);

    let authorization = req.headers().get("Authorization")?;
    let cookie = req.headers().get("Cookie")?;
    let query = req.url()?.query().map(|s| s.to_string());

    let token = extract_token(
        authorization.as_deref(),
        cookie.as_deref(),
        query.as_deref(),
    )
    .ok_or_else(|| Error::from("Authentication required"))?;

    let service = SessionValidationService::new(&auth_store, &session_store);
    let auth_ctx = service
        .validate(&token)
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    Ok(auth_ctx.user.id)
}

// ---------------------------------------------------------------------------
// Namespace handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateNamespaceBody {
    id: Option<String>,
}

pub async fn create_namespace(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let body: CreateNamespaceBody = req.json().await?;
    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let service = NamespaceService::new(&ns_store);

    match service.create(&user_id, body.id.as_deref()).await {
        Ok(ns) => Response::from_json(&ns).map(|r| r.with_status(201)),
        Err(e) => error_response(e),
    }
}

pub async fn list_namespaces(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let service = NamespaceService::new(&ns_store);

    match service.list(&user_id, 100, 0).await {
        Ok(list) => Response::from_json(&list),
        Err(e) => error_response(e),
    }
}

pub async fn get_namespace(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let id = ctx.param("id").ok_or_else(|| Error::from("missing id"))?;
    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let service = NamespaceService::new(&ns_store);

    match service.get(id, &user_id).await {
        Ok(ns) => Response::from_json(&ns),
        Err(e) => error_response(e),
    }
}

pub async fn delete_namespace(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let id = ctx.param("id").ok_or_else(|| Error::from("missing id"))?;
    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let service = NamespaceService::new(&ns_store);

    match service.delete(id, &user_id).await {
        Ok(()) => Response::empty().map(|r| r.with_status(204)),
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Object handlers
// ---------------------------------------------------------------------------

pub async fn put_object(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();
    let key = ctx
        .param("key")
        .ok_or_else(|| Error::from("missing key"))?
        .to_string();

    let mime_type = req
        .headers()
        .get("content-type")?
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let audience = req.headers().get("x-audience")?;
    let bytes = req.bytes().await?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let obj_store = D1ObjectMetaStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

    match service
        .put(
            &ns_id,
            &key,
            &mime_type,
            &bytes,
            audience.as_deref(),
            &user_id,
        )
        .await
    {
        Ok(result) => Response::from_json(
            &serde_json::json!({ "key": result.key, "size_bytes": result.size_bytes }),
        ),
        Err(e) => error_response(e),
    }
}

pub async fn get_object(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?;
    let key = ctx.param("key").ok_or_else(|| Error::from("missing key"))?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let obj_store = D1ObjectMetaStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

    match service.get(ns_id, key, &user_id).await {
        Ok(result) => {
            let mut resp = Response::from_bytes(result.bytes)?;
            resp.headers_mut().set("content-type", &result.mime_type)?;
            Ok(resp)
        }
        Err(e) => error_response(e),
    }
}

pub async fn delete_object(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?;
    let key = ctx.param("key").ok_or_else(|| Error::from("missing key"))?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let obj_store = D1ObjectMetaStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

    match service.delete(ns_id, key, &user_id).await {
        Ok(()) => Response::empty().map(|r| r.with_status(204)),
        Err(e) => error_response(e),
    }
}

pub async fn get_public_object(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?;
    let key = ctx.param("key").ok_or_else(|| Error::from("missing key"))?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let obj_store = D1ObjectMetaStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

    let access = match service.resolve_public_access(ns_id, key).await {
        Ok(a) => a,
        Err(e) => return error_response(e),
    };

    // Enforce access control
    match access.access.as_str() {
        "public" => {}
        "token" => {
            let url = req.url()?;
            let token_str = url
                .query_pairs()
                .find(|(k, _)| k == "audience_token")
                .map(|(_, v)| v.to_string());

            let key_bytes = signing_key(&ctx);
            let valid = token_str
                .as_deref()
                .and_then(|t| validate_audience_token(&key_bytes, t))
                .is_some_and(|claims| {
                    claims.slug == *ns_id && claims.audience == access.audience_name
                });

            if !valid {
                return Response::empty().map(|r| r.with_status(403));
            }
        }
        _ => return Response::empty().map(|r| r.with_status(403)),
    }

    match service
        .fetch_blob(ns_id, key, access.meta.blob_key.as_deref())
        .await
    {
        Ok(result) => {
            let mut resp = Response::from_bytes(result.bytes)?;
            resp.headers_mut().set("content-type", &result.mime_type)?;
            Ok(resp)
        }
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Audience handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SetAudienceBody {
    access: String,
}

pub async fn set_audience(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();
    let name = ctx
        .param("name")
        .ok_or_else(|| Error::from("missing name"))?
        .to_string();
    let body: SetAudienceBody = req.json().await?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = AudienceService::new(&ns_store, &blob_store);

    match service.set(&ns_id, &name, &body.access, &user_id).await {
        Ok(info) => Response::from_json(&info),
        Err(e) => error_response(e),
    }
}

pub async fn list_audiences(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = AudienceService::new(&ns_store, &blob_store);

    match service.list(ns_id, &user_id).await {
        Ok(list) => Response::from_json(&list),
        Err(e) => error_response(e),
    }
}

pub async fn delete_audience(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();
    let name = ctx
        .param("name")
        .ok_or_else(|| Error::from("missing name"))?
        .to_string();

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = AudienceService::new(&ns_store, &blob_store);

    match service.delete(&ns_id, &name, &user_id).await {
        Ok(()) => Response::empty().map(|r| r.with_status(204)),
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Session handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateSessionBody {
    namespace_id: String,
    #[serde(default)]
    read_only: bool,
}

pub async fn create_session(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let body: CreateSessionBody = req.json().await?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let session_store = D1SessionStore::new(db(&ctx)?);
    let service = SessionService::new(&ns_store, &session_store);

    match service
        .create(&body.namespace_id, &user_id, body.read_only)
        .await
    {
        Ok(session) => Response::from_json(&serde_json::json!({
            "code": session.code,
            "namespace_id": session.namespace_id,
            "read_only": session.read_only,
        })),
        Err(e) => error_response(e),
    }
}

pub async fn get_session(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let code = ctx
        .param("code")
        .ok_or_else(|| Error::from("missing code"))?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let session_store = D1SessionStore::new(db(&ctx)?);
    let service = SessionService::new(&ns_store, &session_store);

    match service.get(code).await {
        Ok(session) => Response::from_json(&serde_json::json!({
            "code": session.code,
            "namespace_id": session.namespace_id,
            "read_only": session.read_only,
        })),
        Err(e) => error_response(e),
    }
}

pub async fn delete_session(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let code = ctx
        .param("code")
        .ok_or_else(|| Error::from("missing code"))?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let session_store = D1SessionStore::new(db(&ctx)?);
    let service = SessionService::new(&ns_store, &session_store);

    match service.delete(code, &user_id).await {
        Ok(()) => Response::empty().map(|r| r.with_status(204)),
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Auth handlers
// ---------------------------------------------------------------------------

pub async fn get_current_user(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let auth_store = D1AuthStore::new(db(&ctx)?);
    let session_store = D1AuthSessionStore::new(db(&ctx)?);
    let ns_store = D1NamespaceStore::new(db(&ctx)?);

    let authorization = req.headers().get("Authorization")?;
    let cookie = req.headers().get("Cookie")?;
    let query = req.url()?.query().map(|s| s.to_string());

    let token = extract_token(
        authorization.as_deref(),
        cookie.as_deref(),
        query.as_deref(),
    )
    .ok_or_else(|| Error::from("Authentication required"))?;

    let validation = SessionValidationService::new(&auth_store, &session_store);
    let auth = validation
        .validate(&token)
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    let current_user =
        diaryx_server::use_cases::current_user::CurrentUserService::new(&auth_store, &ns_store);
    match current_user.load(&auth.user.id, &auth.user.email).await {
        Ok(ctx) => Response::from_json(&ctx),
        Err(e) => error_response(e),
    }
}

pub async fn logout(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let auth_store = D1AuthStore::new(db(&ctx)?);
    let session_store = D1AuthSessionStore::new(db(&ctx)?);

    let authorization = req.headers().get("Authorization")?;
    let cookie = req.headers().get("Cookie")?;
    let query = req.url()?.query().map(|s| s.to_string());

    let token = extract_token(
        authorization.as_deref(),
        cookie.as_deref(),
        query.as_deref(),
    )
    .ok_or_else(|| Error::from("Authentication required"))?;

    let service = SessionValidationService::new(&auth_store, &session_store);
    service
        .logout(&token)
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    let mut resp = Response::empty()?.with_status(204);
    let clear_cookie = if config::secure_cookies(&ctx.env) {
        "diaryx_session=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0"
    } else {
        "diaryx_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0"
    };
    resp.headers_mut().set("Set-Cookie", clear_cookie)?;
    Ok(resp)
}

#[derive(Deserialize)]
struct MagicLinkBody {
    email: String,
}

pub async fn request_magic_link(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: MagicLinkBody = req.json().await?;
    let cfg = auth_cfg(&ctx);

    let ml_store = D1MagicLinkStore::new(db(&ctx)?);
    let user_store = D1UserStore::new(db(&ctx)?);
    let device_store = D1DeviceStore::new(db(&ctx)?);
    let session_store = D1AuthSessionStore::new(db(&ctx)?);

    let service =
        AuthenticationService::new(&ml_store, &user_store, &device_store, &session_store, &cfg);

    let (token, code) = match service.request_magic_link(&body.email).await {
        Ok(result) => result,
        Err(e) => {
            return Response::from_json(&serde_json::json!({ "error": e.to_string() }))
                .map(|r| r.with_status(400));
        }
    };

    let app_url = config::app_base_url(&ctx.env);
    let magic_link_url = format!("{}?token={}", app_url, token);

    // Try to send email via Resend
    if let Some(mailer) = config::mailer(&ctx.env, cfg.magic_link_expiry_minutes) {
        if let Err(e) = mailer
            .send_magic_link(&body.email, &magic_link_url, &code)
            .await
        {
            return Response::from_json(
                &serde_json::json!({ "error": format!("Failed to send email: {}", e) }),
            )
            .map(|r| r.with_status(500));
        }

        Response::from_json(&serde_json::json!({
            "success": true,
            "message": "Check your email for a sign-in link.",
        }))
    } else {
        // Dev mode: return the link and code directly
        Response::from_json(&serde_json::json!({
            "success": true,
            "message": "Email not configured. Use the dev link below.",
            "dev_link": magic_link_url,
            "dev_code": code,
        }))
    }
}

#[derive(Deserialize)]
struct VerifyQuery {
    token: String,
    device_name: Option<String>,
    replace_device_id: Option<String>,
}

pub async fn verify_magic_link(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let url = req.url()?;
    let query: VerifyQuery =
        serde_qs::from_str(url.query().unwrap_or("")).map_err(|e| Error::from(e.to_string()))?;
    let cfg = auth_cfg(&ctx);

    let ml_store = D1MagicLinkStore::new(db(&ctx)?);
    let user_store = D1UserStore::new(db(&ctx)?);
    let device_store = D1DeviceStore::new(db(&ctx)?);
    let session_store = D1AuthSessionStore::new(db(&ctx)?);

    let service =
        AuthenticationService::new(&ml_store, &user_store, &device_store, &session_store, &cfg);

    match service
        .verify_magic_link(
            &query.token,
            query.device_name.as_deref(),
            None,
            query.replace_device_id.as_deref(),
        )
        .await
    {
        Ok(result) => {
            let cookie = set_session_cookie(&result.session_token, &ctx.env);
            let mut resp = Response::from_json(&serde_json::json!({
                "success": true,
                "token": result.session_token,
                "user": { "id": result.user_id, "email": result.email },
            }))?;
            resp.headers_mut().set("Set-Cookie", &cookie)?;
            Ok(resp)
        }
        Err(e) => Response::from_json(&serde_json::json!({ "error": e.to_string() }))
            .map(|r| r.with_status(400)),
    }
}

#[derive(Deserialize)]
struct VerifyCodeBody {
    code: String,
    email: String,
    device_name: Option<String>,
    replace_device_id: Option<String>,
}

pub async fn verify_code(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: VerifyCodeBody = req.json().await?;
    let cfg = auth_cfg(&ctx);

    let ml_store = D1MagicLinkStore::new(db(&ctx)?);
    let user_store = D1UserStore::new(db(&ctx)?);
    let device_store = D1DeviceStore::new(db(&ctx)?);
    let session_store = D1AuthSessionStore::new(db(&ctx)?);

    let service =
        AuthenticationService::new(&ml_store, &user_store, &device_store, &session_store, &cfg);

    match service
        .verify_code(
            &body.code,
            &body.email,
            body.device_name.as_deref(),
            None,
            body.replace_device_id.as_deref(),
        )
        .await
    {
        Ok(result) => {
            let cookie = set_session_cookie(&result.session_token, &ctx.env);
            let mut resp = Response::from_json(&serde_json::json!({
                "success": true,
                "token": result.session_token,
                "user": { "id": result.user_id, "email": result.email },
            }))?;
            resp.headers_mut().set("Set-Cookie", &cookie)?;
            Ok(resp)
        }
        Err(e) => Response::from_json(&serde_json::json!({ "error": e.to_string() }))
            .map(|r| r.with_status(400)),
    }
}

// ---------------------------------------------------------------------------
// Usage
// ---------------------------------------------------------------------------

pub async fn get_usage(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let obj_store = D1ObjectMetaStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

    match service.get_usage(&user_id).await {
        Ok(totals) => Response::from_json(&totals),
        Err(e) => error_response(e),
    }
}
