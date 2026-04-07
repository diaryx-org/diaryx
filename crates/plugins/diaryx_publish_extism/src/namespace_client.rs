//! Thin HTTP client for namespace object operations on the sync server.
//!
//! All requests go through `host::http::request_binary()`, so auth tokens
//! are added transparently by the host runtime for same-domain requests.
//!
//! Note: `put_object`, `delete_object`, `list_objects`, and `sync_audience`
//! are now handled by `host::namespace` in publish_plugin.rs. These functions
//! are retained for potential CLI usage but are no longer called from the
//! plugin dispatch path.

#![allow(dead_code)]

use std::collections::HashMap;

use diaryx_plugin_sdk::host;
use serde::{Deserialize, Serialize};

/// Metadata for a single object in a namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub key: String,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

/// Metadata about a namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceInfo {
    pub id: String,
    pub owner_user_id: String,
    pub created_at: i64,
}

/// Metadata about a custom domain bound to a namespace audience.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainInfo {
    pub domain: String,
    pub namespace_id: String,
    pub audience_name: String,
    pub created_at: i64,
    pub verified: bool,
}

/// Sync an audience's access level on the server.
pub fn sync_audience(
    server_url: &str,
    namespace_id: &str,
    audience_name: &str,
    access: &str,
) -> Result<(), String> {
    let url = format!(
        "{}/namespaces/{}/audiences/{}",
        server_url.trim_end_matches('/'),
        namespace_id,
        urlencoding::encode(audience_name)
    );
    let body = serde_json::json!({ "access": access });
    let body_bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;

    let mut headers = HashMap::new();
    headers.insert("Content-Type".into(), "application/json".into());

    let resp = host::http::request_binary("PUT", &url, &headers, &body_bytes)?;

    if resp.status >= 400 {
        return Err(format!(
            "PUT audience {} returned status {}",
            audience_name, resp.status
        ));
    }
    Ok(())
}

/// Upload an object to a namespace.
pub fn put_object(
    server_url: &str,
    namespace_id: &str,
    key: &str,
    bytes: &[u8],
    mime_type: &str,
    audience: &str,
) -> Result<(), String> {
    let url = format!(
        "{}/namespaces/{}/objects/{}",
        server_url.trim_end_matches('/'),
        namespace_id,
        urlencoding::encode(key)
    );

    let mut headers = HashMap::new();
    headers.insert("Content-Type".into(), mime_type.into());
    headers.insert("X-Audience".into(), audience.into());

    let resp = host::http::request_binary("PUT", &url, &headers, bytes)?;

    if resp.status >= 400 {
        return Err(format!(
            "PUT object {} returned status {}",
            key, resp.status
        ));
    }
    Ok(())
}

/// List all objects in a namespace.
pub fn list_objects(server_url: &str, namespace_id: &str) -> Result<Vec<ObjectMeta>, String> {
    let url = format!(
        "{}/namespaces/{}/objects",
        server_url.trim_end_matches('/'),
        namespace_id
    );

    let resp = host::http::request_binary("GET", &url, &HashMap::new(), &[])?;

    if resp.status >= 400 {
        return Err(format!("GET objects returned status {}", resp.status));
    }

    serde_json::from_str(&resp.body).map_err(|e| e.to_string())
}

/// Delete a single object from a namespace.
pub fn delete_object(server_url: &str, namespace_id: &str, key: &str) -> Result<(), String> {
    let url = format!(
        "{}/namespaces/{}/objects/{}",
        server_url.trim_end_matches('/'),
        namespace_id,
        urlencoding::encode(key)
    );

    let resp = host::http::request_binary("DELETE", &url, &HashMap::new(), &[])?;

    if resp.status >= 400 {
        return Err(format!(
            "DELETE object {} returned status {}",
            key, resp.status
        ));
    }
    Ok(())
}

/// Create a new namespace, optionally with a specific ID.
pub fn create_namespace(server_url: &str, id: Option<&str>) -> Result<NamespaceInfo, String> {
    let url = format!("{}/namespaces", server_url.trim_end_matches('/'));
    let body = match id {
        Some(id) => serde_json::json!({ "id": id }),
        None => serde_json::json!({}),
    };
    let body_bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;

    let mut headers = HashMap::new();
    headers.insert("Content-Type".into(), "application/json".into());

    let resp = host::http::request_binary("POST", &url, &headers, &body_bytes)?;

    if resp.status >= 400 {
        return Err(format!("POST /namespaces returned status {}", resp.status));
    }
    serde_json::from_str(&resp.body).map_err(|e| e.to_string())
}

/// Get an audience token for accessing token-protected content.
pub fn get_audience_token(
    server_url: &str,
    namespace_id: &str,
    audience: &str,
) -> Result<serde_json::Value, String> {
    let url = format!(
        "{}/namespaces/{}/audiences/{}/token",
        server_url.trim_end_matches('/'),
        namespace_id,
        urlencoding::encode(audience)
    );

    let resp = host::http::request_binary("GET", &url, &HashMap::new(), &[])?;

    if resp.status >= 400 {
        return Err(format!(
            "GET audience token for {} returned status {}",
            audience, resp.status
        ));
    }
    serde_json::from_str(&resp.body).map_err(|e| e.to_string())
}

/// Register a custom domain for a namespace audience.
pub fn register_domain(
    server_url: &str,
    namespace_id: &str,
    domain: &str,
    audience_name: &str,
) -> Result<DomainInfo, String> {
    let url = format!(
        "{}/namespaces/{}/domains/{}",
        server_url.trim_end_matches('/'),
        namespace_id,
        urlencoding::encode(domain)
    );
    let body = serde_json::json!({ "audience_name": audience_name });
    let body_bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;

    let mut headers = HashMap::new();
    headers.insert("Content-Type".into(), "application/json".into());

    let resp = host::http::request_binary("PUT", &url, &headers, &body_bytes)?;

    if resp.status >= 400 {
        return Err(format!(
            "PUT domain {} returned status {}",
            domain, resp.status
        ));
    }
    serde_json::from_str(&resp.body).map_err(|e| e.to_string())
}

/// Remove a custom domain from a namespace.
pub fn remove_domain(server_url: &str, namespace_id: &str, domain: &str) -> Result<(), String> {
    let url = format!(
        "{}/namespaces/{}/domains/{}",
        server_url.trim_end_matches('/'),
        namespace_id,
        urlencoding::encode(domain)
    );

    let resp = host::http::request_binary("DELETE", &url, &HashMap::new(), &[])?;

    if resp.status >= 400 {
        return Err(format!(
            "DELETE domain {} returned status {}",
            domain, resp.status
        ));
    }
    Ok(())
}

/// Claim a subdomain for a namespace.
pub fn claim_subdomain(
    server_url: &str,
    namespace_id: &str,
    subdomain: &str,
    default_audience: Option<&str>,
) -> Result<serde_json::Value, String> {
    let url = format!(
        "{}/namespaces/{}/subdomain",
        server_url.trim_end_matches('/'),
        namespace_id
    );
    let mut body = serde_json::json!({ "subdomain": subdomain });
    if let Some(aud) = default_audience {
        body["default_audience"] = serde_json::Value::String(aud.to_string());
    }
    let body_bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;

    let mut headers = HashMap::new();
    headers.insert("Content-Type".into(), "application/json".into());

    let resp = host::http::request_binary("PUT", &url, &headers, &body_bytes)?;

    if resp.status >= 400 {
        // Try to extract error message from response body
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&resp.body) {
            if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
                return Err(err.to_string());
            }
        }
        return Err(format!("PUT subdomain returned status {}", resp.status));
    }
    serde_json::from_str(&resp.body).map_err(|e| e.to_string())
}

/// Release a subdomain from a namespace.
pub fn release_subdomain(server_url: &str, namespace_id: &str) -> Result<(), String> {
    let url = format!(
        "{}/namespaces/{}/subdomain",
        server_url.trim_end_matches('/'),
        namespace_id
    );

    let resp = host::http::request_binary("DELETE", &url, &HashMap::new(), &[])?;

    if resp.status >= 400 && resp.status != 404 {
        return Err(format!("DELETE subdomain returned status {}", resp.status));
    }
    Ok(())
}
