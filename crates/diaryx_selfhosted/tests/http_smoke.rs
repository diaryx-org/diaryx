//! HTTP-level integration tests for `diaryx_selfhosted`.
//!
//! These tests drive the real [`axum::Router`] via `tower::ServiceExt::oneshot`
//! against `:memory:` SQLite — no ports, no fixture server, fast. See
//! [`support`] for the harness. Status: seed; extend with handler-level tests
//! as the surface grows.

mod support;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use serde_json::json;

use support::{TestApp, build_test_router, read_body, read_json, read_status_and_json};

async fn authed_put(
    app: &TestApp,
    token: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &'static str,
) -> axum::http::Response<Body> {
    let mut builder = Request::builder()
        .method(Method::PUT)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    app.request(builder.body(Body::from(body)).unwrap()).await
}

async fn sign_in(app: &TestApp, email: &str) -> String {
    let response = app
        .post_json("/api/auth/magic-link", &json!({ "email": email }))
        .await;
    let (status, body) = read_status_and_json(response).await;
    assert_eq!(status, StatusCode::OK, "body was: {body}");

    let dev_link = body
        .get("dev_link")
        .and_then(|v| v.as_str())
        .expect("dev_link should be present when email is not configured");
    let token = dev_link
        .split("token=")
        .nth(1)
        .expect("dev_link should include token")
        .split('&')
        .next()
        .expect("token query value should be present");

    let response = app
        .get(&format!(
            "/api/auth/verify?token={token}&device_name=SmokeDevice"
        ))
        .await;
    let (status, body) = read_status_and_json(response).await;
    assert_eq!(status, StatusCode::OK, "body was: {body}");

    body.get("token")
        .and_then(|v| v.as_str())
        .expect("verify response should include a token")
        .to_string()
}

/// End-to-end ARK Layer 3 (Phase 2): publish only markdown *sources* (no HTML),
/// register ARKs against their dest keys, then `POST /build` and assert the ARK
/// resolves to server-rendered HTML — with templating, nav, and link rewriting
/// reconstructed from the sources.
#[tokio::test]
async fn server_build_renders_html_from_sources() {
    let app = build_test_router();
    let token = sign_in(&app, "build@example.com").await;

    // Create namespace.
    let resp = app
        .request(
            Request::builder()
                .method(Method::POST)
                .uri("/api/namespaces")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&json!({})).unwrap()))
                .unwrap(),
        )
        .await;
    let (status, body) = read_status_and_json(resp).await;
    assert_eq!(status, StatusCode::CREATED, "create namespace: {body}");
    let ns = body["id"].as_str().expect("namespace id").to_string();

    // Public audience.
    let resp = app
        .request(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/namespaces/{ns}/audiences/public"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({ "gates": [] })).unwrap(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let root_ark = "bcdfgr";
    let child_ark = "bcdfgh";

    // Root source — keyed by its workspace name (Welcome.md), registered to the
    // dest index.html, flagged as the workspace index. Uses a template var and
    // a contents link to the child.
    let root_md = "---\ntitle: Welcome\nid: bcdfgr\ncontents:\n  - \"/child.md\"\n---\nHello from {{ title }}.\n";
    let resp = authed_put(
        &app,
        &token,
        &format!("/api/namespaces/{ns}/objects/public/Welcome.md"),
        &[
            ("x-audience", "public"),
            ("content-type", "text/markdown"),
            ("x-diaryx-file-ark", root_ark),
            ("x-diaryx-source-key", "public/Welcome.md"),
            ("x-diaryx-object-key", "public/index.html"),
            ("x-diaryx-is-index", "true"),
        ],
        root_md,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Child source — links back to the root by its workspace name.
    let child_md = "---\ntitle: Child\nid: bcdfgh\npart_of: \"/Welcome.md\"\n---\nChild body. See [home](/Welcome.md).\n";
    let resp = authed_put(
        &app,
        &token,
        &format!("/api/namespaces/{ns}/objects/public/child.md"),
        &[
            ("x-audience", "public"),
            ("content-type", "text/markdown"),
            ("x-diaryx-file-ark", child_ark),
            ("x-diaryx-source-key", "public/child.md"),
            ("x-diaryx-object-key", "public/child.html"),
        ],
        child_md,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Build: server renders HTML from the stored sources.
    let resp = app
        .request(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/namespaces/{ns}/build"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    let (status, body) = read_status_and_json(resp).await;
    assert_eq!(status, StatusCode::OK, "build: {body}");
    assert_eq!(body["pages_rendered"], 2, "build summary: {body}");

    // Root ARK resolves to server-rendered HTML: full document, template
    // expanded, child nav link present.
    let resp = app.get(&format!("/ark/{ns}/{root_ark}")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let html = String::from_utf8_lossy(&read_body(resp).await).into_owned();
    assert!(html.contains("<!DOCTYPE html>"), "not a full doc: {html}");
    assert!(
        html.contains("Hello from Welcome."),
        "template not run: {html}"
    );
    assert!(html.contains("child.html"), "nav link missing: {html}");

    // Child ARK resolves to HTML with the internal link rewritten to index.html.
    let resp = app.get(&format!("/ark/{ns}/{child_ark}")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let child_html = String::from_utf8_lossy(&read_body(resp).await).into_owned();
    assert!(
        child_html.contains("index.html"),
        "child link not rewritten to root index: {child_html}"
    );

    // ?content still serves the original markdown source.
    let resp = app.get(&format!("/ark/{ns}/{child_ark}?content")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        String::from_utf8_lossy(&read_body(resp).await).contains("Child body."),
        "content inflection should serve source markdown"
    );
}

/// End-to-end ARK Layer 2: publish a source sibling + HTML rendition, then
/// resolve the ARK with each inflection.
#[tokio::test]
async fn ark_resolution_serves_html_content_and_info() {
    let app = build_test_router();
    let token = sign_in(&app, "ark@example.com").await;

    // Server mints an ARK workspace blade as the namespace id.
    let resp = app
        .request(
            Request::builder()
                .method(Method::POST)
                .uri("/api/namespaces")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&json!({})).unwrap()))
                .unwrap(),
        )
        .await;
    let (status, body) = read_status_and_json(resp).await;
    assert_eq!(status, StatusCode::CREATED, "create namespace: {body}");
    let ns = body["id"].as_str().expect("namespace id").to_string();

    // Public audience (empty gates = public).
    let resp = app
        .request(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/namespaces/{ns}/audiences/public"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({ "gates": [] })).unwrap(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let file_ark = "bcdfgr"; // a valid file blade
    let source_md = "---\ntitle: Hello\nid: bcdfgr\n---\n\nBody text\n";

    // Upload the markdown source sibling (audience-tagged).
    let resp = authed_put(
        &app,
        &token,
        &format!("/api/namespaces/{ns}/objects/public/note.md"),
        &[("x-audience", "public"), ("content-type", "text/markdown")],
        source_md,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Upload the HTML rendition, registering the ARK + source key.
    let resp = authed_put(
        &app,
        &token,
        &format!("/api/namespaces/{ns}/objects/public/note.html"),
        &[
            ("x-audience", "public"),
            ("x-diaryx-file-ark", file_ark),
            ("x-diaryx-source-key", "public/note.md"),
            ("content-type", "text/html"),
        ],
        "<h1>Hello</h1>",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Default inflection → HTML.
    let resp = app.get(&format!("/ark/{ns}/{file_ark}")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body(resp).await, b"<h1>Hello</h1>");

    // Canonical `ark:{NAAN}/...` alias resolves identically to the bare form.
    let naan = diaryx_server::use_cases::ark::ARK_NAAN;
    let resp = app.get(&format!("/ark:{naan}/{ns}/{file_ark}")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body(resp).await, b"<h1>Hello</h1>");

    // ?content → markdown source.
    let resp = app.get(&format!("/ark/{ns}/{file_ark}?content")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = read_body(resp).await;
    assert!(String::from_utf8_lossy(&body).contains("Body text"));

    // ?info → frontmatter JSON.
    let resp = app.get(&format!("/ark/{ns}/{file_ark}?info")).await;
    let (status, body) = read_status_and_json(resp).await;
    assert_eq!(status, StatusCode::OK, "info: {body}");
    assert_eq!(body["title"], "Hello");

    // Sensitive internal frontmatter keys must never surface via resolution.
    // The publisher strips these before uploading the source sibling; assert
    // that contract holds end-to-end so a regression here can't leak publish
    // config (audiences, plugin settings) to anyone who can resolve the ARK.
    assert!(
        body.get("plugins").is_none(),
        "?info leaked `plugins`: {body}"
    );
    assert!(
        body.get("audiences").is_none(),
        "?info leaked `audiences`: {body}"
    );
    assert!(
        body.get("audiences_migrated").is_none(),
        "?info leaked `audiences_migrated`: {body}"
    );

    // ?meta=title → single field.
    let resp = app.get(&format!("/ark/{ns}/{file_ark}?meta=title")).await;
    let (status, body) = read_status_and_json(resp).await;
    assert_eq!(status, StatusCode::OK, "meta: {body}");
    assert_eq!(body, json!("Hello"));
}

#[tokio::test]
async fn health_endpoint_returns_200_ok() {
    let app: TestApp = build_test_router();

    let response = app.get("/api/health").await;

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn magic_link_dev_mode_returns_dev_link_and_code_in_response() {
    let app: TestApp = build_test_router();

    let response = app
        .post_json(
            "/api/auth/magic-link",
            &json!({ "email": "alice@example.com" }),
        )
        .await;

    let (status, body) = read_status_and_json(response).await;
    assert_eq!(status, StatusCode::OK, "body was: {body}");

    assert_eq!(body.get("success").and_then(|v| v.as_bool()), Some(true));

    // Email is not configured in test config (api_key is empty), so the
    // handler is expected to return the magic link + 6-digit code directly.
    // This is the path the CLI scripts and e2e tests rely on.
    let dev_link = body
        .get("dev_link")
        .and_then(|v| v.as_str())
        .expect("dev_link should be present when email is not configured");
    let dev_code = body
        .get("dev_code")
        .and_then(|v| v.as_str())
        .expect("dev_code should be present when email is not configured");

    assert!(
        dev_link.contains("token="),
        "dev_link should contain a token query param: {dev_link}"
    );
    assert_eq!(
        dev_code.len(),
        6,
        "dev_code should be a 6-digit code, got {dev_code:?}"
    );
    assert!(
        dev_code.chars().all(|c| c.is_ascii_digit()),
        "dev_code should be all digits, got {dev_code:?}"
    );
}

#[tokio::test]
async fn magic_link_rejects_invalid_email() {
    let app: TestApp = build_test_router();

    let response = app
        .post_json("/api/auth/magic-link", &json!({ "email": "nope" }))
        .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = read_json(response).await;
    assert!(
        body.get("error").and_then(|v| v.as_str()).is_some(),
        "should return an error field, got: {body}"
    );
}

#[tokio::test]
async fn delete_current_device_returns_actionable_error_body() {
    let app: TestApp = build_test_router();
    let session_token = sign_in(&app, "alice@example.com").await;

    let response = app
        .request_with_bearer(Method::GET, "/api/auth/me", &session_token)
        .await;
    let (status, body) = read_status_and_json(response).await;
    assert_eq!(status, StatusCode::OK, "body was: {body}");
    let current_device_id = body
        .get("devices")
        .and_then(|v| v.as_array())
        .and_then(|devices| devices.first())
        .and_then(|device| device.get("id"))
        .and_then(|v| v.as_str())
        .expect("signed-in user should have a registered device");

    let response = app
        .request_with_bearer(
            Method::DELETE,
            &format!("/api/auth/devices/{current_device_id}"),
            &session_token,
        )
        .await;
    let (status, body) = read_status_and_json(response).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "body was: {body}");
    assert_eq!(
        body.get("error").and_then(|v| v.as_str()),
        Some(
            "You cannot delete the device you are currently using. Sign out on this device instead."
        ),
    );
}
