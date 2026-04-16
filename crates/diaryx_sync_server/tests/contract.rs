//! Runs the shared [`diaryx_server::contract`] test suite against
//! `diaryx_sync_server`'s Axum router.
//!
//! The point of this file is small but important: it wires the adapter up to
//! the shared contract harness so that when `diaryx_cloudflare` later
//! implements the same [`HttpDispatcher`] (against `wrangler dev` or an
//! equivalent), both adapters will be asserting against the exact same
//! assertions. That's where URL-encoding / route-shape drift gets caught.
//!
//! Status: seed — invokes the one exemplar contract test
//! ([`contract::test_health_endpoint_returns_200_ok`]) so the wiring is
//! end-to-end. Additional contract tests added upstream will be picked up
//! here by appending more invocations below.

mod support;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Method, Request};

use diaryx_server::contract::{self, ContractRequest, ContractResponse, HttpDispatcher};

use support::{TestApp, build_test_router, read_body};

/// Adapter from a [`TestApp`] to a [`HttpDispatcher`], so the sync-server
/// router can be driven by shared [`contract`] tests.
struct TestAppDispatcher(TestApp);

#[async_trait]
impl HttpDispatcher for TestAppDispatcher {
    async fn dispatch(&self, req: ContractRequest) -> ContractResponse {
        let method = Method::from_bytes(req.method.as_bytes()).expect("valid HTTP method");
        let mut builder = Request::builder().method(method).uri(&req.path);

        for (name, value) in &req.headers {
            builder = builder.header(name, value);
        }
        if let Some(ct) = req.content_type {
            builder = builder.header("content-type", ct);
        }

        let body = req.body.map(Body::from).unwrap_or_else(Body::empty);
        let request = builder.body(body).expect("build request");

        let response = self.0.request(request).await;

        let status = response.status().as_u16();
        let mut headers = std::collections::HashMap::new();
        for (name, value) in response.headers() {
            let name: &HeaderName = name;
            let value: &HeaderValue = value;
            if let Ok(s) = value.to_str() {
                headers.insert(name.as_str().to_string(), s.to_string());
            }
        }
        let body = read_body(response).await;

        ContractResponse {
            status,
            headers,
            body,
        }
    }
}

#[tokio::test]
async fn contract_health_endpoint() {
    let dispatcher = TestAppDispatcher(build_test_router());
    contract::test_health_endpoint_returns_200_ok(&dispatcher).await;
}

#[tokio::test]
async fn contract_magic_link_dev_mode() {
    let dispatcher = TestAppDispatcher(build_test_router());
    contract::test_magic_link_dev_mode_returns_dev_credentials(&dispatcher).await;
}

#[tokio::test]
async fn contract_magic_link_rejects_invalid_email() {
    let dispatcher = TestAppDispatcher(build_test_router());
    contract::test_magic_link_rejects_invalid_email(&dispatcher).await;
}
