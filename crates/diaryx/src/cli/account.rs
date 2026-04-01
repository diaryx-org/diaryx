//! CLI handlers for account management (login, logout, whoami).

use diaryx_core::auth::{
    AuthCredentials, AuthError, AuthHttpClient, AuthService, HttpResponse, NativeFileAuthStorage,
};
use std::io::{self, Write};

use super::block_on;

/// Ureq-backed HTTP client for AuthService.
struct UreqHttpClient {
    agent: ureq::Agent,
}

impl UreqHttpClient {
    fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(15)))
            .build()
            .new_agent();
        Self { agent }
    }
}

fn finish_response(
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<HttpResponse, AuthError> {
    match result {
        Ok(mut resp) => {
            let status: u16 = resp.status().into();
            let body = resp.body_mut().read_to_string().unwrap_or_default();
            Ok(HttpResponse { status, body })
        }
        Err(e) => Err(AuthError::network(e.to_string())),
    }
}

#[async_trait::async_trait]
impl AuthHttpClient for UreqHttpClient {
    async fn get(&self, url: &str, token: Option<&str>) -> Result<HttpResponse, AuthError> {
        let mut req = self.agent.get(url);
        if let Some(t) = token {
            req = req.header("Authorization", &format!("Bearer {t}"));
        }
        finish_response(req.call())
    }

    async fn post(
        &self,
        url: &str,
        token: Option<&str>,
        body: Option<&str>,
    ) -> Result<HttpResponse, AuthError> {
        let mut req = self.agent.post(url);
        if let Some(t) = token {
            req = req.header("Authorization", &format!("Bearer {t}"));
        }
        req = req.header("Content-Type", "application/json");
        finish_response(req.send(body.unwrap_or("{}").as_bytes()))
    }

    async fn patch(
        &self,
        url: &str,
        token: Option<&str>,
        body: Option<&str>,
    ) -> Result<HttpResponse, AuthError> {
        let mut req = self.agent.patch(url);
        if let Some(t) = token {
            req = req.header("Authorization", &format!("Bearer {t}"));
        }
        req = req.header("Content-Type", "application/json");
        finish_response(req.send(body.unwrap_or("{}").as_bytes()))
    }

    async fn delete(&self, url: &str, token: Option<&str>) -> Result<HttpResponse, AuthError> {
        let mut req = self.agent.delete(url);
        if let Some(t) = token {
            req = req.header("Authorization", &format!("Bearer {t}"));
        }
        finish_response(req.call())
    }
}

fn build_service() -> AuthService<UreqHttpClient, NativeFileAuthStorage> {
    let http = UreqHttpClient::new();
    let storage = NativeFileAuthStorage::global()
        .expect("Cannot determine config directory for auth storage");
    AuthService::new(http, storage)
}

pub fn handle_login(email: &str, server: Option<&str>) -> bool {
    let service = build_service();

    println!("Requesting magic link for {email}...");
    let result = block_on(service.request_magic_link(email, server));
    match result {
        Ok(resp) => {
            println!("{}", resp.message);
        }
        Err(e) => {
            eprintln!("✗ {e}");
            return false;
        }
    }

    println!();
    print!("Enter the 6-digit code from your email: ");
    io::stdout().flush().unwrap();

    let mut code = String::new();
    if io::stdin().read_line(&mut code).is_err() {
        eprintln!("✗ Failed to read input");
        return false;
    }
    let code = code.trim();

    if code.is_empty() {
        eprintln!("✗ No code entered");
        return false;
    }

    println!("Verifying...");
    match block_on(service.verify_code(code, email, Some("Diaryx CLI"))) {
        Ok(verify) => {
            println!("✓ Logged in as {}", verify.user.email);
            true
        }
        Err(e) => {
            eprintln!("✗ Verification failed: {e}");
            false
        }
    }
}

pub fn handle_logout() -> bool {
    let service = build_service();
    match block_on(service.logout()) {
        Ok(()) => {
            println!("✓ Logged out");
            true
        }
        Err(e) => {
            eprintln!("✗ {e}");
            false
        }
    }
}

pub fn handle_whoami() -> bool {
    let creds: Option<AuthCredentials> = NativeFileAuthStorage::load_global_credentials();
    let creds = match creds {
        Some(c) if c.session_token.is_some() => c,
        _ => {
            println!("Not logged in. Run `diaryx login <email>` to sign in.");
            return true;
        }
    };

    let service = build_service();
    match block_on(service.get_me()) {
        Ok(me) => {
            println!("Email:   {}", me.user.email);
            println!("User ID: {}", me.user.id);
            println!("Tier:    {}", me.tier);
            println!("Server:  {}", creds.server_url);
            if !me.workspaces.is_empty() {
                println!("\nWorkspaces:");
                for ws in &me.workspaces {
                    println!("  {} ({})", ws.name, ws.id);
                }
            }
            true
        }
        Err(e) => {
            if e.is_session_expired() {
                println!("Session expired. Run `diaryx login <email>` to sign in again.");
                println!("Server: {}", creds.server_url);
                if let Some(email) = &creds.email {
                    println!("Email:  {email} (last used)");
                }
            } else {
                eprintln!("✗ {e}");
            }
            false
        }
    }
}
