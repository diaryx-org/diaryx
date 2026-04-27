//! LWW file sync engine.
//!
//! Implements hash-based diffing and last-writer-wins conflict resolution
//! for multi-device personal sync over the namespace object store API.

use diaryx_plugin_sdk::prelude::*;
use percent_encoding::percent_decode_str;
use serde_json::{Value as JsonValue, json};
use std::collections::BTreeMap;

use crate::sync_manifest::{SyncManifest, SyncState};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Decode a potentially percent-encoded key from the server.
///
/// Server listings may return keys with percent-encoding if they were uploaded
/// with an over-eager encode set. Decoding normalises them so that local ↔
/// server comparisons work correctly.
///
/// `+` is left literal: every upload path (web `encodeURIComponent`, Tauri /
/// CLI `percent_encoding::NON_ALPHANUMERIC`, sync-plugin `urlencoding::encode`)
/// percent-encodes `+` as `%2B`, so any `+` we see here came from a literal
/// filename character — not from form-urlencoded space. Treating it as space
/// corrupts filenames like `LGBTQ+.md` on round-trip.
fn decode_server_key(key: &str) -> String {
    percent_decode_str(key).decode_utf8_lossy().into_owned()
}

fn is_not_found_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("404") || lower.contains("not found")
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
    /// Decoded key used for comparison with local file paths.
    pub key: String,
    /// Original key as returned by the server listing.  Used for GET/DELETE
    /// API calls where the server expects the un-decoded form.
    pub server_key: String,
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
    /// Non-markdown file keys deferred for background download.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deferred: Vec<String>,
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
    _params: &JsonValue,
    namespace_id: &str,
) -> Result<Vec<ServerEntry>, String> {
    let mut all_entries = Vec::new();
    let mut offset = 0u32;
    let limit = 500u32;

    loop {
        let items = host::namespace::list_objects_with_options(
            namespace_id,
            host::namespace::ListObjectsOptions {
                prefix: Some("files/".to_string()),
                limit: Some(limit),
                offset: Some(offset),
            },
        )?;
        let count = items.len();

        for item in items {
            // Decode percent-encoded keys so internal comparisons with
            // local filesystem paths work correctly.
            let key = decode_server_key(&item.key);

            all_entries.push(ServerEntry {
                server_key: item.key,
                key,
                content_hash: item.content_hash,
                size_bytes: item.size_bytes.unwrap_or(0),
                updated_at: item.updated_at.unwrap_or(0),
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
    workspace_root: &str,
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
            (Some(me), Some(_se)) if me.state == SyncState::PullFailed => {
                // Last pull failed; local bytes (if any) are stale. Re-pull
                // instead of diffing — never push, which would overwrite
                // the good server version with whatever was sitting on disk.
                plan.pull.push(key.clone());
            }
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
                        // Conflict: last-writer-wins by wall-clock time.
                        //
                        // Units trap: `me.modified_at` comes from the host's
                        // filesystem metadata via `FileMetadata.modified_at_ms`
                        // — it's **milliseconds** since the Unix epoch. But
                        // `se.updated_at` comes from the server, which
                        // persists Unix timestamps in **seconds** (both
                        // `diaryx_sync_server`'s rusqlite `upsert_object` and
                        // `diaryx_cloudflare`'s D1 schema use
                        // `chrono::Utc::now().timestamp()`). Without this
                        // conversion the raw `local_ts >= se.updated_at`
                        // comparison is biased ~1000× in favour of push —
                        // the local side always "wins" and pulls never
                        // happen. Surfaced by the `lww_resolves_conflict…`
                        // E2E test once it was un-ignored.
                        let local_ts_secs = (me.modified_at / 1000) as i64;
                        if local_ts_secs >= se.updated_at {
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
                if me.state == SyncState::PullFailed {
                    // Nothing to pull and we never confirmed local is good —
                    // leave untouched.
                } else if me.state == SyncState::Dirty || me.content_hash.is_empty() {
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
                Some(_me) if _me.state == SyncState::Clean => {
                    // File was clean in our manifest but is no longer in the
                    // local workspace_file_set.  Two possible reasons:
                    //
                    // 1. Tree restructuring — the file still exists on disk
                    //    but isn't reachable via tree-walk (e.g. a parent
                    //    entry's `contents` list was edited).  Pull is safe.
                    //
                    // 2. Genuine deletion — the file was removed from disk
                    //    (rm, CLI, or GUI).  We should delete from server.
                    //
                    // Disambiguate by checking whether the file still exists
                    // on disk.
                    let relative = se.key.strip_prefix("files/").unwrap_or(&se.key);
                    let full_path =
                        join_workspace_path(&workspace_root_dir(workspace_root), relative);
                    let exists_on_disk = host::fs::file_exists(&full_path).unwrap_or(true);
                    if exists_on_disk {
                        // File exists on disk but isn't in tree-walk
                        // (restructuring).  Pull is the safe default.
                        plan.pull.push(se.key.clone());
                    } else {
                        // File is gone from disk → local deletion.
                        plan.delete_remote.push(se.key.clone());
                    }
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
    _params: &JsonValue,
    namespace_id: &str,
    workspace_root: &str,
    plan: &SyncPlan,
    local_scan: &BTreeMap<String, LocalFileInfo>,
    server_entries: &[ServerEntry],
    manifest: &mut SyncManifest,
    progress_base: u64,
    progress_span: u64,
    progress_offset: usize,
    progress_total: usize,
) -> (usize, usize, Vec<String>) {
    let root_dir = workspace_root_dir(workspace_root);

    let mut pushed = 0usize;
    let mut deleted_remote = 0usize;
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
        match host::namespace::put_private_object(namespace_id, key, &bytes, content_type) {
            Ok(()) => {
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
    let server_key_map: BTreeMap<&str, &str> = server_entries
        .iter()
        .map(|e| (e.key.as_str(), e.server_key.as_str()))
        .collect();
    for (index, key) in plan.delete_remote.iter().enumerate() {
        let relative_path = key.strip_prefix("files/").unwrap_or(key);
        let api_key = server_key_map.get(key.as_str()).copied().unwrap_or(key);
        match host::namespace::delete_object(namespace_id, api_key) {
            Ok(()) => {
                deleted_remote += 1;
                manifest.ack_delete(key);
                manifest.files.remove(key.as_str());
            }
            Err(e) if is_not_found_error(&e) => {
                deleted_remote += 1;
                manifest.ack_delete(key);
                manifest.files.remove(key.as_str());
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

    (pushed, deleted_remote, errors)
}

/// Pull remote files to the local workspace.
/// Attempt a batch download for the given API keys. Returns `true` if the
/// batch call succeeded (even if some individual keys had errors), `false`
/// if the batch call itself failed entirely.
fn try_batch_download(
    namespace_id: &str,
    api_keys: &[String],
    api_key_to_key: &BTreeMap<&str, &str>,
    root_dir: &str,
    server_map: &BTreeMap<&str, &ServerEntry>,
    manifest: &mut SyncManifest,
    pulled: &mut usize,
    errors: &mut Vec<String>,
) -> bool {
    match host::namespace::get_objects_batch(namespace_id, api_keys) {
        Ok(batch) => {
            // Process successful downloads.
            for (api_key, entry) in &batch.objects {
                let key = api_key_to_key
                    .get(api_key.as_str())
                    .copied()
                    .unwrap_or(api_key);
                let relative_path = key.strip_prefix("files/").unwrap_or(key);
                let full_path = join_workspace_path(root_dir, relative_path);

                write_pulled_bytes(
                    key,
                    &full_path,
                    &entry.bytes,
                    server_map,
                    manifest,
                    pulled,
                    errors,
                );
            }

            // Process per-key errors from the batch.
            for (api_key, err_msg) in &batch.errors {
                let key = api_key_to_key
                    .get(api_key.as_str())
                    .copied()
                    .unwrap_or(api_key);
                if is_not_found_error(err_msg) {
                    host::log::log(
                        "info",
                        &format!("pull {key}: object not found (ghost entry), cleaning up"),
                    );
                    let _ = host::namespace::delete_object(namespace_id, api_key);
                } else {
                    manifest.mark_pull_failed(key);
                    errors.push(format!("pull {key}: {err_msg}"));
                }
            }
            true
        }
        Err(e) => {
            host::log::log(
                "warn",
                &format!("Batch download failed ({} keys): {e}", api_keys.len()),
            );
            false
        }
    }
}

/// Write downloaded bytes to disk and update the manifest.
///
/// On success, marks the entry clean with the hash of what actually landed
/// on disk (via `host::hash::hash_file`), so a server-provided hash that is
/// missing or differs from the post-write bytes doesn't leave the manifest
/// in a state that triggers spurious pushes on the next sync.
///
/// On failure, marks the entry `PullFailed` so `compute_diff` forces a
/// re-pull next time instead of pushing whatever stale bytes happen to be
/// sitting on disk.
fn write_pulled_bytes(
    key: &str,
    full_path: &str,
    bytes: &[u8],
    server_map: &BTreeMap<&str, &ServerEntry>,
    manifest: &mut SyncManifest,
    pulled: &mut usize,
    errors: &mut Vec<String>,
) {
    // Ensure parent directories exist.
    if let Some(parent_end) = full_path.rfind('/') {
        let parent = &full_path[..parent_end];
        let marker = format!("{parent}/.diaryx_sync_tmp");
        let _ = host::fs::write_file(&marker, "");
        let _ = host::fs::delete_file(&marker);
    }

    // Always write raw bytes — String::from_utf8_lossy on `.md` would
    // silently replace invalid UTF-8 with U+FFFD and desync local content
    // from what the server has.
    match host::fs::write_binary(full_path, bytes) {
        Ok(()) => {
            *pulled += 1;

            // Prefer the hash of what we actually wrote to disk over the
            // server-reported hash: the two should agree, but if the server
            // omits `content_hash` (legacy rows) or our write goes through
            // some host-side transform, we want the manifest to reflect
            // ground truth.
            let hash = host::hash::hash_file(full_path).unwrap_or_else(|| {
                server_map
                    .get(key)
                    .and_then(|se| se.content_hash.clone())
                    .unwrap_or_default()
            });
            let modified_at = host::fs::file_metadata(full_path)
                .ok()
                .filter(|meta| meta.exists)
                .and_then(|meta| meta.modified_at_ms)
                .unwrap_or_else(|| host::time::timestamp_millis().unwrap_or(0) as i64)
                .max(0) as u64;
            manifest.mark_clean(key, &hash, bytes.len() as u64, modified_at);
        }
        Err(e) => {
            manifest.mark_pull_failed(key);
            errors.push(format!("write {key}: {e}"));
        }
    }
}

/// Download and write a single file, updating manifest and error tracking.
fn pull_single_file(
    namespace_id: &str,
    key: &str,
    api_key: &str,
    relative_path: &str,
    root_dir: &str,
    server_map: &BTreeMap<&str, &ServerEntry>,
    manifest: &mut SyncManifest,
    pulled: &mut usize,
    errors: &mut Vec<String>,
) {
    match host::namespace::get_object(namespace_id, api_key) {
        Ok(bytes) => {
            let full_path = join_workspace_path(root_dir, relative_path);
            write_pulled_bytes(
                key, &full_path, &bytes, server_map, manifest, pulled, errors,
            );
        }
        Err(e) => {
            if is_not_found_error(&e) {
                host::log::log(
                    "info",
                    &format!("pull {key}: object not found (ghost entry), cleaning up"),
                );
                let _ = host::namespace::delete_object(namespace_id, api_key);
            } else {
                manifest.mark_pull_failed(key);
                errors.push(format!("pull {key}: {e}"));
            }
        }
    }
}

/// Execute the pull phase of a sync.
///
/// When `skip_non_markdown` is `true`, only `.md` files are downloaded.
/// Non-markdown files are collected into the returned `deferred` list so the
/// host can enqueue them for background download.
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
    skip_non_markdown: bool,
) -> (usize, usize, Vec<String>, Vec<String>) {
    let root_dir = workspace_root_dir(workspace_root);

    let server_map: BTreeMap<&str, &ServerEntry> =
        server_entries.iter().map(|e| (e.key.as_str(), e)).collect();

    let mut pulled = 0usize;
    let mut deleted_local = 0usize;
    let mut errors = Vec::new();
    let total_pull = plan.pull.len();

    if total_pull > 0 {
        host::log::log(
            "info",
            &format!("DownloadWorkspace starting pull of {total_pull} file(s)"),
        );
    }

    // Partition files by size: small files (<5MB) are batched via multipart,
    // large files are downloaded individually to avoid huge batch responses.
    // When `skip_non_markdown` is true, non-.md files are deferred for
    // background download by the host.
    const BATCH_CHUNK_SIZE: usize = 200;
    const BATCH_SIZE_LIMIT: u64 = 5 * 1024 * 1024; // 5MB

    let mut batchable_keys: Vec<&String> = Vec::new();
    let mut large_keys: Vec<&String> = Vec::new();
    let mut deferred_keys: Vec<String> = Vec::new();

    for key in &plan.pull {
        if skip_non_markdown && !key.ends_with(".md") {
            deferred_keys.push(key.clone());
            continue;
        }
        let size = server_map
            .get(key.as_str())
            .map(|se| se.size_bytes)
            .unwrap_or(0);
        if size < BATCH_SIZE_LIMIT {
            batchable_keys.push(key);
        } else {
            large_keys.push(key);
        }
    }

    if skip_non_markdown && !deferred_keys.is_empty() {
        host::log::log(
            "info",
            &format!(
                "Deferring {} non-markdown file(s) for background download",
                deferred_keys.len()
            ),
        );
    }

    let mut files_completed: usize = 0;

    // --- Batch-fetch small files (<5MB) in chunks ---
    for chunk in batchable_keys.chunks(BATCH_CHUNK_SIZE) {
        let chunk_start = files_completed;
        let chunk_end = chunk_start + chunk.len();

        emit_sync_progress(
            staged_percent(
                progress_base,
                progress_span,
                progress_offset + chunk_start,
                progress_total,
            ),
            progress_offset + chunk_start,
            progress_total,
            "download",
            &format!(
                "Downloading batch (files {}-{} of {})",
                chunk_start + 1,
                chunk_end,
                total_pull,
            ),
            None,
        );

        // Build the list of server-side keys for the batch request.
        let batch_api_keys: Vec<String> = chunk
            .iter()
            .map(|key| {
                server_map
                    .get(key.as_str())
                    .map(|se| se.server_key.clone())
                    .unwrap_or_else(|| (*key).clone())
            })
            .collect();

        // Map from server_key back to decoded key for result processing.
        let api_key_to_key: BTreeMap<&str, &str> = chunk
            .iter()
            .zip(batch_api_keys.iter())
            .map(|(key, api_key)| (api_key.as_str(), key.as_str()))
            .collect();

        // Try batch download; on failure, retry with smaller sub-chunks
        // before falling back to individual downloads.
        let batch_ok = try_batch_download(
            namespace_id,
            &batch_api_keys,
            &api_key_to_key,
            &root_dir,
            &server_map,
            manifest,
            &mut pulled,
            &mut errors,
        );

        if !batch_ok {
            // Retry with two smaller sub-chunks (halved).
            let mid = batch_api_keys.len() / 2;
            let (first_half_keys, second_half_keys) = batch_api_keys.split_at(mid);

            for sub_keys in [first_half_keys, second_half_keys] {
                if sub_keys.is_empty() {
                    continue;
                }
                let sub_ok = try_batch_download(
                    namespace_id,
                    sub_keys,
                    &api_key_to_key,
                    &root_dir,
                    &server_map,
                    manifest,
                    &mut pulled,
                    &mut errors,
                );
                if !sub_ok {
                    // Sub-chunk also failed — fall back to individual downloads.
                    host::log::log(
                        "warn",
                        "Sub-chunk batch also failed, falling back to individual downloads",
                    );
                    for api_key in sub_keys {
                        let key = api_key_to_key
                            .get(api_key.as_str())
                            .copied()
                            .unwrap_or(api_key);
                        let relative_path = key.strip_prefix("files/").unwrap_or(key);
                        pull_single_file(
                            namespace_id,
                            key,
                            api_key,
                            relative_path,
                            &root_dir,
                            &server_map,
                            manifest,
                            &mut pulled,
                            &mut errors,
                        );
                    }
                }
            }
        }

        files_completed = chunk_end;
        emit_sync_progress(
            staged_percent(
                progress_base,
                progress_span,
                progress_offset + files_completed,
                progress_total,
            ),
            progress_offset + files_completed,
            progress_total,
            "download",
            &format!(
                "Downloaded batch (files {}-{} of {})",
                chunk_start + 1,
                chunk_end,
                total_pull,
            ),
            None,
        );
    }

    // --- Individually fetch large files (>=5MB) ---
    for key in &large_keys {
        let relative_path = key.strip_prefix("files/").unwrap_or(key);
        let completed = progress_offset + files_completed;
        emit_sync_progress(
            staged_percent(progress_base, progress_span, completed, progress_total),
            completed,
            progress_total,
            "download",
            &format!(
                "Downloading {} ({}/{})",
                relative_path,
                files_completed + 1,
                total_pull
            ),
            Some(relative_path),
        );

        let api_key = server_map
            .get(key.as_str())
            .map(|se| se.server_key.as_str())
            .unwrap_or(key);
        pull_single_file(
            namespace_id,
            key,
            api_key,
            relative_path,
            &root_dir,
            &server_map,
            manifest,
            &mut pulled,
            &mut errors,
        );

        files_completed += 1;
        let completed = progress_offset + files_completed;
        emit_sync_progress(
            staged_percent(progress_base, progress_span, completed, progress_total),
            completed,
            progress_total,
            "download",
            &format!(
                "Downloaded {} ({}/{})",
                relative_path, files_completed, total_pull
            ),
            Some(relative_path),
        );
    }

    // Delete local files that were deleted on another device
    for (index, key) in plan.delete_local.iter().enumerate() {
        let relative_path = key.strip_prefix("files/").unwrap_or(key);
        let full_path = join_workspace_path(&root_dir, relative_path);
        match host::fs::delete_file(&full_path) {
            Ok(()) => {
                deleted_local += 1;
                manifest.files.remove(key.as_str());
            }
            Err(e) => errors.push(format!("delete local {key}: {e}")),
        }
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

    (pulled, deleted_local, errors, deferred_keys)
}

// ---------------------------------------------------------------------------
// Streaming download (DownloadWorkspace)
//
// Eager pagination + checkpointing:
//   - List server objects page-by-page.
//   - As each page arrives, filter against the existing manifest (resumable:
//     keys already pulled with matching hashes are skipped).
//   - Buffer pulls until a wave is full, then dispatch concurrent batches.
//   - Save the manifest after every wave so a crash/cancel can resume.
//   - Adapt batch size and concurrency from the previous wave's throughput.
//   - Poll cancellation between waves; on cancel, save and bail out.
// ---------------------------------------------------------------------------

const PULL_PAGE_LIMIT: u32 = 500;
const MIN_BATCH_SIZE: usize = 50;
const MAX_BATCH_SIZE: usize = 500;
const MIN_CONCURRENCY: u32 = 1;
const MAX_CONCURRENCY: u32 = 6;
const LARGE_FILE_THRESHOLD: u64 = 5 * 1024 * 1024;
const ADAPTIVE_FAST_MS: u64 = 5_000;
const ADAPTIVE_SLOW_MS: u64 = 30_000;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdaptiveState {
    pub batch_size: usize,
    pub concurrency: u32,
}

impl Default for AdaptiveState {
    fn default() -> Self {
        Self {
            batch_size: 200,
            concurrency: 4,
        }
    }
}

fn adaptive_storage_key(namespace_id: &str) -> String {
    format!("download_adaptive::{namespace_id}")
}

pub fn load_adaptive_state(namespace_id: &str) -> AdaptiveState {
    let key = adaptive_storage_key(namespace_id);
    match host::storage::get(&key) {
        Ok(Some(bytes)) => serde_json::from_slice(&bytes).unwrap_or_default(),
        _ => AdaptiveState::default(),
    }
}

pub fn save_adaptive_state(namespace_id: &str, state: &AdaptiveState) {
    let key = adaptive_storage_key(namespace_id);
    if let Ok(bytes) = serde_json::to_vec(state) {
        let _ = host::storage::set(&key, &bytes);
    }
}

fn adapt_after_wave(
    state: &mut AdaptiveState,
    wave_bytes: u64,
    elapsed_ms: u64,
    error_count: usize,
) {
    let _ = wave_bytes; // reserved for future throughput-tuning
    if error_count > 0 || elapsed_ms > ADAPTIVE_SLOW_MS {
        // Back off: halve batch size, drop one concurrency slot.
        state.batch_size = (state.batch_size / 2).max(MIN_BATCH_SIZE);
        if state.concurrency > MIN_CONCURRENCY {
            state.concurrency -= 1;
        }
    } else if elapsed_ms < ADAPTIVE_FAST_MS && error_count == 0 {
        // Going fast: ramp up.
        state.batch_size = (state.batch_size * 3 / 2).min(MAX_BATCH_SIZE);
        if state.concurrency < MAX_CONCURRENCY {
            state.concurrency += 1;
        }
    }
}

fn manifest_already_satisfies(manifest: &SyncManifest, entry: &ServerEntry) -> bool {
    let local_entry = match manifest.files.get(&entry.key) {
        Some(e) => e,
        None => return false,
    };
    if local_entry.state != SyncState::Clean {
        return false;
    }
    if local_entry.content_hash.is_empty() {
        return false;
    }
    match &entry.content_hash {
        Some(server_hash) => &local_entry.content_hash == server_hash,
        // Server didn't report a hash — assume manifest is up to date.
        None => true,
    }
}

/// One page of server entries plus a flag indicating whether more pages remain.
struct ListPage {
    entries: Vec<ServerEntry>,
    has_more: bool,
}

fn fetch_server_page(namespace_id: &str, offset: u32, limit: u32) -> Result<ListPage, String> {
    let items = host::namespace::list_objects_with_options(
        namespace_id,
        host::namespace::ListObjectsOptions {
            prefix: Some("files/".to_string()),
            limit: Some(limit),
            offset: Some(offset),
        },
    )?;
    let count = items.len();
    let entries = items
        .into_iter()
        .map(|item| {
            let key = decode_server_key(&item.key);
            ServerEntry {
                server_key: item.key,
                key,
                content_hash: item.content_hash,
                size_bytes: item.size_bytes.unwrap_or(0),
                updated_at: item.updated_at.unwrap_or(0),
            }
        })
        .collect();
    Ok(ListPage {
        entries,
        has_more: count >= limit as usize,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamingDownloadResult {
    pub pulled: usize,
    pub skipped_resume: usize,
    pub errors: Vec<String>,
    pub deferred: Vec<String>,
    pub cancelled: bool,
}

/// Stream-and-pull the server manifest with resumability + concurrent batches +
/// adaptive sizing + cooperative cancellation. Used by `DownloadWorkspace`.
///
/// `cancel_token` may be empty (no cancellation possible). When non-empty,
/// the plugin polls `host::cancellation::is_cancelled` between waves.
///
/// `manifest` is mutated in place. On cancellation or partial success, the
/// caller should `manifest.save()` to persist progress so a subsequent call
/// can resume.
pub fn execute_streaming_download(
    namespace_id: &str,
    workspace_root: &str,
    manifest: &mut SyncManifest,
    cancel_token: &str,
    skip_non_markdown: bool,
) -> StreamingDownloadResult {
    let root_dir = workspace_root_dir(workspace_root);

    let mut adaptive = load_adaptive_state(namespace_id);
    adaptive.batch_size = adaptive.batch_size.clamp(MIN_BATCH_SIZE, MAX_BATCH_SIZE);
    adaptive.concurrency = adaptive.concurrency.clamp(MIN_CONCURRENCY, MAX_CONCURRENCY);

    let mut pulled = 0usize;
    let mut skipped_resume = 0usize;
    let mut errors: Vec<String> = Vec::new();
    let mut deferred: Vec<String> = Vec::new();
    let mut cancelled = false;

    // Buffered queues across pages.
    let mut pending_small: Vec<ServerEntry> = Vec::new();
    let mut pending_large: Vec<ServerEntry> = Vec::new();
    let mut total_seen: usize = 0;
    let mut total_to_pull: usize = 0;

    let mut offset = 0u32;

    'pages: loop {
        if !cancel_token.is_empty() && host::cancellation::is_cancelled(cancel_token) {
            cancelled = true;
            break 'pages;
        }

        let page = match fetch_server_page(namespace_id, offset, PULL_PAGE_LIMIT) {
            Ok(p) => p,
            Err(e) => {
                errors.push(format!("list page offset={offset}: {e}"));
                break 'pages;
            }
        };

        let count = page.entries.len();
        total_seen += count;
        emit_sync_progress(
            stream_percent(pulled, total_to_pull),
            pulled,
            total_to_pull.max(1),
            "manifest",
            &format!("Discovered {total_seen} remote files..."),
            None,
        );

        for entry in page.entries {
            if skip_non_markdown && !entry.key.ends_with(".md") {
                deferred.push(entry.key);
                continue;
            }
            if manifest_already_satisfies(manifest, &entry) {
                skipped_resume += 1;
                continue;
            }
            total_to_pull += 1;
            if entry.size_bytes >= LARGE_FILE_THRESHOLD {
                pending_large.push(entry);
            } else {
                pending_small.push(entry);
            }
        }

        // Drain small-file waves while we have enough buffered to fill a wave.
        let wave_target = adaptive
            .batch_size
            .saturating_mul(adaptive.concurrency as usize);
        while pending_small.len() >= wave_target {
            if !cancel_token.is_empty() && host::cancellation::is_cancelled(cancel_token) {
                cancelled = true;
                break 'pages;
            }
            let drained: Vec<ServerEntry> = pending_small.drain(..wave_target).collect();
            run_small_wave(
                namespace_id,
                &root_dir,
                drained,
                manifest,
                &mut adaptive,
                &mut pulled,
                &mut errors,
                total_to_pull,
            );
            // Checkpoint after every wave.
            manifest.save();
        }

        if !page.has_more {
            break 'pages;
        }
        offset = offset.saturating_add(PULL_PAGE_LIMIT);
    }

    // Drain remaining small files (possibly with smaller wave size).
    while !pending_small.is_empty() {
        if !cancel_token.is_empty() && host::cancellation::is_cancelled(cancel_token) {
            cancelled = true;
            break;
        }
        let take = pending_small.len().min(
            adaptive
                .batch_size
                .saturating_mul(adaptive.concurrency as usize),
        );
        let drained: Vec<ServerEntry> = pending_small.drain(..take).collect();
        run_small_wave(
            namespace_id,
            &root_dir,
            drained,
            manifest,
            &mut adaptive,
            &mut pulled,
            &mut errors,
            total_to_pull,
        );
        manifest.save();
    }

    // Large files — fetch one at a time (already covered by single-file path).
    if !cancelled {
        let server_map: BTreeMap<&str, &ServerEntry> =
            pending_large.iter().map(|e| (e.key.as_str(), e)).collect();
        for entry in &pending_large {
            if !cancel_token.is_empty() && host::cancellation::is_cancelled(cancel_token) {
                cancelled = true;
                break;
            }
            let relative_path = entry.key.strip_prefix("files/").unwrap_or(&entry.key);
            emit_sync_progress(
                stream_percent(pulled, total_to_pull),
                pulled,
                total_to_pull.max(1),
                "download",
                &format!(
                    "Downloading large file {} ({} bytes)",
                    relative_path, entry.size_bytes
                ),
                Some(relative_path),
            );
            pull_single_file(
                namespace_id,
                &entry.key,
                &entry.server_key,
                relative_path,
                &root_dir,
                &server_map,
                manifest,
                &mut pulled,
                &mut errors,
            );
            manifest.save();
        }
    }

    save_adaptive_state(namespace_id, &adaptive);

    if cancelled {
        host::log::log("info", "DownloadWorkspace cancelled by host");
    }

    StreamingDownloadResult {
        pulled,
        skipped_resume,
        errors,
        deferred,
        cancelled,
    }
}

fn stream_percent(pulled: usize, total: usize) -> u64 {
    if total == 0 {
        return 15;
    }
    let ratio = (pulled as f64 / total as f64).clamp(0.0, 1.0);
    15 + (ratio * 80.0) as u64
}

fn run_small_wave(
    namespace_id: &str,
    root_dir: &str,
    entries: Vec<ServerEntry>,
    manifest: &mut SyncManifest,
    adaptive: &mut AdaptiveState,
    pulled: &mut usize,
    errors: &mut Vec<String>,
    total_to_pull: usize,
) {
    if entries.is_empty() {
        return;
    }

    let server_map: BTreeMap<&str, &ServerEntry> =
        entries.iter().map(|e| (e.key.as_str(), e)).collect();
    let api_key_to_key: BTreeMap<&str, &str> = entries
        .iter()
        .map(|e| (e.server_key.as_str(), e.key.as_str()))
        .collect();

    // Chunk this wave into `concurrency` batches of up to `batch_size` keys.
    let mut batches: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for entry in &entries {
        if current.len() >= adaptive.batch_size {
            batches.push(std::mem::take(&mut current));
        }
        current.push(entry.server_key.clone());
    }
    if !current.is_empty() {
        batches.push(current);
    }

    let wave_bytes: u64 = entries.iter().map(|e| e.size_bytes).sum();
    let start_ms = host::time::timestamp_millis().unwrap_or(0);

    let result = host::namespace::get_objects_batches_concurrent(
        namespace_id,
        &batches,
        adaptive.concurrency,
    );

    match result {
        Ok(batch) => {
            // Process successful downloads.
            for (api_key, entry) in &batch.objects {
                let key = api_key_to_key
                    .get(api_key.as_str())
                    .copied()
                    .unwrap_or(api_key);
                let relative_path = key.strip_prefix("files/").unwrap_or(key);
                let full_path = join_workspace_path(root_dir, relative_path);

                write_pulled_bytes(
                    key,
                    &full_path,
                    &entry.bytes,
                    &server_map,
                    manifest,
                    pulled,
                    errors,
                );
            }

            // Process per-key errors from the batch.
            let mut wave_errors = 0usize;
            for (api_key, err_msg) in &batch.errors {
                if api_key.starts_with("__batch_error_") || api_key.starts_with("__batch_panic_") {
                    errors.push(format!("wave: {err_msg}"));
                    wave_errors += 1;
                    continue;
                }
                let key = api_key_to_key
                    .get(api_key.as_str())
                    .copied()
                    .unwrap_or(api_key);
                if is_not_found_error(err_msg) {
                    host::log::log(
                        "info",
                        &format!("pull {key}: object not found (ghost entry), cleaning up"),
                    );
                    let _ = host::namespace::delete_object(namespace_id, api_key);
                } else {
                    manifest.mark_pull_failed(key);
                    errors.push(format!("pull {key}: {err_msg}"));
                    wave_errors += 1;
                }
            }

            let elapsed = host::time::timestamp_millis()
                .unwrap_or(start_ms)
                .saturating_sub(start_ms);
            adapt_after_wave(adaptive, wave_bytes, elapsed, wave_errors);

            emit_sync_progress(
                stream_percent(*pulled, total_to_pull),
                *pulled,
                total_to_pull.max(1),
                "download",
                &format!("Downloaded {} files...", *pulled),
                None,
            );
        }
        Err(e) => {
            host::log::log(
                "warn",
                &format!(
                    "Wave failed ({} batches, {} keys): {e}; falling back to single-batch path",
                    batches.len(),
                    entries.len()
                ),
            );
            // Whole-wave failure (network drop?) — fall back to sequential
            // single-batch downloads so partial progress is still made.
            for batch in &batches {
                let _ = try_batch_download(
                    namespace_id,
                    batch,
                    &api_key_to_key,
                    root_dir,
                    &server_map,
                    manifest,
                    pulled,
                    errors,
                );
            }
            // Treat as slow wave for adaptive tuning.
            adapt_after_wave(adaptive, wave_bytes, ADAPTIVE_SLOW_MS, batches.len());
        }
    }
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
                deferred: Vec::new(),
            };
        }
    };

    let plan = compute_diff(manifest, &local_scan, &server_entries, workspace_root);
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

    let (pushed, deleted_remote, mut push_errors) = execute_push(
        params,
        namespace_id,
        &dir_root,
        &plan,
        &local_scan,
        &server_entries,
        manifest,
        20,
        45,
        0,
        total_ops.max(1),
    );
    let (pulled, deleted_local, pull_errors, deferred) = execute_pull(
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
        true, // defer non-markdown for background download
    );

    push_errors.extend(pull_errors);

    // Mark untracked local files as clean ONLY if the server also has them
    // (i.e. matching content, no manifest entry yet).  Files that are
    // local-only must stay untracked so the next sync correctly pushes them
    // instead of treating them as "deleted on server".
    let server_key_set: std::collections::BTreeSet<&str> =
        server_entries.iter().map(|e| e.key.as_str()).collect();
    for (key, info) in &local_scan {
        if !manifest.files.contains_key(key.as_str()) && server_key_set.contains(key.as_str()) {
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
        deleted_remote,
        deleted_local,
        errors: push_errors,
        deferred,
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
    fn decode_server_key_preserves_literal_plus() {
        // Filenames containing `+` must round-trip unchanged; the previous
        // `+` → space conversion corrupted names like "LGBTQ+.md" on pull.
        assert_eq!(
            decode_server_key("Archive/Essays/LGBTQ+.md"),
            "Archive/Essays/LGBTQ+.md"
        );
        assert_eq!(
            decode_server_key("School/Your passion + meaningful experience.md"),
            "School/Your passion + meaningful experience.md"
        );
    }

    #[test]
    fn decode_server_key_decodes_percent_encoded_chars() {
        assert_eq!(decode_server_key("Foo%20Bar.md"), "Foo Bar.md");
        assert_eq!(decode_server_key("LGBTQ%2B.md"), "LGBTQ+.md");
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
            server_key: key.to_string(),
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

        let plan = compute_diff(&manifest, &local_scan, &[], "");
        assert_eq!(plan.push, vec!["files/new.md"]);
        assert!(plan.pull.is_empty());
    }

    #[test]
    fn new_remote_file_pulls() {
        let manifest = make_manifest();
        let local_scan = BTreeMap::new();
        let server = vec![server("files/remote.md", Some("xyz"), 1000)];

        let plan = compute_diff(&manifest, &local_scan, &server, "");
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

        let plan = compute_diff(&manifest, &local_scan, &server, "");
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

        let plan = compute_diff(&manifest, &local_scan, &server, "");
        assert_eq!(plan.pull, vec!["files/doc.md"]);
        assert!(plan.push.is_empty());
    }

    // Units convention (see `compute_diff` LWW branch):
    //   - `FileEntry.modified_at` / `LocalFileInfo.modified_at` — milliseconds
    //     (mirrors `FileMetadata.modified_at_ms` from the host fs API).
    //   - `ServerEntry.updated_at` — seconds (mirrors the server's
    //     `chrono::Utc::now().timestamp()`).
    // LWW compares them in seconds by scaling the manifest side down.
    #[test]
    fn conflict_lww_local_newer_pushes() {
        let mut manifest = make_manifest();
        // Local mtime = 700_000 ms = 700 s. Server mtime = 600 s. Local wins.
        manifest.mark_clean("files/doc.md", "hash_v1", 100, 700_000);
        manifest.mark_dirty("files/doc.md");

        let mut local_scan = BTreeMap::new();
        local_scan.insert("files/doc.md".to_string(), local("hash_v2_local", 120));

        let server = vec![server("files/doc.md", Some("hash_v2_remote"), 600)];

        let plan = compute_diff(&manifest, &local_scan, &server, "");
        assert_eq!(plan.push, vec!["files/doc.md"]);
        assert!(plan.pull.is_empty());
    }

    #[test]
    fn conflict_lww_remote_newer_pulls() {
        let mut manifest = make_manifest();
        // Local mtime = 400_000 ms = 400 s. Server mtime = 600 s. Server wins.
        manifest.mark_clean("files/doc.md", "hash_v1", 100, 400_000);
        manifest.mark_dirty("files/doc.md");

        let mut local_scan = BTreeMap::new();
        local_scan.insert("files/doc.md".to_string(), local("hash_v2_local", 120));

        let server = vec![server("files/doc.md", Some("hash_v2_remote"), 600)];

        let plan = compute_diff(&manifest, &local_scan, &server, "");
        assert_eq!(plan.pull, vec!["files/doc.md"]);
        assert!(plan.push.is_empty());
    }

    /// Regression guard for the unit-mismatch LWW bug: if local mtime is
    /// compared in ms against server mtime in s, the raw comparison biases
    /// ~1000× toward push. Here both sides' wall-clock is "now-ish"
    /// (~1.76e12 ms / ~1.76e9 s); the local side is strictly *older* than
    /// the server side in wall-clock terms, so the plan must be a pull.
    ///
    /// Before the fix, `1_760_000_000_000 >= 1_760_000_001` collapsed to
    /// "local wins" and the client would overwrite the newer remote.
    #[test]
    fn conflict_lww_does_not_confuse_ms_and_s() {
        let mut manifest = make_manifest();
        manifest.mark_clean("files/doc.md", "hash_v1", 100, 1_760_000_000_000); // ms
        manifest.mark_dirty("files/doc.md");

        let mut local_scan = BTreeMap::new();
        local_scan.insert("files/doc.md".to_string(), local("hash_v2_local", 120));

        // Server mtime is 1 second newer (in seconds). 1_760_000_000 ms
        // < 1_760_000_001 s when compared in seconds.
        let server = vec![server(
            "files/doc.md",
            Some("hash_v2_remote"),
            1_760_000_001,
        )];

        let plan = compute_diff(&manifest, &local_scan, &server, "");
        assert_eq!(
            plan.pull,
            vec!["files/doc.md"],
            "server (newer by 1s) must win; previously lost to unit-mismatch"
        );
        assert!(plan.push.is_empty());
    }

    #[test]
    fn clean_file_missing_from_server_deletes_local() {
        let mut manifest = make_manifest();
        manifest.mark_clean("files/gone.md", "hash", 100, 500);

        let mut local_scan = BTreeMap::new();
        local_scan.insert("files/gone.md".to_string(), local("hash", 100));

        let plan = compute_diff(&manifest, &local_scan, &[], "");
        assert_eq!(plan.delete_local, vec!["files/gone.md"]);
    }

    #[test]
    fn clean_file_gone_from_local_scan_pulls_when_still_on_disk() {
        let mut manifest = make_manifest();
        manifest.mark_clean("files/old-name.md", "hash", 100, 500);

        // File is gone from workspace_file_set (e.g. tree restructured)
        // but still on server.  In production, compute_diff checks
        // file_exists on disk to disambiguate restructuring vs deletion.
        // In tests, file_exists errors and defaults to true (safe pull).
        let local_scan = BTreeMap::new();
        let server = vec![server("files/old-name.md", Some("hash"), 500)];

        let plan = compute_diff(&manifest, &local_scan, &server, "");
        assert!(plan.delete_remote.is_empty());
        assert_eq!(plan.pull, vec!["files/old-name.md"]);
    }

    #[test]
    fn pending_delete_sends_remote_delete() {
        let mut manifest = make_manifest();
        manifest.record_delete("files/deleted.md");

        let local_scan = BTreeMap::new();
        let server = vec![server("files/deleted.md", Some("hash"), 500)];

        let plan = compute_diff(&manifest, &local_scan, &server, "");
        assert_eq!(plan.delete_remote, vec!["files/deleted.md"]);
    }

    #[test]
    fn pull_failed_entry_is_re_pulled_not_pushed() {
        // Regression: after a download write-failure, the file sits on disk
        // with stale content. The dangerous path was (Some(Dirty), Some(se))
        // hashing the stale local bytes and uploading them. Now the failed
        // pull is tracked as PullFailed and compute_diff always plan.pulls
        // it, never plan.pushes.
        let mut manifest = make_manifest();
        manifest.mark_pull_failed("files/stale.md");

        // Local scan sees the old bytes on disk with a hash that differs
        // from the server's authoritative content_hash.
        let mut local_scan = BTreeMap::new();
        local_scan.insert("files/stale.md".to_string(), local("stale_hash", 100));

        let server = vec![server("files/stale.md", Some("fresh_server_hash"), 600)];

        let plan = compute_diff(&manifest, &local_scan, &server, "");
        assert_eq!(
            plan.pull,
            vec!["files/stale.md"],
            "PullFailed entry must be re-pulled"
        );
        assert!(
            plan.push.is_empty(),
            "PullFailed must never push stale local content. push={:?}",
            plan.push
        );
    }

    #[test]
    fn pull_failed_entry_missing_from_server_is_left_alone() {
        // If the server no longer has it, don't push (could corrupt) and
        // don't delete (we never confirmed local was good). Leave it for
        // manual intervention or a future successful pull elsewhere.
        let mut manifest = make_manifest();
        manifest.mark_pull_failed("files/stale.md");

        let mut local_scan = BTreeMap::new();
        local_scan.insert("files/stale.md".to_string(), local("stale_hash", 100));

        let plan = compute_diff(&manifest, &local_scan, &[], "");
        assert!(plan.push.is_empty());
        assert!(plan.pull.is_empty());
        assert!(plan.delete_local.is_empty());
        assert!(plan.delete_remote.is_empty());
    }

    // -- Streaming download: resumability + adaptive sizing --

    fn server_sized(key: &str, hash: Option<&str>, size: u64) -> ServerEntry {
        ServerEntry {
            server_key: key.to_string(),
            key: key.to_string(),
            content_hash: hash.map(String::from),
            size_bytes: size,
            updated_at: 1,
        }
    }

    #[test]
    fn manifest_satisfies_when_clean_and_hash_matches() {
        let mut manifest = make_manifest();
        manifest.mark_clean("files/a.md", "hash_v1", 100, 500);
        let entry = server_sized("files/a.md", Some("hash_v1"), 100);
        assert!(manifest_already_satisfies(&manifest, &entry));
    }

    #[test]
    fn manifest_does_not_satisfy_when_hashes_differ() {
        let mut manifest = make_manifest();
        manifest.mark_clean("files/a.md", "hash_v1", 100, 500);
        let entry = server_sized("files/a.md", Some("hash_v2"), 100);
        assert!(!manifest_already_satisfies(&manifest, &entry));
    }

    #[test]
    fn manifest_does_not_satisfy_when_pull_failed() {
        let mut manifest = make_manifest();
        manifest.mark_pull_failed("files/a.md");
        let entry = server_sized("files/a.md", Some("hash_v1"), 100);
        // PullFailed entries are never satisfied: we must re-pull.
        assert!(!manifest_already_satisfies(&manifest, &entry));
    }

    #[test]
    fn manifest_satisfies_when_server_omits_hash_and_local_is_clean() {
        let mut manifest = make_manifest();
        manifest.mark_clean("files/legacy.md", "hash_v1", 100, 500);
        // Legacy server rows may not report content_hash; fall back to
        // trusting the manifest rather than re-pulling redundantly.
        let entry = server_sized("files/legacy.md", None, 100);
        assert!(manifest_already_satisfies(&manifest, &entry));
    }

    #[test]
    fn adaptive_state_clamps_to_bounds_after_load_corruption() {
        // Mirror the clamp the streaming download applies on entry.
        let mut s = AdaptiveState {
            batch_size: 10_000,
            concurrency: 99,
        };
        s.batch_size = s.batch_size.clamp(MIN_BATCH_SIZE, MAX_BATCH_SIZE);
        s.concurrency = s.concurrency.clamp(MIN_CONCURRENCY, MAX_CONCURRENCY);
        assert_eq!(s.batch_size, MAX_BATCH_SIZE);
        assert_eq!(s.concurrency, MAX_CONCURRENCY);

        let mut s = AdaptiveState {
            batch_size: 0,
            concurrency: 0,
        };
        s.batch_size = s.batch_size.clamp(MIN_BATCH_SIZE, MAX_BATCH_SIZE);
        s.concurrency = s.concurrency.clamp(MIN_CONCURRENCY, MAX_CONCURRENCY);
        assert_eq!(s.batch_size, MIN_BATCH_SIZE);
        assert_eq!(s.concurrency, MIN_CONCURRENCY);
    }

    #[test]
    fn adapt_after_wave_ramps_up_on_fast_clean_run() {
        let mut s = AdaptiveState {
            batch_size: 200,
            concurrency: 4,
        };
        adapt_after_wave(&mut s, 1_000_000, 1_000, 0);
        assert!(s.batch_size > 200, "batch_size should grow on fast wave");
        assert_eq!(s.concurrency, 5);
    }

    #[test]
    fn adapt_after_wave_backs_off_on_errors() {
        let mut s = AdaptiveState {
            batch_size: 200,
            concurrency: 4,
        };
        adapt_after_wave(&mut s, 1_000_000, 1_000, 3);
        assert_eq!(s.batch_size, 100);
        assert_eq!(s.concurrency, 3);
    }

    #[test]
    fn adapt_after_wave_backs_off_on_slow_run_even_without_errors() {
        let mut s = AdaptiveState {
            batch_size: 400,
            concurrency: 4,
        };
        adapt_after_wave(&mut s, 1_000_000, ADAPTIVE_SLOW_MS + 1, 0);
        assert_eq!(s.batch_size, 200);
        assert_eq!(s.concurrency, 3);
    }

    #[test]
    fn adapt_after_wave_respects_minimum_batch_size() {
        let mut s = AdaptiveState {
            batch_size: MIN_BATCH_SIZE,
            concurrency: MIN_CONCURRENCY,
        };
        adapt_after_wave(&mut s, 1_000, 1_000, 1);
        assert_eq!(s.batch_size, MIN_BATCH_SIZE);
        assert_eq!(s.concurrency, MIN_CONCURRENCY);
    }

    #[test]
    fn stream_percent_stays_in_bounds() {
        assert_eq!(stream_percent(0, 0), 15);
        assert_eq!(stream_percent(0, 100), 15);
        assert_eq!(stream_percent(50, 100), 55);
        let p = stream_percent(100, 100);
        assert!((90..=95).contains(&p), "saturates near 95, got {p}");
        // Doesn't underflow on weird inputs (pulled > total).
        let p = stream_percent(200, 100);
        assert!((90..=95).contains(&p), "clamps when overshooting, got {p}");
    }
}
