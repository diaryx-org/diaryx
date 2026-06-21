use crate::auth::RequireAuth;
use crate::db::AuthRepo;
use crate::rate_limit::RateLimiter;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use diaryx_server::UserTier;
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::{collections::HashSet, sync::Arc, time::Duration};
use tracing::{error, warn};

/// Shared state for managed AI proxy handlers.
#[derive(Clone)]
pub struct AiState {
    pub repo: Arc<AuthRepo>,
    pub rate_limiter: Arc<RateLimiter>,
    pub http_client: reqwest::Client,
    pub openrouter_api_key: String,
    pub openrouter_endpoint: String,
    pub allowed_models: HashSet<String>,
    pub rate_limit_per_minute: usize,
    pub monthly_quota: u64,
}

#[derive(Debug, Serialize)]
struct AiErrorResponse {
    error: String,
    message: String,
}

pub fn ai_routes(state: AiState) -> Router {
    Router::new()
        .route("/ai/chat/completions", post(chat_completions))
        .with_state(state)
}

fn ai_error(status: StatusCode, error_code: &str, message: &str) -> axum::response::Response {
    (
        status,
        Json(AiErrorResponse {
            error: error_code.to_string(),
            message: message.to_string(),
        }),
    )
        .into_response()
}

fn current_period_utc() -> String {
    chrono::Utc::now().format("%Y-%m").to_string()
}

/// POST /api/ai/chat/completions - Managed AI proxy (auth required).
async fn chat_completions(
    State(state): State<AiState>,
    RequireAuth(auth): RequireAuth,
    Json(payload): Json<JsonValue>,
) -> impl IntoResponse {
    if auth.user.tier != UserTier::Plus {
        return ai_error(
            StatusCode::FORBIDDEN,
            "plus_required",
            "Diaryx Plus is required for managed AI.",
        );
    }

    let model = payload
        .get("model")
        .and_then(|value| value.as_str())
        .map(|value| value.trim())
        .unwrap_or("");
    if model.is_empty() || !state.allowed_models.contains(model) {
        return ai_error(
            StatusCode::BAD_REQUEST,
            "model_not_allowed",
            "Requested model is not in the managed allowlist.",
        );
    }

    if let Err(retry_after) = state.rate_limiter.check(
        &auth.user.id,
        "managed_ai_chat",
        state.rate_limit_per_minute,
        Duration::from_secs(60),
    ) {
        let mut headers = HeaderMap::new();
        if let Ok(value) = retry_after.to_string().parse() {
            headers.insert(header::RETRY_AFTER, value);
        }
        return (
            StatusCode::TOO_MANY_REQUESTS,
            headers,
            Json(AiErrorResponse {
                error: "rate_limited".to_string(),
                message: "Managed AI rate limit exceeded.".to_string(),
            }),
        )
            .into_response();
    }

    let period = current_period_utc();
    let used = match state
        .repo
        .get_user_ai_usage_monthly_count(&auth.user.id, &period)
    {
        Ok(count) => count,
        Err(err) => {
            error!(
                "Managed AI usage lookup failed for user {}: {}",
                auth.user.id, err
            );
            return ai_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "provider_unavailable",
                "Managed AI is temporarily unavailable.",
            );
        }
    };
    if used >= state.monthly_quota {
        return ai_error(
            StatusCode::TOO_MANY_REQUESTS,
            "quota_exceeded",
            "Managed AI monthly quota exceeded.",
        );
    }

    if state.openrouter_api_key.trim().is_empty() || state.openrouter_endpoint.trim().is_empty() {
        return ai_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_unavailable",
            "Managed AI provider is not configured.",
        );
    }

    let upstream = match state
        .http_client
        .post(&state.openrouter_endpoint)
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", state.openrouter_api_key),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            warn!("Managed AI provider request failed: {}", err);
            return ai_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_unavailable",
                "Managed AI provider request failed.",
            );
        }
    };

    let upstream_status = upstream.status();
    let upstream_json: JsonValue = match upstream.json().await {
        Ok(value) => value,
        Err(err) => {
            warn!("Managed AI provider returned non-JSON response: {}", err);
            return ai_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_unavailable",
                "Managed AI provider returned an invalid response.",
            );
        }
    };

    if !upstream_status.is_success() {
        warn!(
            "Managed AI provider returned non-success status {}: {}",
            upstream_status, upstream_json
        );
        return ai_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_unavailable",
            "Managed AI provider is unavailable.",
        );
    }

    if let Err(err) = state
        .repo
        .increment_user_ai_usage_monthly_count(&auth.user.id, &period)
    {
        error!(
            "Managed AI usage increment failed for user {}: {}",
            auth.user.id, err
        );
    }

    (upstream_status, Json(upstream_json)).into_response()
}

#[cfg(test)]
mod tests {
    use super::{AiState, ai_routes, current_period_utc};
    use crate::{
        auth::AuthExtractor,
        db::{AuthRepo, init_database},
        rate_limit::RateLimiter,
    };
    use axum::{
        Extension, Json, Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        routing::post,
    };
    use chrono::{Duration, Utc};
    use diaryx_server::UserTier;
    use rusqlite::Connection;
    use serde_json::{Value as JsonValue, json};
    use std::sync::Arc;
    use tower::Service;

    fn setup_repo() -> Arc<AuthRepo> {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_database(&conn).expect("init database");
        Arc::new(AuthRepo::new(conn))
    }

    fn create_session_token(repo: &AuthRepo, email: &str, tier: UserTier) -> String {
        let user_id = repo.get_or_create_user(email).expect("create user");
        let db_tier = match tier {
            UserTier::Free => crate::db::UserTier::Free,
            UserTier::Plus => crate::db::UserTier::Plus,
        };
        repo.set_user_tier(&user_id, db_tier).expect("set tier");
        let device_id = repo
            .create_device(&user_id, Some("test"), Some("test-agent"))
            .expect("create device");
        repo.create_session(&user_id, &device_id, Utc::now() + Duration::hours(1))
            .expect("create session")
    }

    fn build_state(
        repo: Arc<AuthRepo>,
        endpoint: String,
        rate_limit_per_minute: usize,
        monthly_quota: u64,
    ) -> AiState {
        AiState {
            repo,
            rate_limiter: Arc::new(RateLimiter::new()),
            http_client: reqwest::Client::new(),
            openrouter_api_key: "test-openrouter-key".to_string(),
            openrouter_endpoint: endpoint,
            allowed_models: ["openai/gpt-5.2".to_string()].into_iter().collect(),
            rate_limit_per_minute,
            monthly_quota,
        }
    }

    fn build_app(state: AiState, repo: Arc<AuthRepo>) -> Router {
        use crate::adapters::{NativeAuthSessionStore, NativeAuthStore};
        let auth_store = Arc::new(NativeAuthStore::new(repo.clone()));
        let session_store = Arc::new(NativeAuthSessionStore::new(repo));
        Router::new()
            .nest("/api", ai_routes(state))
            .layer(Extension(AuthExtractor::new(auth_store, session_store)))
    }

    async fn post_chat(
        app: &mut Router,
        token: Option<&str>,
        body: JsonValue,
    ) -> axum::response::Response {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/api/ai/chat/completions")
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {}", token));
        }
        let request = builder.body(Body::from(body.to_string())).expect("request");
        app.call(request).await.expect("router response")
    }

    async fn response_json(response: axum::response::Response) -> JsonValue {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        serde_json::from_slice(&bytes).expect("json response")
    }

    async fn spawn_mock_provider(response: JsonValue) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new().route(
            "/v1/chat/completions",
            post({
                let response = response.clone();
                move || {
                    let response = response.clone();
                    async move { (StatusCode::OK, Json(response)) }
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock provider");
        let addr = listener.local_addr().expect("mock provider addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("mock provider serve");
        });

        (format!("http://{}/v1/chat/completions", addr), handle)
    }

    #[tokio::test]
    async fn managed_chat_requires_authentication() {
        let repo = setup_repo();
        let state = build_state(repo.clone(), "http://127.0.0.1:1".to_string(), 30, 1000);
        let mut app = build_app(state, repo);
        let response = post_chat(
            &mut app,
            None,
            json!({ "model": "openai/gpt-5.2", "messages": [] }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn managed_chat_rejects_free_tier() {
        let repo = setup_repo();
        let token = create_session_token(&repo, "free@example.com", UserTier::Free);
        let state = build_state(repo.clone(), "http://127.0.0.1:1".to_string(), 30, 1000);
        let mut app = build_app(state, repo);
        let response = post_chat(
            &mut app,
            Some(&token),
            json!({ "model": "openai/gpt-5.2", "messages": [] }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = response_json(response).await;
        assert_eq!(
            body.get("error").and_then(|v| v.as_str()),
            Some("plus_required")
        );
    }

    #[tokio::test]
    async fn managed_chat_rejects_disallowed_model() {
        let repo = setup_repo();
        let token = create_session_token(&repo, "plus@example.com", UserTier::Plus);
        let state = build_state(repo.clone(), "http://127.0.0.1:1".to_string(), 30, 1000);
        let mut app = build_app(state, repo);
        let response = post_chat(
            &mut app,
            Some(&token),
            json!({ "model": "anthropic/claude-unknown", "messages": [] }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert_eq!(
            body.get("error").and_then(|v| v.as_str()),
            Some("model_not_allowed")
        );
    }

    #[tokio::test]
    async fn managed_chat_enforces_rate_limit() {
        let repo = setup_repo();
        let token = create_session_token(&repo, "rate@example.com", UserTier::Plus);
        let state = build_state(repo.clone(), "http://127.0.0.1:1".to_string(), 1, 1000);
        let mut app = build_app(state, repo);

        let first = post_chat(
            &mut app,
            Some(&token),
            json!({ "model": "openai/gpt-5.2", "messages": [] }),
        )
        .await;
        assert_eq!(first.status(), StatusCode::SERVICE_UNAVAILABLE);

        let second = post_chat(
            &mut app,
            Some(&token),
            json!({ "model": "openai/gpt-5.2", "messages": [] }),
        )
        .await;
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = response_json(second).await;
        assert_eq!(
            body.get("error").and_then(|v| v.as_str()),
            Some("rate_limited")
        );
    }

    #[tokio::test]
    async fn managed_chat_enforces_monthly_quota() {
        let repo = setup_repo();
        let token = create_session_token(&repo, "quota@example.com", UserTier::Plus);
        let provider_response = json!({
            "id": "cmpl-1",
            "choices": [{ "message": { "role": "assistant", "content": "hello" } }]
        });
        let (endpoint, handle) = spawn_mock_provider(provider_response).await;
        let state = build_state(repo.clone(), endpoint, 30, 1);
        let mut app = build_app(state, repo.clone());

        let first = post_chat(
            &mut app,
            Some(&token),
            json!({ "model": "openai/gpt-5.2", "messages": [] }),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);

        let second = post_chat(
            &mut app,
            Some(&token),
            json!({ "model": "openai/gpt-5.2", "messages": [] }),
        )
        .await;
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = response_json(second).await;
        assert_eq!(
            body.get("error").and_then(|v| v.as_str()),
            Some("quota_exceeded")
        );

        let user_id = repo
            .validate_session(&token)
            .expect("validate session")
            .expect("session")
            .user_id;
        assert_eq!(
            repo.get_user_ai_usage_monthly_count(&user_id, &current_period_utc())
                .expect("usage count"),
            1
        );

        handle.abort();
    }

    #[tokio::test]
    async fn managed_chat_success_proxies_provider_response() {
        let repo = setup_repo();
        let token = create_session_token(&repo, "success@example.com", UserTier::Plus);
        let provider_response = json!({
            "id": "cmpl-42",
            "choices": [{
                "message": { "role": "assistant", "content": "Managed response" }
            }]
        });
        let (endpoint, handle) = spawn_mock_provider(provider_response.clone()).await;
        let state = build_state(repo.clone(), endpoint, 30, 1000);
        let mut app = build_app(state, repo);
        let response = post_chat(
            &mut app,
            Some(&token),
            json!({ "model": "openai/gpt-5.2", "messages": [] }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body, provider_response);

        handle.abort();
    }
}
