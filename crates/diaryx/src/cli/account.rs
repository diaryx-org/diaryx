//! CLI handlers for account management (login, logout, whoami).

use std::io::{self, Write};

use diaryx_core::auth::{AuthError, AuthService, Device};

use super::args::DeviceCommands;
use super::auth_client::FsAuthenticatedClient;
use super::block_on;

fn build_service(server_override: Option<&str>) -> Option<AuthService<FsAuthenticatedClient>> {
    let client = FsAuthenticatedClient::from_default_path(server_override)?;
    Some(AuthService::new(client))
}

pub fn handle_login(
    email: &str,
    server: Option<&str>,
    device_name: Option<&str>,
    replace_device: Option<&str>,
) -> bool {
    let Some(service) = build_service(server) else {
        eprintln!("✗ Cannot determine config directory for auth storage");
        return false;
    };
    let device_name = device_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Diaryx CLI");

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
    match block_on(service.verify_code(code, email, Some(device_name), replace_device)) {
        Ok(verify) => {
            println!("✓ Logged in as {}", verify.user.email);
            true
        }
        Err(e) => {
            if replace_device.is_none()
                && let Some(devices) = e.devices.as_ref()
                && !devices.is_empty()
            {
                return handle_device_limit_retry(&service, code, email, device_name, devices);
            }
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
            if !me.devices.is_empty() {
                println!("\nDevices:");
                print_device_list(&me.devices);
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

pub fn handle_devices_command(command: DeviceCommands) -> bool {
    let Some(service) = build_service(None) else {
        eprintln!("✗ Cannot determine config directory for auth storage");
        return false;
    };

    match command {
        DeviceCommands::List { json } => handle_devices_list(&service, json),
        DeviceCommands::Rename { id, name } => handle_devices_rename(&service, &id, &name),
        DeviceCommands::Remove { id, yes } => handle_devices_remove(&service, &id, yes),
    }
}

fn handle_device_limit_retry(
    service: &AuthService<FsAuthenticatedClient>,
    code: &str,
    email: &str,
    device_name: &str,
    devices: &[Device],
) -> bool {
    println!("Device limit reached. Replacing a device signs it out on the server.");
    print_device_list(devices);

    let Some(replace_id) = prompt_for_replacement(devices) else {
        eprintln!("✗ Sign-in cancelled; no device was replaced.");
        return false;
    };

    println!("Replacing device and verifying...");
    match block_on(service.verify_code(code, email, Some(device_name), Some(&replace_id))) {
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

fn handle_devices_list(service: &AuthService<FsAuthenticatedClient>, json: bool) -> bool {
    match block_on(service.get_devices()) {
        Ok(devices) => {
            if json {
                match serde_json::to_string_pretty(&devices) {
                    Ok(output) => println!("{output}"),
                    Err(e) => {
                        eprintln!("✗ Failed to serialize devices: {e}");
                        return false;
                    }
                }
            } else if devices.is_empty() {
                println!("No registered devices.");
            } else {
                println!("Registered devices:");
                print_device_list(&devices);
            }
            true
        }
        Err(e) => {
            print_auth_error(e);
            false
        }
    }
}

fn handle_devices_rename(
    service: &AuthService<FsAuthenticatedClient>,
    id: &str,
    name: &str,
) -> bool {
    let name = name.trim();
    if name.is_empty() {
        eprintln!("✗ Device name cannot be empty");
        return false;
    }

    match block_on(service.rename_device(id, name)) {
        Ok(()) => {
            println!("✓ Renamed device {id}");
            true
        }
        Err(e) => {
            print_auth_error(e);
            false
        }
    }
}

fn handle_devices_remove(
    service: &AuthService<FsAuthenticatedClient>,
    id: &str,
    yes: bool,
) -> bool {
    if !yes && !confirm(&format!("Remove device {id}?")) {
        println!("Cancelled.");
        return true;
    }

    match block_on(service.delete_device(id)) {
        Ok(()) => {
            println!("✓ Removed device {id}");
            true
        }
        Err(e) => {
            print_auth_error(e);
            false
        }
    }
}

fn print_auth_error(error: AuthError) {
    if error.is_session_expired() {
        eprintln!("✗ Session expired. Run `diaryx login <email>` to sign in again.");
    } else {
        eprintln!("✗ {error}");
    }
}

fn print_device_list(devices: &[Device]) {
    for (index, device) in devices.iter().enumerate() {
        println!(
            "  {}. {} ({}) - last seen {}",
            index + 1,
            device_name(device),
            device.id,
            device_last_seen(device)
        );
    }
}

fn device_name(device: &Device) -> &str {
    device.name.as_deref().unwrap_or("Unnamed device")
}

fn device_last_seen(device: &Device) -> &str {
    device.last_seen_at.as_deref().unwrap_or("unknown")
}

fn prompt_for_replacement(devices: &[Device]) -> Option<String> {
    print!("Replace device [1-{}, or blank to cancel]: ", devices.len());
    io::stdout().flush().ok()?;

    let mut choice = String::new();
    io::stdin().read_line(&mut choice).ok()?;
    resolve_device_choice(&choice, devices)
}

fn resolve_device_choice(input: &str, devices: &[Device]) -> Option<String> {
    let choice = input.trim();
    if choice.is_empty() {
        return None;
    }

    if let Ok(index) = choice.parse::<usize>()
        && (1..=devices.len()).contains(&index)
    {
        return Some(devices[index - 1].id.clone());
    }

    devices
        .iter()
        .find(|device| device.id == choice)
        .map(|device| device.id.clone())
}

fn confirm(prompt: &str) -> bool {
    print!("{prompt} [y/N] ");
    if io::stdout().flush().is_err() {
        return false;
    }

    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return false;
    }

    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices() -> Vec<Device> {
        vec![
            Device {
                id: "dev-1".into(),
                name: Some("Mac".into()),
                last_seen_at: Some("2026-05-24T10:00:00Z".into()),
            },
            Device {
                id: "dev-2".into(),
                name: Some("iPhone".into()),
                last_seen_at: None,
            },
        ]
    }

    #[test]
    fn resolve_device_choice_accepts_one_based_index() {
        assert_eq!(
            resolve_device_choice("2", &devices()).as_deref(),
            Some("dev-2")
        );
    }

    #[test]
    fn resolve_device_choice_accepts_exact_id() {
        assert_eq!(
            resolve_device_choice("dev-1", &devices()).as_deref(),
            Some("dev-1")
        );
    }

    #[test]
    fn resolve_device_choice_rejects_empty_or_out_of_range_input() {
        assert_eq!(resolve_device_choice("", &devices()), None);
        assert_eq!(resolve_device_choice("3", &devices()), None);
        assert_eq!(resolve_device_choice("dev", &devices()), None);
    }
}
