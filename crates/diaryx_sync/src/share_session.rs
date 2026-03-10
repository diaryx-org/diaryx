//! Live share session REST client.
//!
//! Platform-agnostic client for the share session REST API.
//! The HTTP transport is abstracted via `HttpClient`, which has
//! platform-specific implementations:
//!
//! - WASM: `web_sys::WorkerGlobalScope::fetch()`
//! - Native: `reqwest` or `ureq`

use serde::{Deserialize, Serialize};

// ============================================================================
// HttpClient trait
// ============================================================================

/// Simple HTTP response.
#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn json<T: for<'de> Deserialize<'de>>(&self) -> Result<T, String> {
        serde_json::from_slice(&self.body).map_err(|e| format!("JSON parse error: {}", e))
    }
}

/// Minimal async HTTP client trait.
///
/// Platform implementations provide the actual transport:
/// - WASM: `web_sys::fetch`
/// - Native: `reqwest` or similar
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait HttpClient {
    async fn request(
        &self,
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse, String>;
}

// ============================================================================
// Share Session Types
// ============================================================================

/// Request to create a share session.
#[derive(Debug, Serialize)]
struct CreateSessionRequest {
    workspace_id: String,
    read_only: bool,
}

/// Response from session creation.
#[derive(Debug, Deserialize)]
pub struct SessionCreatedResponse {
    pub code: String,
    pub workspace_id: String,
    pub read_only: bool,
}

/// Response from session lookup.
#[derive(Debug, Deserialize)]
pub struct SessionInfoResponse {
    pub code: String,
    pub workspace_id: String,
    pub read_only: bool,
    pub peer_count: usize,
}

/// Error response from the server.
#[derive(Debug, Deserialize)]
struct ErrorResponse {
    #[serde(default)]
    error: String,
}

// ============================================================================
// ShareSessionClient
// ============================================================================

/// REST client for the live share session API.
///
/// Wraps an `HttpClient` and provides typed methods for session lifecycle:
/// - `create_session()` — `POST /api/share/sessions`
/// - `lookup_session()` — `GET /api/share/sessions/{code}`
/// - `delete_session()` — `DELETE /api/share/sessions/{code}`
/// - `update_read_only()` — `PATCH /api/share/sessions/{code}`
pub struct ShareSessionClient<H: HttpClient> {
    http: H,
    base_url: String,
    auth_token: Option<String>,
}

impl<H: HttpClient> ShareSessionClient<H> {
    pub fn new(http: H, base_url: String, auth_token: Option<String>) -> Self {
        // Normalize: strip trailing /sync2, /sync, trailing slash
        let base_url = base_url
            .trim_end_matches("/sync2")
            .trim_end_matches("/sync")
            .trim_end_matches('/')
            .to_string();
        Self {
            http,
            base_url,
            auth_token,
        }
    }

    /// Build common headers (Content-Type + Authorization).
    fn headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
        if let Some(ref token) = self.auth_token {
            headers.push(("Authorization".to_string(), format!("Bearer {}", token)));
        }
        headers
    }

    fn sessions_base_path(&self) -> String {
        format!("{}/api/share/sessions", self.base_url)
    }

    /// Create a new share session.
    ///
    /// `POST /api/share/sessions`
    pub async fn create_session(
        &self,
        workspace_id: &str,
        read_only: bool,
    ) -> Result<SessionCreatedResponse, String> {
        let url = self.sessions_base_path();
        let body = serde_json::to_vec(&CreateSessionRequest {
            workspace_id: workspace_id.to_string(),
            read_only,
        })
        .map_err(|e| format!("Serialize error: {}", e))?;

        let headers = self.headers();

        let resp = self
            .http
            .request("POST".to_string(), url, headers, Some(body))
            .await?;

        if !resp.ok() {
            let err = resp.json::<ErrorResponse>().unwrap_or(ErrorResponse {
                error: format!("HTTP {}", resp.status),
            });
            return Err(err.error);
        }

        resp.json()
    }

    /// Look up an existing session by join code.
    ///
    /// `GET /api/share/sessions/{code}`
    pub async fn lookup_session(&self, join_code: &str) -> Result<SessionInfoResponse, String> {
        let code = join_code.to_uppercase();
        let url = format!("{}/{}", self.sessions_base_path(), code);

        let headers = self.headers();

        let resp = self
            .http
            .request("GET".to_string(), url, headers, None)
            .await?;

        if !resp.ok() {
            let err = resp.json::<ErrorResponse>().unwrap_or(ErrorResponse {
                error: format!("HTTP {}", resp.status),
            });
            return Err(err.error);
        }

        resp.json()
    }

    /// Delete a session (owner only).
    ///
    /// `DELETE /api/share/sessions/{code}`
    pub async fn delete_session(&self, join_code: &str) -> Result<(), String> {
        let code = join_code.to_uppercase();
        let url = format!("{}/{}", self.sessions_base_path(), code);

        let headers = self.headers();

        let resp = self
            .http
            .request("DELETE".to_string(), url, headers, None)
            .await?;

        if resp.ok() || resp.status == 204 {
            Ok(())
        } else {
            let err = resp.json::<ErrorResponse>().unwrap_or(ErrorResponse {
                error: format!("HTTP {}", resp.status),
            });
            Err(err.error)
        }
    }

    /// Update session read-only status (owner only).
    ///
    /// `PATCH /api/share/sessions/{code}`
    pub async fn update_read_only(&self, join_code: &str, read_only: bool) -> Result<(), String> {
        let code = join_code.to_uppercase();
        let url = format!("{}/{}", self.sessions_base_path(), code);
        let body = serde_json::to_vec(&serde_json::json!({ "read_only": read_only }))
            .map_err(|e| format!("Serialize error: {}", e))?;

        let headers = self.headers();

        let resp = self
            .http
            .request("PATCH".to_string(), url, headers, Some(body))
            .await?;

        if resp.ok() {
            Ok(())
        } else {
            let err = resp.json::<ErrorResponse>().unwrap_or(ErrorResponse {
                error: format!("HTTP {}", resp.status),
            });
            Err(err.error)
        }
    }
}
