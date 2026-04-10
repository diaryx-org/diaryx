//! Platform-agnostic auth service.

use super::types::*;

/// Parse a non-success [`HttpResponse`] into an [`AuthError`].
///
/// Extracts the server's `error` string (falling back to `fallback_msg`) and
/// any `devices` array returned on 403 device-limit responses. Both fields
/// are best-effort — a malformed body just yields an error with the fallback
/// message and no device list.
fn parse_error_response(resp: &HttpResponse, fallback_msg: &str) -> AuthError {
    let value = resp.json::<serde_json::Value>().ok();
    let msg = value
        .as_ref()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| fallback_msg.to_string());

    let mut err = AuthError::new(msg, resp.status);
    if resp.status == 403
        && let Some(devices_val) = value.as_ref().and_then(|v| v.get("devices"))
        && let Ok(devices) = serde_json::from_value::<Vec<Device>>(devices_val.clone())
    {
        err.devices = Some(devices);
    }
    err
}

/// Platform-agnostic authentication service.
///
/// Handles magic link authentication, session management, and user info
/// queries. The session token is never exposed at this layer — it's
/// encapsulated inside the [`AuthenticatedClient`] implementation.
pub struct AuthService<C: AuthenticatedClient> {
    client: C,
}

impl<C: AuthenticatedClient> AuthService<C> {
    /// Create a new auth service wrapping an [`AuthenticatedClient`].
    pub fn new(client: C) -> Self {
        Self { client }
    }

    /// Borrow the underlying client.
    pub fn client(&self) -> &C {
        &self.client
    }

    /// Server URL this service talks to.
    pub fn server_url(&self) -> &str {
        self.client.server_url()
    }

    /// Whether the user currently has an active session.
    pub async fn is_authenticated(&self) -> bool {
        self.client.has_session().await
    }

    /// Load non-secret session metadata.
    pub async fn get_metadata(&self) -> Option<AuthMetadata> {
        self.client.load_metadata().await
    }

    // =========================================================================
    // Magic Link Flow
    // =========================================================================

    /// Request a magic link for the given email.
    ///
    /// Sends a POST to `/auth/magic-link` and stores the email in metadata
    /// for later convenience.
    pub async fn request_magic_link(&self, email: &str) -> Result<MagicLinkResponse, AuthError> {
        let body = serde_json::json!({ "email": email }).to_string();
        let resp = self
            .client
            .post_unauth("/auth/magic-link", Some(&body))
            .await?;

        if !resp.is_success() {
            let msg = resp
                .json::<serde_json::Value>()
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
                .unwrap_or_else(|| format!("Failed to request magic link: HTTP {}", resp.status));
            return Err(AuthError::new(msg, resp.status));
        }

        let mut meta = self.client.load_metadata().await.unwrap_or_default();
        meta.email = Some(email.to_string());
        self.client.save_metadata(&meta).await;

        resp.json()
    }

    /// Verify a magic link token and obtain a session token.
    ///
    /// Sends a GET to `/auth/verify?token=...&device_name=...` and persists
    /// the resulting session token inside the client. When `replace_device_id`
    /// is provided, the server will evict that device to make room if the
    /// account is at its device limit.
    pub async fn verify_magic_link(
        &self,
        token: &str,
        device_name: Option<&str>,
        replace_device_id: Option<&str>,
    ) -> Result<VerifyResponse, AuthError> {
        let device = device_name.unwrap_or("Diaryx");
        let mut path = format!(
            "/auth/verify?token={}&device_name={}",
            urlencoding::encode(token),
            urlencoding::encode(device)
        );
        if let Some(replace) = replace_device_id {
            path.push_str("&replace_device_id=");
            path.push_str(&urlencoding::encode(replace));
        }

        let resp = self.client.get_unauth(&path).await?;

        if !resp.is_success() {
            if resp.status == 401 || resp.status == 400 {
                return Err(parse_error_response(&resp, "Invalid or expired token"));
            }
            return Err(parse_error_response(
                &resp,
                &format!("Verification failed: HTTP {}", resp.status),
            ));
        }

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
            .or_else(|| user_id.clone());

        self.client.store_session_token(&session_token).await;

        let mut meta = self.client.load_metadata().await.unwrap_or_default();
        if let Some(ref e) = email {
            meta.email = Some(e.clone());
        }
        if let Some(ref wid) = workspace_id {
            meta.workspace_id = Some(wid.clone());
        }
        self.client.save_metadata(&meta).await;

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
    ///
    /// When `replace_device_id` is provided, the server will evict that device
    /// to make room if the account is at its device limit.
    pub async fn verify_code(
        &self,
        code: &str,
        email: &str,
        device_name: Option<&str>,
        replace_device_id: Option<&str>,
    ) -> Result<VerifyResponse, AuthError> {
        let body = serde_json::json!({
            "code": code,
            "email": email,
            "device_name": device_name.unwrap_or("Diaryx"),
            "replace_device_id": replace_device_id,
        })
        .to_string();

        let resp = self
            .client
            .post_unauth("/auth/verify-code", Some(&body))
            .await?;

        if !resp.is_success() {
            return Err(parse_error_response(
                &resp,
                &format!("Failed to verify code: HTTP {}", resp.status),
            ));
        }

        let verify: VerifyResponse = resp.json()?;

        self.client.store_session_token(&verify.token).await;

        let mut meta = self.client.load_metadata().await.unwrap_or_default();
        meta.email = Some(verify.user.email.clone());
        self.client.save_metadata(&meta).await;

        Ok(verify)
    }

    // =========================================================================
    // Session Management
    // =========================================================================

    /// Get current user info from the server.
    pub async fn get_me(&self) -> Result<MeResponse, AuthError> {
        let resp = self.client.get("/auth/me").await?;

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

    /// Log out — notify the server and clear local session state.
    ///
    /// Server notification is best-effort (failures are ignored) because the
    /// primary goal is to clear local state. On browser clients the server
    /// response's `Set-Cookie: Max-Age=0` is the only way to clear the
    /// HttpOnly cookie, so it's important to call the server first.
    pub async fn logout(&self) -> Result<(), AuthError> {
        let _ = self.client.post("/auth/logout", None).await;
        self.client.clear_session().await;
        Ok(())
    }

    /// Refresh token by re-validating with the server.
    pub async fn refresh_token(&self) -> Result<MeResponse, AuthError> {
        self.get_me().await
    }

    // =========================================================================
    // Device Management
    // =========================================================================

    /// Get the user's registered devices.
    pub async fn get_devices(&self) -> Result<Vec<Device>, AuthError> {
        let resp = self.client.get("/auth/devices").await?;
        if !resp.is_success() {
            return Err(AuthError::new("Failed to get devices", resp.status));
        }
        resp.json()
    }

    /// Rename a device.
    pub async fn rename_device(&self, device_id: &str, new_name: &str) -> Result<(), AuthError> {
        let path = format!("/auth/devices/{}", device_id);
        let body = serde_json::json!({ "name": new_name }).to_string();

        let resp = self.client.patch(&path, Some(&body)).await?;
        if !resp.is_success() {
            return Err(AuthError::new("Failed to rename device", resp.status));
        }
        Ok(())
    }

    /// Delete a device.
    pub async fn delete_device(&self, device_id: &str) -> Result<(), AuthError> {
        let path = format!("/auth/devices/{}", device_id);
        let resp = self.client.delete(&path).await?;
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
        let resp = self.client.delete("/auth/account").await?;

        if !resp.is_success() {
            return Err(AuthError::new("Failed to delete account", resp.status));
        }

        self.client.clear_session().await;
        Ok(())
    }

    // =========================================================================
    // Workspace CRUD
    // =========================================================================

    /// Create a workspace on the server.
    pub async fn create_workspace(&self, name: &str) -> Result<ServerWorkspace, AuthError> {
        let body = serde_json::json!({ "name": name }).to_string();
        let resp = self.client.post("/api/workspaces", Some(&body)).await?;

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
        let path = format!("/api/workspaces/{}", workspace_id);
        let body = serde_json::json!({ "name": new_name }).to_string();

        let resp = self.client.patch(&path, Some(&body)).await?;
        if !resp.is_success() {
            return Err(AuthError::new("Failed to rename workspace", resp.status));
        }
        Ok(())
    }

    /// Delete a workspace on the server.
    pub async fn delete_workspace(&self, workspace_id: &str) -> Result<(), AuthError> {
        let path = format!("/api/workspaces/{}", workspace_id);
        let resp = self.client.delete(&path).await?;
        if !resp.is_success() {
            return Err(AuthError::new("Failed to delete workspace", resp.status));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // =========================================================================
    // MockClient — a single AuthenticatedClient impl for testing AuthService.
    // =========================================================================

    struct MockClient {
        server_url: String,
        responses: Mutex<Vec<HttpResponse>>,
        metadata: Mutex<Option<AuthMetadata>>,
        session_token: Mutex<Option<String>>,
    }

    impl MockClient {
        fn new(responses: Vec<HttpResponse>) -> Self {
            Self {
                server_url: "https://app.diaryx.org/api".to_string(),
                responses: Mutex::new(responses),
                metadata: Mutex::new(None),
                session_token: Mutex::new(None),
            }
        }

        fn with_session(self, token: impl Into<String>) -> Self {
            *self.session_token.lock().unwrap() = Some(token.into());
            self
        }

        fn with_metadata(self, metadata: AuthMetadata) -> Self {
            *self.metadata.lock().unwrap() = Some(metadata);
            self
        }

        fn next_response(&self) -> Result<HttpResponse, AuthError> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Err(AuthError::network("No mock response"))
            } else {
                Ok(responses.remove(0))
            }
        }
    }

    #[async_trait::async_trait]
    impl AuthenticatedClient for MockClient {
        fn server_url(&self) -> &str {
            &self.server_url
        }

        async fn has_session(&self) -> bool {
            self.session_token.lock().unwrap().is_some()
        }

        async fn load_metadata(&self) -> Option<AuthMetadata> {
            self.metadata.lock().unwrap().clone()
        }

        async fn save_metadata(&self, metadata: &AuthMetadata) {
            *self.metadata.lock().unwrap() = Some(metadata.clone());
        }

        async fn store_session_token(&self, token: &str) {
            *self.session_token.lock().unwrap() = Some(token.to_string());
        }

        async fn clear_session(&self) {
            *self.session_token.lock().unwrap() = None;
        }

        async fn get(&self, _path: &str) -> Result<HttpResponse, AuthError> {
            self.next_response()
        }
        async fn post(&self, _path: &str, _body: Option<&str>) -> Result<HttpResponse, AuthError> {
            self.next_response()
        }
        async fn put(&self, _path: &str, _body: Option<&str>) -> Result<HttpResponse, AuthError> {
            self.next_response()
        }
        async fn patch(&self, _path: &str, _body: Option<&str>) -> Result<HttpResponse, AuthError> {
            self.next_response()
        }
        async fn delete(&self, _path: &str) -> Result<HttpResponse, AuthError> {
            self.next_response()
        }
        async fn get_unauth(&self, _path: &str) -> Result<HttpResponse, AuthError> {
            self.next_response()
        }
        async fn post_unauth(
            &self,
            _path: &str,
            _body: Option<&str>,
        ) -> Result<HttpResponse, AuthError> {
            self.next_response()
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
            let client = MockClient::new(vec![HttpResponse {
                status: 200,
                body: r#"{"success":true,"message":"Check your email"}"#.to_string(),
            }]);
            let service = AuthService::new(client);

            let result = service.request_magic_link("user@example.com").await;
            assert!(result.is_ok());
            assert!(result.unwrap().success);
        });
    }

    #[test]
    fn test_request_magic_link_saves_email() {
        run(async {
            let client = MockClient::new(vec![HttpResponse {
                status: 200,
                body: r#"{"success":true,"message":"Check your email"}"#.to_string(),
            }]);
            let service = AuthService::new(client);

            let _ = service.request_magic_link("user@example.com").await;

            let meta = service.get_metadata().await.unwrap();
            assert_eq!(meta.email.as_deref(), Some("user@example.com"));
        });
    }

    #[test]
    fn test_verify_magic_link_success() {
        run(async {
            let client = MockClient::new(vec![HttpResponse {
                status: 200,
                body: r#"{"token":"session-123","user":{"id":"uid","email":"user@example.com"}}"#
                    .to_string(),
            }])
            .with_metadata(AuthMetadata {
                email: Some("user@example.com".to_string()),
                workspace_id: None,
            });
            let service = AuthService::new(client);

            let result = service
                .verify_magic_link("token123", Some("CLI"), None)
                .await;
            assert!(result.is_ok());
            let verify = result.unwrap();
            assert_eq!(verify.token, "session-123");
            assert_eq!(verify.user.email, "user@example.com");
        });
    }

    #[test]
    fn test_verify_stores_token_and_metadata() {
        run(async {
            let client = MockClient::new(vec![HttpResponse {
                status: 200,
                body: r#"{"token":"sess-tok","user":{"id":"uid","email":"user@example.com"},"workspace_id":"ws-1"}"#
                    .to_string(),
            }]);
            let service = AuthService::new(client);

            let _ = service.verify_magic_link("tok", None, None).await;

            assert!(service.is_authenticated().await);
            let meta = service.get_metadata().await.unwrap();
            assert_eq!(meta.email.as_deref(), Some("user@example.com"));
            assert_eq!(meta.workspace_id.as_deref(), Some("ws-1"));
        });
    }

    #[test]
    fn test_verify_invalid_token() {
        run(async {
            let client = MockClient::new(vec![HttpResponse {
                status: 401,
                body: r#"{"error":"expired"}"#.to_string(),
            }]);
            let service = AuthService::new(client);

            let result = service.verify_magic_link("bad-token", None, None).await;
            assert!(result.is_err());
            assert!(result.unwrap_err().is_unauthorized());
        });
    }

    #[test]
    fn test_logout_clears_session() {
        run(async {
            let client = MockClient::new(vec![HttpResponse {
                status: 200,
                body: "{}".to_string(),
            }])
            .with_session("tok")
            .with_metadata(AuthMetadata {
                email: Some("user@example.com".to_string()),
                workspace_id: None,
            });
            let service = AuthService::new(client);

            assert!(service.is_authenticated().await);
            let _ = service.logout().await;
            assert!(!service.is_authenticated().await);
        });
    }

    #[test]
    fn test_get_me_success() {
        run(async {
            let client = MockClient::new(vec![HttpResponse {
                status: 200,
                body: r#"{
                    "user": {"id": "uid", "email": "u@e.com"},
                    "workspaces": [{"id": "ws1", "name": "My Journal"}],
                    "devices": [],
                    "workspace_limit": 10,
                    "tier": "plus",
                    "published_site_limit": 1,
                    "attachment_limit_bytes": 2147483648
                }"#
                .to_string(),
            }])
            .with_session("tok");
            let service = AuthService::new(client);

            let me = service.get_me().await.unwrap();
            assert_eq!(me.tier, "plus");
            assert_eq!(me.workspace_limit, 10);
            assert_eq!(me.workspaces.len(), 1);
        });
    }

    #[test]
    fn test_get_me_session_expired() {
        run(async {
            let client = MockClient::new(vec![HttpResponse {
                status: 401,
                body: "Unauthorized".to_string(),
            }])
            .with_session("expired-tok");
            let service = AuthService::new(client);

            let result = service.get_me().await;
            assert!(result.is_err());
            assert!(result.unwrap_err().is_session_expired());
        });
    }
}
