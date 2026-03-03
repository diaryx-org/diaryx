//! Google Drive REST API operations mapping to AsyncFileSystem methods.

use crate::host_bridge;
use crate::multipart;
use std::collections::HashMap;

const API_BASE: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD_BASE: &str = "https://www.googleapis.com/upload/drive/v3";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";

/// Google Drive configuration.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct GDriveConfig {
    pub access_token: String,
    pub refresh_token: String,
    pub client_id: String,
    pub client_secret: String,
    /// Root folder ID in Google Drive (defaults to "root").
    #[serde(default = "default_root")]
    pub root_folder_id: String,
}

fn default_root() -> String {
    "root".to_string()
}

impl GDriveConfig {
    fn auth_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert(
            "Authorization".to_string(),
            format!("Bearer {}", self.access_token),
        );
        headers
    }
}

/// Refresh the OAuth access token. Returns the new access token.
pub fn refresh_token(config: &GDriveConfig) -> Result<String, String> {
    let body = format!(
        "client_id={}&client_secret={}&refresh_token={}&grant_type=refresh_token",
        uri_encode(&config.client_id),
        uri_encode(&config.client_secret),
        uri_encode(&config.refresh_token),
    );
    let mut headers = HashMap::new();
    headers.insert(
        "Content-Type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    );
    let resp = host_bridge::http_request(
        "https://oauth2.googleapis.com/token",
        "POST",
        &headers,
        Some(&body),
    )?;
    if resp.status != 200 {
        return Err(format!(
            "Token refresh failed ({}): {}",
            resp.status, resp.body
        ));
    }
    let parsed: serde_json::Value = serde_json::from_str(&resp.body)
        .map_err(|e| format!("Failed to parse token response: {e}"))?;
    parsed
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No access_token in response".to_string())
}

/// Exchange an authorization code for access and refresh tokens.
/// Returns (access_token, refresh_token).
pub fn exchange_token(
    config: &GDriveConfig,
    code: &str,
    redirect_uri: &str,
) -> Result<(String, String), String> {
    let body = format!(
        "client_id={}&client_secret={}&code={}&redirect_uri={}&grant_type=authorization_code",
        uri_encode(&config.client_id),
        uri_encode(&config.client_secret),
        uri_encode(code),
        uri_encode(redirect_uri),
    );
    let mut headers = HashMap::new();
    headers.insert(
        "Content-Type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    );
    let resp = host_bridge::http_request(
        "https://oauth2.googleapis.com/token",
        "POST",
        &headers,
        Some(&body),
    )?;
    if resp.status != 200 {
        return Err(format!(
            "Token exchange failed ({}): {}",
            resp.status, resp.body
        ));
    }
    let parsed: serde_json::Value = serde_json::from_str(&resp.body)
        .map_err(|e| format!("Failed to parse token response: {e}"))?;
    let access_token = parsed
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No access_token in response".to_string())?;
    let refresh_token = parsed
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No refresh_token in response".to_string())?;
    Ok((access_token, refresh_token))
}

/// Resolve a path like "dir/subdir/file.md" to a Google Drive file ID.
/// Returns (file_id, is_folder).
fn resolve_path(config: &GDriveConfig, path: &str) -> Result<Option<(String, bool)>, String> {
    let parts: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    if parts.is_empty() {
        return Ok(Some((config.root_folder_id.clone(), true)));
    }

    let mut parent_id = config.root_folder_id.clone();

    for (i, part) in parts.iter().enumerate() {
        let is_last = i == parts.len() - 1;
        let query = format!(
            "name = '{}' and '{}' in parents and trashed = false",
            escape_query(part),
            parent_id
        );
        let url = format!(
            "{API_BASE}/files?q={}&fields=files(id,mimeType)&pageSize=1",
            uri_encode(&query)
        );

        let resp = host_bridge::http_request(&url, "GET", &config.auth_headers(), None)?;
        if resp.status != 200 {
            return Err(format!(
                "GDrive search failed ({}): {}",
                resp.status, resp.body
            ));
        }

        let parsed: serde_json::Value = serde_json::from_str(&resp.body)
            .map_err(|e| format!("Failed to parse search response: {e}"))?;
        let files = parsed
            .get("files")
            .and_then(|v| v.as_array())
            .ok_or("No files array in response")?;

        if files.is_empty() {
            return Ok(None);
        }

        let file = &files[0];
        let id = file
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("No file ID")?
            .to_string();
        let mime = file.get("mimeType").and_then(|v| v.as_str()).unwrap_or("");
        let is_folder = mime == FOLDER_MIME;

        if is_last {
            return Ok(Some((id, is_folder)));
        }

        if !is_folder {
            return Err(format!("Path component '{part}' is not a folder"));
        }
        parent_id = id;
    }

    Ok(None)
}

/// Get or create the folder hierarchy for a path, returning the parent folder ID.
fn get_or_create_parent(config: &GDriveConfig, path: &str) -> Result<String, String> {
    let parts: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    if parts.len() <= 1 {
        return Ok(config.root_folder_id.clone());
    }

    let mut parent_id = config.root_folder_id.clone();
    // Navigate/create all directories except the last component (the filename)
    for part in &parts[..parts.len() - 1] {
        parent_id = get_or_create_folder(config, &parent_id, part)?;
    }
    Ok(parent_id)
}

/// Get or create a single folder within a parent.
fn get_or_create_folder(
    config: &GDriveConfig,
    parent_id: &str,
    name: &str,
) -> Result<String, String> {
    // Search for existing folder
    let query = format!(
        "name = '{}' and '{}' in parents and mimeType = '{}' and trashed = false",
        escape_query(name),
        parent_id,
        FOLDER_MIME
    );
    let url = format!(
        "{API_BASE}/files?q={}&fields=files(id)&pageSize=1",
        uri_encode(&query)
    );
    let resp = host_bridge::http_request(&url, "GET", &config.auth_headers(), None)?;
    if resp.status == 200 {
        let parsed: serde_json::Value =
            serde_json::from_str(&resp.body).map_err(|e| format!("parse: {e}"))?;
        if let Some(files) = parsed.get("files").and_then(|v| v.as_array()) {
            if let Some(first) = files.first() {
                if let Some(id) = first.get("id").and_then(|v| v.as_str()) {
                    return Ok(id.to_string());
                }
            }
        }
    }

    // Create folder
    let metadata = serde_json::json!({
        "name": name,
        "mimeType": FOLDER_MIME,
        "parents": [parent_id]
    });
    let mut headers = config.auth_headers();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    let resp = host_bridge::http_request(
        &format!("{API_BASE}/files"),
        "POST",
        &headers,
        Some(&metadata.to_string()),
    )?;
    if resp.status != 200 {
        return Err(format!(
            "Create folder failed ({}): {}",
            resp.status, resp.body
        ));
    }
    let parsed: serde_json::Value =
        serde_json::from_str(&resp.body).map_err(|e| format!("parse: {e}"))?;
    parsed
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No id in create folder response".to_string())
}

/// Read a file's content as string.
pub fn read_file(config: &GDriveConfig, path: &str) -> Result<String, String> {
    let (file_id, _) = resolve_path(config, path)?.ok_or_else(|| format!("NotFound: {path}"))?;
    let url = format!("{API_BASE}/files/{file_id}?alt=media");
    let resp = host_bridge::http_request(&url, "GET", &config.auth_headers(), None)?;
    if resp.status == 200 {
        Ok(resp.body)
    } else {
        Err(format!(
            "GDrive read failed ({}): {}",
            resp.status, resp.body
        ))
    }
}

/// Read a file's content as binary.
pub fn read_binary(config: &GDriveConfig, path: &str) -> Result<Vec<u8>, String> {
    let (file_id, _) = resolve_path(config, path)?.ok_or_else(|| format!("NotFound: {path}"))?;
    let url = format!("{API_BASE}/files/{file_id}?alt=media");
    let resp = host_bridge::http_request(&url, "GET", &config.auth_headers(), None)?;
    if resp.status == 200 {
        host_bridge::decode_response_body(&resp)
    } else {
        Err(format!(
            "GDrive read failed ({}): {}",
            resp.status, resp.body
        ))
    }
}

/// Write a file (create or update).
pub fn write_file(config: &GDriveConfig, path: &str, content: &str) -> Result<(), String> {
    write_bytes(
        config,
        path,
        content.as_bytes(),
        "text/plain; charset=utf-8",
    )
}

/// Write binary content (create or update).
pub fn write_binary(config: &GDriveConfig, path: &str, content: &[u8]) -> Result<(), String> {
    write_bytes(config, path, content, "application/octet-stream")
}

fn write_bytes(
    config: &GDriveConfig,
    path: &str,
    content: &[u8],
    content_type: &str,
) -> Result<(), String> {
    let filename = path.trim_matches('/').rsplit('/').next().unwrap_or(path);

    // Check if file already exists
    if let Some((file_id, _)) = resolve_path(config, path)? {
        // Update existing file
        let url = format!("{UPLOAD_BASE}/files/{file_id}?uploadType=media",);
        let mut headers = config.auth_headers();
        headers.insert("Content-Type".to_string(), content_type.to_string());
        let resp = host_bridge::http_request_binary(&url, "PATCH", &headers, content)?;
        if resp.status == 200 {
            Ok(())
        } else {
            Err(format!(
                "GDrive update failed ({}): {}",
                resp.status, resp.body
            ))
        }
    } else {
        // Create new file
        let parent_id = get_or_create_parent(config, path)?;
        let metadata = serde_json::json!({
            "name": filename,
            "parents": [parent_id]
        });
        let (ct, body) =
            multipart::build_multipart_upload(&metadata.to_string(), content, content_type);
        let url = format!("{UPLOAD_BASE}/files?uploadType=multipart");
        let mut headers = config.auth_headers();
        headers.insert("Content-Type".to_string(), ct);
        let resp = host_bridge::http_request_binary(&url, "POST", &headers, &body)?;
        if resp.status == 200 {
            Ok(())
        } else {
            Err(format!(
                "GDrive create failed ({}): {}",
                resp.status, resp.body
            ))
        }
    }
}

/// Delete a file.
pub fn delete_file(config: &GDriveConfig, path: &str) -> Result<(), String> {
    let resolved = resolve_path(config, path)?;
    if let Some((file_id, _)) = resolved {
        let url = format!("{API_BASE}/files/{file_id}");
        let resp = host_bridge::http_request(&url, "DELETE", &config.auth_headers(), None)?;
        if resp.status == 204 || resp.status == 200 || resp.status == 404 {
            Ok(())
        } else {
            Err(format!(
                "GDrive delete failed ({}): {}",
                resp.status, resp.body
            ))
        }
    } else {
        Ok(()) // File doesn't exist, nothing to delete
    }
}

/// Check if a file exists.
pub fn exists(config: &GDriveConfig, path: &str) -> Result<bool, String> {
    Ok(resolve_path(config, path)?.is_some())
}

/// Check if a path is a directory (folder).
pub fn is_dir(config: &GDriveConfig, path: &str) -> Result<bool, String> {
    match resolve_path(config, path)? {
        Some((_, is_folder)) => Ok(is_folder),
        None => Ok(false),
    }
}

/// List files in a directory.
pub fn list_files(config: &GDriveConfig, dir: &str) -> Result<Vec<String>, String> {
    let parent_id = match resolve_path(config, dir)? {
        Some((id, true)) => id,
        Some((_, false)) => return Err(format!("Not a directory: {dir}")),
        None => return Ok(Vec::new()),
    };

    let query = format!("'{}' in parents and trashed = false", parent_id);
    let url = format!(
        "{API_BASE}/files?q={}&fields=files(name)&pageSize=1000",
        uri_encode(&query)
    );
    let resp = host_bridge::http_request(&url, "GET", &config.auth_headers(), None)?;
    if resp.status != 200 {
        return Err(format!(
            "GDrive list failed ({}): {}",
            resp.status, resp.body
        ));
    }
    let parsed: serde_json::Value =
        serde_json::from_str(&resp.body).map_err(|e| format!("parse: {e}"))?;
    let files = parsed
        .get("files")
        .and_then(|v| v.as_array())
        .ok_or("No files array")?;
    Ok(files
        .iter()
        .filter_map(|f| {
            f.get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect())
}

/// List .md files in a directory.
pub fn list_md_files(config: &GDriveConfig, dir: &str) -> Result<Vec<String>, String> {
    let all = list_files(config, dir)?;
    Ok(all.into_iter().filter(|f| f.ends_with(".md")).collect())
}

/// Create directory hierarchy (all intermediate folders).
pub fn create_dir_all(config: &GDriveConfig, path: &str) -> Result<(), String> {
    let parts: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let mut parent_id = config.root_folder_id.clone();
    for part in parts {
        parent_id = get_or_create_folder(config, &parent_id, part)?;
    }
    Ok(())
}

/// Move a file from one path to another.
pub fn move_file(config: &GDriveConfig, from: &str, to: &str) -> Result<(), String> {
    let (file_id, _) = resolve_path(config, from)?.ok_or_else(|| format!("NotFound: {from}"))?;

    let new_parent_id = get_or_create_parent(config, to)?;
    let new_name = to.trim_matches('/').rsplit('/').next().unwrap_or(to);

    // Get current parent
    let info_url = format!("{API_BASE}/files/{file_id}?fields=parents");
    let info_resp = host_bridge::http_request(&info_url, "GET", &config.auth_headers(), None)?;
    let old_parent = if info_resp.status == 200 {
        let parsed: serde_json::Value = serde_json::from_str(&info_resp.body).unwrap_or_default();
        parsed
            .get("parents")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    };

    let url =
        format!("{API_BASE}/files/{file_id}?addParents={new_parent_id}&removeParents={old_parent}");
    let metadata = serde_json::json!({ "name": new_name });
    let mut headers = config.auth_headers();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    let resp = host_bridge::http_request(&url, "PATCH", &headers, Some(&metadata.to_string()))?;
    if resp.status == 200 {
        Ok(())
    } else {
        Err(format!(
            "GDrive move failed ({}): {}",
            resp.status, resp.body
        ))
    }
}

/// Get modified time for a file (ms since epoch).
pub fn get_modified_time(config: &GDriveConfig, path: &str) -> Result<Option<i64>, String> {
    let resolved = resolve_path(config, path)?;
    let (file_id, _) = match resolved {
        Some(r) => r,
        None => return Ok(None),
    };
    let url = format!("{API_BASE}/files/{file_id}?fields=modifiedTime");
    let resp = host_bridge::http_request(&url, "GET", &config.auth_headers(), None)?;
    if resp.status != 200 {
        return Ok(None);
    }
    // modifiedTime is RFC 3339 — just return current time as approximation
    // since parsing RFC 3339 without chrono is verbose
    let ts = host_bridge::storage_get("_last_timestamp")
        .ok()
        .flatten()
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.parse::<i64>().ok());
    Ok(ts)
}

/// Escape single quotes in a Google Drive query string.
fn escape_query(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

/// URI-encode a string.
fn uri_encode(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            _ => {
                encoded.push_str(&format!("%{b:02X}"));
            }
        }
    }
    encoded
}
