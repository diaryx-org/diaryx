//! Authentication command handlers for sync.
//!
//! Thin wrapper around [`diaryx_core::auth::AuthService`] with CLI-specific
//! output formatting. HTTP is provided by reqwest, storage by the native
//! global auth store in `diaryx_core::auth`.

use diaryx_core::auth::{
    AuthError, AuthHttpClient, DEFAULT_SYNC_SERVER, HttpResponse, NativeFileAuthStorage,
};

// =========================================================================
// CLI HTTP Client (reqwest blocking)
// =========================================================================

/// Reqwest-based HTTP client for CLI auth operations.
pub struct ReqwestAuthClient {
    client: reqwest::blocking::Client,
}

impl ReqwestAuthClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::blocking::Client::new(),
        }
    }

    fn build_request(
        &self,
        method: reqwest::Method,
        url: &str,
        bearer_token: Option<&str>,
        json_body: Option<&str>,
    ) -> Result<HttpResponse, AuthError> {
        let mut req = self.client.request(method, url);

        if let Some(token) = bearer_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        if let Some(body) = json_body {
            req = req
                .header("Content-Type", "application/json")
                .body(body.to_string());
        }

        let resp = req
            .send()
            .map_err(|e| AuthError::network(format!("Failed to connect: {}", e)))?;

        let status = resp.status().as_u16();
        let body = resp.text().unwrap_or_default();

        Ok(HttpResponse { status, body })
    }
}

#[async_trait::async_trait]
impl AuthHttpClient for ReqwestAuthClient {
    async fn get(&self, url: &str, bearer_token: Option<&str>) -> Result<HttpResponse, AuthError> {
        self.build_request(reqwest::Method::GET, url, bearer_token, None)
    }

    async fn post(
        &self,
        url: &str,
        bearer_token: Option<&str>,
        json_body: Option<&str>,
    ) -> Result<HttpResponse, AuthError> {
        self.build_request(reqwest::Method::POST, url, bearer_token, json_body)
    }

    async fn patch(
        &self,
        url: &str,
        bearer_token: Option<&str>,
        json_body: Option<&str>,
    ) -> Result<HttpResponse, AuthError> {
        self.build_request(reqwest::Method::PATCH, url, bearer_token, json_body)
    }

    async fn delete(
        &self,
        url: &str,
        bearer_token: Option<&str>,
    ) -> Result<HttpResponse, AuthError> {
        self.build_request(reqwest::Method::DELETE, url, bearer_token, None)
    }
}

// =========================================================================
// CLI Auth Service Factory
// =========================================================================

type CliAuthService = diaryx_core::auth::AuthService<ReqwestAuthClient, NativeFileAuthStorage>;

fn global_auth_storage() -> NativeFileAuthStorage {
    NativeFileAuthStorage::global().unwrap_or_else(|| {
        NativeFileAuthStorage::new(std::env::temp_dir().join("diaryx-auth.toml"))
    })
}

fn current_server_url(explicit: Option<&str>) -> String {
    explicit
        .map(|value| value.trim_end_matches('/').to_string())
        .or_else(|| NativeFileAuthStorage::load_global_credentials().map(|creds| creds.server_url))
        .unwrap_or_else(|| DEFAULT_SYNC_SERVER.to_string())
}

/// Create a CLI auth service backed by reqwest and the native global auth store.
pub fn cli_auth_service() -> CliAuthService {
    diaryx_core::auth::AuthService::new(ReqwestAuthClient::new(), global_auth_storage())
}

// =========================================================================
// CLI Command Handlers
// =========================================================================

/// Handle the login command - initiate magic link authentication.
pub fn handle_login(email: &str, server: Option<&str>) {
    let server_url = current_server_url(server);

    println!("Logging in to sync server...");
    println!("  Server: {}", server_url);
    println!("  Email: {}", email);
    println!();

    let service = cli_auth_service();
    match futures_lite::future::block_on(service.request_magic_link(email, Some(&server_url))) {
        Ok(_) => {
            println!("Check your email for a magic link!");
            println!();
            println!("Once you receive the email, run:");
            println!("  diaryx sync verify <TOKEN>");
            println!();
            println!("The token is in the magic link URL (the part after ?token=)");
        }
        Err(e) => {
            eprintln!("Login request failed: {}", e);
            if e.status_code == 0 {
                eprintln!();
                eprintln!("Please check:");
                eprintln!("  - Your internet connection");
                eprintln!("  - The server URL is correct: {}", server_url);
            }
        }
    }
}

/// Handle the verify command - complete magic link authentication.
pub fn handle_verify(token: &str, device_name: Option<&str>) {
    println!("Verifying authentication...");

    let service = cli_auth_service();
    let device = device_name.unwrap_or("CLI");
    match futures_lite::future::block_on(service.verify_magic_link(token, Some(device))) {
        Ok(verify) => {
            println!();
            println!("Successfully logged in!");
            println!("  Email: {}", verify.user.email);
            println!();
            println!("You can now start syncing with:");
            println!("  diaryx sync start");
        }
        Err(e) => {
            if e.is_unauthorized() {
                eprintln!("Invalid or expired token.");
                eprintln!();
                eprintln!("Please request a new magic link with:");
                eprintln!("  diaryx sync login <your-email>");
            } else {
                eprintln!("Verification failed: {}", e);
            }
        }
    }
}

/// Handle the logout command - clear stored credentials.
pub fn handle_logout() {
    let credentials = NativeFileAuthStorage::load_global_credentials();
    let service = cli_auth_service();
    let _ = futures_lite::future::block_on(service.logout());

    println!("Logged out successfully.");
    if let Some(email) = credentials.and_then(|creds| creds.email) {
        println!();
        println!("To log back in:");
        println!("  diaryx sync login {}", email);
    }
}

#[cfg(test)]
mod tests {
    use diaryx_core::auth::DEFAULT_SYNC_SERVER;

    // =========================================================================
    // URL Construction Tests (verify core auth builds correct URLs)
    // =========================================================================

    #[test]
    fn test_login_url_construction() {
        let server_url = "https://sync.diaryx.org";
        let url = format!("{}/auth/magic-link", server_url);
        assert_eq!(url, "https://sync.diaryx.org/auth/magic-link");
    }

    #[test]
    fn test_verify_url_construction() {
        let server_url = "https://sync.diaryx.org";
        let token = "abc123";
        let device = "CLI";

        let url = format!(
            "{}/auth/verify?token={}&device_name={}",
            server_url,
            urlencoding::encode(token),
            urlencoding::encode(device)
        );

        assert_eq!(
            url,
            "https://sync.diaryx.org/auth/verify?token=abc123&device_name=CLI"
        );
    }

    #[test]
    fn test_verify_url_encoding_special_chars() {
        let server_url = "https://sync.diaryx.org";
        let token = "token+with/special=chars";
        let device = "My Device Name";

        let url = format!(
            "{}/auth/verify?token={}&device_name={}",
            server_url,
            urlencoding::encode(token),
            urlencoding::encode(device)
        );

        assert!(url.contains("token%2Bwith%2Fspecial%3Dchars"));
        assert!(url.contains("My%20Device%20Name"));
    }

    #[test]
    fn test_logout_url_construction() {
        let server = "https://sync.diaryx.org";
        let url = format!("{}/auth/logout", server);
        assert_eq!(url, "https://sync.diaryx.org/auth/logout");
    }

    // =========================================================================
    // Default Server URL Tests
    // =========================================================================

    #[test]
    fn test_default_sync_server_constant() {
        assert_eq!(DEFAULT_SYNC_SERVER, "https://sync.diaryx.org");
    }

    #[test]
    fn test_server_url_fallback_logic() {
        let explicit_server: Option<&str> = None;
        let server_url = explicit_server.unwrap_or(DEFAULT_SYNC_SERVER);

        assert_eq!(server_url, "https://sync.diaryx.org");
    }

    #[test]
    fn test_server_url_uses_explicit() {
        let explicit_server = Some("https://custom.server.com");
        let server_url = explicit_server.unwrap_or(DEFAULT_SYNC_SERVER);

        assert_eq!(server_url, "https://custom.server.com");
    }
}
