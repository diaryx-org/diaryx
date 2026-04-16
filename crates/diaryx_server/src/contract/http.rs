//! [`HttpDispatcher`] implementation backed by [`reqwest`], for driving the
//! shared contract suite against any live HTTP server (typically
//! `diaryx_sync_server` on a local port or `wrangler dev` serving
//! `diaryx_cloudflare`).
//!
//! Gated behind the `reqwest-dispatcher` feature so crates that don't need
//! reqwest don't pay the compile cost.

use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

use super::{ContractRequest, ContractResponse, HttpDispatcher};

/// [`HttpDispatcher`] that issues real HTTP requests to a base URL.
///
/// Intended for tests that need to drive a live server — e.g. a locally
/// spawned `wrangler dev` or a `diaryx_sync_server` bound on
/// `127.0.0.1:0`. The base URL must not end with a trailing slash; the
/// dispatcher appends the request path verbatim.
pub struct ReqwestDispatcher {
    client: reqwest::Client,
    base_url: String,
}

impl ReqwestDispatcher {
    /// Build a dispatcher with sensible defaults: 30-second request timeout,
    /// no connection pooling quirks.
    pub fn new(base_url: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client");
        Self::with_client(base_url, client)
    }

    pub fn with_client(base_url: impl Into<String>, client: reqwest::Client) -> Self {
        let mut url = base_url.into();
        while url.ends_with('/') {
            url.pop();
        }
        Self {
            client,
            base_url: url,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

#[async_trait]
impl HttpDispatcher for ReqwestDispatcher {
    async fn dispatch(&self, req: ContractRequest) -> ContractResponse {
        let url = format!("{}{}", self.base_url, req.path);
        let method = reqwest::Method::from_bytes(req.method.as_bytes()).expect("valid HTTP method");
        let mut builder = self.client.request(method, &url);
        for (name, value) in &req.headers {
            builder = builder.header(name, value);
        }
        if let Some(ct) = req.content_type {
            builder = builder.header("content-type", ct);
        }
        if let Some(body) = req.body {
            builder = builder.body(body);
        }
        let response = builder
            .send()
            .await
            .unwrap_or_else(|e| panic!("HTTP request to {url} failed: {e}"));

        let status = response.status().as_u16();
        let mut headers = HashMap::new();
        for (name, value) in response.headers() {
            if let Ok(s) = value.to_str() {
                headers.insert(name.as_str().to_string(), s.to_string());
            }
        }
        let body = response
            .bytes()
            .await
            .unwrap_or_else(|e| panic!("failed to read response body from {url}: {e}"))
            .to_vec();

        ContractResponse {
            status,
            headers,
            body,
        }
    }
}
