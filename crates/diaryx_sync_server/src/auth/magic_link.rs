use crate::config::Config;
use crate::db::AuthRepo;
use chrono::{Duration, Utc};
use serde::Serialize;
use std::sync::Arc;

/// Magic link authentication service
pub struct MagicLinkService {
    repo: Arc<AuthRepo>,
    config: Arc<Config>,
}

/// Result of magic link verification
#[derive(Debug)]
pub struct VerifyResult {
    pub session_token: String,
    pub user_id: String,
    pub device_id: String,
    pub email: String,
}

/// Info about a device, returned when the device limit is reached so the
/// client can offer a "replace this device?" prompt.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceLimitDevice {
    pub id: String,
    pub name: Option<String>,
    pub last_seen_at: String,
}

/// Error types for magic link operations
#[derive(Debug)]
pub enum MagicLinkError {
    /// Token not found or expired
    InvalidToken,
    /// Too many magic link requests (rate limited)
    RateLimited,
    /// Account already has the maximum number of registered devices
    DeviceLimitReached {
        limit: u32,
        devices: Vec<DeviceLimitDevice>,
    },
    /// The replace_device_id didn't match any device owned by this user
    InvalidReplaceDevice,
    /// Database error
    DatabaseError(String),
    /// Email sending error
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
            MagicLinkError::DatabaseError(e) => write!(f, "Database error: {}", e),
            MagicLinkError::EmailError(e) => write!(f, "Email error: {}", e),
        }
    }
}

impl std::error::Error for MagicLinkError {}

impl MagicLinkService {
    /// Create a new MagicLinkService
    pub fn new(repo: Arc<AuthRepo>, config: Arc<Config>) -> Self {
        Self { repo, config }
    }

    /// Request a magic link for the given email
    ///
    /// Returns (token, verification_code)
    pub fn request_magic_link(&self, email: &str) -> Result<(String, String), MagicLinkError> {
        // Normalize email
        let email = email.trim().to_lowercase();

        // Rate limiting: max 3 tokens per hour per email
        let one_hour_ago = Utc::now() - Duration::hours(1);
        let recent_count = self
            .repo
            .count_recent_magic_tokens(&email, one_hour_ago)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        if recent_count >= 3 {
            return Err(MagicLinkError::RateLimited);
        }

        // Create token with configured expiration
        let expires_at = Utc::now() + Duration::minutes(self.config.magic_link_expiry_minutes);
        let (token, code) = self
            .repo
            .create_magic_token(&email, expires_at)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        Ok((token, code))
    }

    /// Verify a magic link token and create a session
    ///
    /// Returns the session token and user info on success.
    /// If `replace_device_id` is provided and the device limit has been
    /// reached, the specified device is removed first.
    pub fn verify_magic_link(
        &self,
        token: &str,
        device_name: Option<&str>,
        user_agent: Option<&str>,
        replace_device_id: Option<&str>,
    ) -> Result<VerifyResult, MagicLinkError> {
        // Validate the token without consuming it — consumption happens only
        // after the session is successfully created, so the token stays valid
        // if the caller needs to retry (e.g. after choosing a device to replace).
        let email = self
            .repo
            .peek_magic_token(token)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?
            .ok_or(MagicLinkError::InvalidToken)?;

        let result =
            self.create_session_for_email(&email, device_name, user_agent, replace_device_id)?;

        // Session created — now consume the token so it can't be reused.
        self.repo
            .consume_magic_token(token)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        Ok(result)
    }

    /// Verify a 6-digit code and create a session.
    /// If `replace_device_id` is provided and the device limit has been
    /// reached, the specified device is removed first.
    pub fn verify_code(
        &self,
        code: &str,
        email: &str,
        device_name: Option<&str>,
        user_agent: Option<&str>,
        replace_device_id: Option<&str>,
    ) -> Result<VerifyResult, MagicLinkError> {
        let email = email.trim().to_lowercase();

        // Validate the code without consuming it.
        self.repo
            .peek_magic_code(code, &email)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?
            .ok_or(MagicLinkError::InvalidToken)?;

        let result =
            self.create_session_for_email(&email, device_name, user_agent, replace_device_id)?;

        // Session created — now consume the code.
        self.repo
            .consume_magic_code(code, &email)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        Ok(result)
    }

    /// Shared session-creation logic used by link, code, and passkey verification.
    ///
    /// When `replace_device_id` is `Some`, the specified device will be removed
    /// to make room if the device limit is reached, instead of returning an error.
    pub(crate) fn create_session_for_email(
        &self,
        email: &str,
        device_name: Option<&str>,
        user_agent: Option<&str>,
        replace_device_id: Option<&str>,
    ) -> Result<VerifyResult, MagicLinkError> {
        // Get or create user
        let user_id = self
            .repo
            .get_or_create_user(email)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        let device_limit = self
            .repo
            .get_effective_device_limit(&user_id)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;
        let device_count = self
            .repo
            .count_user_devices(&user_id)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        if device_count >= device_limit {
            if let Some(replace_id) = replace_device_id {
                // Verify the device belongs to this user before deleting
                let devices = self
                    .repo
                    .get_user_devices(&user_id)
                    .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;
                if !devices.iter().any(|d| d.id == replace_id) {
                    return Err(MagicLinkError::InvalidReplaceDevice);
                }
                self.repo
                    .delete_device(replace_id)
                    .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;
            } else {
                let devices = self
                    .repo
                    .get_user_devices(&user_id)
                    .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;
                return Err(MagicLinkError::DeviceLimitReached {
                    limit: device_limit,
                    devices: devices
                        .into_iter()
                        .map(|d| DeviceLimitDevice {
                            id: d.id,
                            name: d.name,
                            last_seen_at: d.last_seen_at.to_rfc3339(),
                        })
                        .collect(),
                });
            }
        }

        // Update last login
        self.repo
            .update_last_login(&user_id)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        // Create device
        let device_id = self
            .repo
            .create_device(&user_id, device_name, user_agent)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        // Create session
        let expires_at = Utc::now() + Duration::days(self.config.session_expiry_days);
        let session_token = self
            .repo
            .create_session(&user_id, &device_id, expires_at)
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;

        Ok(VerifyResult {
            session_token,
            user_id,
            device_id,
            email: email.to_string(),
        })
    }

    /// Build the magic link URL for a token
    pub fn build_magic_link_url(&self, token: &str) -> String {
        format!("{}?token={}", self.config.app_base_url, token)
    }

    /// Clean up expired tokens (should be called periodically)
    pub fn cleanup_expired(&self) -> Result<(usize, usize), MagicLinkError> {
        let tokens_deleted = self
            .repo
            .cleanup_expired_magic_tokens()
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;
        let sessions_deleted = self
            .repo
            .cleanup_expired_sessions()
            .map_err(|e| MagicLinkError::DatabaseError(e.to_string()))?;
        Ok((tokens_deleted, sessions_deleted))
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

    #[test]
    fn test_magic_link_flow() {
        let service = setup_test_service();

        // Request magic link
        let (token, code) = service.request_magic_link("test@example.com").unwrap();
        assert!(!token.is_empty());
        assert_eq!(code.len(), 6);

        // Verify magic link
        let result = service
            .verify_magic_link(&token, Some("Test"), None, None)
            .unwrap();
        assert_eq!(result.email, "test@example.com");
        assert!(!result.session_token.is_empty());
        assert!(!result.user_id.is_empty());
        assert!(!result.device_id.is_empty());

        // Token should be consumed
        let second_try = service.verify_magic_link(&token, None, None, None);
        assert!(matches!(second_try, Err(MagicLinkError::InvalidToken)));
    }

    #[test]
    fn test_verification_code_flow() {
        let service = setup_test_service();

        let (_token, code) = service.request_magic_link("code@example.com").unwrap();

        // Verify using the code
        let result = service
            .verify_code(&code, "code@example.com", Some("Test Device"), None, None)
            .unwrap();
        assert_eq!(result.email, "code@example.com");
        assert!(!result.session_token.is_empty());

        // Code should be consumed — second attempt should fail
        let second_try = service.verify_code(&code, "code@example.com", None, None, None);
        assert!(matches!(second_try, Err(MagicLinkError::InvalidToken)));
    }

    #[test]
    fn test_code_and_link_consume_same_row() {
        let service = setup_test_service();

        let (token, code) = service.request_magic_link("both@example.com").unwrap();

        // Using the code should also consume the link
        service
            .verify_code(&code, "both@example.com", None, None, None)
            .unwrap();

        // Link should now be invalid
        let link_try = service.verify_magic_link(&token, None, None, None);
        assert!(matches!(link_try, Err(MagicLinkError::InvalidToken)));
    }

    #[test]
    fn test_rate_limiting() {
        let service = setup_test_service();
        let email = "ratelimit@example.com";

        // First 3 should succeed
        for _ in 0..3 {
            service.request_magic_link(email).unwrap();
        }

        // 4th should be rate limited
        let result = service.request_magic_link(email);
        assert!(matches!(result, Err(MagicLinkError::RateLimited)));
    }

    #[test]
    fn test_free_tier_allows_two_devices_only() {
        let service = setup_test_service();
        let first = service
            .create_session_for_email("free-devices@example.com", Some("MacBook"), None, None)
            .unwrap();
        let second = service
            .create_session_for_email("free-devices@example.com", Some("iPhone"), None, None)
            .unwrap();

        assert_ne!(first.device_id, second.device_id);

        let third =
            service.create_session_for_email("free-devices@example.com", Some("iPad"), None, None);
        assert!(matches!(
            third,
            Err(MagicLinkError::DeviceLimitReached { limit: 2, .. })
        ));
    }
}
