//! Workspace storage and snapshot operations.
//!
//! This module provides:
//! - `StorageCache`: shared cache of per-workspace `SqliteStorage` connections
//! - `WorkspaceStore`: snapshot export/import and file queries for HTTP API handlers

use diaryx_core::crdt::{
    BodyDocManager, SqliteStorage, WorkspaceCrdt, materialize_workspace, parse_snapshot_markdown,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;
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
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::Zip(e) => write!(f, "Zip I/O error: {}", e),
            SnapshotError::Json(e) => write!(f, "JSON error: {}", e),
            SnapshotError::Storage(e) => write!(f, "Storage error: {}", e),
            SnapshotError::Parse(e) => write!(f, "Parse error: {}", e),
            SnapshotError::ZipFormat(e) => write!(f, "Zip format error: {}", e),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum SnapshotImportMode {
    Replace,
    Merge,
}

#[derive(Debug, Serialize)]
pub struct SnapshotImportResult {
    pub files_imported: usize,
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

    /// Export a workspace snapshot as a zip archive (markdown only).
    pub fn export_snapshot_zip(&self, workspace_id: &str) -> Result<Vec<u8>, SnapshotError> {
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
        let options = FileOptions::<()>::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);

        for file in result.files {
            zip.start_file(file.path.replace('\\', "/"), options)?;
            zip.write_all(file.content.as_bytes())?;
        }

        let cursor = zip.finish()?;
        Ok(cursor.into_inner())
    }

    /// Import a workspace snapshot zip into the CRDT store.
    pub fn import_snapshot_zip(
        &self,
        workspace_id: &str,
        bytes: &[u8],
        mode: SnapshotImportMode,
    ) -> Result<SnapshotImportResult, SnapshotError> {
        let storage = self
            .storage_cache
            .get_storage(workspace_id)
            .map_err(SnapshotError::Storage)?;

        let workspace_doc_name = format!("workspace:{}", workspace_id);
        let workspace = WorkspaceCrdt::load_with_name(storage.clone(), workspace_doc_name)
            .map_err(|e| SnapshotError::Storage(e.to_string()))?;
        let body_docs = BodyDocManager::new(storage.clone());

        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;

        if mode == SnapshotImportMode::Replace {
            // Clear file_index for v2 file manifest
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

        let mut files_imported = 0usize;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            if entry.is_dir() {
                continue;
            }

            let name = entry.name().to_string();
            if !name.ends_with(".md") {
                continue;
            }

            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .map_err(|e| SnapshotError::Parse(format!("Failed reading {}: {}", name, e)))?;

            let (metadata, body) =
                parse_snapshot_markdown(&name, &content).map_err(SnapshotError::Parse)?;

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

        info!(
            "Imported {} files into workspace {} (mode: {:?})",
            files_imported, workspace_id, mode
        );

        Ok(SnapshotImportResult { files_imported })
    }
}
