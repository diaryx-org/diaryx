//! `diaryx edit` command handler.
//!
//! Starts a local REST server backed by `diaryx_core` and opens the Diaryx
//! web app, which connects via the `HttpBackend`. Non-API requests are
//! proxied to the upstream web app so everything is same-origin.

use std::path::Path;
use tokio::net::TcpListener;

/// Run the edit command: start local server and open browser.
pub async fn handle_edit(workspace_root: &Path, url: Option<String>, port: Option<u16>) -> bool {
    let upstream_url = url.unwrap_or_else(|| "https://app.diaryx.org".to_string());

    let (router, diaryx) =
        super::edit_server::edit_router(workspace_root.to_path_buf(), upstream_url.clone());

    // Initialize all registered plugins (they need async init for lifecycle setup).
    let failures = diaryx.init_plugins().await;
    for (id, err) in &failures {
        eprintln!("[edit-server] Plugin '{}' failed to init: {}", id.0, err);
    }

    // Bind to the requested port (or auto-select)
    let addr = format!("127.0.0.1:{}", port.unwrap_or(0));
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to {}: {}", addr, e);
            return false;
        }
    };

    let bound_addr = listener.local_addr().unwrap();

    // Use <workspace>.localhost as the hostname so the browser URL reflects
    // the workspace name. All modern browsers resolve *.localhost → 127.0.0.1
    // per RFC 6761, so no /etc/hosts changes are needed.
    let workspace_name = workspace_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    let subdomain = sanitize_subdomain(workspace_name);
    let local_url = format!("http://{}.localhost:{}", subdomain, bound_addr.port());

    // Open the local server URL — the SPA is proxied from the upstream,
    // and API calls hit our local handlers. Same origin, no mixed content.
    let browser_url = format!(
        "{}?backend=http&api_url={}",
        local_url,
        urlencoding::encode(&local_url)
    );

    println!("Starting local edit server on {}", bound_addr);
    println!("Proxying web app from {}", upstream_url);
    println!("Opening: {}", browser_url);
    println!();
    println!("Press Ctrl+C to stop.");

    // Open the browser
    if let Err(e) = open::that(&browser_url) {
        eprintln!("Failed to open browser: {}", e);
        eprintln!("Please open the URL manually: {}", browser_url);
    }

    // Serve until shutdown
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    println!("\nLocal edit server stopped.");
    true
}

/// Wait for Ctrl+C or termination signal.
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install Ctrl+C handler");
}

/// Convert a workspace name into a valid DNS subdomain label.
///
/// Lowercases, replaces spaces/underscores with hyphens, strips anything
/// that isn't alphanumeric or a hyphen, collapses consecutive hyphens, and
/// trims leading/trailing hyphens. Falls back to "workspace" if the result
/// is empty.
fn sanitize_subdomain(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c == ' ' || c == '_' { '-' } else { c })
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();

    // Collapse consecutive hyphens and trim leading/trailing hyphens.
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '-' && result.ends_with('-') {
            continue;
        }
        result.push(c);
    }
    let result = result.trim_matches('-').to_string();

    if result.is_empty() {
        "workspace".to_string()
    } else {
        result
    }
}
