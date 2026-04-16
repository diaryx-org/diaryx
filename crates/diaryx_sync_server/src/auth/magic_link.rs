use crate::adapters::{
    NativeAuthSessionStore, NativeDeviceStore, NativeMagicLinkStore, NativeUserStore,
};
use crate::config::Config;
use crate::db::AuthRepo;
use diaryx_server::use_cases::auth::{AuthConfig, AuthError, AuthenticationService};

// Re-export core types so they're accessible via `crate::auth::*`
pub use diaryx_server::use_cases::auth::{DeviceLimitDevice, VerifyResult};
use std::sync::Arc;

/// Magic link authentication service.
///
/// Thin wrapper around the portable `AuthenticationService` that provides
/// the native adapters and config.
pub struct MagicLinkService {
    magic_link_store: Arc<NativeMagicLinkStore>,
    user_store: Arc<NativeUserStore>,
    device_store: Arc<NativeDeviceStore>,
    session_store: Arc<NativeAuthSessionStore>,
    auth_config: AuthConfig,
    app_base_url: String,
}

/// Error types for magic link operations (re-exported from core with extras).
#[derive(Debug)]
pub enum MagicLinkError {
    InvalidToken,
    RateLimited,
    DeviceLimitReached {
        limit: u32,
        devices: Vec<DeviceLimitDevice>,
    },
    InvalidReplaceDevice,
    InvalidInput(String),
    DatabaseError(String),
    EmailError(String),
}

impl std::fmt::Display for MagicLinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MagicLinkError::InvalidToken => write!(f, "Invalid or expired magic link"),
            MagicLinkError::RateLimited => {
                write!(f, "Too many requests. Please try again later.")
            }
            MagicLinkError::DeviceLimitReached { limit, .. } => {
                write!(f, "Device limit reached for this account (max {})", limit)
            }
            MagicLinkError::InvalidReplaceDevice => {
                write!(f, "The device to replace was not found on this account")
            }
            MagicLinkError::InvalidInput(msg) => write!(f, "{}", msg),
            MagicLinkError::DatabaseError(e) => write!(f, "Database error: {}", e),
            MagicLinkError::EmailError(e) => write!(f, "Email error: {}", e),
        }
    }
}

impl std::error::Error for MagicLinkError {}

impl From<AuthError> for MagicLinkError {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::InvalidToken => MagicLinkError::InvalidToken,
            AuthError::RateLimited => MagicLinkError::RateLimited,
            AuthError::DeviceLimitReached { limit, devices } => {
                MagicLinkError::DeviceLimitReached { limit, devices }
            }
            AuthError::InvalidReplaceDevice => MagicLinkError::InvalidReplaceDevice,
            AuthError::InvalidInput(msg) => MagicLinkError::InvalidInput(msg),
            AuthError::Internal(e) => MagicLinkError::DatabaseError(e.to_string()),
        }
    }
}

impl MagicLinkService {
    pub fn new(repo: Arc<AuthRepo>, config: Arc<Config>) -> Self {
        let magic_link_store = Arc::new(NativeMagicLinkStore::new(repo.clone()));
        let user_store = Arc::new(NativeUserStore::new(repo.clone()));
        let device_store = Arc::new(NativeDeviceStore::new(repo.clone()));
        let session_store = Arc::new(NativeAuthSessionStore::new(repo));

        let auth_config = AuthConfig {
            magic_link_expiry_minutes: config.magic_link_expiry_minutes,
            session_expiry_days: config.session_expiry_days,
            rate_limit_per_hour: 3,
        };

        Self {
            magic_link_store,
            user_store,
            device_store,
            session_store,
            auth_config,
            app_base_url: config.app_base_url.clone(),
        }
    }

    fn service(&self) -> AuthenticationService<'_> {
        AuthenticationService::new(
            self.magic_link_store.as_ref(),
            self.user_store.as_ref(),
            self.device_store.as_ref(),
            self.session_store.as_ref(),
            &self.auth_config,
        )
    }

    /// Request a magic link for the given email.
    /// Returns (token, verification_code).
    pub async fn request_magic_link(
        &self,
        email: &str,
    ) -> Result<(String, String), MagicLinkError> {
        self.service()
            .request_magic_link(email)
            .await
            .map_err(MagicLinkError::from)
    }

    /// Verify a magic link token and create a session.
    pub async fn verify_magic_link(
        &self,
        token: &str,
        device_name: Option<&str>,
        user_agent: Option<&str>,
        replace_device_id: Option<&str>,
    ) -> Result<VerifyResult, MagicLinkError> {
        self.service()
            .verify_magic_link(token, device_name, user_agent, replace_device_id)
            .await
            .map_err(MagicLinkError::from)
    }

    /// Verify a 6-digit code and create a session.
    pub async fn verify_code(
        &self,
        code: &str,
        email: &str,
        device_name: Option<&str>,
        user_agent: Option<&str>,
        replace_device_id: Option<&str>,
    ) -> Result<VerifyResult, MagicLinkError> {
        self.service()
            .verify_code(code, email, device_name, user_agent, replace_device_id)
            .await
            .map_err(MagicLinkError::from)
    }

    /// Shared session-creation logic used by passkey verification.
    pub(crate) async fn create_session_for_email(
        &self,
        email: &str,
        device_name: Option<&str>,
        user_agent: Option<&str>,
        replace_device_id: Option<&str>,
    ) -> Result<VerifyResult, MagicLinkError> {
        self.service()
            .create_session_for_email(email, device_name, user_agent, replace_device_id)
            .await
            .map_err(MagicLinkError::from)
    }

    /// Build the magic link URL for a token.
    pub fn build_magic_link_url(&self, token: &str) -> String {
        format!("{}?token={}", self.app_base_url, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_database;
    use rusqlite::Connection;

    fn setup_test_service() -> MagicLinkService {
        let conn = Connection::open_in_memory().unwrap();
        init_database(&conn).unwrap();
        let repo = Arc::new(AuthRepo::new(conn));
        let config = Arc::new(Config::from_env().unwrap());
        MagicLinkService::new(repo, config)
    }

    #[tokio::test]
    async fn test_magic_link_flow() {
        let service = setup_test_service();

        let (token, code) = service
            .request_magic_link("test@example.com")
            .await
            .unwrap();
        assert!(!token.is_empty());
        assert_eq!(code.len(), 6);

        let result = service
            .verify_magic_link(&token, Some("Test"), None, None)
            .await
            .unwrap();
        assert_eq!(result.email, "test@example.com");
        assert!(!result.session_token.is_empty());
        assert!(!result.user_id.is_empty());
        assert!(!result.device_id.is_empty());

        let second_try = service.verify_magic_link(&token, None, None, None).await;
        assert!(matches!(second_try, Err(MagicLinkError::InvalidToken)));
    }

    #[tokio::test]
    async fn test_verification_code_flow() {
        let service = setup_test_service();

        let (_token, code) = service
            .request_magic_link("code@example.com")
            .await
            .unwrap();

        let result = service
            .verify_code(&code, "code@example.com", Some("Test Device"), None, None)
            .await
            .unwrap();
        assert_eq!(result.email, "code@example.com");
        assert!(!result.session_token.is_empty());

        let second_try = service
            .verify_code(&code, "code@example.com", None, None, None)
            .await;
        assert!(matches!(second_try, Err(MagicLinkError::InvalidToken)));
    }

    #[tokio::test]
    async fn test_code_and_link_consume_same_row() {
        let service = setup_test_service();

        let (token, code) = service
            .request_magic_link("both@example.com")
            .await
            .unwrap();

        service
            .verify_code(&code, "both@example.com", None, None, None)
            .await
            .unwrap();

        let link_try = service.verify_magic_link(&token, None, None, None).await;
        assert!(matches!(link_try, Err(MagicLinkError::InvalidToken)));
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let service = setup_test_service();
        let email = "ratelimit@example.com";

        for _ in 0..3 {
            service.request_magic_link(email).await.unwrap();
        }

        let result = service.request_magic_link(email).await;
        assert!(matches!(result, Err(MagicLinkError::RateLimited)));
    }

    #[tokio::test]
    async fn test_free_tier_allows_two_devices_only() {
        let service = setup_test_service();
        let first = service
            .create_session_for_email("free-devices@example.com", Some("MacBook"), None, None)
            .await
            .unwrap();
        let second = service
            .create_session_for_email("free-devices@example.com", Some("iPhone"), None, None)
            .await
            .unwrap();

        assert_ne!(first.device_id, second.device_id);

        let third = service
            .create_session_for_email("free-devices@example.com", Some("iPad"), None, None)
            .await;
        assert!(matches!(
            third,
            Err(MagicLinkError::DeviceLimitReached { limit: 2, .. })
        ));
    }
}
