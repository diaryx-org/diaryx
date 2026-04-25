//! Synchronous HTTP-backed [`NamespaceProvider`] for driving a real Diaryx
//! sync server from plugin host contexts (CLI, Tauri, plugin E2E tests).
//!
//! The trait is sync because Extism host functions run in a synchronous WASM
//! guest context, so this uses `ureq` rather than an async HTTP client. The
//! logic was originally extracted from the CLI's private `CliNamespaceProvider`
//! to make it reusable by E2E tests (see
//! `crates/plugins/diaryx_sync_extism/tests/sync_e2e.rs`) and eventually by
//! the CLI itself.
//!
//! # URL contract
//!
//! `base_url` must include whatever API prefix the target server expects (e.g.
//! `http://localhost:3030/api` for `diaryx_sync_server`,
//! `http://localhost:8789/api` for `diaryx_cloudflare` under `wrangler dev`).
//! The provider appends `/namespaces`, `/namespaces/{id}/objects/{key}`, etc.
//!
//! # Auth
//!
//! If `auth_token` is `Some`, every request gets an `Authorization: Bearer
//! <token>` header. Pass `None` for unauthenticated servers.

use std::time::Duration;

use crate::host_fns::{
    BatchGetEntry, BatchGetResult, NamespaceEntry, NamespaceObjectMeta, NamespaceProvider,
    parse_multipart_batch,
};

/// Sync HTTP client that talks to the Diaryx namespace API and satisfies
/// [`NamespaceProvider`]. Cheap to clone via `Arc`.
///
/// See module docs for URL and auth conventions.
pub struct HttpNamespaceProvider {
    base_url: String,
    auth_token: Option<String>,
    agent: ureq::Agent,
}

impl HttpNamespaceProvider {
    /// Create with an explicit base URL and optional Bearer token.
    pub fn new(base_url: impl Into<String>, auth_token: Option<String>) -> Self {
        Self::with_timeout(base_url, auth_token, Duration::from_secs(120))
    }

    pub fn with_timeout(
        base_url: impl Into<String>,
        auth_token: Option<String>,
        timeout: Duration,
    ) -> Self {
        let mut base_url = base_url.into();
        while base_url.ends_with('/') {
            base_url.pop();
        }
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .build()
            .into();
        Self {
            base_url,
            auth_token,
            agent,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn auth_token(&self) -> Option<&str> {
        self.auth_token.as_deref()
    }

    fn encode_component(value: &str) -> String {
        urlencoding::encode(value).into_owned()
    }

    /// URL-encodes each path segment in a slash-separated key so multi-segment
    /// keys like `files/subdir/note.md` round-trip correctly. Mirrors the CLI
    /// provider's behavior — both must agree, or server-side routing diverges.
    fn encode_key(key: &str) -> String {
        key.split('/')
            .map(Self::encode_component)
            .collect::<Vec<_>>()
            .join("/")
    }

    fn request_bytes(&self, url: String) -> Result<Vec<u8>, String> {
        let mut builder = ureq::http::Request::builder().method("GET").uri(&url);
        if let Some(token) = &self.auth_token {
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }
        let request = builder
            .body(())
            .map_err(|e| format!("Failed to build namespace request: {e}"))?;
        let response = self
            .agent
            .run(request)
            .map_err(|e| format!("Namespace request failed: {e}"))?;
        let status = response.status();
        if !status.is_success() {
            let text = response.into_body().read_to_string().unwrap_or_default();
            return Err(if text.is_empty() {
                format!("Namespace request failed with status {status}")
            } else {
                text
            });
        }
        response
            .into_body()
            .with_config()
            .limit(100 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| format!("Failed to read namespace response: {e}"))
    }

    fn request_multipart_batch(
        &self,
        url: String,
        body: Vec<u8>,
    ) -> Result<BatchGetResult, String> {
        let mut builder = ureq::http::Request::builder()
            .method("POST")
            .uri(&url)
            .header("Content-Type", "application/json");
        if let Some(token) = &self.auth_token {
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }
        let request = builder
            .body(body)
            .map_err(|e| format!("Failed to build multipart batch request: {e}"))?;
        let response = self
            .agent
            .run(request)
            .map_err(|e| format!("Multipart batch request failed: {e}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "Multipart batch request failed with status {status}"
            ));
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let boundary = content_type
            .split("boundary=")
            .nth(1)
            .ok_or_else(|| "Missing boundary in multipart response".to_string())?
            .trim()
            .to_string();
        let resp_bytes = response
            .into_body()
            .with_config()
            .limit(100 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| format!("Failed to read multipart response: {e}"))?;
        Ok(parse_multipart_batch(&resp_bytes, &boundary))
    }

    fn request_json<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        url: String,
        body: Option<Vec<u8>>,
        content_type: Option<&str>,
        audience: Option<&str>,
    ) -> Result<Option<T>, String> {
        let mut builder = ureq::http::Request::builder().method(method).uri(&url);
        if let Some(token) = &self.auth_token {
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }
        if let Some(ct) = content_type {
            builder = builder.header("Content-Type", ct);
        }
        if let Some(aud) = audience {
            builder = builder.header("X-Audience", aud);
        }
        let response = if let Some(body) = body {
            let request = builder
                .body(body)
                .map_err(|e| format!("Failed to build namespace request: {e}"))?;
            self.agent
                .run(request)
                .map_err(|e| format!("Namespace request failed: {e}"))?
        } else {
            let request = builder
                .body(())
                .map_err(|e| format!("Failed to build namespace request: {e}"))?;
            self.agent
                .run(request)
                .map_err(|e| format!("Namespace request failed: {e}"))?
        };
        let status = response.status();
        if !status.is_success() {
            let text = response.into_body().read_to_string().unwrap_or_default();
            return Err(if text.is_empty() {
                format!("Namespace request failed with status {status}")
            } else {
                text
            });
        }
        if status == ureq::http::StatusCode::NO_CONTENT {
            return Ok(None);
        }
        let bytes = response
            .into_body()
            .read_to_vec()
            .map_err(|e| format!("Failed to read namespace response: {e}"))?;
        if bytes.is_empty() {
            return Ok(None);
        }
        serde_json::from_slice::<T>(&bytes)
            .map(Some)
            .map_err(|e| format!("Failed to parse namespace response JSON: {e}"))
    }
}

impl NamespaceProvider for HttpNamespaceProvider {
    fn create_namespace(
        &self,
        metadata: Option<&serde_json::Value>,
    ) -> Result<NamespaceEntry, String> {
        let url = format!("{}/namespaces", self.base_url);
        let body = serde_json::to_vec(&serde_json::json!({ "metadata": metadata }))
            .map_err(|e| format!("Failed to serialize namespace request: {e}"))?;
        self.request_json::<NamespaceEntry>(
            "POST",
            url,
            Some(body),
            Some("application/json"),
            None,
        )?
        .ok_or_else(|| "Namespace create returned an empty response".to_string())
    }

    fn put_object(
        &self,
        ns_id: &str,
        key: &str,
        bytes: &[u8],
        mime_type: &str,
        audience: Option<&str>,
    ) -> Result<(), String> {
        let url = format!(
            "{}/namespaces/{}/objects/{}",
            self.base_url,
            Self::encode_component(ns_id),
            Self::encode_key(key)
        );
        self.request_json::<serde_json::Value>(
            "PUT",
            url,
            Some(bytes.to_vec()),
            Some(mime_type),
            audience,
        )?;
        Ok(())
    }

    fn get_object(&self, ns_id: &str, key: &str) -> Result<Vec<u8>, String> {
        let url = format!(
            "{}/namespaces/{}/objects/{}",
            self.base_url,
            Self::encode_component(ns_id),
            Self::encode_key(key)
        );
        self.request_bytes(url)
    }

    fn delete_object(&self, ns_id: &str, key: &str) -> Result<(), String> {
        let url = format!(
            "{}/namespaces/{}/objects/{}",
            self.base_url,
            Self::encode_component(ns_id),
            Self::encode_key(key)
        );
        self.request_json::<serde_json::Value>("DELETE", url, None, None, None)?;
        Ok(())
    }

    fn list_objects(
        &self,
        ns_id: &str,
        prefix: Option<&str>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<NamespaceObjectMeta>, String> {
        let mut url = format!(
            "{}/namespaces/{}/objects",
            self.base_url,
            Self::encode_component(ns_id)
        );
        let mut query = Vec::new();
        if let Some(prefix) = prefix {
            query.push(format!("prefix={}", Self::encode_component(prefix)));
        }
        if let Some(limit) = limit {
            query.push(format!("limit={limit}"));
        }
        if let Some(offset) = offset {
            query.push(format!("offset={offset}"));
        }
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query.join("&"));
        }
        Ok(self
            .request_json::<Vec<NamespaceObjectMeta>>("GET", url, None, None, None)?
            .unwrap_or_default())
    }

    fn sync_audience(
        &self,
        ns_id: &str,
        audience: &str,
        gates: &serde_json::Value,
    ) -> Result<(), String> {
        let url = format!(
            "{}/namespaces/{}/audiences/{}",
            self.base_url,
            Self::encode_component(ns_id),
            Self::encode_component(audience)
        );
        let body = serde_json::to_vec(&serde_json::json!({ "gates": gates }))
            .map_err(|e| format!("Failed to serialize audience request: {e}"))?;
        self.request_json::<serde_json::Value>(
            "PUT",
            url,
            Some(body),
            Some("application/json"),
            None,
        )?;
        Ok(())
    }

    fn get_objects_batch(&self, ns_id: &str, keys: &[String]) -> Result<BatchGetResult, String> {
        let body = serde_json::to_vec(&serde_json::json!({ "keys": keys }))
            .map_err(|e| format!("Failed to serialize batch request: {e}"))?;

        // Prefer the multipart endpoint (no base64 overhead). Fall back to the
        // JSON+base64 endpoint on any failure so this works against older
        // server builds.
        let multipart_url = format!(
            "{}/namespaces/{}/batch/objects/multipart",
            self.base_url,
            Self::encode_component(ns_id),
        );
        if let Ok(result) = self.request_multipart_batch(multipart_url, body.clone()) {
            return Ok(result);
        }

        use base64::Engine as _;
        let url = format!(
            "{}/namespaces/{}/batch/objects",
            self.base_url,
            Self::encode_component(ns_id),
        );
        let resp: serde_json::Value = self
            .request_json("POST", url, Some(body), Some("application/json"), None)?
            .ok_or_else(|| "Batch get returned an empty response".to_string())?;

        let mut result = BatchGetResult::default();
        if let Some(objects) = resp.get("objects").and_then(|v| v.as_object()) {
            for (key, entry) in objects {
                let data = entry
                    .get("data")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| format!("Missing data for key {key}"))?;
                let mime_type = entry
                    .get("mime_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let encoding = entry
                    .get("encoding")
                    .and_then(|v| v.as_str())
                    .unwrap_or("base64");
                let bytes = if encoding == "text" {
                    data.as_bytes().to_vec()
                } else {
                    base64::engine::general_purpose::STANDARD
                        .decode(data)
                        .map_err(|e| format!("Failed to decode base64 for {key}: {e}"))?
                };
                result
                    .objects
                    .insert(key.clone(), BatchGetEntry { bytes, mime_type });
            }
        }
        if let Some(errors) = resp.get("errors").and_then(|v| v.as_object()) {
            for (key, msg) in errors {
                result.errors.insert(
                    key.clone(),
                    msg.as_str().unwrap_or("unknown error").to_string(),
                );
            }
        }
        Ok(result)
    }

    fn list_namespaces(&self) -> Result<Vec<NamespaceEntry>, String> {
        let url = format!("{}/namespaces", self.base_url);
        Ok(self
            .request_json::<Vec<NamespaceEntry>>("GET", url, None, None, None)?
            .unwrap_or_default())
    }
}
