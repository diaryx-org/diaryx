//! Workspace storage and snapshot operations.
//!
//! This module provides:
//! - `StorageCache`: shared cache of per-workspace `SqliteStorage` connections
//! - `WorkspaceStore`: snapshot export/import and file queries for HTTP API handlers

use crate::blob_store::BlobStore;
use crate::db::{AuthRepo, WorkspaceAttachmentRefRecord};
use chrono::Utc;
use diaryx_core::crdt::{
    BodyDocManager, SqliteStorage, WorkspaceCrdt, materialize_workspace, parse_snapshot_markdown,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use tracing::{info, warn};
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

// ==================== StorageCache ====================

/// Shared cache of per-workspace `SqliteStorage` connections.
///
/// Used by both `DiaryxHook` (for sync persistence) and `WorkspaceStore`
/// (for HTTP API snapshot operations) to avoid duplicate connections.
pub struct StorageCache {
    workspaces_dir: PathBuf,
    cache: RwLock<HashMap<String, Arc<SqliteStorage>>>,
}

impl StorageCache {
    pub fn new(workspaces_dir: PathBuf) -> Self {
        Self {
            workspaces_dir,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Get the path where the bare git repo for a workspace lives.
    pub fn git_repo_path(&self, workspace_id: &str) -> PathBuf {
        self.workspaces_dir.join(format!("{}.git", workspace_id))
    }

    /// Get or create storage for a workspace.
    pub fn get_storage(&self, workspace_id: &str) -> Result<Arc<SqliteStorage>, String> {
        // Check cache first
        {
            let cache = self.cache.read().unwrap();
            if let Some(storage) = cache.get(workspace_id) {
                return Ok(storage.clone());
            }
        }

        // Create new storage
        let db_path = self.workspaces_dir.join(format!("{}.db", workspace_id));
        let storage = SqliteStorage::open(&db_path)
            .map_err(|e| format!("Failed to open storage for {}: {}", workspace_id, e))?;
        let storage = Arc::new(storage);

        // Cache it
        {
            let mut cache = self.cache.write().unwrap();
            cache.insert(workspace_id.to_string(), storage.clone());
        }

        Ok(storage)
    }
}

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
                    if let Some(path) = normalize_workspace_path(&attachment.path)
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
                let Some(normalized_attachment_path) = normalize_workspace_path(&attachment.path)
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

        let mut refs: Vec<WorkspaceAttachmentRefRecord> = Vec::new();
        for file in result.files {
            let Some(file_path) = normalize_workspace_path(&file.path) else {
                continue;
            };
            for attachment in file.metadata.attachments {
                if attachment.deleted || attachment.hash.is_empty() {
                    continue;
                }
                let Some(attachment_path) = normalize_workspace_path(&attachment.path) else {
                    continue;
                };
                refs.push(WorkspaceAttachmentRefRecord {
                    file_path: file_path.clone(),
                    attachment_path,
                    blob_hash: attachment.hash,
                    size_bytes: attachment.size,
                    mime_type: if attachment.mime_type.is_empty() {
                        "application/octet-stream".to_string()
                    } else {
                        attachment.mime_type
                    },
                });
            }
        }

        repo.replace_workspace_attachment_refs(workspace_id, &refs)?;
        Ok(refs.len())
    }
}
