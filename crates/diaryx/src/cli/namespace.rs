//! CLI handler for server namespace management (list, delete, objects).

use diaryx_core::auth::{AuthCredentials, NativeFileAuthStorage};
use serde::Deserialize;
use std::io::{self, Write};

use super::args::{NamespaceCommands, NamespaceObjectCommands, NamespaceSubdomainCommands};

#[derive(Debug, Deserialize)]
struct NamespaceResponse {
    id: String,
    #[allow(dead_code)]
    owner_user_id: String,
    created_at: i64,
    metadata: Option<serde_json::Value>,
}

fn load_auth() -> Result<(String, String), String> {
    let creds: AuthCredentials = NativeFileAuthStorage::load_global_credentials()
        .ok_or("Not logged in. Run `diaryx login <email>` first.")?;

    let token = creds
        .session_token
        .ok_or("No session token found. Run `diaryx login <email>` first.")?;

    let server_url = creds.server_url.trim_end_matches('/').to_string();
    Ok((server_url, token))
}

fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .build()
        .new_agent()
}

fn format_timestamp(epoch_secs: i64) -> String {
    let secs_per_day: i64 = 86400;
    let days = epoch_secs / secs_per_day;
    let time_of_day = epoch_secs % secs_per_day;

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;

    let mut y = 1970i64;
    let mut remaining = days;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0usize;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < md {
            m = i;
            break;
        }
        remaining -= md;
    }
    let d = remaining + 1;
    format!("{y}-{:02}-{:02} {:02}:{:02} UTC", m + 1, d, hours, minutes)
}

#[derive(Debug, Deserialize)]
struct ObjectMeta {
    key: String,
    mime_type: String,
    size_bytes: u64,
    updated_at: i64,
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub fn handle_namespace_command(command: NamespaceCommands) -> bool {
    match command {
        NamespaceCommands::List { json } => handle_list(json),
        NamespaceCommands::Delete { id, yes } => handle_delete(&id, yes),
        NamespaceCommands::Objects { command } => handle_objects_command(command),
        NamespaceCommands::Subdomain { command } => handle_subdomain_command(command),
    }
}

fn handle_objects_command(command: NamespaceObjectCommands) -> bool {
    match command {
        NamespaceObjectCommands::List { id, prefix, json } => {
            handle_objects_list(&id, prefix.as_deref(), json)
        }
        NamespaceObjectCommands::Delete { id, key, yes } => handle_objects_delete(&id, &key, yes),
    }
}

fn handle_list(json_output: bool) -> bool {
    let (server_url, token) = match load_auth() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("✗ {e}");
            return false;
        }
    };

    let agent = http_agent();
    let url = format!("{server_url}/namespaces?limit=500");
    let mut response = match agent
        .get(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("✗ Failed to list namespaces: {e}");
            return false;
        }
    };

    let body = match response.body_mut().read_to_string() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("✗ Failed to read response: {e}");
            return false;
        }
    };
    let namespaces: Vec<NamespaceResponse> = match serde_json::from_str(&body) {
        Ok(ns) => ns,
        Err(e) => {
            eprintln!("✗ Failed to parse response: {e}");
            return false;
        }
    };

    if json_output {
        let json = serde_json::to_string_pretty(
            &namespaces
                .iter()
                .map(|ns| {
                    serde_json::json!({
                        "id": ns.id,
                        "created_at": ns.created_at,
                        "metadata": ns.metadata,
                    })
                })
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default();
        println!("{json}");
        return true;
    }

    if namespaces.is_empty() {
        println!("No namespaces found.");
        return true;
    }

    println!("{:<40} {:<22} {:<15} {}", "ID", "CREATED", "KIND", "NAME");
    println!("{}", "-".repeat(95));
    for ns in &namespaces {
        let name = ns
            .metadata
            .as_ref()
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let kind = ns
            .metadata
            .as_ref()
            .and_then(|m| m.get("kind"))
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let created = format_timestamp(ns.created_at);
        println!("{:<40} {:<22} {:<15} {}", ns.id, created, kind, name);
    }
    println!("\n{} namespace(s) total.", namespaces.len());
    true
}

fn handle_delete(id: &str, yes: bool) -> bool {
    let (server_url, token) = match load_auth() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("✗ {e}");
            return false;
        }
    };

    let agent = http_agent();

    // Fetch namespace info first
    let get_url = format!("{server_url}/namespaces/{}", urlencoding::encode(id));
    let mut get_resp = match agent
        .get(&get_url)
        .header("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("✗ Namespace not found: {e}");
            return false;
        }
    };

    let body = match get_resp.body_mut().read_to_string() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("✗ Failed to read response: {e}");
            return false;
        }
    };
    let ns: NamespaceResponse = match serde_json::from_str(&body) {
        Ok(ns) => ns,
        Err(e) => {
            eprintln!("✗ Failed to parse namespace: {e}");
            return false;
        }
    };

    let name = ns
        .metadata
        .as_ref()
        .and_then(|m| m.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("(no name)");

    println!("Namespace: {}", ns.id);
    println!("Name:      {name}");
    println!("Created:   {}", format_timestamp(ns.created_at));
    println!();

    if !yes {
        print!("Delete this namespace? This cannot be undone. [y/N] ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            eprintln!("✗ Failed to read input");
            return false;
        }

        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            println!("Cancelled.");
            return true;
        }
    }

    let delete_url = format!("{server_url}/namespaces/{}", urlencoding::encode(id));
    match agent
        .delete(&delete_url)
        .header("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(r) if r.status() == 204 => {
            println!("✓ Deleted namespace {id}");
            true
        }
        Ok(r) => {
            eprintln!("✗ Unexpected response: HTTP {}", r.status());
            false
        }
        Err(e) => {
            eprintln!("✗ Failed to delete namespace: {e}");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Objects subcommands
// ---------------------------------------------------------------------------

fn fetch_objects(
    server_url: &str,
    token: &str,
    ns_id: &str,
    prefix: Option<&str>,
) -> Result<Vec<ObjectMeta>, String> {
    let agent = http_agent();
    let mut url = format!(
        "{server_url}/namespaces/{}/objects?limit=500",
        urlencoding::encode(ns_id),
    );
    if let Some(p) = prefix {
        url.push_str(&format!("&prefix={}", urlencoding::encode(p)));
    }

    let mut response = agent
        .get(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| format!("Failed to list objects: {e}"))?;

    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Failed to read response: {e}"))?;

    serde_json::from_str(&body).map_err(|e| format!("Failed to parse response: {e}"))
}

fn handle_objects_list(ns_id: &str, prefix: Option<&str>, json_output: bool) -> bool {
    let (server_url, token) = match load_auth() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("✗ {e}");
            return false;
        }
    };

    let objects = match fetch_objects(&server_url, &token, ns_id, prefix) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("✗ {e}");
            return false;
        }
    };

    if json_output {
        let json = serde_json::to_string_pretty(
            &objects
                .iter()
                .map(|o| {
                    serde_json::json!({
                        "key": o.key,
                        "mime_type": o.mime_type,
                        "size_bytes": o.size_bytes,
                        "updated_at": o.updated_at,
                    })
                })
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default();
        println!("{json}");
        return true;
    }

    if objects.is_empty() {
        println!("No objects found.");
        return true;
    }

    println!(
        "{:<50} {:<25} {:<10} {}",
        "KEY", "MIME TYPE", "SIZE", "UPDATED"
    );
    println!("{}", "-".repeat(110));
    for obj in &objects {
        println!(
            "{:<50} {:<25} {:<10} {}",
            obj.key,
            obj.mime_type,
            format_size(obj.size_bytes),
            format_timestamp(obj.updated_at),
        );
    }
    println!("\n{} object(s) total.", objects.len());
    true
}

fn handle_objects_delete(ns_id: &str, key: &str, yes: bool) -> bool {
    let (server_url, token) = match load_auth() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("✗ {e}");
            return false;
        }
    };

    if !yes {
        println!("Namespace: {ns_id}");
        println!("Object:    {key}");
        println!();
        print!("Delete this object? [y/N] ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            eprintln!("✗ Failed to read input");
            return false;
        }
        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            println!("Cancelled.");
            return true;
        }
    }

    let agent = http_agent();
    // Key contains path separators (e.g. "files/src/main.rs") — don't encode
    // slashes since the server route uses a wildcard path `{*key}`.
    let url = format!(
        "{server_url}/namespaces/{}/objects/{key}",
        urlencoding::encode(ns_id),
    );
    match agent
        .delete(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(r) if r.status() == 204 => {
            println!("✓ Deleted {key}");
            true
        }
        Ok(r) => {
            eprintln!("✗ Unexpected response: HTTP {}", r.status());
            false
        }
        Err(e) => {
            eprintln!("✗ Failed to delete object: {e}");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Subdomain subcommands
// ---------------------------------------------------------------------------

fn handle_subdomain_command(command: NamespaceSubdomainCommands) -> bool {
    match command {
        NamespaceSubdomainCommands::Claim {
            id,
            subdomain,
            audience,
        } => handle_subdomain_claim(&id, &subdomain, audience.as_deref()),
        NamespaceSubdomainCommands::Release { id } => handle_subdomain_release(&id),
    }
}

fn handle_subdomain_claim(ns_id: &str, subdomain: &str, audience: Option<&str>) -> bool {
    let (server_url, token) = match load_auth() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("✗ {e}");
            return false;
        }
    };

    let agent = http_agent();
    let url = format!(
        "{server_url}/namespaces/{}/subdomain",
        urlencoding::encode(ns_id),
    );

    let mut body = serde_json::json!({ "subdomain": subdomain });
    if let Some(aud) = audience {
        body["default_audience"] = serde_json::Value::String(aud.to_string());
    }

    let body_str = serde_json::to_string(&body).unwrap_or_default();

    match agent
        .put(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .send(body_str.as_bytes())
    {
        Ok(mut r) if r.status().is_success() => {
            let resp_body = r.body_mut().read_to_string().unwrap_or_default();
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&resp_body) {
                let site_domain = parsed
                    .get("site_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if !site_domain.is_empty() {
                    println!("✓ Claimed subdomain: {site_domain}");
                } else {
                    println!("✓ Claimed subdomain '{subdomain}' for namespace {ns_id}");
                }
            } else {
                println!("✓ Claimed subdomain '{subdomain}' for namespace {ns_id}");
            }
            true
        }
        Ok(mut r) => {
            let resp_body = r.body_mut().read_to_string().unwrap_or_default();
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&resp_body) {
                if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
                    eprintln!("✗ {err}");
                    return false;
                }
            }
            eprintln!("✗ Failed to claim subdomain: HTTP {}", r.status());
            false
        }
        Err(e) => {
            eprintln!("✗ Failed to claim subdomain: {e}");
            false
        }
    }
}

fn handle_subdomain_release(ns_id: &str) -> bool {
    let (server_url, token) = match load_auth() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("✗ {e}");
            return false;
        }
    };

    let agent = http_agent();
    let url = format!(
        "{server_url}/namespaces/{}/subdomain",
        urlencoding::encode(ns_id),
    );

    match agent
        .delete(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(r) if r.status() == 204 || r.status() == 200 => {
            println!("✓ Released subdomain for namespace {ns_id}");
            true
        }
        Ok(r) if r.status() == 404 => {
            println!("No subdomain configured for namespace {ns_id}");
            true
        }
        Ok(r) => {
            eprintln!("✗ Unexpected response: HTTP {}", r.status());
            false
        }
        Err(e) => {
            eprintln!("✗ Failed to release subdomain: {e}");
            false
        }
    }
}
