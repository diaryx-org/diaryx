use diaryx_server::domain::{AuthContext, AuthSessionInfo, UserInfo};
use diaryx_server::ports::{AuthSessionStore, AuthStore};
use diaryx_server::use_cases::auth::{SessionValidationService, extract_token};

use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};
use std::sync::Arc;

/// Authenticated user extracted from request.
/// Uses the portable core types directly.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub session: AuthSessionInfo,
    pub user: UserInfo,
}

impl From<AuthContext> for AuthUser {
    fn from(ctx: AuthContext) -> Self {
        Self {
            session: ctx.session,
            user: ctx.user,
        }
    }
}

/// Extension holding the trait objects needed for session validation.
#[derive(Clone)]
pub struct AuthExtractor {
    auth_store: Arc<dyn AuthStore>,
    session_store: Arc<dyn AuthSessionStore>,
}

/// Extractor for optional authentication
///
/// Use this when auth is optional (e.g., public endpoints that behave differently for authenticated users)
#[derive(Debug, Clone)]
pub struct OptionalAuth(pub Option<AuthUser>);

/// Extractor for required authentication
///
/// Use this for protected endpoints - returns 401 if not authenticated
#[derive(Debug, Clone)]
pub struct RequireAuth(pub AuthUser);

impl AuthExtractor {
    pub fn new(auth_store: Arc<dyn AuthStore>, session_store: Arc<dyn AuthSessionStore>) -> Self {
        Self {
            auth_store,
            session_store,
        }
    }

    /// Extract authentication from request headers, cookies, or query parameters.
    pub async fn extract_auth(&self, parts: &Parts) -> Option<AuthUser> {
        let authorization = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok());

        // Collect all cookie header values into a single semicolon-separated string
        let cookie_header: Option<String> = {
            let cookies: Vec<&str> = parts
                .headers
                .get_all("cookie")
                .iter()
                .filter_map(|v| v.to_str().ok())
                .collect();
            if cookies.is_empty() {
                None
            } else {
                Some(cookies.join("; "))
            }
        };

        let query = parts.uri.query();

        let token = extract_token(authorization, cookie_header.as_deref(), query)?;

        let service =
            SessionValidationService::new(self.auth_store.as_ref(), self.session_store.as_ref());
        service.validate(&token).await.ok().map(AuthUser::from)
    }
}

impl<S> FromRequestParts<S> for OptionalAuth
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let extractor = parts
            .extensions
            .get::<AuthExtractor>()
            .cloned()
            .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "Auth not configured"))?;

        Ok(OptionalAuth(extractor.extract_auth(parts).await))
    }
}

impl<S> FromRequestParts<S> for RequireAuth
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let OptionalAuth(auth) = OptionalAuth::from_request_parts(parts, state).await?;

        match auth {
            Some(user) => Ok(RequireAuth(user)),
            None => Err((StatusCode::UNAUTHORIZED, "Authentication required")),
        }
    }
}

/// Extract token from WebSocket upgrade request query parameters
pub fn extract_token_from_query(query: Option<&str>) -> Option<String> {
    extract_token(None, None, query)
}

/// Validate a token and return the auth user.
/// Convenience for non-middleware contexts (e.g., WebSocket upgrade).
pub async fn validate_token(
    auth_store: &dyn AuthStore,
    session_store: &dyn AuthSessionStore,
    token: &str,
) -> Option<AuthUser> {
    let service = SessionValidationService::new(auth_store, session_store);
    service.validate(token).await.ok().map(AuthUser::from)
}
