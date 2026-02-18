//! Workspace storage and snapshot operations.
//!
//! This module provides:
//! - `WorkspaceStore`: snapshot export/import and file queries for HTTP API handlers
//!
//! `StorageCache` is re-exported from `diaryx_sync::storage`.

use crate::blob_store::BlobStore;
use crate::db::{AuthRepo, CompletedAttachmentUploadInfo, WorkspaceAttachmentRefRecord};
use chrono::Utc;
use diaryx_core::crdt::{
    BodyDocManager, WorkspaceCrdt, materialize_workspace, parse_snapshot_markdown,
};
use diaryx_core::link_parser;
pub use diaryx_sync::storage::StorageCache;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{BufWriter, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

// ==================== Error Types ====================

#[derive(Debug)]
pub enum SnapshotError {
    Zip(std::io::Error),
    Json(serde_json::Error),
    Storage(String),
    Parse(String),
    ZipFormat(zip::result::ZipError),
    Db(rusqlite::Error),
    Blob(String),
    QuotaExceeded {
        used_bytes: u64,
        limit_bytes: u64,
        requested_bytes: u64,
    },
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::Zip(e) => write!(f, "Zip I/O error: {}", e),
            SnapshotError::Json(e) => write!(f, "JSON error: {}", e),
            SnapshotError::Storage(e) => write!(f, "Storage error: {}", e),
            SnapshotError::Parse(e) => write!(f, "Parse error: {}", e),
            SnapshotError::ZipFormat(e) => write!(f, "Zip format error: {}", e),
            SnapshotError::Db(e) => write!(f, "Database error: {}", e),
            SnapshotError::Blob(e) => write!(f, "Blob store error: {}", e),
            SnapshotError::QuotaExceeded {
                used_bytes,
                limit_bytes,
                requested_bytes,
            } => write!(
                f,
                "Attachment quota exceeded (used={}, limit={}, requested={})",
                used_bytes, limit_bytes, requested_bytes
            ),
        }
    }
}

impl From<std::io::Error> for SnapshotError {
    fn from(error: std::io::Error) -> Self {
        SnapshotError::Zip(error)
    }
}

impl From<serde_json::Error> for SnapshotError {
    fn from(error: serde_json::Error) -> Self {
        SnapshotError::Json(error)
    }
}

impl From<diaryx_core::error::DiaryxError> for SnapshotError {
    fn from(error: diaryx_core::error::DiaryxError) -> Self {
        SnapshotError::Storage(error.to_string())
    }
}

impl From<zip::result::ZipError> for SnapshotError {
    fn from(error: zip::result::ZipError) -> Self {
        SnapshotError::ZipFormat(error)
    }
}

impl From<rusqlite::Error> for SnapshotError {
    fn from(error: rusqlite::Error) -> Self {
        SnapshotError::Db(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum SnapshotImportMode {
    Replace,
    Merge,
}

#[derive(Debug, Serialize)]
pub struct SnapshotImportResult {
    pub files_imported: usize,
}

#[derive(Debug, Clone)]
struct UploadedBlob {
    hash: String,
    size_bytes: u64,
    mime_type: String,
}

#[derive(Debug, Clone)]
struct SnapshotBinaryEntry {
    bytes: Vec<u8>,
    hash: String,
    mime_type: String,
    key: String,
}

fn normalize_workspace_path(path: &str) -> Option<String> {
    let mut normalized = PathBuf::new();

    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    if normalized.as_os_str().is_empty() {
        None
    } else {
        Some(normalized.to_string_lossy().replace('\\', "/"))
    }
}

fn normalize_attachment_path(file_path: &str, raw_attachment_path: &str) -> Option<String> {
    fn normalize_attachment_path_once(
        file_path: &str,
        raw_attachment_path: &str,
    ) -> Option<String> {
        let parsed = link_parser::parse_link(raw_attachment_path);
        let current_file = Path::new(file_path);

        let canonical = if parsed.path_type == link_parser::PathType::Ambiguous {
            let current_dir = current_file
                .parent()
                .and_then(|parent| parent.to_str())
                .unwrap_or("");
            let plain_path_looks_canonical = !current_dir.is_empty()
                && parsed.path.starts_with(current_dir)
                && parsed
                    .path
                    .as_bytes()
                    .get(current_dir.len())
                    .is_some_and(|ch| *ch == b'/');

            if plain_path_looks_canonical {
                link_parser::to_canonical_with_link_format(
                    &parsed,
                    current_file,
                    Some(link_parser::LinkFormat::PlainCanonical),
                )
            } else {
                link_parser::to_canonical(&parsed, current_file)
            }
        } else {
            link_parser::to_canonical(&parsed, current_file)
        };

        normalize_workspace_path(&canonical)
    }

    let trimmed = raw_attachment_path.trim();
    if trimmed.starts_with('[') && trimmed.contains("](") && !trimmed.ends_with(')') {
        if let Some(normalized) =
            normalize_attachment_path_once(file_path, &format!("{})", trimmed))
        {
            return Some(normalized);
        }
    }

    normalize_attachment_path_once(file_path, trimmed)
}

// It's only because of this function that attachment tracking works server-side
// (And the `refs.push()` in the `sha256_hex` function)
// because `file.metadata.attachments` is always empty even when
// the markdown contains attachments and the upload is completed
fn extract_attachment_paths_from_markdown(file_path: &str, content: &str) -> Vec<String> {
    fn find_closing_paren(s: &str) -> Option<usize> {
        let mut depth = 0;
        for (i, c) in s.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    if depth == 0 {
                        return Some(i);
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        None
    }

    let mut out = HashSet::new();
    let mut cursor = 0usize;
    while let Some(rel) = content[cursor..].find("](") {
        let start = cursor + rel + 2;
        let rest = &content[start..];
        let Some(close) = find_closing_paren(rest) else {
            break;
        };
        let raw = rest[..close].trim();
        let raw_unwrapped = raw
            .strip_prefix('<')
            .and_then(|s| s.strip_suffix('>'))
            .unwrap_or(raw);
        if raw_unwrapped.contains("_attachments")
            && let Some(normalized) = normalize_attachment_path(file_path, raw_unwrapped)
        {
            out.insert(normalized);
        }
        cursor = start + close + 1;
    }

    out.into_iter().collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn mime_for_path(path: &str) -> String {
    mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

// StorageCache is re-exported from diaryx_sync::storage (see pub use above)

// ==================== WorkspaceStore ====================

/// Provides snapshot export/import and file queries for HTTP API handlers.
///
/// This is a stateless wrapper around `StorageCache` that operates directly
/// on `SqliteStorage`, `WorkspaceCrdt`, and `BodyDocManager` without needing
/// broadcast channels or connection tracking.
pub struct WorkspaceStore {
    storage_cache: Arc<StorageCache>,
}

impl WorkspaceStore {
    pub fn new(storage_cache: Arc<StorageCache>) -> Self {
        Self { storage_cache }
    }

    /// Get file count for a workspace (non-deleted files).
    pub fn get_file_count(&self, workspace_id: &str) -> usize {
        let storage = match self.storage_cache.get_storage(workspace_id) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        let workspace_doc_name = format!("workspace:{}", workspace_id);
        let workspace = match WorkspaceCrdt::load_with_name(storage, workspace_doc_name) {
            Ok(w) => w,
            Err(_) => return 0,
        };

        workspace.file_count()
    }

    /// Export a workspace snapshot as a zip archive.
    ///
    /// Markdown files are always included. Attachments are included when
    /// `include_attachments` is true and `BinaryRef.hash` is available.
    pub async fn export_snapshot_zip(
        &self,
        workspace_id: &str,
        user_id: &str,
        include_attachments: bool,
        blob_store: &dyn BlobStore,
    ) -> Result<Vec<u8>, SnapshotError> {
        let storage = self
            .storage_cache
            .get_storage(workspace_id)
            .map_err(SnapshotError::Storage)?;

        let workspace_doc_name = format!("workspace:{}", workspace_id);
        let workspace = WorkspaceCrdt::load_with_name(storage.clone(), workspace_doc_name)
            .map_err(|e| SnapshotError::Storage(e.to_string()))?;
        let body_docs = BodyDocManager::new(storage);

        let result = materialize_workspace(&workspace, &body_docs, workspace_id);

        for skipped in &result.skipped {
            if skipped.reason == diaryx_core::crdt::materialize::SkipReason::UnresolvedPath {
                warn!(
                    "Snapshot export: skipping unresolved path for {}",
                    skipped.key
                );
            }
        }

        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        let markdown_options = FileOptions::<()>::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);
        let binary_options = FileOptions::<()>::default()
            .compression_method(CompressionMethod::Stored)
            .unix_permissions(0o644);

        let mut unique_attachments: HashSet<String> = HashSet::new();
        let mut attachment_requests: Vec<(String, String)> = Vec::new();

        for file in result.files {
            let normalized = normalize_workspace_path(&file.path).ok_or_else(|| {
                SnapshotError::Parse(format!("Invalid file path while exporting: {}", file.path))
            })?;
            zip.start_file(normalized.clone(), markdown_options)?;
            zip.write_all(file.content.as_bytes())?;

            if include_attachments {
                for attachment in file.metadata.attachments {
                    if attachment.deleted || attachment.hash.is_empty() {
                        continue;
                    }
                    if let Some(path) = normalize_attachment_path(&normalized, &attachment.path)
                        && unique_attachments.insert(path.clone())
                    {
                        attachment_requests.push((path, attachment.hash));
                    }
                }
            }
        }

        if include_attachments {
            for (attachment_path, hash) in attachment_requests {
                let key = blob_store.blob_key(user_id, &hash);
                let bytes = blob_store.get(&key).await.map_err(SnapshotError::Blob)?;

                match bytes {
                    Some(payload) => {
                        zip.start_file(attachment_path, binary_options)?;
                        zip.write_all(&payload)?;
                    }
                    None => {
                        warn!(
                            "Snapshot export: missing blob for hash {} (workspace {})",
                            hash, workspace_id
                        );
                    }
                }
            }
        }

        let cursor = zip.finish()?;
        Ok(cursor.into_inner())
    }

    /// Export the workspace as a zip, writing to a temporary file instead of RAM.
    ///
    /// Returns a `NamedTempFile` whose file descriptor can be streamed.
    /// On Unix the file is unlinked on drop but the open fd keeps it readable.
    pub async fn export_snapshot_zip_to_file(
        &self,
        workspace_id: &str,
        user_id: &str,
        include_attachments: bool,
        blob_store: &dyn BlobStore,
    ) -> Result<tempfile::NamedTempFile, SnapshotError> {
        let storage = self
            .storage_cache
            .get_storage(workspace_id)
            .map_err(SnapshotError::Storage)?;

        let workspace_doc_name = format!("workspace:{}", workspace_id);
        let workspace = WorkspaceCrdt::load_with_name(storage.clone(), workspace_doc_name)
            .map_err(|e| SnapshotError::Storage(e.to_string()))?;
        let body_docs = BodyDocManager::new(storage);

        let result = materialize_workspace(&workspace, &body_docs, workspace_id);

        for skipped in &result.skipped {
            if skipped.reason == diaryx_core::crdt::materialize::SkipReason::UnresolvedPath {
                warn!(
                    "Snapshot export: skipping unresolved path for {}",
                    skipped.key
                );
            }
        }

        let temp_file = tempfile::NamedTempFile::new().map_err(SnapshotError::Zip)?;
        let buf_writer = BufWriter::new(temp_file.reopen().map_err(SnapshotError::Zip)?);
        let mut zip = ZipWriter::new(buf_writer);
        let markdown_options = FileOptions::<()>::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);
        let binary_options = FileOptions::<()>::default()
            .compression_method(CompressionMethod::Stored)
            .unix_permissions(0o644);

        let mut unique_attachments: HashSet<String> = HashSet::new();
        let mut attachment_requests: Vec<(String, String)> = Vec::new();

        for file in result.files {
            let normalized = normalize_workspace_path(&file.path).ok_or_else(|| {
                SnapshotError::Parse(format!("Invalid file path while exporting: {}", file.path))
            })?;
            zip.start_file(normalized.clone(), markdown_options)?;
            zip.write_all(file.content.as_bytes())?;

            if include_attachments {
                for attachment in file.metadata.attachments {
                    if attachment.deleted || attachment.hash.is_empty() {
                        continue;
                    }
                    if let Some(path) = normalize_attachment_path(&normalized, &attachment.path)
                        && unique_attachments.insert(path.clone())
                    {
                        attachment_requests.push((path, attachment.hash));
                    }
                }
            }
        }

        if include_attachments {
            for (attachment_path, hash) in attachment_requests {
                let key = blob_store.blob_key(user_id, &hash);
                let bytes = blob_store.get(&key).await.map_err(SnapshotError::Blob)?;

                match bytes {
                    Some(payload) => {
                        zip.start_file(attachment_path, binary_options)?;
                        zip.write_all(&payload)?;
                    }
                    None => {
                        warn!(
                            "Snapshot export: missing blob for hash {} (workspace {})",
                            hash, workspace_id
                        );
                    }
                }
            }
        }

        zip.finish()?;
        Ok(temp_file)
    }

    /// Import a workspace snapshot zip into the CRDT store.
    ///
    /// If `include_attachments` is true, binary entries are uploaded to blob
    /// storage and attachment metadata is patched with hash/size/mime.
    pub async fn import_snapshot_zip_from_path(
        &self,
        workspace_id: &str,
        user_id: &str,
        zip_path: &Path,
        mode: SnapshotImportMode,
        include_attachments: bool,
        repo: &AuthRepo,
        blob_store: &dyn BlobStore,
    ) -> Result<SnapshotImportResult, SnapshotError> {
        let storage = self
            .storage_cache
            .get_storage(workspace_id)
            .map_err(SnapshotError::Storage)?;

        let workspace_doc_name = format!("workspace:{}", workspace_id);
        let workspace = WorkspaceCrdt::load_with_name(storage.clone(), workspace_doc_name)
            .map_err(|e| SnapshotError::Storage(e.to_string()))?;
        let body_docs = BodyDocManager::new(storage.clone());

        let file = std::fs::File::open(zip_path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        if mode == SnapshotImportMode::Replace {
            storage.clear_file_index()?;

            let existing: Vec<String> = workspace
                .list_files()
                .into_iter()
                .map(|(path, _)| path)
                .collect();
            for path in existing {
                let _ = workspace.delete_file(&path);
            }
        }

        let mut blobs_by_path: HashMap<String, UploadedBlob> = HashMap::new();
        let mut binary_entries: Vec<SnapshotBinaryEntry> = Vec::new();

        if include_attachments {
            let mut unique_hash_sizes: HashMap<String, u64> = HashMap::new();
            for i in 0..archive.len() {
                let Some((name, bytes, hash, mime_type, key)) = ({
                    let mut entry = archive.by_index(i)?;
                    if entry.is_dir() {
                        None
                    } else {
                        let Some(name) = normalize_workspace_path(entry.name()) else {
                            continue;
                        };

                        if name.ends_with(".md") {
                            None
                        } else {
                            let mut bytes = Vec::new();
                            entry.read_to_end(&mut bytes)?;
                            let hash = sha256_hex(&bytes);
                            let mime_type = mime_for_path(&name);
                            let key = blob_store.blob_key(user_id, &hash);
                            Some((name, bytes, hash, mime_type, key))
                        }
                    }
                }) else {
                    continue;
                };

                let bytes_len = bytes.len() as u64;
                unique_hash_sizes.entry(hash.clone()).or_insert(bytes_len);
                binary_entries.push(SnapshotBinaryEntry {
                    bytes,
                    hash: hash.clone(),
                    mime_type: mime_type.clone(),
                    key: key.clone(),
                });

                blobs_by_path.insert(
                    name,
                    UploadedBlob {
                        hash,
                        size_bytes: bytes_len,
                        mime_type,
                    },
                );
            }

            let used_bytes = repo.get_user_storage_usage(user_id)?.used_bytes;
            let limit_bytes = repo.get_effective_user_attachment_limit(user_id)?;
            let mut net_new_bytes = 0u64;
            for (hash, size) in unique_hash_sizes {
                if repo.get_user_blob(user_id, &hash)?.is_none() {
                    net_new_bytes = net_new_bytes.saturating_add(size);
                }
            }
            let projected = used_bytes.saturating_add(net_new_bytes);
            if projected > limit_bytes {
                return Err(SnapshotError::QuotaExceeded {
                    used_bytes,
                    limit_bytes,
                    requested_bytes: net_new_bytes,
                });
            }

            let mut uploaded_hashes = HashSet::new();
            for entry in &binary_entries {
                if !uploaded_hashes.insert(entry.hash.clone()) {
                    continue;
                }
                // Avoid HEAD/exists checks against R2 here. Some bucket policies
                // allow PutObject but reject HeadObject, which would fail imports.
                // Since keys are content-addressed by SHA-256, put is idempotent.
                blob_store
                    .put(&entry.key, &entry.bytes, &entry.mime_type)
                    .await
                    .map_err(SnapshotError::Blob)?;

                repo.upsert_blob(
                    user_id,
                    &entry.hash,
                    &entry.key,
                    entry.bytes.len() as u64,
                    &entry.mime_type,
                )?;
            }
        }

        let mut files_imported = 0usize;
        let mut workspace_refs: Vec<WorkspaceAttachmentRefRecord> = Vec::new();

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            if entry.is_dir() {
                continue;
            }

            let name = match normalize_workspace_path(entry.name()) {
                Some(v) => v,
                None => continue,
            };

            if !name.ends_with(".md") {
                continue;
            }

            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .map_err(|e| SnapshotError::Parse(format!("Failed reading {}: {}", name, e)))?;

            let (mut metadata, body) =
                parse_snapshot_markdown(&name, &content).map_err(SnapshotError::Parse)?;

            let now = Utc::now().timestamp_millis();
            for attachment in &mut metadata.attachments {
                let Some(normalized_attachment_path) =
                    normalize_attachment_path(&name, &attachment.path)
                else {
                    continue;
                };
                attachment.path = normalized_attachment_path.clone();

                if include_attachments
                    && let Some(blob) = blobs_by_path.get(&normalized_attachment_path)
                {
                    attachment.hash = blob.hash.clone();
                    attachment.size = blob.size_bytes;
                    attachment.mime_type = blob.mime_type.clone();
                    attachment.uploaded_at = Some(now);
                }

                if !attachment.hash.is_empty() {
                    workspace_refs.push(WorkspaceAttachmentRefRecord {
                        file_path: name.clone(),
                        attachment_path: normalized_attachment_path,
                        blob_hash: attachment.hash.clone(),
                        size_bytes: attachment.size,
                        mime_type: if attachment.mime_type.is_empty() {
                            "application/octet-stream".to_string()
                        } else {
                            attachment.mime_type.clone()
                        },
                    });
                }
            }

            workspace.set_file(&name, metadata.clone())?;
            let body_key = format!("body:{}/{}", workspace_id, name);
            body_docs.get_or_create(&body_key).set_body(&body)?;

            // Populate file_index for v2 file manifest handshake
            storage.update_file_index(
                &name,
                metadata.title.as_deref(),
                metadata.part_of.as_deref(),
                metadata.deleted,
                metadata.modified_at,
            )?;

            files_imported += 1;
        }

        if include_attachments {
            repo.replace_workspace_attachment_refs(workspace_id, &workspace_refs)?;
        }

        info!(
            "Imported {} files into workspace {} (mode: {:?}, include_attachments: {})",
            files_imported, workspace_id, mode, include_attachments
        );

        Ok(SnapshotImportResult { files_imported })
    }

    /// Reconcile workspace attachment references from current workspace metadata.
    ///
    /// This is used for ongoing incremental sync so usage/ref counts stay accurate
    /// without requiring snapshot imports.
    pub fn reconcile_workspace_attachment_refs(
        &self,
        workspace_id: &str,
        repo: &AuthRepo,
    ) -> Result<usize, SnapshotError> {
        let storage = self
            .storage_cache
            .get_storage(workspace_id)
            .map_err(SnapshotError::Storage)?;
        let workspace_doc_name = format!("workspace:{}", workspace_id);
        let workspace = WorkspaceCrdt::load_with_name(storage.clone(), workspace_doc_name)
            .map_err(|e| SnapshotError::Storage(e.to_string()))?;

        let body_docs = BodyDocManager::new(storage);
        let result = materialize_workspace(&workspace, &body_docs, workspace_id);

        debug!(
            "Reconciling attachments for workspace {}: {} files materialized, {} skipped",
            workspace_id,
            result.files.len(),
            result.skipped.len()
        );

        let mut refs: Vec<WorkspaceAttachmentRefRecord> = Vec::new();
        for file in result.files {
            let Some(file_path) = normalize_workspace_path(&file.path) else {
                continue;
            };
            let mut file_has_refs = false;
            if !file.metadata.attachments.is_empty() {
                debug!(
                    "File '{}' has {} attachment refs: {:?}",
                    file_path,
                    file.metadata.attachments.len(),
                    file.metadata
                        .attachments
                        .iter()
                        .map(|a| format!(
                            "path={}, hash={}, deleted={}",
                            a.path,
                            if a.hash.is_empty() {
                                "<empty>"
                            } else {
                                &a.hash
                            },
                            a.deleted
                        ))
                        .collect::<Vec<_>>()
                );
            }
            for attachment in file.metadata.attachments {
                if attachment.deleted {
                    continue;
                }
                let Some(attachment_path) = normalize_attachment_path(&file_path, &attachment.path)
                else {
                    debug!(
                        "Skipping attachment with unnormalizable path: file={}, attachment={}",
                        file_path, attachment.path
                    );
                    continue;
                };
                let fallback = if attachment.hash.is_empty() {
                    repo.get_latest_completed_attachment_upload(workspace_id, &attachment_path)?
                } else {
                    None
                };
                let CompletedAttachmentUploadInfo {
                    blob_hash,
                    size_bytes,
                    mime_type,
                } = match fallback {
                    Some(upload) => upload,
                    None => CompletedAttachmentUploadInfo {
                        blob_hash: attachment.hash,
                        size_bytes: attachment.size,
                        mime_type: attachment.mime_type,
                    },
                };
                if blob_hash.is_empty() {
                    debug!(
                        "Skipping attachment with empty hash (no fallback): file={}, attachment={}",
                        file_path, attachment_path
                    );
                    continue;
                }
                refs.push(WorkspaceAttachmentRefRecord {
                    file_path: file_path.clone(),
                    attachment_path,
                    blob_hash,
                    size_bytes,
                    mime_type: if mime_type.is_empty() {
                        "application/octet-stream".to_string()
                    } else {
                        mime_type
                    },
                });
                file_has_refs = true;
            }

            if !file_has_refs {
                let candidates = extract_attachment_paths_from_markdown(&file_path, &file.content);
                if !candidates.is_empty() {
                    debug!(
                        "File '{}' has {} markdown-derived attachment candidates",
                        file_path,
                        candidates.len()
                    );
                }
                for attachment_path in candidates {
                    let Some(CompletedAttachmentUploadInfo {
                        blob_hash,
                        size_bytes,
                        mime_type,
                    }) = repo
                        .get_latest_completed_attachment_upload(workspace_id, &attachment_path)?
                    else {
                        continue;
                    };
                    if blob_hash.is_empty() {
                        continue;
                    }
                    refs.push(WorkspaceAttachmentRefRecord {
                        file_path: file_path.clone(),
                        attachment_path,
                        blob_hash,
                        size_bytes,
                        mime_type: if mime_type.is_empty() {
                            "application/octet-stream".to_string()
                        } else {
                            mime_type
                        },
                    });
                }
            }
        }

        repo.replace_workspace_attachment_refs(workspace_id, &refs)?;
        Ok(refs.len())
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_attachment_paths_from_markdown, normalize_attachment_path};

    #[test]
    fn normalize_attachment_path_handles_markdown_root_links() {
        let normalized = normalize_attachment_path(
            "my-journal.md",
            "[diaryx-icon.jpg](/_attachments/diaryx-icon.jpg)",
        );
        assert_eq!(normalized.as_deref(), Some("_attachments/diaryx-icon.jpg"));
    }

    #[test]
    fn normalize_attachment_path_handles_relative_and_canonical_paths() {
        let relative = normalize_attachment_path("notes/day.md", "_attachments/icon.jpg");
        assert_eq!(relative.as_deref(), Some("notes/_attachments/icon.jpg"));

        let canonical = normalize_attachment_path("notes/day.md", "notes/_attachments/icon.jpg");
        assert_eq!(canonical.as_deref(), Some("notes/_attachments/icon.jpg"));
    }

    #[test]
    fn extract_attachment_paths_from_markdown_finds_image_links() {
        let content = "# Title\n\n![one](_attachments/a.png)\ntext [two](<_attachments/b c.jpg>)\n";
        let mut paths = extract_attachment_paths_from_markdown("README.md", content);
        paths.sort();
        assert_eq!(
            paths,
            vec![
                "_attachments/a.png".to_string(),
                "_attachments/b c.jpg".to_string()
            ]
        );
    }
}
