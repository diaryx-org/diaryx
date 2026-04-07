//! LWW file sync engine.
//!
//! Implements hash-based diffing and last-writer-wins conflict resolution
//! for multi-device personal sync over the namespace object store API.

use diaryx_plugin_sdk::prelude::*;
use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};
use serde_json::{Value as JsonValue, json};
use std::collections::BTreeMap;

use crate::sync_manifest::{SyncManifest, SyncState};
use crate::{
    auth_headers, http_error, http_request_binary_compat, http_request_compat, load_extism_config,
    parse_http_body, parse_http_body_json, parse_http_status, resolve_auth_token,
    resolve_server_url,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Percent-encode set for URL path segments per RFC 3986.
///
/// Encodes everything EXCEPT unreserved characters: `A-Z a-z 0-9 - . _ ~`
/// This is critical — the previous `NON_ALPHANUMERIC` set encoded `.`, `-`,
/// `_`, and `~` which corrupted server keys and caused double-encoding 404s.
const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// Build the URL for a single object in a namespace, percent-encoding each
/// path segment so that spaces, unicode, and other reserved characters don't
/// produce invalid URIs.
fn object_url(server: &str, namespace_id: &str, key: &str) -> String {
    let encoded_key: String = key
        .split('/')
        .map(|seg| utf8_percent_encode(seg, PATH_SEGMENT_ENCODE_SET).to_string())
        .collect::<Vec<_>>()
        .join("/");
    format!("{server}/namespaces/{namespace_id}/objects/{encoded_key}")
}

/// Decode a potentially percent-encoded key from the server.
///
/// Server listings may return keys with percent-encoding if they were uploaded
/// with an over-eager encode set. Decoding normalises them so that local ↔
/// server comparisons work correctly.
fn decode_server_key(key: &str) -> String {
    percent_decode_str(key).decode_utf8_lossy().into_owned()
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LocalFileInfo {
    pub hash: String,
    pub size: u64,
    pub modified_at: u64,
}

#[derive(Debug, Clone)]
pub struct ServerEntry {
    pub key: String,
    pub content_hash: Option<String>,
    pub size_bytes: u64,
    pub updated_at: i64,
}

#[derive(Debug, Default)]
pub struct SyncPlan {
    pub push: Vec<String>,
    pub pull: Vec<String>,
    pub delete_remote: Vec<String>,
    pub delete_local: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncResult {
    pub pushed: usize,
    pub pulled: usize,
    pub deleted_remote: usize,
    pub deleted_local: usize,
    pub errors: Vec<String>,
}

pub fn emit_sync_status(status: &str, error: Option<&str>) {
    let _ = host::events::emit_typed(&json!({
        "type": "SyncStatusChanged",
        "status": status,
        "error": error,
    }));
}

fn emit_sync_progress(
    percent: u64,
    completed: usize,
    total: usize,
    phase: &str,
    message: &str,
    path: Option<&str>,
) {
    let _ = host::events::emit_typed(&json!({
        "type": "SyncProgress",
        "completed": completed,
        "total": total,
        "percent": percent,
        "phase": phase,
        "message": message,
        "path": path,
    }));
}

fn staged_percent(base: u64, span: u64, completed: usize, total: usize) -> u64 {
    if total == 0 {
        return base + span;
    }
    let ratio = completed as f64 / total as f64;
    base + (ratio * span as f64).round() as u64
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

/// Scan local workspace files and compute hashes.
///
/// `workspace_root` should be a directory path (use `workspace_root_dir` to
/// convert a file path like `My Workspace/index.md` to its parent directory).
pub fn scan_local(
    workspace_root: &str,
    manifest: Option<&SyncManifest>,
    progress_base: Option<u64>,
    progress_span: Option<u64>,
) -> BTreeMap<String, LocalFileInfo> {
    let mut map = BTreeMap::new();
    let root_dir = workspace_root_dir(workspace_root);
    let files = match host::fs::workspace_file_set() {
        Ok(files) => files,
        Err(e) => {
            host::log::log(
                "warn",
                &format!("[sync_engine] workspace_file_set failed: {e}"),
            );
            return map;
        }
    };
    let total = files.len().max(1);

    for (index, relative) in files.into_iter().enumerate() {
        let completed = index + 1;
        if let (Some(base), Some(span)) = (progress_base, progress_span) {
            emit_sync_progress(
                staged_percent(base, span, completed, total),
                completed,
                total,
                "scan",
                &format!("Scanning {} ({}/{})", relative, completed, total),
                Some(&relative),
            );
        }

        let file_path = join_workspace_path(&root_dir, &relative);
        let key = format!("files/{relative}");
        let metadata = host::fs::file_metadata(&file_path)
            .ok()
            .filter(|meta| meta.exists);
        let size = metadata
            .as_ref()
            .and_then(|meta| meta.size_bytes)
            .unwrap_or(0);
        let modified_at = metadata
            .as_ref()
            .and_then(|meta| meta.modified_at_ms)
            .unwrap_or(0)
            .max(0) as u64;

        if let Some(entry) = manifest
            .and_then(|m| m.files.get(&key))
            .filter(|entry| should_reuse_manifest_hash(entry, size, modified_at))
        {
            map.insert(
                key,
                LocalFileInfo {
                    hash: entry.content_hash.clone(),
                    size,
                    modified_at,
                },
            );
            continue;
        }

        if let Some(hash) = host::hash::hash_file(&file_path) {
            map.insert(
                key,
                LocalFileInfo {
                    hash,
                    size,
                    modified_at,
                },
            );
        }
    }

    map
}

fn should_reuse_manifest_hash(
    entry: &crate::sync_manifest::FileEntry,
    size: u64,
    modified_at: u64,
) -> bool {
    entry.state == SyncState::Clean
        && !entry.content_hash.is_empty()
        && entry.size_bytes == size
        && modified_at > 0
        && modified_at <= entry.modified_at
}

/// If workspace_root is a file path (ends in `.md`), return its parent directory.
pub fn workspace_root_dir(root: &str) -> String {
    let normalized = root.replace('\\', "/");
    if normalized.ends_with(".md") {
        if let Some(pos) = normalized.rfind('/') {
            return normalized[..pos].to_string();
        }
        // No slash — the root is just a filename like "index.md", use "."
        return ".".to_string();
    }
    normalized
}

fn join_workspace_path(root: &str, relative: &str) -> String {
    if root.is_empty() || root == "." {
        relative.to_string()
    } else {
        format!("{}/{}", root.trim_end_matches('/'), relative)
    }
}

// ---------------------------------------------------------------------------
// Server manifest fetch
// ---------------------------------------------------------------------------

/// Fetch object metadata from the server for the given namespace.
pub fn fetch_server_manifest(
    params: &JsonValue,
    namespace_id: &str,
) -> Result<Vec<ServerEntry>, String> {
    let config = load_extism_config();
    let server = resolve_server_url(params, &config).ok_or("Missing server_url")?;
    let headers = auth_headers(resolve_auth_token(params, &config));

    let mut all_entries = Vec::new();
    let mut offset = 0u32;
    let limit = 500u32;

    loop {
        let url = format!(
            "{server}/namespaces/{namespace_id}/objects?prefix=files/&limit={limit}&offset={offset}"
        );
        let response = http_request_compat("GET", &url, &headers, None)?;
        let status = parse_http_status(&response);
        if status != 200 {
            return Err(http_error(status, &parse_http_body(&response)));
        }

        let body = parse_http_body_json(&response).unwrap_or(JsonValue::Array(Vec::new()));
        let items = body.as_array().cloned().unwrap_or_default();
        let count = items.len();

        for item in items {
            let raw_key = item.get("key").and_then(|v| v.as_str()).unwrap_or_default();
            // Decode percent-encoded keys so internal comparisons with
            // local filesystem paths work correctly.
            let key = decode_server_key(raw_key);
            let content_hash = item
                .get("content_hash")
                .and_then(|v| v.as_str())
                .map(String::from);
            let size_bytes = item.get("size_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            let updated_at = item.get("updated_at").and_then(|v| v.as_i64()).unwrap_or(0);

            all_entries.push(ServerEntry {
                key,
                content_hash,
                size_bytes,
                updated_at,
            });
        }

        if count < limit as usize {
            break;
        }
        offset += limit;
    }

    Ok(all_entries)
}

// ---------------------------------------------------------------------------
// Diff computation
// ---------------------------------------------------------------------------

/// Compute what needs to be pushed and pulled.
pub fn compute_diff(
    manifest: &SyncManifest,
    local_scan: &BTreeMap<String, LocalFileInfo>,
    server_entries: &[ServerEntry],
) -> SyncPlan {
    let mut plan = SyncPlan::default();

    let server_map: BTreeMap<&str, &ServerEntry> =
        server_entries.iter().map(|e| (e.key.as_str(), e)).collect();

    // Check local files against server
    for (key, local_info) in local_scan {
        let manifest_entry = manifest.files.get(key.as_str());
        let server_entry = server_map.get(key.as_str());

        match (manifest_entry, server_entry) {
            // File exists locally and on server
            (Some(me), Some(se)) => {
                let local_changed = me.state == SyncState::Dirty
                    || me.content_hash.is_empty()
                    || me.content_hash != local_info.hash;
                let server_changed = se
                    .content_hash
                    .as_ref()
                    .map(|sh| sh != &me.content_hash)
                    .unwrap_or(false);

                match (local_changed, server_changed) {
                    (true, true) => {
                        // Conflict: LWW by modified_at
                        let local_ts = me.modified_at as i64;
                        if local_ts >= se.updated_at {
                            plan.push.push(key.clone());
                        } else {
                            plan.pull.push(key.clone());
                        }
                    }
                    (true, false) => plan.push.push(key.clone()),
                    (false, true) => plan.pull.push(key.clone()),
                    (false, false) => {} // in sync
                }
            }
            // File exists locally but not on server
            (Some(me), None) => {
                if me.state == SyncState::Dirty || me.content_hash.is_empty() {
                    // New local file → push
                    plan.push.push(key.clone());
                } else {
                    // Was clean but server deleted → delete locally
                    plan.delete_local.push(key.clone());
                }
            }
            // File exists locally but has no manifest entry (new)
            (None, Some(_se)) => {
                // Both local and remote have it, but we have no manifest entry.
                // Check if hashes match
                if _se
                    .content_hash
                    .as_ref()
                    .map(|sh| sh != &local_info.hash)
                    .unwrap_or(true)
                {
                    // Different content - push local (new local file takes precedence)
                    plan.push.push(key.clone());
                }
                // else: same content, just mark clean during execution
            }
            (None, None) => {
                // New local file, not on server → push
                plan.push.push(key.clone());
            }
        }
    }

    // Check server files not present locally
    for se in server_entries {
        if !local_scan.contains_key(&se.key) {
            let manifest_entry = manifest.files.get(&se.key);
            match manifest_entry {
                Some(me) if me.state == SyncState::Clean => {
                    // Was clean locally but now gone → deleted on this device.
                    // Delete from server.  This covers moves and renames where
                    // no explicit pending_delete was recorded.
                    plan.delete_remote.push(se.key.clone());
                }
                Some(_) => {
                    // Was dirty but file is gone? Pull it back.
                    plan.pull.push(se.key.clone());
                }
                None => {
                    // New file from another device → pull
                    plan.pull.push(se.key.clone());
                }
            }
        }
    }

    // Pending deletes → delete from server
    for delete in &manifest.pending_deletes {
        let key = if delete.path.starts_with("files/") {
            delete.path.clone()
        } else {
            format!("files/{}", delete.path)
        };
        if server_map.contains_key(key.as_str()) {
            plan.delete_remote.push(key);
        }
    }

    plan
}

// ---------------------------------------------------------------------------
// Push / Pull execution
// ---------------------------------------------------------------------------

/// Push local files to the server.
pub fn execute_push(
    params: &JsonValue,
    namespace_id: &str,
    workspace_root: &str,
    plan: &SyncPlan,
    local_scan: &BTreeMap<String, LocalFileInfo>,
    manifest: &mut SyncManifest,
    progress_base: u64,
    progress_span: u64,
    progress_offset: usize,
    progress_total: usize,
) -> (usize, Vec<String>) {
    let config = load_extism_config();
    let server = match resolve_server_url(params, &config) {
        Some(s) => s,
        None => return (0, vec!["Missing server_url".to_string()]),
    };
    let token = resolve_auth_token(params, &config);
    let root_dir = workspace_root_dir(workspace_root);

    let mut pushed = 0usize;
    let mut errors = Vec::new();

    for (index, key) in plan.push.iter().enumerate() {
        let relative_path = key.strip_prefix("files/").unwrap_or(key);
        let full_path = join_workspace_path(&root_dir, relative_path);
        let completed = progress_offset + index;
        emit_sync_progress(
            staged_percent(progress_base, progress_span, completed, progress_total),
            completed,
            progress_total,
            "upload",
            &format!(
                "Uploading {} ({}/{})",
                relative_path,
                index + 1,
                plan.push.len()
            ),
            Some(relative_path),
        );

        let bytes = match host::fs::read_binary(&full_path) {
            Ok(b) => b,
            Err(e) => {
                errors.push(format!("read {key}: {e}"));
                continue;
            }
        };

        let content_type = guess_content_type(relative_path);
        let mut headers: Vec<(String, String)> =
            vec![("Content-Type".to_string(), content_type.to_string())];
        if let Some(t) = &token {
            headers.push(("Authorization".to_string(), format!("Bearer {t}")));
        }

        let url = object_url(&server, &namespace_id, key);
        match http_request_binary_compat("PUT", &url, &headers, &bytes) {
            Ok(response) => {
                let status = parse_http_status(&response);
                if status == 200 {
                    pushed += 1;
                    let hash = local_scan
                        .get(key)
                        .map(|i| i.hash.clone())
                        .unwrap_or_default();
                    let modified_at = local_scan
                        .get(key)
                        .map(|i| i.modified_at)
                        .unwrap_or_else(|| host::time::timestamp_millis().unwrap_or(0) as u64);
                    manifest.mark_clean(key, &hash, bytes.len() as u64, modified_at);
                } else {
                    errors.push(format!("push {key}: HTTP {status}"));
                }
            }
            Err(e) => errors.push(format!("push {key}: {e}")),
        }

        let completed = progress_offset + index + 1;
        emit_sync_progress(
            staged_percent(progress_base, progress_span, completed, progress_total),
            completed,
            progress_total,
            "upload",
            &format!(
                "Uploaded {} ({}/{})",
                relative_path,
                index + 1,
                plan.push.len()
            ),
            Some(relative_path),
        );
    }

    // Delete remote files
    let headers = auth_headers(token);
    for (index, key) in plan.delete_remote.iter().enumerate() {
        let url = object_url(&server, &namespace_id, key);
        let relative_path = key.strip_prefix("files/").unwrap_or(key);
        match http_request_compat("DELETE", &url, &headers, None) {
            Ok(response) => {
                let status = parse_http_status(&response);
                if status != 204 && status != 200 {
                    errors.push(format!("delete remote {key}: HTTP {status}"));
                }
            }
            Err(e) => errors.push(format!("delete remote {key}: {e}")),
        }
        let completed = progress_offset + plan.push.len() + index + 1;
        emit_sync_progress(
            staged_percent(progress_base, progress_span, completed, progress_total),
            completed,
            progress_total,
            "delete-remote",
            &format!(
                "Deleted {}/{} remote files...",
                index + 1,
                plan.delete_remote.len()
            ),
            Some(relative_path),
        );
    }
    manifest.clear_deletes();

    (pushed, errors)
}

/// Pull remote files to the local workspace.
pub fn execute_pull(
    _params: &JsonValue,
    namespace_id: &str,
    workspace_root: &str,
    plan: &SyncPlan,
    server_entries: &[ServerEntry],
    manifest: &mut SyncManifest,
    progress_base: u64,
    progress_span: u64,
    progress_offset: usize,
    progress_total: usize,
) -> (usize, Vec<String>) {
    let root_dir = workspace_root_dir(workspace_root);

    let server_map: BTreeMap<&str, &ServerEntry> =
        server_entries.iter().map(|e| (e.key.as_str(), e)).collect();

    let mut pulled = 0usize;
    let mut errors = Vec::new();
    let total_pull = plan.pull.len();

    if total_pull > 0 {
        host::log::log(
            "info",
            &format!("DownloadWorkspace starting pull of {total_pull} file(s)"),
        );
    }

    for (index, key) in plan.pull.iter().enumerate() {
        let relative_path = key.strip_prefix("files/").unwrap_or(key);
        let completed = progress_offset + index;
        emit_sync_progress(
            staged_percent(progress_base, progress_span, completed, progress_total),
            completed,
            progress_total,
            "download",
            &format!(
                "Downloading {} ({}/{})",
                relative_path,
                index + 1,
                total_pull
            ),
            Some(relative_path),
        );
        host::log::log(
            "debug",
            &format!(
                "DownloadWorkspace pulling {}/{}: {}",
                index + 1,
                total_pull,
                key
            ),
        );
        match host::namespace::get_object(namespace_id, key) {
            Ok(bytes) => {
                let full_path = join_workspace_path(&root_dir, relative_path);

                // Ensure parent directories exist
                if let Some(parent_end) = full_path.rfind('/') {
                    let parent = &full_path[..parent_end];
                    let marker = format!("{parent}/.diaryx_sync_tmp");
                    let _ = host::fs::write_file(&marker, "");
                    let _ = host::fs::delete_file(&marker);
                }

                let write_result = if relative_path.ends_with(".md") {
                    let content = String::from_utf8_lossy(&bytes);
                    host::fs::write_file(&full_path, &content)
                } else {
                    host::fs::write_binary(&full_path, &bytes)
                };

                match write_result {
                    Ok(()) => {
                        pulled += 1;
                        let hash = server_map
                            .get(key.as_str())
                            .and_then(|se| se.content_hash.clone())
                            .unwrap_or_default();
                        let modified_at = host::fs::file_metadata(&full_path)
                            .ok()
                            .filter(|meta| meta.exists)
                            .and_then(|meta| meta.modified_at_ms)
                            .unwrap_or_else(|| host::time::timestamp_millis().unwrap_or(0) as i64)
                            .max(0) as u64;
                        manifest.mark_clean(key, &hash, bytes.len() as u64, modified_at);
                    }
                    Err(e) => errors.push(format!("write {key}: {e}")),
                }
            }
            Err(e) => errors.push(format!("pull {key}: {e}")),
        }
        let completed = progress_offset + index + 1;
        emit_sync_progress(
            staged_percent(progress_base, progress_span, completed, progress_total),
            completed,
            progress_total,
            "download",
            &format!(
                "Downloaded {} ({}/{})",
                relative_path,
                index + 1,
                total_pull
            ),
            Some(relative_path),
        );
    }

    // Delete local files that were deleted on another device
    for (index, key) in plan.delete_local.iter().enumerate() {
        let relative_path = key.strip_prefix("files/").unwrap_or(key);
        let full_path = join_workspace_path(&root_dir, relative_path);
        if let Err(e) = host::fs::delete_file(&full_path) {
            errors.push(format!("delete local {key}: {e}"));
        }
        manifest.files.remove(key.as_str());
        let completed = progress_offset + plan.pull.len() + index + 1;
        emit_sync_progress(
            staged_percent(progress_base, progress_span, completed, progress_total),
            completed,
            progress_total,
            "delete-local",
            &format!(
                "Removed {}/{} local files...",
                index + 1,
                plan.delete_local.len()
            ),
            Some(relative_path),
        );
    }

    if total_pull > 0 {
        host::log::log(
            "info",
            &format!(
                "DownloadWorkspace finished pull: {pulled} succeeded, {} error(s)",
                errors.len()
            ),
        );
    }

    (pulled, errors)
}

// ---------------------------------------------------------------------------
// Full sync cycle
// ---------------------------------------------------------------------------

/// Run a full push+pull sync cycle.
pub fn sync(
    params: &JsonValue,
    namespace_id: &str,
    workspace_root: &str,
    manifest: &mut SyncManifest,
) -> SyncResult {
    emit_sync_status("syncing", None);
    emit_sync_progress(5, 0, 1, "scan", "Scanned 0/0 files...", None);
    // workspace_root should already be a directory (resolve_workspace_root
    // normalises it), but apply workspace_root_dir defensively.
    let dir_root = workspace_root_dir(workspace_root);
    let local_scan = scan_local(&dir_root, Some(manifest), Some(5), Some(7));
    emit_sync_progress(
        12,
        0,
        1,
        "manifest",
        "Fetched 0/1 remote manifests...",
        None,
    );
    let server_entries = match fetch_server_manifest(params, namespace_id) {
        Ok(entries) => entries,
        Err(e) => {
            emit_sync_status("error", Some(&format!("fetch server manifest: {e}")));
            return SyncResult {
                pushed: 0,
                pulled: 0,
                deleted_remote: 0,
                deleted_local: 0,
                errors: vec![format!("fetch server manifest: {e}")],
            };
        }
    };

    let plan = compute_diff(manifest, &local_scan, &server_entries);
    let total_ops =
        plan.push.len() + plan.pull.len() + plan.delete_remote.len() + plan.delete_local.len();
    emit_sync_progress(
        20,
        0,
        total_ops.max(1),
        "plan",
        &format!("Planned 0/{} sync operations...", total_ops.max(1)),
        None,
    );

    let (pushed, mut push_errors) = execute_push(
        params,
        namespace_id,
        &dir_root,
        &plan,
        &local_scan,
        manifest,
        20,
        45,
        0,
        total_ops.max(1),
    );
    let (pulled, pull_errors) = execute_pull(
        params,
        namespace_id,
        &dir_root,
        &plan,
        &server_entries,
        manifest,
        65,
        25,
        plan.push.len() + plan.delete_remote.len(),
        total_ops.max(1),
    );

    push_errors.extend(pull_errors);

    // Mark any remaining untracked local files as clean
    for (key, info) in &local_scan {
        if !manifest.files.contains_key(key.as_str()) {
            manifest.mark_clean(key, &info.hash, info.size, info.modified_at);
        }
    }

    manifest.last_sync_at = Some(host::time::timestamp_millis().unwrap_or(0) as u64);
    manifest.save();
    emit_sync_progress(98, 1, 1, "finalize", "Finalized 1/1 sync operations.", None);
    if push_errors.is_empty() {
        emit_sync_status("synced", None);
    } else {
        emit_sync_status("error", Some(&push_errors.join("; ")));
    }

    SyncResult {
        pushed,
        pulled,
        deleted_remote: plan.delete_remote.len(),
        deleted_local: plan.delete_local.len(),
        errors: push_errors,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn guess_content_type(path: &str) -> &'static str {
    if path.ends_with(".md") {
        "text/markdown"
    } else if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".yaml") || path.ends_with(".yml") {
        "application/x-yaml"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg"
    } else if path.ends_with(".gif") {
        "image/gif"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".pdf") {
        "application/pdf"
    } else if path.ends_with(".html") {
        "text/html"
    } else if path.ends_with(".css") {
        "text/css"
    } else if path.ends_with(".txt") {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manifest() -> SyncManifest {
        SyncManifest::new("test-ns".to_string())
    }

    fn local(hash: &str, size: u64) -> LocalFileInfo {
        LocalFileInfo {
            hash: hash.to_string(),
            size,
            modified_at: 500,
        }
    }

    #[test]
    fn reuse_manifest_hash_only_for_clean_unchanged_files() {
        let mut manifest = make_manifest();
        manifest.mark_clean("files/doc.md", "hash_v1", 100, 500);
        let entry = manifest.files.get("files/doc.md").unwrap();
        assert!(should_reuse_manifest_hash(entry, 100, 500));
        assert!(should_reuse_manifest_hash(entry, 100, 450));
        assert!(!should_reuse_manifest_hash(entry, 101, 500));
        assert!(!should_reuse_manifest_hash(entry, 100, 501));

        manifest.mark_dirty("files/doc.md");
        let dirty_entry = manifest.files.get("files/doc.md").unwrap();
        assert!(!should_reuse_manifest_hash(dirty_entry, 100, 500));
    }

    #[test]
    fn workspace_paths_are_joined_relative_to_root() {
        assert_eq!(workspace_root_dir("workspace/index.md"), "workspace");
        assert_eq!(workspace_root_dir("workspace"), "workspace");
        assert_eq!(workspace_root_dir("index.md"), ".");
        assert_eq!(
            join_workspace_path("workspace", "Notes/index.md"),
            "workspace/Notes/index.md"
        );
        assert_eq!(join_workspace_path(".", "Notes/index.md"), "Notes/index.md");
    }

    fn server(key: &str, hash: Option<&str>, updated_at: i64) -> ServerEntry {
        ServerEntry {
            key: key.to_string(),
            content_hash: hash.map(String::from),
            size_bytes: 100,
            updated_at,
        }
    }

    #[test]
    fn new_local_file_pushes() {
        let manifest = make_manifest();
        let mut local_scan = BTreeMap::new();
        local_scan.insert("files/new.md".to_string(), local("abc123", 100));

        let plan = compute_diff(&manifest, &local_scan, &[]);
        assert_eq!(plan.push, vec!["files/new.md"]);
        assert!(plan.pull.is_empty());
    }

    #[test]
    fn new_remote_file_pulls() {
        let manifest = make_manifest();
        let local_scan = BTreeMap::new();
        let server = vec![server("files/remote.md", Some("xyz"), 1000)];

        let plan = compute_diff(&manifest, &local_scan, &server);
        assert_eq!(plan.pull, vec!["files/remote.md"]);
        assert!(plan.push.is_empty());
    }

    #[test]
    fn dirty_local_file_pushes() {
        let mut manifest = make_manifest();
        manifest.mark_clean("files/doc.md", "old_hash", 100, 500);
        manifest.mark_dirty("files/doc.md");

        let mut local_scan = BTreeMap::new();
        local_scan.insert("files/doc.md".to_string(), local("new_hash", 120));

        let server = vec![server("files/doc.md", Some("old_hash"), 500)];

        let plan = compute_diff(&manifest, &local_scan, &server);
        assert_eq!(plan.push, vec!["files/doc.md"]);
        assert!(plan.pull.is_empty());
    }

    #[test]
    fn clean_local_server_changed_pulls() {
        let mut manifest = make_manifest();
        manifest.mark_clean("files/doc.md", "hash_v1", 100, 500);

        let mut local_scan = BTreeMap::new();
        local_scan.insert("files/doc.md".to_string(), local("hash_v1", 100));

        let server = vec![server("files/doc.md", Some("hash_v2"), 600)];

        let plan = compute_diff(&manifest, &local_scan, &server);
        assert_eq!(plan.pull, vec!["files/doc.md"]);
        assert!(plan.push.is_empty());
    }

    #[test]
    fn conflict_lww_local_newer_pushes() {
        let mut manifest = make_manifest();
        manifest.mark_clean("files/doc.md", "hash_v1", 100, 700);
        manifest.mark_dirty("files/doc.md");

        let mut local_scan = BTreeMap::new();
        local_scan.insert("files/doc.md".to_string(), local("hash_v2_local", 120));

        let server = vec![server("files/doc.md", Some("hash_v2_remote"), 600)];

        let plan = compute_diff(&manifest, &local_scan, &server);
        assert_eq!(plan.push, vec!["files/doc.md"]);
        assert!(plan.pull.is_empty());
    }

    #[test]
    fn conflict_lww_remote_newer_pulls() {
        let mut manifest = make_manifest();
        manifest.mark_clean("files/doc.md", "hash_v1", 100, 400);
        manifest.mark_dirty("files/doc.md");

        let mut local_scan = BTreeMap::new();
        local_scan.insert("files/doc.md".to_string(), local("hash_v2_local", 120));

        let server = vec![server("files/doc.md", Some("hash_v2_remote"), 600)];

        let plan = compute_diff(&manifest, &local_scan, &server);
        assert_eq!(plan.pull, vec!["files/doc.md"]);
        assert!(plan.push.is_empty());
    }

    #[test]
    fn clean_file_missing_from_server_deletes_local() {
        let mut manifest = make_manifest();
        manifest.mark_clean("files/gone.md", "hash", 100, 500);

        let mut local_scan = BTreeMap::new();
        local_scan.insert("files/gone.md".to_string(), local("hash", 100));

        let plan = compute_diff(&manifest, &local_scan, &[]);
        assert_eq!(plan.delete_local, vec!["files/gone.md"]);
    }

    #[test]
    fn clean_file_gone_locally_deletes_from_server() {
        let mut manifest = make_manifest();
        manifest.mark_clean("files/old-name.md", "hash", 100, 500);

        // File is gone locally (e.g. renamed) but still on server
        let local_scan = BTreeMap::new();
        let server = vec![server("files/old-name.md", Some("hash"), 500)];

        let plan = compute_diff(&manifest, &local_scan, &server);
        assert_eq!(plan.delete_remote, vec!["files/old-name.md"]);
        assert!(plan.pull.is_empty());
    }

    #[test]
    fn pending_delete_sends_remote_delete() {
        let mut manifest = make_manifest();
        manifest.record_delete("files/deleted.md");

        let local_scan = BTreeMap::new();
        let server = vec![server("files/deleted.md", Some("hash"), 500)];

        let plan = compute_diff(&manifest, &local_scan, &server);
        assert_eq!(plan.delete_remote, vec!["files/deleted.md"]);
    }
}
