//! Thin HTTP handlers that wire CF Worker requests to portable services.

use crate::adapters::d1::*;
use crate::adapters::kv::KvDomainMappingCache;
use crate::adapters::r2::R2BlobStore;
use crate::config;
use crate::tokens::validate_audience_token;
use diaryx_server::ports::{Mailer, NamespaceStore, ServerCoreError};
use diaryx_server::use_cases::auth::{
    AuthConfig, AuthError, AuthenticationService, SessionValidationService, extract_token,
};
use diaryx_server::use_cases::{
    audiences::AudienceService, domains::DomainService, namespaces::NamespaceService,
    objects::ObjectService, sessions::SessionService,
};
use serde::Deserialize;
use worker::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

mod bindings {
    include!(concat!(env!("OUT_DIR"), "/wrangler_bindings.rs"));
}

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
    ctx.env.d1(bindings::D1_BINDING)
}

fn bucket(ctx: &RouteContext<()>) -> Result<Bucket> {
    ctx.env.bucket(bindings::R2_BINDING)
}

fn domains_kv(ctx: &RouteContext<()>) -> Result<worker::kv::KvStore> {
    ctx.env.kv(bindings::KV_BINDING)
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
    metadata: Option<serde_json::Value>,
}

pub async fn create_namespace(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let body: CreateNamespaceBody = req.json().await?;
    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let service = NamespaceService::new(&ns_store);
    let metadata_str = body.metadata.as_ref().map(|v| v.to_string());

    match service
        .create(&user_id, body.id.as_deref(), metadata_str.as_deref())
        .await
    {
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

#[derive(Deserialize)]
struct UpdateNamespaceBody {
    metadata: Option<serde_json::Value>,
}

pub async fn update_namespace(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let id = ctx.param("id").ok_or_else(|| Error::from("missing id"))?;
    let body: UpdateNamespaceBody = req.json().await?;
    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let service = NamespaceService::new(&ns_store);
    let metadata_str = body.metadata.as_ref().map(|v| v.to_string());

    match service
        .update_metadata(id, &user_id, metadata_str.as_deref())
        .await
    {
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

pub async fn list_objects(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();

    let url = req.url()?;
    let params: std::collections::HashMap<String, String> = url
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let limit: u32 = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let offset: u32 = params
        .get("offset")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let obj_store = D1ObjectMetaStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

    match service.list(&ns_id, limit, offset, &user_id).await {
        Ok(objects) => {
            let response: Vec<serde_json::Value> = objects
                .into_iter()
                .map(|m| {
                    serde_json::json!({
                        "namespace_id": m.namespace_id,
                        "key": m.key,
                        "r2_key": m.blob_key,
                        "mime_type": m.mime_type,
                        "size_bytes": m.size_bytes,
                        "updated_at": m.updated_at,
                        "audience": m.audience,
                    })
                })
                .collect();
            Response::from_json(&response)
        }
        Err(e) => error_response(e),
    }
}

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
// Domain handlers
// ---------------------------------------------------------------------------

pub async fn list_domains(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let _user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    match ns_store.list_custom_domains(&ns_id).await {
        Ok(domains) => {
            let response: Vec<serde_json::Value> = domains
                .into_iter()
                .map(|d| {
                    serde_json::json!({
                        "domain": d.domain,
                        "namespace_id": d.namespace_id,
                        "audience_name": d.audience_name,
                        "created_at": d.created_at,
                        "verified": d.verified,
                    })
                })
                .collect();
            Response::from_json(&response)
        }
        Err(e) => error_response(e),
    }
}

#[derive(Deserialize)]
struct RegisterDomainBody {
    audience_name: String,
}

pub async fn register_domain(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let _user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();
    let domain = ctx
        .param("domain")
        .ok_or_else(|| Error::from("missing domain"))?
        .to_string();
    let body: RegisterDomainBody = req.json().await?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let domain_cache = KvDomainMappingCache::new(domains_kv(&ctx)?);
    let service = DomainService::new(&ns_store, &domain_cache);

    let info = match service
        .register_domain(&ns_id, &domain, &body.audience_name)
        .await
    {
        Ok(info) => info,
        Err(e) => return error_response(e),
    };

    // Register as a Cloudflare custom hostname for automatic SSL.
    if let (Some(zone_id), Some(api_token)) =
        (config::cf_zone_id(&ctx.env), config::cf_api_token(&ctx.env))
    {
        let cf_url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/custom_hostnames",
            zone_id
        );
        let cf_body = serde_json::json!({
            "hostname": domain,
            "ssl": { "method": "http", "type": "dv" },
        });
        let headers = Headers::new();
        headers.set("Authorization", &format!("Bearer {}", api_token))?;
        headers.set("Content-Type", "application/json")?;
        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_body(Some(wasm_bindgen::JsValue::from_str(&cf_body.to_string())));
        match Fetch::Request(Request::new_with_init(&cf_url, &init)?)
            .send()
            .await
        {
            Ok(mut resp) => {
                if resp.status_code() >= 400 {
                    let text = resp.text().await.unwrap_or_default();
                    worker::console_log!(
                        "Cloudflare custom hostname creation failed for {}: {} {}",
                        domain,
                        resp.status_code(),
                        text
                    );
                }
            }
            Err(e) => {
                worker::console_log!("Cloudflare custom hostname API error for {}: {}", domain, e);
            }
        }
    }

    Response::from_json(&serde_json::json!({
        "domain": info.domain,
        "namespace_id": info.namespace_id,
        "audience_name": info.audience_name,
        "created_at": info.created_at,
        "verified": info.verified,
    }))
}

pub async fn remove_domain(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let _user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();
    let domain = ctx
        .param("domain")
        .ok_or_else(|| Error::from("missing domain"))?
        .to_string();

    // Find and delete the Cloudflare custom hostname first.
    if let (Some(zone_id), Some(api_token)) =
        (config::cf_zone_id(&ctx.env), config::cf_api_token(&ctx.env))
    {
        // Look up the custom hostname ID by hostname.
        let list_url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/custom_hostnames?hostname={}",
            zone_id, domain
        );
        let headers = Headers::new();
        headers.set("Authorization", &format!("Bearer {}", api_token))?;
        let mut init = RequestInit::new();
        init.with_method(Method::Get).with_headers(headers);
        if let Ok(mut resp) = Fetch::Request(Request::new_with_init(&list_url, &init)?)
            .send()
            .await
        {
            if let Ok(body) = resp.text().await {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) {
                    if let Some(results) = parsed["result"].as_array() {
                        for entry in results {
                            if let Some(ch_id) = entry["id"].as_str() {
                                let del_url = format!(
                                    "https://api.cloudflare.com/client/v4/zones/{}/custom_hostnames/{}",
                                    zone_id, ch_id
                                );
                                let del_headers = Headers::new();
                                del_headers
                                    .set("Authorization", &format!("Bearer {}", api_token))?;
                                let mut del_init = RequestInit::new();
                                del_init
                                    .with_method(Method::Delete)
                                    .with_headers(del_headers);
                                let _ =
                                    Fetch::Request(Request::new_with_init(&del_url, &del_init)?)
                                        .send()
                                        .await;
                            }
                        }
                    }
                }
            }
        }
    }

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let domain_cache = KvDomainMappingCache::new(domains_kv(&ctx)?);
    let service = DomainService::new(&ns_store, &domain_cache);

    match service.remove_domain(&ns_id, &domain).await {
        Ok(()) => Response::empty().map(|r| r.with_status(204)),
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Subdomain handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ClaimSubdomainBody {
    subdomain: String,
    #[serde(default)]
    default_audience: Option<String>,
}

pub async fn claim_subdomain(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let _user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();
    let body: ClaimSubdomainBody = req.json().await?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let domain_cache = KvDomainMappingCache::new(domains_kv(&ctx)?);
    let service = DomainService::new(&ns_store, &domain_cache);

    match service
        .claim_subdomain(&ns_id, &body.subdomain, body.default_audience.as_deref())
        .await
    {
        Ok(claimed) => Response::from_json(&serde_json::json!({
            "subdomain": claimed.subdomain,
            "namespace_id": claimed.namespace_id,
            "url": format!("https://{}.diaryx.org", claimed.subdomain),
        })),
        Err(e) => error_response(e),
    }
}

pub async fn release_subdomain(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let _user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let domain_cache = KvDomainMappingCache::new(domains_kv(&ctx)?);
    let service = DomainService::new(&ns_store, &domain_cache);

    match service.release_subdomain(&ns_id).await {
        Ok(_) => Response::empty().map(|r| r.with_status(204)),
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
        Ok(ctx) => {
            // Flatten to match the shape the frontend expects (MeResponse).
            let devices: Vec<serde_json::Value> = ctx
                .devices
                .into_iter()
                .map(|d| {
                    serde_json::json!({
                        "id": d.id,
                        "name": d.name,
                        "last_seen_at": d.last_seen_at.to_rfc3339(),
                    })
                })
                .collect();
            let workspaces: Vec<serde_json::Value> = ctx
                .namespaces
                .into_iter()
                .map(|ns| serde_json::json!({ "id": &ns.id, "name": &ns.id }))
                .collect();
            Response::from_json(&serde_json::json!({
                "user": { "id": auth.user.id, "email": ctx.user.email },
                "workspaces": workspaces,
                "devices": devices,
                "tier": ctx.user.tier.as_str(),
                "workspace_limit": ctx.limits.workspace_limit,
                "published_site_limit": ctx.limits.published_site_limit,
                "attachment_limit_bytes": ctx.limits.attachment_limit_bytes,
            }))
        }
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
        Err(AuthError::DeviceLimitReached { devices, .. }) => {
            Response::from_json(&serde_json::json!({
                "error": "Device limit reached. Remove a device to sign in on a new one.",
                "devices": devices,
            }))
            .map(|r| r.with_status(403))
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
        Err(AuthError::DeviceLimitReached { devices, .. }) => {
            Response::from_json(&serde_json::json!({
                "error": "Device limit reached. Remove a device to sign in on a new one.",
                "devices": devices,
            }))
            .map(|r| r.with_status(403))
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
