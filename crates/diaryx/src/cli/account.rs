//! CLI handlers for account management (login, logout, whoami).

use diaryx_core::auth::AuthService;

use super::auth_client::FsAuthenticatedClient;
use super::block_on;

fn build_service(server_override: Option<&str>) -> Option<AuthService<FsAuthenticatedClient>> {
    let client = FsAuthenticatedClient::from_default_path(server_override)?;
    Some(AuthService::new(client))
}

pub fn handle_login(email: &str, server: Option<&str>) -> bool {
    let Some(service) = build_service(server) else {
        eprintln!("✗ Cannot determine config directory for auth storage");
        return false;
    };

    println!("Requesting magic link for {email}...");
    match block_on(service.request_magic_link(email)) {
        Ok(resp) => {
            println!("{}", resp.message);
        }
        Err(e) => {
            eprintln!("✗ {e}");
            return false;
        }
    }

    println!();
    use std::io::{self, Write};
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
    match block_on(service.verify_code(code, email, Some("Diaryx CLI"), None)) {
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
    let Some(service) = build_service(None) else {
        eprintln!("✗ Cannot determine config directory for auth storage");
        return false;
    };
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
    let Some(service) = build_service(None) else {
        eprintln!("✗ Cannot determine config directory for auth storage");
        return false;
    };

    if !block_on(service.is_authenticated()) {
        println!("Not logged in. Run `diaryx login <email>` to sign in.");
        return true;
    }

    let server_url = service.server_url().to_string();
    let metadata = block_on(service.get_metadata());

    match block_on(service.get_me()) {
        Ok(me) => {
            println!("Email:   {}", me.user.email);
            println!("User ID: {}", me.user.id);
            println!("Tier:    {}", me.tier);
            println!("Server:  {server_url}");
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
                println!("Server: {server_url}");
                if let Some(email) = metadata.as_ref().and_then(|m| m.email.as_ref()) {
                    println!("Email:  {email} (last used)");
                }
            } else {
                eprintln!("✗ {e}");
            }
            false
        }
    }
}
