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
        Self::no_body("GET", path)
    }

    pub fn delete(path: impl Into<String>) -> Self {
        Self::no_body("DELETE", path)
    }

    fn no_body(method: &'static str, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            headers: Vec::new(),
            body: None,
            content_type: None,
        }
    }

    pub fn post_json(path: impl Into<String>, body: &serde_json::Value) -> Self {
        Self::with_json("POST", path, body)
    }

    pub fn put_json(path: impl Into<String>, body: &serde_json::Value) -> Self {
        Self::with_json("PUT", path, body)
    }

    pub fn patch_json(path: impl Into<String>, body: &serde_json::Value) -> Self {
        Self::with_json("PATCH", path, body)
    }

    fn with_json(method: &'static str, path: impl Into<String>, body: &serde_json::Value) -> Self {
        Self {
            method,
            path: path.into(),
            headers: Vec::new(),
            body: Some(serde_json::to_vec(body).expect("serializable json body")),
            content_type: Some("application/json"),
        }
    }

    /// Send a raw byte body with a caller-chosen content type — used for
    /// object uploads (`PUT /namespaces/.../objects/key`) that carry
    /// non-JSON payloads.
    pub fn put_bytes(path: impl Into<String>, body: Vec<u8>, content_type: &'static str) -> Self {
        Self {
            method: "PUT",
            path: path.into(),
            headers: Vec::new(),
            body: Some(body),
            content_type: Some(content_type),
        }
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Convenience for authenticated requests. Adds
    /// `Authorization: Bearer <token>` to the header list.
    pub fn with_bearer(self, token: &str) -> Self {
        self.with_header("Authorization", format!("Bearer {token}"))
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

// ===========================================================================
// Authenticated-flow helpers + tests
// ===========================================================================

/// Build an email that won't collide with previous test runs on an adapter
/// that persists user state across runs (e.g. `wrangler dev --local` backs
/// D1 with an on-disk SQLite that's reused between invocations). Without
/// this, repeatedly signing in as the same email hits the per-user
/// device-limit after ~2 runs.
///
/// `label` is a short test-function-specific tag so log output identifies
/// which test created which user.
pub fn unique_email(label: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("contract-{label}-{nanos}@example.com")
}

/// Complete the dev-mode magic-link → verify dance against any adapter and
/// return a session token. Used by every authenticated contract test below.
///
/// Uses a `{email_tag}` parameter so tests running against a shared
/// wrangler-dev instance don't step on each other — each test should pick
/// a distinct tag.
///
/// Panics on any non-2xx response. That's intentional: if the dev-mode
/// magic-link flow is broken, every authenticated test should fail with a
/// clear pointer to *this* helper rather than each test failing with its
/// own 401.
pub async fn sign_in_via_magic_link<D: HttpDispatcher>(dispatcher: &D, email: &str) -> String {
    // POST /api/auth/magic-link
    let resp = dispatcher
        .dispatch(ContractRequest::post_json(
            "/api/auth/magic-link",
            &serde_json::json!({ "email": email }),
        ))
        .await;
    assert_eq!(
        resp.status,
        200,
        "sign_in: magic-link POST for {email} failed: {} / {}",
        resp.status,
        resp.body_text()
    );
    let body = resp.body_json();
    let dev_link = body
        .get("dev_link")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            panic!(
                "sign_in: no dev_link for {email}. Is DEV_MODE on / RESEND_API_KEY empty? Got: {body}"
            )
        });
    let token = dev_link
        .split("token=")
        .nth(1)
        .unwrap_or_else(|| panic!("sign_in: dev_link missing token param: {dev_link}"))
        .split('&')
        .next()
        .unwrap_or("")
        .to_string();

    // GET /api/auth/verify?token=...&device_name=contract-test
    let resp = dispatcher
        .dispatch(ContractRequest::get(format!(
            "/api/auth/verify?token={token}&device_name=contract-test"
        )))
        .await;
    assert_eq!(
        resp.status,
        200,
        "sign_in: verify for {email} failed: {} / {}",
        resp.status,
        resp.body_text()
    );
    let body = resp.body_json();
    body.get("token")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("sign_in: verify response missing token: {body}"))
        .to_string()
}

/// `GET /api/auth/verify` should complete the magic-link flow: consume the
/// token and return a session token + user id/email.
pub async fn test_magic_link_verify_returns_session<D: HttpDispatcher>(dispatcher: &D) {
    let email = unique_email("verify");
    let token = sign_in_via_magic_link(dispatcher, &email).await;

    assert!(!token.is_empty(), "session token should be non-empty");

    // The returned token must be usable for subsequent authenticated calls.
    let resp = dispatcher
        .dispatch(ContractRequest::get("/api/auth/me").with_bearer(&token))
        .await;
    assert_eq!(
        resp.status,
        200,
        "/auth/me with fresh token should succeed: {} / {}",
        resp.status,
        resp.body_text()
    );
    let body = resp.body_json();
    let me_email = body
        .get("user")
        .and_then(|u| u.get("email"))
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("/auth/me missing user.email: {body}"));
    assert_eq!(
        me_email,
        email.as_str(),
        "/auth/me should echo the signed-in email"
    );
}

/// Unauthenticated requests to `/api/auth/me` must return 401. A 200 here
/// would mean the adapter is leaking user data to anonymous callers.
pub async fn test_me_without_auth_returns_401<D: HttpDispatcher>(dispatcher: &D) {
    let resp = dispatcher
        .dispatch(ContractRequest::get("/api/auth/me"))
        .await;
    assert_eq!(
        resp.status,
        401,
        "/auth/me without Authorization must be 401, got {}: {}",
        resp.status,
        resp.body_text()
    );
}

// ===========================================================================
// Namespace lifecycle
// ===========================================================================

/// Create, list, and GET a namespace; assert the shape matches what the
/// sync plugin and CLI expect (`id` + `owner_user_id` + optional
/// `metadata.name`).
pub async fn test_namespace_create_list_get_lifecycle<D: HttpDispatcher>(dispatcher: &D) {
    let token = sign_in_via_magic_link(dispatcher, &unique_email("ns")).await;

    // POST /api/namespaces — create
    let name = format!(
        "contract-ns-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let create_resp = dispatcher
        .dispatch(
            ContractRequest::post_json(
                "/api/namespaces",
                &serde_json::json!({ "metadata": { "name": &name } }),
            )
            .with_bearer(&token),
        )
        .await;
    assert!(
        (200..300).contains(&create_resp.status),
        "create namespace should 2xx, got {}: {}",
        create_resp.status,
        create_resp.body_text()
    );
    let created = create_resp.body_json();
    let ns_id = created
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("create response missing id: {created}"))
        .to_string();

    // GET /api/namespaces — should include the new one
    let list_resp = dispatcher
        .dispatch(ContractRequest::get("/api/namespaces").with_bearer(&token))
        .await;
    assert_eq!(list_resp.status, 200);
    let list = list_resp.body_json();
    let arr = list
        .as_array()
        .unwrap_or_else(|| panic!("list response should be an array: {list}"));
    assert!(
        arr.iter()
            .any(|n| n.get("id").and_then(|v| v.as_str()) == Some(&ns_id)),
        "new namespace {ns_id} should appear in listing. Got: {arr:?}"
    );

    // GET /api/namespaces/{id} — should echo metadata
    let get_resp = dispatcher
        .dispatch(ContractRequest::get(format!("/api/namespaces/{ns_id}")).with_bearer(&token))
        .await;
    assert_eq!(
        get_resp.status,
        200,
        "GET /namespaces/{ns_id} should 200, got {}: {}",
        get_resp.status,
        get_resp.body_text()
    );
    let got = get_resp.body_json();
    let got_name = got
        .get("metadata")
        .and_then(|m| m.get("name"))
        .and_then(|v| v.as_str());
    assert_eq!(
        got_name,
        Some(name.as_str()),
        "metadata.name should round-trip: {got}"
    );
}

/// Alice's token must not grant access to Bob's namespace. Tests the
/// authz boundary at the HTTP layer — independent of what happens inside
/// `ObjectService::require_namespace_owner` in core.
pub async fn test_namespace_access_forbidden_for_other_users<D: HttpDispatcher>(dispatcher: &D) {
    let alice_token = sign_in_via_magic_link(dispatcher, &unique_email("alice")).await;
    let bob_token = sign_in_via_magic_link(dispatcher, &unique_email("bob")).await;

    // Alice creates a namespace.
    let resp = dispatcher
        .dispatch(
            ContractRequest::post_json(
                "/api/namespaces",
                &serde_json::json!({ "metadata": { "name": "alice-only" } }),
            )
            .with_bearer(&alice_token),
        )
        .await;
    assert!((200..300).contains(&resp.status));
    let ns_id = resp
        .body_json()
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    // Bob's listing must not include alice's namespace.
    let bob_list = dispatcher
        .dispatch(ContractRequest::get("/api/namespaces").with_bearer(&bob_token))
        .await;
    assert_eq!(bob_list.status, 200);
    let bob_arr = bob_list.body_json();
    let bob_arr = bob_arr.as_array().unwrap();
    assert!(
        !bob_arr
            .iter()
            .any(|n| n.get("id").and_then(|v| v.as_str()) == Some(&ns_id)),
        "bob should not see alice's namespace in his listing. Got: {bob_arr:?}"
    );

    // Bob trying to list alice's objects must be denied (403 or 404 both
    // acceptable — the server may hide existence for privacy).
    let bob_objects = dispatcher
        .dispatch(
            ContractRequest::get(format!("/api/namespaces/{ns_id}/objects"))
                .with_bearer(&bob_token),
        )
        .await;
    assert!(
        bob_objects.status == 403 || bob_objects.status == 404,
        "bob accessing alice's namespace should 403/404, got {}: {}",
        bob_objects.status,
        bob_objects.body_text()
    );
}

// ===========================================================================
// Object lifecycle
// ===========================================================================

/// Full object CRUD: PUT bytes, GET them back byte-exact, then DELETE.
/// Re-reading the deleted key returns 404.
pub async fn test_object_put_get_delete_roundtrip<D: HttpDispatcher>(dispatcher: &D) {
    let token = sign_in_via_magic_link(dispatcher, &unique_email("obj")).await;
    let ns_id = create_scratch_namespace(dispatcher, &token, "obj-roundtrip").await;

    let key = "hello.md";
    let body = b"# Hello from the contract suite\n";

    // PUT
    let put_resp = dispatcher
        .dispatch(
            ContractRequest::put_bytes(
                format!("/api/namespaces/{ns_id}/objects/{key}"),
                body.to_vec(),
                "text/markdown",
            )
            .with_bearer(&token),
        )
        .await;
    assert!(
        (200..300).contains(&put_resp.status),
        "PUT /objects/{key} should 2xx, got {}: {}",
        put_resp.status,
        put_resp.body_text()
    );

    // GET
    let get_resp = dispatcher
        .dispatch(
            ContractRequest::get(format!("/api/namespaces/{ns_id}/objects/{key}"))
                .with_bearer(&token),
        )
        .await;
    assert_eq!(
        get_resp.status,
        200,
        "GET should 200: {}",
        get_resp.body_text()
    );
    assert_eq!(
        get_resp.body, body,
        "GET body should match PUT bytes exactly"
    );

    // DELETE
    let del_resp = dispatcher
        .dispatch(
            ContractRequest::delete(format!("/api/namespaces/{ns_id}/objects/{key}"))
                .with_bearer(&token),
        )
        .await;
    assert!(
        (200..300).contains(&del_resp.status),
        "DELETE should 2xx, got {}: {}",
        del_resp.status,
        del_resp.body_text()
    );

    // GET after DELETE → 404
    let get_again = dispatcher
        .dispatch(
            ContractRequest::get(format!("/api/namespaces/{ns_id}/objects/{key}"))
                .with_bearer(&token),
        )
        .await;
    assert_eq!(
        get_again.status,
        404,
        "GET after DELETE should 404, got {}: {}",
        get_again.status,
        get_again.body_text()
    );
}

/// Upload a handful of objects; assert the listing returns every key.
pub async fn test_object_list_returns_uploaded_keys<D: HttpDispatcher>(dispatcher: &D) {
    let token = sign_in_via_magic_link(dispatcher, &unique_email("list")).await;
    let ns_id = create_scratch_namespace(dispatcher, &token, "obj-list").await;

    let keys = ["a.md", "b.md", "nested/c.md"];
    for key in &keys {
        let resp = dispatcher
            .dispatch(
                ContractRequest::put_bytes(
                    format!("/api/namespaces/{ns_id}/objects/{key}"),
                    format!("body of {key}").into_bytes(),
                    "text/markdown",
                )
                .with_bearer(&token),
            )
            .await;
        assert!((200..300).contains(&resp.status), "put {key}");
    }

    let list_resp = dispatcher
        .dispatch(
            ContractRequest::get(format!("/api/namespaces/{ns_id}/objects")).with_bearer(&token),
        )
        .await;
    assert_eq!(list_resp.status, 200);
    let listed = list_resp.body_json();
    let arr = listed
        .as_array()
        .unwrap_or_else(|| panic!("list should be array: {listed}"));
    let listed_keys: std::collections::BTreeSet<&str> = arr
        .iter()
        .filter_map(|o| o.get("key").and_then(|v| v.as_str()))
        .collect();
    for key in &keys {
        assert!(
            listed_keys.contains(*key),
            "object listing should contain {key:?}. Got: {listed_keys:?}"
        );
    }

    // Every listed entry must include a non-empty `content_hash`. The sync
    // plugin's `compute_diff` uses `server_entry.content_hash` to decide
    // whether remote bytes differ from the local manifest: if the server
    // omits it (as `diaryx_cloudflare` did before the fix), `server_changed`
    // is always false and the client never pulls remote edits. This is a
    // silent correctness bug for sync — hence the contract-level assertion.
    for obj in arr {
        let key = obj.get("key").and_then(|v| v.as_str()).unwrap_or("?");
        let hash = obj
            .get("content_hash")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "list_objects entry for {key:?} should include content_hash; \
                     the sync plugin relies on it to detect remote changes. Got: {obj}"
                )
            });
        assert!(
            !hash.is_empty(),
            "content_hash for {key:?} should be non-empty. Got: {obj}"
        );
    }
}

/// Round-trip the "safe" subset of [`URL_KEY_CORPUS`] through real HTTP
/// path extraction. This is the test that most directly catches
/// URL-encoding drift between adapters — a regression in either's path
/// decoding will surface here first.
pub async fn test_url_corpus_keys_roundtrip_through_http<D: HttpDispatcher>(dispatcher: &D) {
    let token = sign_in_via_magic_link(dispatcher, &unique_email("urlfuzz")).await;
    let ns_id = create_scratch_namespace(dispatcher, &token, "url-fuzz").await;

    // Match the sync-plugin-side fuzz (crates/plugins/diaryx_sync_extism/
    // tests/sync_e2e.rs::VALID_FUZZ_KEYS).
    let keys = [
        "hello.md",
        "hello world.md",
        "hello+world.md",
        "notes/today.md",
        "a/b/c.md",
        "emoji-🎉.md",
        "café.md",
        ".hidden",
    ];

    for key in &keys {
        // Client percent-encodes each path segment (matches
        // HttpNamespaceProvider::encode_key).
        let encoded = key
            .split('/')
            .map(|seg| urlencoding_encode(seg))
            .collect::<Vec<_>>()
            .join("/");
        let put_url = format!("/api/namespaces/{ns_id}/objects/{encoded}");
        let body = format!("body for {key}").into_bytes();

        let put_resp = dispatcher
            .dispatch(
                ContractRequest::put_bytes(put_url.clone(), body.clone(), "text/plain")
                    .with_bearer(&token),
            )
            .await;
        assert!(
            (200..300).contains(&put_resp.status),
            "PUT {key:?} (encoded as {put_url:?}) failed: {} / {}",
            put_resp.status,
            put_resp.body_text()
        );

        let get_resp = dispatcher
            .dispatch(
                ContractRequest::get(format!("/api/namespaces/{ns_id}/objects/{encoded}"))
                    .with_bearer(&token),
            )
            .await;
        assert_eq!(
            get_resp.status,
            200,
            "GET {key:?} (encoded as {encoded:?}) failed: {} / {}",
            get_resp.status,
            get_resp.body_text()
        );
        assert_eq!(
            get_resp.body, body,
            "GET body for {key:?} should match PUT bytes exactly"
        );
    }
}

// ===========================================================================
// Batch endpoints
// ===========================================================================

/// `POST /namespaces/{ns}/batch/objects` returns a JSON blob with the
/// requested objects base64-encoded. Both adapters implement this.
pub async fn test_batch_objects_json_returns_all_keys<D: HttpDispatcher>(dispatcher: &D) {
    let token = sign_in_via_magic_link(dispatcher, &unique_email("batch-json")).await;
    let ns_id = create_scratch_namespace(dispatcher, &token, "batch-json").await;

    let keys = ["one.md", "two.md", "three.md"];
    for key in &keys {
        let resp = dispatcher
            .dispatch(
                ContractRequest::put_bytes(
                    format!("/api/namespaces/{ns_id}/objects/{key}"),
                    format!("# {key}").into_bytes(),
                    "text/markdown",
                )
                .with_bearer(&token),
            )
            .await;
        assert!((200..300).contains(&resp.status));
    }

    let batch_resp = dispatcher
        .dispatch(
            ContractRequest::post_json(
                format!("/api/namespaces/{ns_id}/batch/objects"),
                &serde_json::json!({ "keys": keys }),
            )
            .with_bearer(&token),
        )
        .await;
    assert_eq!(
        batch_resp.status,
        200,
        "batch/objects should 200, got {}: {}",
        batch_resp.status,
        batch_resp.body_text()
    );
    let body = batch_resp.body_json();
    let objects = body
        .get("objects")
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| panic!("batch response missing `objects` map: {body}"));
    for key in &keys {
        assert!(
            objects.contains_key(*key),
            "batch response missing key {key:?}. Got: {objects:?}"
        );
    }
}

/// `POST /namespaces/{ns}/batch/objects/multipart` returns a
/// `multipart/mixed` body with raw binary parts (no base64 overhead).
///
/// **Currently expected to surface a real cloudflare gap**: the endpoint
/// exists on `diaryx_sync_server` but not on `diaryx_cloudflare`. Keeping
/// the test here makes the drift visible at CI time rather than as a
/// silent 404 + JSON fallback in the `HttpNamespaceProvider`.
///
/// If the team decides to remove the multipart endpoint entirely (simplify
/// to JSON-only), delete this test too. Until then, it's the marker.
pub async fn test_batch_objects_multipart_returns_all_keys<D: HttpDispatcher>(dispatcher: &D) {
    let token = sign_in_via_magic_link(dispatcher, &unique_email("batch-mp")).await;
    let ns_id = create_scratch_namespace(dispatcher, &token, "batch-multipart").await;

    let keys = ["one.md", "two.md"];
    for key in &keys {
        let resp = dispatcher
            .dispatch(
                ContractRequest::put_bytes(
                    format!("/api/namespaces/{ns_id}/objects/{key}"),
                    format!("# {key}").into_bytes(),
                    "text/markdown",
                )
                .with_bearer(&token),
            )
            .await;
        assert!((200..300).contains(&resp.status));
    }

    let batch_resp = dispatcher
        .dispatch(
            ContractRequest::post_json(
                format!("/api/namespaces/{ns_id}/batch/objects/multipart"),
                &serde_json::json!({ "keys": keys }),
            )
            .with_bearer(&token),
        )
        .await;
    assert_eq!(
        batch_resp.status,
        200,
        "batch/objects/multipart should 200 — both adapters should implement it. \
         If your adapter 404s here, either implement the endpoint or delete both \
         server-side endpoint and this test. Got {}: {}",
        batch_resp.status,
        batch_resp.body_text()
    );
    let ct = batch_resp
        .headers
        .get("content-type")
        .cloned()
        .unwrap_or_default();
    assert!(
        ct.starts_with("multipart/mixed"),
        "content-type should be multipart/mixed, got {ct:?}"
    );
    // We don't parse the body here — if the content-type is right and
    // the server 200'd with a non-empty body, the structural contract is
    // met. End-to-end binary correctness is covered by the sync plugin
    // E2E via `HttpNamespaceProvider::get_objects_batch`.
    assert!(
        !batch_resp.body.is_empty(),
        "multipart response body should be non-empty"
    );
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

async fn create_scratch_namespace<D: HttpDispatcher>(
    dispatcher: &D,
    token: &str,
    label: &str,
) -> String {
    let name = format!(
        "contract-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let resp = dispatcher
        .dispatch(
            ContractRequest::post_json(
                "/api/namespaces",
                &serde_json::json!({ "metadata": { "name": name } }),
            )
            .with_bearer(token),
        )
        .await;
    assert!(
        (200..300).contains(&resp.status),
        "create_scratch_namespace: {} / {}",
        resp.status,
        resp.body_text()
    );
    resp.body_json()
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("scratch namespace response missing id"))
        .to_string()
}

/// Stand-in for the `urlencoding` crate so this module doesn't need an
/// extra dep just for the URL-fuzz contract test. Percent-encodes
/// everything that isn't an unreserved URI character per RFC 3986.
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            other => out.push_str(&format!("%{:02X}", other)),
        }
    }
    out
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
