//! Auth types shared across platforms.

use serde::{Deserialize, Serialize};

/// Default sync server URL.
pub const DEFAULT_SYNC_SERVER: &str = "https://app.diaryx.org/api";

/// Non-secret session metadata.
///
/// Unlike a raw session token, this struct is safe to log, serialize, pass to
/// UI layers, or expose through IPC. It intentionally does **not** contain the
/// session token — that stays encapsulated inside each [`AuthenticatedClient`]
/// implementation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthMetadata {
    /// Authenticated user's email.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Workspace ID on the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthError {
    /// Human-readable error message.
    pub message: String,
    /// HTTP status code (0 if not HTTP-related).
    pub status_code: u16,
    /// When the error is "device limit reached" (403), this holds the device
    /// list returned by the server so the UI can offer device replacement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub devices: Option<Vec<Device>>,
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
            devices: None,
        }
    }

    /// Attach a device list (used for 403 device-limit errors).
    pub fn with_devices(mut self, devices: Vec<Device>) -> Self {
        self.devices = Some(devices);
        self
    }

    /// Create a network/connection error (no HTTP status).
    pub fn network(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status_code: 0,
            devices: None,
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

/// HTTP response from an [`AuthenticatedClient`].
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

/// Platform-agnostic authenticated HTTP client.
///
/// Bundles credential storage and HTTP transport so the session token never
/// appears in service-level code. Each implementation owns its server URL and
/// decides how authentication is injected:
///
/// - **CLI (`FsAuthenticatedClient`)**: token is stored in `auth.md` frontmatter
///   and attached as `Authorization: Bearer` headers.
/// - **Tauri (`KeyringAuthenticatedClient`)**: token lives in the OS keyring and
///   is looked up per request.
/// - **Web (`BrowserAuthenticatedClient`)**: token lives in an HttpOnly cookie
///   set by the server; the client sets `credentials: 'include'` on every fetch
///   and never sees the token string.
///
/// `path` arguments to request methods should be server-relative (e.g.
/// `"/auth/me"`); the implementation prepends its configured server URL.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait AuthenticatedClient {
    /// Server URL this client talks to.
    fn server_url(&self) -> &str;

    /// True iff a session is currently established.
    async fn has_session(&self) -> bool;

    /// Load non-secret session metadata, if any.
    async fn load_metadata(&self) -> Option<AuthMetadata>;

    /// Persist non-secret session metadata.
    async fn save_metadata(&self, metadata: &AuthMetadata);

    /// Persist a newly-issued session token. On browser this is typically a
    /// no-op — the server sets the HttpOnly cookie directly on the response.
    async fn store_session_token(&self, token: &str);

    /// Clear local session state. On browser this only clears the local
    /// "has session" flag; the actual cookie deletion happens server-side via
    /// `/auth/logout` (which the caller is expected to invoke first).
    async fn clear_session(&self);

    // ========================================================================
    // Authenticated requests — implementations inject auth.
    // ========================================================================

    /// Send an authenticated GET request.
    async fn get(&self, path: &str) -> Result<HttpResponse, AuthError>;

    /// Send an authenticated POST request with an optional JSON body.
    async fn post(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError>;

    /// Send an authenticated PUT request with an optional JSON body.
    async fn put(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError>;

    /// Send an authenticated PATCH request with an optional JSON body.
    async fn patch(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError>;

    /// Send an authenticated DELETE request.
    async fn delete(&self, path: &str) -> Result<HttpResponse, AuthError>;

    // ========================================================================
    // Unauthenticated requests — used for the login flow (magic link request,
    // magic link verify, code verify). Implementations must NOT attach auth.
    // ========================================================================

    /// Send an unauthenticated GET request (used for the magic-link verify flow).
    async fn get_unauth(&self, path: &str) -> Result<HttpResponse, AuthError>;

    /// Send an unauthenticated POST request (used for magic-link request and code verify).
    async fn post_unauth(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError>;
}
