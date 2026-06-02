//! HTTP-level integration tests for `diaryx_sync_server`.
//!
//! These tests drive the real [`axum::Router`] via `tower::ServiceExt::oneshot`
//! against `:memory:` SQLite — no ports, no fixture server, fast. See
//! [`support`] for the harness. Status: seed; extend with handler-level tests
//! as the surface grows.

mod support;

use axum::http::{Method, StatusCode};
use serde_json::json;

use support::{TestApp, build_test_router, read_json, read_status_and_json};

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
