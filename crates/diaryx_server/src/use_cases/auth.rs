use crate::domain::{AuthContext, DeviceInfo};
use crate::ports::{
    AuthSessionStore, AuthStore, DeviceStore, MagicLinkStore, ServerCoreError, UserStore,
};
use chrono::{Duration, Utc};
use serde::Serialize;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for the authentication service.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// How long a magic link token is valid (minutes).
    pub magic_link_expiry_minutes: i64,
    /// How long a session is valid (days).
    pub session_expiry_days: i64,
    /// Maximum magic link requests per email per hour.
    pub rate_limit_per_hour: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            magic_link_expiry_minutes: 15,
            session_expiry_days: 30,
            rate_limit_per_hour: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Result / error types
// ---------------------------------------------------------------------------

/// Result of a successful authentication (magic link, code, or passkey).
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub session_token: String,
    pub user_id: String,
    pub device_id: String,
    pub email: String,
}

/// Info about a device, returned when the device limit is reached.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceLimitDevice {
    pub id: String,
    pub name: Option<String>,
    pub last_seen_at: String,
}

impl From<DeviceInfo> for DeviceLimitDevice {
    fn from(d: DeviceInfo) -> Self {
        Self {
            id: d.id,
            name: d.name,
            last_seen_at: d.last_seen_at.to_rfc3339(),
        }
    }
}

/// Errors specific to the authentication flow.
#[derive(Debug)]
pub enum AuthError {
    InvalidToken,
    RateLimited,
    DeviceLimitReached {
        limit: u32,
        devices: Vec<DeviceLimitDevice>,
    },
    InvalidReplaceDevice,
    Internal(ServerCoreError),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::InvalidToken => write!(f, "Invalid or expired token"),
            AuthError::RateLimited => write!(f, "Too many requests. Please try again later."),
            AuthError::DeviceLimitReached { limit, .. } => {
                write!(f, "Device limit reached (max {})", limit)
            }
            AuthError::InvalidReplaceDevice => {
                write!(f, "The device to replace was not found on this account")
            }
            AuthError::Internal(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for AuthError {}

impl From<ServerCoreError> for AuthError {
    fn from(e: ServerCoreError) -> Self {
        AuthError::Internal(e)
    }
}

// ---------------------------------------------------------------------------
// AuthenticationService
// ---------------------------------------------------------------------------

pub struct AuthenticationService<'a> {
    magic_link_store: &'a dyn MagicLinkStore,
    user_store: &'a dyn UserStore,
    device_store: &'a dyn DeviceStore,
    session_store: &'a dyn AuthSessionStore,
    config: &'a AuthConfig,
}

impl<'a> AuthenticationService<'a> {
    pub fn new(
        magic_link_store: &'a dyn MagicLinkStore,
        user_store: &'a dyn UserStore,
        device_store: &'a dyn DeviceStore,
        session_store: &'a dyn AuthSessionStore,
        config: &'a AuthConfig,
    ) -> Self {
        Self {
            magic_link_store,
            user_store,
            device_store,
            session_store,
            config,
        }
    }

    /// Request a magic link for the given email.
    /// Returns (token, verification_code).
    pub async fn request_magic_link(&self, email: &str) -> Result<(String, String), AuthError> {
        let email = email.trim().to_lowercase();

        // Rate limiting
        let one_hour_ago = (Utc::now() - Duration::hours(1)).timestamp();
        let recent_count = self
            .magic_link_store
            .count_recent_magic_tokens(&email, one_hour_ago)
            .await?;

        if recent_count >= self.config.rate_limit_per_hour {
            return Err(AuthError::RateLimited);
        }

        let expires_at =
            (Utc::now() + Duration::minutes(self.config.magic_link_expiry_minutes)).timestamp();
        let (token, code) = self
            .magic_link_store
            .create_magic_token(&email, expires_at)
            .await?;

        Ok((token, code))
    }

    /// Verify a magic link token and create a session.
    pub async fn verify_magic_link(
        &self,
        token: &str,
        device_name: Option<&str>,
        user_agent: Option<&str>,
        replace_device_id: Option<&str>,
    ) -> Result<VerifyResult, AuthError> {
        let email = self
            .magic_link_store
            .peek_magic_token(token)
            .await?
            .ok_or(AuthError::InvalidToken)?;

        let result = self
            .create_session_for_email(&email, device_name, user_agent, replace_device_id)
            .await?;

        self.magic_link_store.consume_magic_token(token).await?;

        Ok(result)
    }

    /// Verify a 6-digit code and create a session.
    pub async fn verify_code(
        &self,
        code: &str,
        email: &str,
        device_name: Option<&str>,
        user_agent: Option<&str>,
        replace_device_id: Option<&str>,
    ) -> Result<VerifyResult, AuthError> {
        let email = email.trim().to_lowercase();

        self.magic_link_store
            .peek_magic_code(code, &email)
            .await?
            .ok_or(AuthError::InvalidToken)?;

        let result = self
            .create_session_for_email(&email, device_name, user_agent, replace_device_id)
            .await?;

        self.magic_link_store
            .consume_magic_code(code, &email)
            .await?;

        Ok(result)
    }

    /// Shared session-creation logic used by link, code, and passkey flows.
    pub async fn create_session_for_email(
        &self,
        email: &str,
        device_name: Option<&str>,
        user_agent: Option<&str>,
        replace_device_id: Option<&str>,
    ) -> Result<VerifyResult, AuthError> {
        let user_id = self.user_store.get_or_create_user(email).await?;

        let device_limit = self.user_store.get_effective_device_limit(&user_id).await?;
        let device_count = self.device_store.count_user_devices(&user_id).await?;

        if device_count >= device_limit {
            if let Some(replace_id) = replace_device_id {
                let devices = self.device_store.list_user_devices(&user_id).await?;
                if !devices.iter().any(|d| d.id == replace_id) {
                    return Err(AuthError::InvalidReplaceDevice);
                }
                self.device_store.delete_device(replace_id).await?;
            } else {
                let devices = self.device_store.list_user_devices(&user_id).await?;
                return Err(AuthError::DeviceLimitReached {
                    limit: device_limit,
                    devices: devices.into_iter().map(DeviceLimitDevice::from).collect(),
                });
            }
        }

        self.user_store.update_last_login(&user_id).await?;

        let device_id = self
            .device_store
            .create_device(&user_id, device_name, user_agent)
            .await?;

        let expires_at = (Utc::now() + Duration::days(self.config.session_expiry_days)).timestamp();
        let session_token = self
            .session_store
            .create_auth_session(&user_id, &device_id, expires_at)
            .await?;

        Ok(VerifyResult {
            session_token,
            user_id,
            device_id,
            email: email.to_string(),
        })
    }
}

pub struct SessionValidationService<'a> {
    auth_store: &'a dyn AuthStore,
    session_store: &'a dyn AuthSessionStore,
}

impl<'a> SessionValidationService<'a> {
    pub fn new(auth_store: &'a dyn AuthStore, session_store: &'a dyn AuthSessionStore) -> Self {
        Self {
            auth_store,
            session_store,
        }
    }

    /// Validate a session token and return the full auth context.
    ///
    /// This is the core of authentication middleware: given a token string
    /// (extracted from a header, cookie, or query parameter by the HTTP layer),
    /// validate it against the session store, update the device heartbeat,
    /// and load the user info.
    pub async fn validate(&self, token: &str) -> Result<AuthContext, ServerCoreError> {
        let session = self
            .session_store
            .validate_session(token)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Invalid or expired session"))?;

        // Best-effort device heartbeat — don't fail the request if this errors.
        let _ = self
            .session_store
            .update_device_last_seen(&session.device_id)
            .await;

        let user = self
            .auth_store
            .get_user(&session.user_id)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("User not found"))?;

        Ok(AuthContext { session, user })
    }

    /// Delete a session (logout).
    pub async fn logout(&self, token: &str) -> Result<(), ServerCoreError> {
        self.session_store.delete_session(token).await
    }
}

/// Extract a session token from multiple sources in priority order:
/// 1. `Authorization: Bearer <token>` header
/// 2. `Cookie: diaryx_session=<token>` cookie
/// 3. `?token=<token>` query parameter
///
/// This is a pure function — no I/O. The HTTP layer provides the raw values;
/// a CF Worker and Axum extract them differently but both call this.
pub fn extract_token(
    authorization_header: Option<&str>,
    cookie_header: Option<&str>,
    query_string: Option<&str>,
) -> Option<String> {
    // 1. Bearer token
    if let Some(token) = authorization_header
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
    {
        return Some(token);
    }

    // 2. Cookie
    if let Some(token) = cookie_header
        .into_iter()
        .flat_map(|v| v.split(';'))
        .map(|c| c.trim())
        .find(|c| c.starts_with("diaryx_session="))
        .and_then(|c| c.strip_prefix("diaryx_session="))
        .map(|s| s.to_string())
    {
        return Some(token);
    }

    // 3. Query parameter
    query_string
        .into_iter()
        .flat_map(|q| q.split('&'))
        .find(|p| p.starts_with("token="))
        .and_then(|p| p.strip_prefix("token="))
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::{SessionValidationService, extract_token};
    use crate::domain::{AuthSessionInfo, DeviceInfo, UserInfo, UserTier};
    use crate::ports::{AuthSessionStore, AuthStore, ServerCoreError};
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct TestAuthStore {
        users: Mutex<HashMap<String, UserInfo>>,
    }

    crate::cfg_async_trait! {
    impl AuthStore for TestAuthStore {
        async fn get_user(&self, user_id: &str) -> Result<Option<UserInfo>, ServerCoreError> {
            Ok(self.users.lock().unwrap().get(user_id).cloned())
        }
        async fn list_user_devices(&self, _: &str) -> Result<Vec<DeviceInfo>, ServerCoreError> {
            Ok(vec![])
        }
        async fn rename_device(&self, _: &str, _: &str) -> Result<bool, ServerCoreError> {
            Ok(true)
        }
        async fn delete_device(&self, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn get_user_tier(&self, _: &str) -> Result<UserTier, ServerCoreError> {
            Ok(UserTier::Free)
        }
    }
    }

    struct TestSessionStore {
        sessions: Mutex<HashMap<String, AuthSessionInfo>>,
        last_seen: Mutex<Vec<String>>,
    }

    crate::cfg_async_trait! {
    impl AuthSessionStore for TestSessionStore {
        async fn validate_session(&self, token: &str) -> Result<Option<AuthSessionInfo>, ServerCoreError> {
            Ok(self.sessions.lock().unwrap().get(token).cloned())
        }
        async fn create_auth_session(&self, _: &str, _: &str, _: i64) -> Result<String, ServerCoreError> {
            Ok("new-session".to_string())
        }
        async fn delete_session(&self, token: &str) -> Result<(), ServerCoreError> {
            self.sessions.lock().unwrap().remove(token);
            Ok(())
        }
        async fn update_device_last_seen(&self, device_id: &str) -> Result<(), ServerCoreError> {
            self.last_seen.lock().unwrap().push(device_id.to_string());
            Ok(())
        }
    }
    }

    fn make_stores() -> (TestAuthStore, TestSessionStore) {
        let auth_store = TestAuthStore {
            users: Mutex::new(HashMap::from([(
                "user1".to_string(),
                UserInfo {
                    id: "user1".to_string(),
                    email: "user@example.com".to_string(),
                    created_at: Utc.timestamp_opt(1, 0).unwrap(),
                    last_login_at: None,
                    attachment_limit_bytes: None,
                    workspace_limit: None,
                    tier: UserTier::Free,
                    published_site_limit: None,
                },
            )])),
        };
        let session_store = TestSessionStore {
            sessions: Mutex::new(HashMap::from([(
                "valid-token".to_string(),
                AuthSessionInfo {
                    token: "valid-token".to_string(),
                    user_id: "user1".to_string(),
                    device_id: "dev1".to_string(),
                    expires_at: Utc.timestamp_opt(9999999999, 0).unwrap(),
                    created_at: Utc.timestamp_opt(1, 0).unwrap(),
                },
            )])),
            last_seen: Mutex::new(vec![]),
        };
        (auth_store, session_store)
    }

    #[tokio::test]
    async fn validate_returns_auth_context() {
        let (auth_store, session_store) = make_stores();
        let service = SessionValidationService::new(&auth_store, &session_store);

        let ctx = service.validate("valid-token").await.unwrap();
        assert_eq!(ctx.user.id, "user1");
        assert_eq!(ctx.session.device_id, "dev1");

        // Device heartbeat was updated
        assert_eq!(
            *session_store.last_seen.lock().unwrap(),
            vec!["dev1".to_string()]
        );
    }

    #[tokio::test]
    async fn validate_rejects_invalid_token() {
        let (auth_store, session_store) = make_stores();
        let service = SessionValidationService::new(&auth_store, &session_store);

        let err = service.validate("bad-token").await.unwrap_err();
        assert!(matches!(err, ServerCoreError::NotFound(_)));
    }

    #[tokio::test]
    async fn logout_deletes_session() {
        let (auth_store, session_store) = make_stores();
        let service = SessionValidationService::new(&auth_store, &session_store);

        service.logout("valid-token").await.unwrap();

        let err = service.validate("valid-token").await.unwrap_err();
        assert!(matches!(err, ServerCoreError::NotFound(_)));
    }

    #[test]
    fn extract_token_from_bearer() {
        let token = extract_token(Some("Bearer abc123"), None, None);
        assert_eq!(token.as_deref(), Some("abc123"));
    }

    #[test]
    fn extract_token_from_cookie() {
        let token = extract_token(None, Some("other=x; diaryx_session=abc123; foo=bar"), None);
        assert_eq!(token.as_deref(), Some("abc123"));
    }

    #[test]
    fn extract_token_from_query() {
        let token = extract_token(None, None, Some("foo=bar&token=abc123&baz=1"));
        assert_eq!(token.as_deref(), Some("abc123"));
    }

    #[test]
    fn extract_token_bearer_takes_priority() {
        let token = extract_token(
            Some("Bearer from-header"),
            Some("diaryx_session=from-cookie"),
            Some("token=from-query"),
        );
        assert_eq!(token.as_deref(), Some("from-header"));
    }

    #[test]
    fn extract_token_cookie_over_query() {
        let token = extract_token(
            None,
            Some("diaryx_session=from-cookie"),
            Some("token=from-query"),
        );
        assert_eq!(token.as_deref(), Some("from-cookie"));
    }

    #[test]
    fn extract_token_returns_none_when_empty() {
        assert!(extract_token(None, None, None).is_none());
        assert!(extract_token(Some("Basic xyz"), None, None).is_none());
    }
}
