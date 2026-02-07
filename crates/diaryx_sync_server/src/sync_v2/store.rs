//! Workspace storage and snapshot operations.
//!
//! This module provides:
//! - `StorageCache`: shared cache of per-workspace `SqliteStorage` connections
//! - `WorkspaceStore`: snapshot export/import and file queries for HTTP API handlers

use diaryx_core::crdt::{BodyDocManager, FileMetadata, SqliteStorage, WorkspaceCrdt};
use diaryx_core::metadata_writer::FrontmatterMetadata;
use diaryx_core::{frontmatter, link_parser};
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

        let files = workspace.list_files();

        let mut id_to_path: HashMap<String, String> = HashMap::new();
        for (key, _meta) in &files {
            if key.contains('/') || key.ends_with(".md") {
                id_to_path.insert(key.clone(), key.clone());
            } else if let Some(path) = workspace.get_path(key) {
                id_to_path.insert(key.clone(), path.to_string_lossy().to_string());
            }
        }

        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        let options = FileOptions::<()>::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);

        for (key, meta) in files {
            if meta.deleted {
                continue;
            }

            let path = match Self::resolve_snapshot_path(&key, &id_to_path) {
                Some(path) => path,
                None => {
                    warn!("Snapshot export: skipping unresolved path for {}", key);
                    continue;
                }
            };

            let mut export_meta = meta.clone();
            export_meta.part_of = export_meta
                .part_of
                .and_then(|value| Self::resolve_snapshot_path(&value, &id_to_path));

            if let Some(contents) = export_meta.contents.take() {
                let resolved: Vec<String> = contents
                    .into_iter()
                    .filter_map(|value| Self::resolve_snapshot_path(&value, &id_to_path))
                    .collect();
                export_meta.contents = Some(resolved);
            }

            let metadata_json = serde_json::to_value(&export_meta)?;
            let fm = FrontmatterMetadata::from_json_with_file_path(&metadata_json, Some(&path));
            let yaml = fm.to_yaml();

            let body_key = format!("body:{}/{}", workspace_id, path);
            let mut body = body_docs.get_or_create(&body_key).get_body();
            if body.is_empty() && key != path {
                let alt_key = format!("body:{}/{}", workspace_id, key);
                body = body_docs.get_or_create(&alt_key).get_body();
            }
            let content = if yaml.is_empty() {
                body
            } else {
                format!("---\n{}\n---\n\n{}", yaml, body)
            };

            zip.start_file(path.replace('\\', "/"), options)?;
            zip.write_all(content.as_bytes())?;
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

            let (metadata, body) = Self::parse_snapshot_markdown(&name, &content)?;

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

    // ==================== Private Helpers ====================

    fn resolve_snapshot_path(value: &str, id_to_path: &HashMap<String, String>) -> Option<String> {
        if value.contains('/') || value.ends_with(".md") {
            Some(value.to_string())
        } else {
            id_to_path.get(value).cloned()
        }
    }

    fn parse_updated_value(value: &serde_yaml::Value) -> Option<i64> {
        if let Some(num) = value.as_i64() {
            return Some(num);
        }
        if let Some(num) = value.as_f64() {
            return Some(num as i64);
        }
        if let Some(raw) = value.as_str() {
            if let Ok(num) = raw.parse::<i64>() {
                return Some(num);
            }
            if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(raw) {
                return Some(parsed.timestamp_millis());
            }
        }
        None
    }

    fn parse_snapshot_markdown(
        path: &str,
        content: &str,
    ) -> Result<(FileMetadata, String), SnapshotError> {
        let parsed = frontmatter::parse_or_empty(content)
            .map_err(|e| SnapshotError::Parse(e.to_string()))?;
        let fm = &parsed.frontmatter;
        let body = parsed.body;
        let file_path = std::path::Path::new(path);

        let filename = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let part_of = fm.get("part_of").and_then(|v| v.as_str()).map(|raw| {
            let parsed = link_parser::parse_link(raw);
            link_parser::to_canonical(&parsed, file_path)
        });

        let contents = fm.get("contents").and_then(|v| {
            v.as_sequence().map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str())
                    .map(|raw| {
                        let parsed = link_parser::parse_link(raw);
                        link_parser::to_canonical(&parsed, file_path)
                    })
                    .collect::<Vec<String>>()
            })
        });

        let audience = fm.get("audience").and_then(|v| {
            v.as_sequence().map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<String>>()
            })
        });

        let description = fm
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);

        let attachments = fm
            .get("attachments")
            .and_then(|v| {
                v.as_sequence().map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str())
                        .map(|raw| {
                            let parsed = link_parser::parse_link(raw);
                            let canonical = link_parser::to_canonical(&parsed, file_path);
                            diaryx_core::crdt::BinaryRef {
                                path: canonical,
                                source: "local".to_string(),
                                hash: String::new(),
                                mime_type: String::new(),
                                size: 0,
                                uploaded_at: None,
                                deleted: false,
                            }
                        })
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_default();

        let modified_at = fm
            .get("updated")
            .and_then(Self::parse_updated_value)
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

        let metadata = FileMetadata {
            filename,
            title: fm.get("title").and_then(|v| v.as_str()).map(String::from),
            part_of,
            contents,
            attachments,
            deleted: fm.get("deleted").and_then(|v| v.as_bool()).unwrap_or(false),
            audience,
            description,
            extra: HashMap::new(),
            modified_at,
        };

        Ok((metadata, body))
    }
}
