//! Auth types shared across platforms.

use serde::{Deserialize, Serialize};

/// Default sync server URL.
pub const DEFAULT_SYNC_SERVER: &str = "https://sync.diaryx.org";

/// Stored authentication credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthCredentials {
    /// Sync server URL.
    pub server_url: String,
    /// Session token for authenticated requests.
    pub session_token: Option<String>,
    /// Authenticated user's email.
    pub email: Option<String>,
    /// Workspace ID on the server.
    pub workspace_id: Option<String>,
}

/// User info returned by the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// Server-assigned user ID.
    pub id: String,
    /// User's email address.
    pub email: String,
}

/// Server workspace info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerWorkspace {
    /// Server-assigned workspace ID.
    pub id: String,
    /// Workspace display name.
    pub name: String,
}

/// Device info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    /// Device ID.
    pub id: String,
    /// Device name (may be null).
    pub name: Option<String>,
    /// Last seen timestamp.
    pub last_seen_at: Option<String>,
}

/// Response from magic link request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MagicLinkResponse {
    /// Whether the request succeeded.
    pub success: bool,
    /// Human-readable message.
    pub message: String,
    /// Dev-only: direct verification link.
    pub dev_link: Option<String>,
    /// Dev-only: verification code.
    pub dev_code: Option<String>,
}

/// Response from magic link or code verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResponse {
    /// Whether verification succeeded.
    pub success: bool,
    /// Session token for subsequent requests.
    pub token: String,
    /// Authenticated user.
    pub user: User,
}

/// Response from /auth/me endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeResponse {
    /// Authenticated user.
    pub user: User,
    /// User's workspaces on the server.
    pub workspaces: Vec<ServerWorkspace>,
    /// User's registered devices.
    pub devices: Vec<Device>,
    /// Maximum number of workspaces allowed.
    pub workspace_limit: u32,
    /// User's billing tier (e.g. "free", "plus").
    pub tier: String,
    /// Maximum number of published sites.
    pub published_site_limit: u32,
    /// Attachment storage limit in bytes.
    pub attachment_limit_bytes: u64,
}

/// Auth error with HTTP status code.
#[derive(Debug, Clone)]
pub struct AuthError {
    /// Human-readable error message.
    pub message: String,
    /// HTTP status code (0 if not HTTP-related).
    pub status_code: u16,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.status_code > 0 {
            write!(f, "{} (HTTP {})", self.message, self.status_code)
        } else {
            write!(f, "{}", self.message)
        }
    }
}

impl std::error::Error for AuthError {}

impl AuthError {
    /// Create a new auth error.
    pub fn new(message: impl Into<String>, status_code: u16) -> Self {
        Self {
            message: message.into(),
            status_code,
        }
    }

    /// Create a network/connection error (no HTTP status).
    pub fn network(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status_code: 0,
        }
    }

    /// Whether this is a 401 Unauthorized error.
    pub fn is_unauthorized(&self) -> bool {
        self.status_code == 401
    }

    /// Whether this is a session expired error.
    pub fn is_session_expired(&self) -> bool {
        self.status_code == 401
    }
}

/// HTTP response from [`AuthHttpClient`].
#[derive(Debug)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response body as string.
    pub body: String,
}

impl HttpResponse {
    /// Whether the response has a success status code (2xx).
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Parse the response body as JSON.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, AuthError> {
        serde_json::from_str(&self.body)
            .map_err(|e| AuthError::new(format!("Failed to parse response: {}", e), self.status))
    }
}

/// Trait for platform-specific HTTP requests.
///
/// Implementations:
/// - CLI: `reqwest::blocking::Client` (or async reqwest)
/// - WASM: `js-sys fetch` / `proxyFetch`
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait AuthHttpClient {
    /// Send a GET request.
    async fn get(&self, url: &str, bearer_token: Option<&str>) -> Result<HttpResponse, AuthError>;

    /// Send a POST request with a JSON body.
    async fn post(
        &self,
        url: &str,
        bearer_token: Option<&str>,
        json_body: Option<&str>,
    ) -> Result<HttpResponse, AuthError>;

    /// Send a PATCH request with a JSON body.
    async fn patch(
        &self,
        url: &str,
        bearer_token: Option<&str>,
        json_body: Option<&str>,
    ) -> Result<HttpResponse, AuthError>;

    /// Send a DELETE request.
    async fn delete(
        &self,
        url: &str,
        bearer_token: Option<&str>,
    ) -> Result<HttpResponse, AuthError>;
}

/// Trait for platform-specific credential storage.
///
/// Implementations:
/// - CLI: reads/writes `Config` TOML file
/// - WASM: reads/writes `localStorage`
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait AuthStorage {
    /// Load stored credentials.
    async fn load_credentials(&self) -> Option<AuthCredentials>;

    /// Save credentials.
    async fn save_credentials(&self, credentials: &AuthCredentials);

    /// Clear stored session token (keep email/server for re-login convenience).
    async fn clear_session(&self);
}
