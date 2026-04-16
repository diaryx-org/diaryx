//! Shared HTTP-level contract tests that **any** adapter implementing the
//! Diaryx server API must pass.
//!
//! The goal is to catch divergence between adapters (Axum + SQLite in
//! [`diaryx_sync_server`], Workers + D1/R2 in `diaryx_cloudflare`) — the kind
//! of bug where one adapter URL-decodes path segments and the other doesn't,
//! or where they disagree on a response shape.
//!
//! # How it works
//!
//! The suite is dispatcher-agnostic: it calls its target through an
//! [`HttpDispatcher`] trait. Each adapter implements that trait in its own
//! tests — typically with `tower::ServiceExt::oneshot()` for in-process Axum,
//! or with `reqwest` against `wrangler dev` for Cloudflare.
//!
//! # Current status: **seed**
//!
//! Exactly one exemplar contract test is defined
//! ([`test_health_endpoint_returns_200_ok`]) so the wiring can be exercised
//! end-to-end. Scenario coverage (auth flow, object round-trip, URL-encoding
//! fuzz over [`URL_KEY_CORPUS`]) is the follow-up.
//!
//! # Adding a test
//!
//! Add an `async fn test_<scenario><D: HttpDispatcher>(d: &D)` here. Adapters
//! that have already wired up an [`HttpDispatcher`] will pick it up
//! automatically when they add an invocation in their own tests.

#[cfg(feature = "reqwest-dispatcher")]
pub mod http;

use std::collections::HashMap;

/// A corpus of object-key strings chosen to exercise URL-encoding and path
/// edge cases. Shared between unit tests of key-derivation functions and
/// HTTP contract tests that round-trip keys through the real routing layer.
///
/// Concrete cases this corpus is meant to catch:
///
/// - Spaces (commit `044a78c9` fixed publish paths with spaces).
/// - Percent-encoded vs plus-encoded spaces (`%20` vs `+`) — the sync plugin's
///   `decode_server_key` normalizes both; adapters must agree.
/// - Slashes inside keys — Axum extracts these differently from Workers-rs.
/// - `..` and `.` — path-traversal attempts must be blocked at the same layer
///   in both adapters (commit `a03a0732` added decoding; we still want these
///   rejected once decoded).
/// - Non-ASCII / emoji.
/// - Empty strings (adapter-specific behavior — some reject, some 404).
///
/// Not every case in this corpus is expected to **succeed** round-tripping.
/// A contract test that uses this corpus must declare per-case expectations.
pub const URL_KEY_CORPUS: &[&str] = &[
    "hello.md",
    "hello world.md",
    "hello%20world.md",
    "hello+world.md",
    "notes/today.md",
    "a/b/c.md",
    "emoji-🎉.md",
    "café.md",
    "with?query.md",
    "with#hash.md",
    "with&amp.md",
    "with=equals.md",
    "trailing-space .md",
    " leading-space.md",
    "..",
    ".",
    "../escape.md",
    ".hidden",
];

/// Minimal request/response shape that the contract suite uses to drive a
/// server. Deliberately small: method, path, optional headers, optional
/// body. Adapters are free to implement this against an in-process
/// Axum `Router`, a live `reqwest` client against `wrangler dev`, or
/// anything else that can answer HTTP semantics.
#[derive(Debug, Clone)]
pub struct ContractRequest {
    pub method: &'static str,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub content_type: Option<&'static str>,
}

impl ContractRequest {
    pub fn get(path: impl Into<String>) -> Self {
        Self {
            method: "GET",
            path: path.into(),
            headers: Vec::new(),
            body: None,
            content_type: None,
        }
    }

    pub fn post_json(path: impl Into<String>, body: &serde_json::Value) -> Self {
        Self {
            method: "POST",
            path: path.into(),
            headers: Vec::new(),
            body: Some(serde_json::to_vec(body).expect("serializable json body")),
            content_type: Some("application/json"),
        }
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

#[derive(Debug, Clone)]
pub struct ContractResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl ContractResponse {
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn body_json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| {
            panic!(
                "expected JSON response body but failed to parse: {e}; body was: {}",
                self.body_text()
            )
        })
    }
}

/// Something that can accept a [`ContractRequest`] and produce a
/// [`ContractResponse`]. Adapters implement this in their test crates.
#[async_trait::async_trait]
pub trait HttpDispatcher: Send + Sync {
    async fn dispatch(&self, req: ContractRequest) -> ContractResponse;
}

// ---------------------------------------------------------------------------
// Contract tests (seed: one exemplar)
// ---------------------------------------------------------------------------

/// Every adapter must expose a health endpoint at `/api/health` that returns
/// a 2xx status. The body format is adapter-defined.
pub async fn test_health_endpoint_returns_200_ok<D: HttpDispatcher>(dispatcher: &D) {
    let resp = dispatcher
        .dispatch(ContractRequest::get("/api/health"))
        .await;
    assert!(
        (200..300).contains(&resp.status),
        "expected 2xx from /api/health, got {}: {}",
        resp.status,
        resp.body_text()
    );
}

/// When email isn't configured (i.e. `RESEND_API_KEY` is empty on sync_server,
/// or `DEV_MODE=true` on the Cloudflare worker), `POST /api/auth/magic-link`
/// must return the verification link + 6-digit code directly in the JSON
/// response body under `dev_link` and `dev_code`. The CLI, e2e harness, and
/// any other scripted flow relies on this path — **both adapters must agree
/// on the response shape.**
pub async fn test_magic_link_dev_mode_returns_dev_credentials<D: HttpDispatcher>(dispatcher: &D) {
    let resp = dispatcher
        .dispatch(ContractRequest::post_json(
            "/api/auth/magic-link",
            &serde_json::json!({ "email": "contract-test@example.com" }),
        ))
        .await;

    assert_eq!(
        resp.status,
        200,
        "magic-link should return 200 in dev mode, got {}: {}",
        resp.status,
        resp.body_text()
    );

    let body = resp.body_json();
    assert_eq!(body.get("success").and_then(|v| v.as_bool()), Some(true));

    let dev_link = body
        .get("dev_link")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("dev_link missing in dev-mode response: {body}"));
    let dev_code = body
        .get("dev_code")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("dev_code missing in dev-mode response: {body}"));

    assert!(
        dev_link.contains("token="),
        "dev_link should contain a token query param: {dev_link}"
    );
    assert_eq!(
        dev_code.len(),
        6,
        "dev_code should be 6 characters, got {dev_code:?}"
    );
    assert!(
        dev_code.chars().all(|c| c.is_ascii_digit()),
        "dev_code should be all digits, got {dev_code:?}"
    );
}

/// `POST /api/auth/magic-link` must reject obviously-malformed emails with
/// 400. This keeps both adapters honest about the validation boundary —
/// a 500 here would mean the adapter is accepting garbage that the real
/// service rejects, and vice versa.
pub async fn test_magic_link_rejects_invalid_email<D: HttpDispatcher>(dispatcher: &D) {
    let resp = dispatcher
        .dispatch(ContractRequest::post_json(
            "/api/auth/magic-link",
            &serde_json::json!({ "email": "nope" }),
        ))
        .await;

    assert_eq!(
        resp.status,
        400,
        "invalid email should return 400, got {}: {}",
        resp.status,
        resp.body_text()
    );
    let body = resp.body_json();
    assert!(
        body.get("error").and_then(|v| v.as_str()).is_some(),
        "error response should carry an `error` field, got: {body}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_corpus_is_nonempty_and_unique() {
        assert!(!URL_KEY_CORPUS.is_empty());
        let mut seen = std::collections::HashSet::new();
        for s in URL_KEY_CORPUS {
            assert!(
                seen.insert(*s),
                "duplicate entry in URL_KEY_CORPUS: {:?}",
                s
            );
        }
    }
}
