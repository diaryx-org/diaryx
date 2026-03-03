//! Platform-agnostic auth service.

use super::types::*;

/// Platform-agnostic authentication service.
///
/// Handles magic link authentication, session management, and user info
/// queries. Platform-specific HTTP and storage are injected via traits.
pub struct AuthService<H: AuthHttpClient, S: AuthStorage> {
    http: H,
    storage: S,
}

impl<H: AuthHttpClient, S: AuthStorage> AuthService<H, S> {
    /// Create a new auth service.
    pub fn new(http: H, storage: S) -> Self {
        Self { http, storage }
    }

    /// Resolve the server URL from explicit value, stored credentials, or default.
    async fn resolve_server_url(&self, explicit: Option<&str>) -> String {
        if let Some(url) = explicit {
            return url.trim_end_matches('/').to_string();
        }
        if let Some(creds) = self.storage.load_credentials().await
            && !creds.server_url.is_empty()
        {
            return creds.server_url.trim_end_matches('/').to_string();
        }
        DEFAULT_SYNC_SERVER.to_string()
    }

    /// Get the current auth token from storage, if any.
    pub async fn get_token(&self) -> Option<String> {
        self.storage
            .load_credentials()
            .await
            .and_then(|c| c.session_token)
    }

    /// Get stored credentials.
    pub async fn get_credentials(&self) -> Option<AuthCredentials> {
        self.storage.load_credentials().await
    }

    /// Check whether the user has a stored session token.
    pub async fn is_authenticated(&self) -> bool {
        self.get_token().await.is_some()
    }

    // =========================================================================
    // Magic Link Flow
    // =========================================================================

    /// Request a magic link for the given email.
    ///
    /// Sends a POST to `{server}/auth/magic-link` and saves the server URL
    /// and email to storage for later use.
    pub async fn request_magic_link(
        &self,
        email: &str,
        server: Option<&str>,
    ) -> Result<MagicLinkResponse, AuthError> {
        let server_url = self.resolve_server_url(server).await;
        let url = format!("{}/auth/magic-link", server_url);
        let body = serde_json::json!({ "email": email }).to_string();

        let resp = self.http.post(&url, None, Some(&body)).await?;

        if !resp.is_success() {
            let msg = resp
                .json::<serde_json::Value>()
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
                .unwrap_or_else(|| format!("Failed to request magic link: HTTP {}", resp.status));
            return Err(AuthError::new(msg, resp.status));
        }

        // Save server URL and email for verify step
        let mut creds = self
            .storage
            .load_credentials()
            .await
            .unwrap_or(AuthCredentials {
                server_url: server_url.clone(),
                session_token: None,
                email: None,
                workspace_id: None,
            });
        creds.server_url = server_url;
        creds.email = Some(email.to_string());
        self.storage.save_credentials(&creds).await;

        resp.json()
    }

    /// Verify a magic link token and obtain a session token.
    ///
    /// Sends a GET to `{server}/auth/verify?token=...&device_name=...` and
    /// saves the resulting session token to storage.
    pub async fn verify_magic_link(
        &self,
        token: &str,
        device_name: Option<&str>,
    ) -> Result<VerifyResponse, AuthError> {
        let server_url = self.resolve_server_url(None).await;
        let device = device_name.unwrap_or("Diaryx");

        let url = format!(
            "{}/auth/verify?token={}&device_name={}",
            server_url,
            urlencoding::encode(token),
            urlencoding::encode(device)
        );

        let resp = self.http.get(&url, None).await?;

        if !resp.is_success() {
            if resp.status == 401 || resp.status == 400 {
                return Err(AuthError::new("Invalid or expired token", resp.status));
            }
            return Err(AuthError::new(
                format!("Verification failed: HTTP {}", resp.status),
                resp.status,
            ));
        }

        // Parse response — server may return "token" or "session_token"
        let json: serde_json::Value = resp.json()?;
        let session_token = json
            .get("token")
            .or_else(|| json.get("session_token"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| {
                AuthError::new("No session token in verification response", resp.status)
            })?;

        let email = json
            .get("user")
            .and_then(|u| u.get("email"))
            .and_then(|v| v.as_str())
            .or_else(|| json.get("email").and_then(|v| v.as_str()))
            .map(String::from);

        let user_id = json
            .get("user")
            .and_then(|u| u.get("id"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let workspace_id = json
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or(user_id.clone());

        // Save credentials
        let mut creds = self
            .storage
            .load_credentials()
            .await
            .unwrap_or(AuthCredentials {
                server_url: server_url.clone(),
                session_token: None,
                email: None,
                workspace_id: None,
            });
        creds.session_token = Some(session_token.clone());
        if let Some(ref e) = email {
            creds.email = Some(e.clone());
        }
        if let Some(ref wid) = workspace_id {
            creds.workspace_id = Some(wid.clone());
        }
        self.storage.save_credentials(&creds).await;

        // Build typed response
        let user = User {
            id: user_id.unwrap_or_default(),
            email: email.unwrap_or_default(),
        };

        Ok(VerifyResponse {
            success: true,
            token: session_token,
            user,
        })
    }

    /// Verify a 6-digit code and obtain a session token.
    pub async fn verify_code(
        &self,
        code: &str,
        email: &str,
        device_name: Option<&str>,
    ) -> Result<VerifyResponse, AuthError> {
        let server_url = self.resolve_server_url(None).await;
        let url = format!("{}/auth/verify-code", server_url);
        let body = serde_json::json!({
            "code": code,
            "email": email,
            "device_name": device_name.unwrap_or("Diaryx"),
        })
        .to_string();

        let resp = self.http.post(&url, None, Some(&body)).await?;

        if !resp.is_success() {
            let msg = resp
                .json::<serde_json::Value>()
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
                .unwrap_or_else(|| format!("Failed to verify code: HTTP {}", resp.status));
            return Err(AuthError::new(msg, resp.status));
        }

        let verify: VerifyResponse = resp.json()?;

        // Save credentials
        let mut creds = self
            .storage
            .load_credentials()
            .await
            .unwrap_or(AuthCredentials {
                server_url,
                session_token: None,
                email: None,
                workspace_id: None,
            });
        creds.session_token = Some(verify.token.clone());
        creds.email = Some(verify.user.email.clone());
        self.storage.save_credentials(&creds).await;

        Ok(verify)
    }

    // =========================================================================
    // Session Management
    // =========================================================================

    /// Get current user info from the server.
    ///
    /// Requires an active session token.
    pub async fn get_me(&self) -> Result<MeResponse, AuthError> {
        let creds = self
            .storage
            .load_credentials()
            .await
            .ok_or_else(|| AuthError::new("Not authenticated", 0))?;
        let token = creds
            .session_token
            .as_deref()
            .ok_or_else(|| AuthError::new("No session token", 0))?;

        let url = format!("{}/auth/me", creds.server_url);
        let resp = self.http.get(&url, Some(token)).await?;

        if !resp.is_success() {
            if resp.status == 401 {
                return Err(AuthError::new("Session expired", 401));
            }
            return Err(AuthError::new(
                format!("Failed to get user info: HTTP {}", resp.status),
                resp.status,
            ));
        }

        resp.json()
    }

    /// Log out — clear the session token and notify the server.
    pub async fn logout(&self) -> Result<(), AuthError> {
        let creds = self.storage.load_credentials().await;

        // Best-effort server notification
        if let Some(ref creds) = creds
            && let Some(ref token) = creds.session_token
        {
            let url = format!("{}/auth/logout", creds.server_url);
            let _ = self.http.post(&url, Some(token), None).await;
        }

        // Clear local session
        self.storage.clear_session().await;

        Ok(())
    }

    /// Refresh token by re-validating with the server.
    ///
    /// Calls `/auth/me` to check if the token is still valid. Returns the
    /// server response if valid, or an error if expired.
    pub async fn refresh_token(&self) -> Result<MeResponse, AuthError> {
        self.get_me().await
    }

    // =========================================================================
    // Device Management
    // =========================================================================

    /// Get the user's registered devices.
    pub async fn get_devices(&self) -> Result<Vec<Device>, AuthError> {
        let (url, token) = self.authenticated_url("/auth/devices").await?;
        let resp = self.http.get(&url, Some(&token)).await?;

        if !resp.is_success() {
            return Err(AuthError::new("Failed to get devices", resp.status));
        }

        resp.json()
    }

    /// Rename a device.
    pub async fn rename_device(&self, device_id: &str, new_name: &str) -> Result<(), AuthError> {
        let (base_url, token) = self.authenticated_url("").await?;
        let url = format!("{}/auth/devices/{}", base_url, device_id);
        let body = serde_json::json!({ "name": new_name }).to_string();

        let resp = self.http.patch(&url, Some(&token), Some(&body)).await?;
        if !resp.is_success() {
            return Err(AuthError::new("Failed to rename device", resp.status));
        }
        Ok(())
    }

    /// Delete a device.
    pub async fn delete_device(&self, device_id: &str) -> Result<(), AuthError> {
        let (base_url, token) = self.authenticated_url("").await?;
        let url = format!("{}/auth/devices/{}", base_url, device_id);

        let resp = self.http.delete(&url, Some(&token)).await?;
        if !resp.is_success() {
            return Err(AuthError::new("Failed to delete device", resp.status));
        }
        Ok(())
    }

    // =========================================================================
    // Account Management
    // =========================================================================

    /// Delete the user's account and all server data.
    pub async fn delete_account(&self) -> Result<(), AuthError> {
        let (url, token) = self.authenticated_url("/auth/account").await?;
        let resp = self.http.delete(&url, Some(&token)).await?;

        if !resp.is_success() {
            return Err(AuthError::new("Failed to delete account", resp.status));
        }

        self.storage.clear_session().await;
        Ok(())
    }

    // =========================================================================
    // Workspace CRUD
    // =========================================================================

    /// Create a workspace on the server.
    pub async fn create_workspace(&self, name: &str) -> Result<ServerWorkspace, AuthError> {
        let (base_url, token) = self.authenticated_url("").await?;
        let url = format!("{}/api/workspaces", base_url);
        let body = serde_json::json!({ "name": name }).to_string();

        let resp = self.http.post(&url, Some(&token), Some(&body)).await?;

        if !resp.is_success() {
            let msg = resp
                .json::<serde_json::Value>()
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from));
            if resp.status == 403 {
                return Err(AuthError::new(
                    msg.unwrap_or_else(|| "Workspace limit reached".into()),
                    403,
                ));
            }
            if resp.status == 409 {
                return Err(AuthError::new(
                    msg.unwrap_or_else(|| "Workspace name already taken".into()),
                    409,
                ));
            }
            return Err(AuthError::new(
                msg.unwrap_or_else(|| "Failed to create workspace".into()),
                resp.status,
            ));
        }

        resp.json()
    }

    /// Rename a workspace on the server.
    pub async fn rename_workspace(
        &self,
        workspace_id: &str,
        new_name: &str,
    ) -> Result<(), AuthError> {
        let (base_url, token) = self.authenticated_url("").await?;
        let url = format!("{}/api/workspaces/{}", base_url, workspace_id);
        let body = serde_json::json!({ "name": new_name }).to_string();

        let resp = self.http.patch(&url, Some(&token), Some(&body)).await?;
        if !resp.is_success() {
            return Err(AuthError::new("Failed to rename workspace", resp.status));
        }
        Ok(())
    }

    /// Delete a workspace on the server.
    pub async fn delete_workspace(&self, workspace_id: &str) -> Result<(), AuthError> {
        let (base_url, token) = self.authenticated_url("").await?;
        let url = format!("{}/api/workspaces/{}", base_url, workspace_id);

        let resp = self.http.delete(&url, Some(&token)).await?;
        if !resp.is_success() {
            return Err(AuthError::new("Failed to delete workspace", resp.status));
        }
        Ok(())
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Build a full URL and extract the auth token. Returns (full_url, token).
    async fn authenticated_url(&self, path: &str) -> Result<(String, String), AuthError> {
        let creds = self
            .storage
            .load_credentials()
            .await
            .ok_or_else(|| AuthError::new("Not authenticated", 0))?;
        let token = creds
            .session_token
            .ok_or_else(|| AuthError::new("No session token", 0))?;
        let url = format!("{}{}", creds.server_url, path);
        Ok((url, token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Mock implementations for testing
    // =========================================================================

    struct MockHttp {
        responses: std::sync::Mutex<Vec<HttpResponse>>,
    }

    impl MockHttp {
        fn new(responses: Vec<HttpResponse>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses),
            }
        }
    }

    #[async_trait::async_trait]
    impl AuthHttpClient for MockHttp {
        async fn get(&self, _url: &str, _token: Option<&str>) -> Result<HttpResponse, AuthError> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Err(AuthError::network("No mock response"))
            } else {
                Ok(responses.remove(0))
            }
        }

        async fn post(
            &self,
            _url: &str,
            _token: Option<&str>,
            _body: Option<&str>,
        ) -> Result<HttpResponse, AuthError> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Err(AuthError::network("No mock response"))
            } else {
                Ok(responses.remove(0))
            }
        }

        async fn patch(
            &self,
            _url: &str,
            _token: Option<&str>,
            _body: Option<&str>,
        ) -> Result<HttpResponse, AuthError> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Err(AuthError::network("No mock response"))
            } else {
                Ok(responses.remove(0))
            }
        }

        async fn delete(
            &self,
            _url: &str,
            _token: Option<&str>,
        ) -> Result<HttpResponse, AuthError> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Err(AuthError::network("No mock response"))
            } else {
                Ok(responses.remove(0))
            }
        }
    }

    struct MockStorage {
        creds: std::sync::Mutex<Option<AuthCredentials>>,
    }

    impl MockStorage {
        fn new() -> Self {
            Self {
                creds: std::sync::Mutex::new(None),
            }
        }

        fn with_creds(creds: AuthCredentials) -> Self {
            Self {
                creds: std::sync::Mutex::new(Some(creds)),
            }
        }
    }

    #[async_trait::async_trait]
    impl AuthStorage for MockStorage {
        async fn load_credentials(&self) -> Option<AuthCredentials> {
            self.creds.lock().unwrap().clone()
        }

        async fn save_credentials(&self, credentials: &AuthCredentials) {
            *self.creds.lock().unwrap() = Some(credentials.clone());
        }

        async fn clear_session(&self) {
            if let Some(ref mut creds) = *self.creds.lock().unwrap() {
                creds.session_token = None;
            }
        }
    }

    // =========================================================================
    // Tests
    // =========================================================================

    fn run<F: std::future::Future>(f: F) -> F::Output {
        futures_lite::future::block_on(f)
    }

    #[test]
    fn test_request_magic_link_success() {
        run(async {
            let http = MockHttp::new(vec![HttpResponse {
                status: 200,
                body: r#"{"success":true,"message":"Check your email"}"#.to_string(),
            }]);
            let storage = MockStorage::new();
            let service = AuthService::new(http, storage);

            let result = service.request_magic_link("user@example.com", None).await;
            assert!(result.is_ok());
            let resp = result.unwrap();
            assert!(resp.success);
        });
    }

    #[test]
    fn test_request_magic_link_saves_credentials() {
        run(async {
            let http = MockHttp::new(vec![HttpResponse {
                status: 200,
                body: r#"{"success":true,"message":"Check your email"}"#.to_string(),
            }]);
            let storage = MockStorage::new();
            let service = AuthService::new(http, storage);

            let _ = service
                .request_magic_link("user@example.com", Some("https://custom.server"))
                .await;

            let creds = service.get_credentials().await.unwrap();
            assert_eq!(creds.server_url, "https://custom.server");
            assert_eq!(creds.email.as_deref(), Some("user@example.com"));
        });
    }

    #[test]
    fn test_verify_magic_link_success() {
        run(async {
            let http = MockHttp::new(vec![HttpResponse {
                status: 200,
                body: r#"{"token":"session-123","user":{"id":"uid","email":"user@example.com"}}"#
                    .to_string(),
            }]);
            let storage = MockStorage::with_creds(AuthCredentials {
                server_url: "https://sync.diaryx.org".to_string(),
                session_token: None,
                email: Some("user@example.com".to_string()),
                workspace_id: None,
            });
            let service = AuthService::new(http, storage);

            let result = service.verify_magic_link("token123", Some("CLI")).await;
            assert!(result.is_ok());
            let verify = result.unwrap();
            assert_eq!(verify.token, "session-123");
            assert_eq!(verify.user.email, "user@example.com");
        });
    }

    #[test]
    fn test_verify_saves_session_token() {
        run(async {
            let http = MockHttp::new(vec![HttpResponse {
                status: 200,
                body: r#"{"token":"sess-tok","user":{"id":"uid","email":"user@example.com"},"workspace_id":"ws-1"}"#
                    .to_string(),
            }]);
            let storage = MockStorage::with_creds(AuthCredentials {
                server_url: "https://sync.diaryx.org".to_string(),
                session_token: None,
                email: None,
                workspace_id: None,
            });
            let service = AuthService::new(http, storage);

            let _ = service.verify_magic_link("tok", None).await;

            let creds = service.get_credentials().await.unwrap();
            assert_eq!(creds.session_token.as_deref(), Some("sess-tok"));
            assert_eq!(creds.workspace_id.as_deref(), Some("ws-1"));
        });
    }

    #[test]
    fn test_verify_invalid_token() {
        run(async {
            let http = MockHttp::new(vec![HttpResponse {
                status: 401,
                body: r#"{"error":"expired"}"#.to_string(),
            }]);
            let storage = MockStorage::with_creds(AuthCredentials {
                server_url: "https://sync.diaryx.org".to_string(),
                session_token: None,
                email: None,
                workspace_id: None,
            });
            let service = AuthService::new(http, storage);

            let result = service.verify_magic_link("bad-token", None).await;
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(err.is_unauthorized());
        });
    }

    #[test]
    fn test_logout_clears_session() {
        run(async {
            let http = MockHttp::new(vec![HttpResponse {
                status: 200,
                body: "{}".to_string(),
            }]);
            let storage = MockStorage::with_creds(AuthCredentials {
                server_url: "https://sync.diaryx.org".to_string(),
                session_token: Some("tok".to_string()),
                email: Some("user@example.com".to_string()),
                workspace_id: None,
            });
            let service = AuthService::new(http, storage);

            assert!(service.is_authenticated().await);
            let _ = service.logout().await;
            assert!(!service.is_authenticated().await);
        });
    }

    #[test]
    fn test_get_me_success() {
        run(async {
            let http = MockHttp::new(vec![HttpResponse {
                status: 200,
                body: r#"{
                    "user": {"id": "uid", "email": "u@e.com"},
                    "workspaces": [{"id": "ws1", "name": "My Journal"}],
                    "devices": [],
                    "workspace_limit": 10,
                    "tier": "plus",
                    "published_site_limit": 5,
                    "attachment_limit_bytes": 2147483648
                }"#
                .to_string(),
            }]);
            let storage = MockStorage::with_creds(AuthCredentials {
                server_url: "https://sync.diaryx.org".to_string(),
                session_token: Some("tok".to_string()),
                email: None,
                workspace_id: None,
            });
            let service = AuthService::new(http, storage);

            let me = service.get_me().await.unwrap();
            assert_eq!(me.tier, "plus");
            assert_eq!(me.workspace_limit, 10);
            assert_eq!(me.workspaces.len(), 1);
        });
    }

    #[test]
    fn test_get_me_session_expired() {
        run(async {
            let http = MockHttp::new(vec![HttpResponse {
                status: 401,
                body: "Unauthorized".to_string(),
            }]);
            let storage = MockStorage::with_creds(AuthCredentials {
                server_url: "https://sync.diaryx.org".to_string(),
                session_token: Some("expired-tok".to_string()),
                email: None,
                workspace_id: None,
            });
            let service = AuthService::new(http, storage);

            let result = service.get_me().await;
            assert!(result.is_err());
            assert!(result.unwrap_err().is_session_expired());
        });
    }

    #[test]
    fn test_default_server_url() {
        run(async {
            let http = MockHttp::new(vec![HttpResponse {
                status: 200,
                body: r#"{"success":true,"message":"ok"}"#.to_string(),
            }]);
            let storage = MockStorage::new();
            let service = AuthService::new(http, storage);

            let url = service.resolve_server_url(None).await;
            assert_eq!(url, DEFAULT_SYNC_SERVER);
        });
    }

    #[test]
    fn test_explicit_server_url_overrides() {
        run(async {
            let http = MockHttp::new(vec![]);
            let storage = MockStorage::with_creds(AuthCredentials {
                server_url: "https://stored.server".to_string(),
                session_token: None,
                email: None,
                workspace_id: None,
            });
            let service = AuthService::new(http, storage);

            let url = service
                .resolve_server_url(Some("https://explicit.server/"))
                .await;
            assert_eq!(url, "https://explicit.server");
        });
    }

    #[test]
    fn test_stored_server_url_used() {
        run(async {
            let http = MockHttp::new(vec![]);
            let storage = MockStorage::with_creds(AuthCredentials {
                server_url: "https://stored.server".to_string(),
                session_token: None,
                email: None,
                workspace_id: None,
            });
            let service = AuthService::new(http, storage);

            let url = service.resolve_server_url(None).await;
            assert_eq!(url, "https://stored.server");
        });
    }
}
