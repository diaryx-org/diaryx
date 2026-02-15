//! CRDT-updating filesystem decorator.
//!
//! This module provides [`CrdtFs`], a decorator that automatically updates the
//! workspace CRDT when filesystem operations occur. This ensures that local file
//! changes are automatically synchronized to the CRDT layer.
//!
//! # Doc-ID Bridge Layer
//!
//! CrdtFs bridges path-based filesystem operations to the doc-ID-based CRDT:
//!
//! ```text
//! Path Operation → CrdtFs → find_by_path() → doc_id → CRDT Update
//!                         ↘ or create_file() ↗
//! ```
//!
//! - For writes: Look up doc_id by path, or create new file with UUID if not found
//! - For renames/moves in doc-ID mode: update `filename`/`part_of` (doc_id is stable)
//! - For renames/moves in legacy path-key mode: use delete+create key migration
//! - For deletes: Mark the file as deleted (tombstone)
//!
//! # Architecture
//!
//! ```text
//! Local Write → CrdtFs.write_file() → Inner FS → Update WorkspaceCrdt
//!                                                       ↓
//!                                              WorkspaceCrdt.observe_updates()
//!                                                       ↓
//!                                              RustSyncBridge (syncs to server)
//! ```
//!
//! # Feature Gate
//!
//! This module requires the `crdt` feature to be enabled.

use std::collections::HashSet;
use std::io::Result;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use crate::crdt::{BodyDocManager, FileMetadata, WorkspaceCrdt};
use crate::frontmatter;
use crate::fs::{AsyncFileSystem, BoxFuture};
use crate::link_parser;
use crate::path_utils::normalize_sync_path;

/// A filesystem decorator that automatically updates the CRDT on file operations.
///
/// This decorator intercepts filesystem writes and updates the workspace CRDT
/// with file metadata extracted from frontmatter. It supports:
///
/// - Automatic CRDT updates on file write/create
/// - Soft deletion (tombstone) on file delete
/// - Path tracking on file move/rename
/// - Runtime enable/disable toggle
///
/// # Example
///
/// ```ignore
/// use diaryx_core::fs::{CrdtFs, InMemoryFileSystem, SyncToAsyncFs};
/// use diaryx_core::crdt::{WorkspaceCrdt, MemoryStorage};
/// use std::sync::Arc;
///
/// let inner_fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
/// let storage = Arc::new(MemoryStorage::new());
/// let workspace_crdt = Arc::new(WorkspaceCrdt::new(storage.clone()));
/// let body_manager = Arc::new(BodyDocManager::new(storage));
///
/// let crdt_fs = CrdtFs::new(inner_fs, workspace_crdt, body_manager);
///
/// // All writes now automatically update the CRDT
/// crdt_fs.write_file(Path::new("test.md"), "---\ntitle: Test\n---\nContent").await?;
/// ```
pub struct CrdtFs<FS: AsyncFileSystem> {
    /// The underlying filesystem.
    inner: FS,
    /// The workspace CRDT for file metadata.
    workspace_crdt: Arc<WorkspaceCrdt>,
    /// Manager for per-file body documents.
    body_doc_manager: Arc<BodyDocManager>,
    /// Whether CRDT updates are enabled.
    enabled: Arc<AtomicBool>,
    /// Paths currently being written locally (for loop prevention).
    local_writes_in_progress: RwLock<HashSet<PathBuf>>,
    /// Paths currently being written from sync (skip CRDT updates entirely).
    /// This prevents feedback loops where remote sync writes trigger new CRDT updates.
    sync_writes_in_progress: RwLock<HashSet<PathBuf>>,
}

impl<FS: AsyncFileSystem> CrdtFs<FS> {
    /// Create a new CRDT filesystem decorator.
    pub fn new(
        inner: FS,
        workspace_crdt: Arc<WorkspaceCrdt>,
        body_doc_manager: Arc<BodyDocManager>,
    ) -> Self {
        Self {
            inner,
            workspace_crdt,
            body_doc_manager,
            enabled: Arc::new(AtomicBool::new(false)),
            local_writes_in_progress: RwLock::new(HashSet::new()),
            sync_writes_in_progress: RwLock::new(HashSet::new()),
        }
    }

    /// Normalize a path to a canonical form for CRDT storage.
    ///
    /// Strips leading "./" and "/" prefixes to ensure consistent keys
    /// across the CRDT. This matches how `InitializeWorkspaceCrdt` derives
    /// canonical paths from the workspace tree.
    fn normalize_crdt_path(path: &Path) -> String {
        normalize_sync_path(&path.to_string_lossy())
    }

    fn is_temp_path(path: &Path) -> bool {
        super::is_temp_file(&path.to_string_lossy())
    }

    /// Check if CRDT updates are enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Enable or disable CRDT updates.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    /// Get a reference to the workspace CRDT.
    pub fn workspace_crdt(&self) -> &Arc<WorkspaceCrdt> {
        &self.workspace_crdt
    }

    /// Get a reference to the body document manager.
    pub fn body_doc_manager(&self) -> &Arc<BodyDocManager> {
        &self.body_doc_manager
    }

    /// Get a reference to the inner filesystem.
    pub fn inner(&self) -> &FS {
        &self.inner
    }

    /// Check if a path is currently being written locally.
    ///
    /// Used to prevent loops when CRDT observers trigger writes.
    pub fn is_local_write_in_progress(&self, path: &Path) -> bool {
        let writes = self.local_writes_in_progress.read().unwrap();
        writes.contains(&path.to_path_buf())
    }

    /// Mark a path as being written locally.
    fn mark_local_write_start(&self, path: &Path) {
        let mut writes = self.local_writes_in_progress.write().unwrap();
        writes.insert(path.to_path_buf());
    }

    /// Clear the local write marker for a path.
    fn mark_local_write_end(&self, path: &Path) {
        let mut writes = self.local_writes_in_progress.write().unwrap();
        writes.remove(&path.to_path_buf());
    }

    /// Check if a path is currently being written from sync.
    ///
    /// Sync writes should skip CRDT updates entirely to prevent feedback loops.
    pub fn is_sync_write_in_progress(&self, path: &Path) -> bool {
        let writes = self.sync_writes_in_progress.read().unwrap();
        writes.contains(&path.to_path_buf())
    }

    /// Mark a path as being written from sync (internal implementation).
    fn mark_sync_write_start_internal(&self, path: &Path) {
        let mut writes = self.sync_writes_in_progress.write().unwrap();
        writes.insert(path.to_path_buf());
        log::debug!(
            "CrdtFs: Marked sync write start for {:?} (total: {})",
            path,
            writes.len()
        );
    }

    /// Clear the sync write marker for a path (internal implementation).
    fn mark_sync_write_end_internal(&self, path: &Path) {
        let mut writes = self.sync_writes_in_progress.write().unwrap();
        writes.remove(&path.to_path_buf());
        log::debug!(
            "CrdtFs: Marked sync write end for {:?} (remaining: {})",
            path,
            writes.len()
        );
    }

    /// Parse a link format string from frontmatter.
    fn parse_link_format(raw: &str) -> Option<link_parser::LinkFormat> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "markdown_root" => Some(link_parser::LinkFormat::MarkdownRoot),
            "markdown_relative" => Some(link_parser::LinkFormat::MarkdownRelative),
            "plain_relative" => Some(link_parser::LinkFormat::PlainRelative),
            "plain_canonical" => Some(link_parser::LinkFormat::PlainCanonical),
            _ => None,
        }
    }

    /// Try to detect link format by checking the current file and ancestor indexes.
    async fn detect_link_format_hint(
        &self,
        path: &Path,
        current_frontmatter: Option<&indexmap::IndexMap<String, serde_yaml::Value>>,
    ) -> Option<link_parser::LinkFormat> {
        if let Some(frontmatter) = current_frontmatter
            && let Some(raw) = frontmatter.get("link_format").and_then(|v| v.as_str())
            && let Some(parsed) = Self::parse_link_format(raw)
        {
            return Some(parsed);
        }

        let mut dir = path.parent().map(Path::to_path_buf).unwrap_or_default();

        loop {
            for index_name in ["README.md", "index.md"] {
                let candidate = if dir.as_os_str().is_empty() {
                    PathBuf::from(index_name)
                } else {
                    dir.join(index_name)
                };

                if !self.inner.exists(&candidate).await {
                    continue;
                }

                if let Ok(content) = self.inner.read_to_string(&candidate).await
                    && let Ok(parsed) = frontmatter::parse_or_empty(&content)
                {
                    if let Some(raw) = parsed
                        .frontmatter
                        .get("link_format")
                        .and_then(|v| v.as_str())
                        && let Some(format) = Self::parse_link_format(raw)
                    {
                        return Some(format);
                    }

                    if parsed.frontmatter.get("part_of").is_none()
                        && parsed.frontmatter.get("contents").is_some()
                    {
                        return Some(link_parser::LinkFormat::default());
                    }
                }
            }

            if dir.as_os_str().is_empty() {
                break;
            }

            let Some(parent) = dir.parent() else {
                break;
            };

            if parent == dir {
                break;
            }
            dir = parent.to_path_buf();
        }

        None
    }

    /// Resolve frontmatter path references to canonical workspace paths.
    async fn resolve_frontmatter_path(
        &self,
        current_file_path: &Path,
        raw_value: &str,
        link_format_hint: Option<link_parser::LinkFormat>,
    ) -> String {
        let parsed = link_parser::parse_link(raw_value);
        let resolved = link_parser::to_canonical_with_link_format(
            &parsed,
            current_file_path,
            link_format_hint,
        );

        // For ambiguous paths without a PlainCanonical hint, disambiguate
        // by checking which interpretation exists on disk.
        if parsed.path_type == link_parser::PathType::Ambiguous
            && link_format_hint != Some(link_parser::LinkFormat::PlainCanonical)
        {
            let relative =
                link_parser::to_canonical_with_link_format(&parsed, current_file_path, None);
            let workspace_root = link_parser::normalize_workspace_path(&parsed.path);

            if relative != workspace_root {
                let relative_exists = self.inner.exists(Path::new(&relative)).await;
                let root_exists = self.inner.exists(Path::new(&workspace_root)).await;

                if root_exists && !relative_exists {
                    return workspace_root;
                }
                if relative_exists && !root_exists {
                    return relative;
                }
            }
        }

        resolved
    }

    /// Extract FileMetadata from file content, including the filename.
    ///
    /// Parses frontmatter and converts known fields to FileMetadata.
    /// Paths in `part_of`, `contents`, and `attachments` are converted to canonical
    /// (workspace-relative) paths for consistent CRDT storage.
    async fn extract_metadata(&self, path: &Path, content: &str) -> FileMetadata {
        let parsed = frontmatter::parse_or_empty(content).ok();
        let mut metadata = parsed
            .as_ref()
            .map(|p| self.frontmatter_to_metadata(&p.frontmatter))
            .unwrap_or_default();

        // Set the filename from the path
        metadata.filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let link_format_hint = self
            .detect_link_format_hint(path, parsed.as_ref().map(|p| &p.frontmatter))
            .await;

        if let Some(part_of) = metadata.part_of.clone() {
            metadata.part_of = Some(
                self.resolve_frontmatter_path(path, &part_of, link_format_hint)
                    .await,
            );
        }

        if let Some(contents) = metadata.contents.take() {
            let mut canonical_contents = Vec::with_capacity(contents.len());
            for link_str in contents {
                canonical_contents.push(
                    self.resolve_frontmatter_path(path, &link_str, link_format_hint)
                        .await,
                );
            }
            metadata.contents = Some(canonical_contents);
        }

        for attachment in &mut metadata.attachments {
            if !attachment.path.is_empty() {
                attachment.path = self
                    .resolve_frontmatter_path(path, &attachment.path, link_format_hint)
                    .await;
            }
        }

        metadata
    }

    /// Look up a doc_id by path, returning the path as the key for backward compatibility.
    ///
    /// This maintains backward compatibility with existing code that expects
    /// path-based CRDT keys. The doc-ID based system is used when:
    /// 1. The workspace has been migrated (needs_migration() returns false)
    /// 2. A file is explicitly created with create_file()
    ///
    /// For now, this returns the path as the key, which maintains compatibility
    /// with all existing tests and functionality. The migration to doc-IDs
    /// will be triggered explicitly via migrate_to_doc_ids().
    fn path_to_doc_id(&self, path: &Path, _metadata: &FileMetadata) -> Option<String> {
        // Normalize the path to a canonical form for CRDT storage
        let normalized = Self::normalize_crdt_path(path);

        // For backward compatibility, always use path as the key
        // The doc-ID based system is opt-in via explicit migration
        //
        // In the future, after migration:
        // 1. Try find_by_path() to get existing doc_id
        // 2. If not found, create_file() to generate new UUID
        //
        // But for now, maintain compatibility with existing code

        Some(normalized)
    }

    /// Convert frontmatter to FileMetadata.
    fn frontmatter_to_metadata(
        &self,
        fm: &indexmap::IndexMap<String, serde_yaml::Value>,
    ) -> FileMetadata {
        FileMetadata::from_frontmatter(fm)
    }

    /// Update CRDT with metadata from a file.
    ///
    /// This is skipped if:
    /// - CRDT updates are disabled globally
    /// - The path is marked as a sync write (to prevent feedback loops)
    async fn update_crdt_for_file(&self, path: &Path, content: &str) {
        self.update_crdt_for_file_internal(path, content, false)
            .await;
    }

    /// Update CRDT for a newly created file.
    ///
    /// This clears any stale state from storage before creating the body doc,
    /// preventing concatenation with old content from deleted files.
    async fn update_crdt_for_new_file(&self, path: &Path, content: &str) {
        self.update_crdt_for_file_internal(path, content, true)
            .await;
    }

    /// Internal implementation for CRDT updates.
    ///
    /// If `is_new_file` is true, any existing body doc storage is deleted first
    /// to prevent stale state from being merged with new content.
    async fn update_crdt_for_file_internal(&self, path: &Path, content: &str, is_new_file: bool) {
        if !self.is_enabled() {
            log::warn!(
                "[CrdtFs] DEBUG: update_crdt_for_file_internal SKIPPED (disabled) path={:?}",
                path
            );
            return;
        }

        // Skip CRDT update if this is a sync write (prevents feedback loops)
        if self.is_sync_write_in_progress(path) {
            log::warn!("CrdtFs: Skipping CRDT update for sync write: {:?}", path);
            return;
        }

        let path_str = path.to_string_lossy().to_string();

        // Skip temporary files created by the metadata writer's safe write process
        // These files should never be synced to the server
        if super::is_temp_file(&path_str) {
            log::debug!(
                "CrdtFs: Skipping CRDT update for temporary file: {}",
                path_str
            );
            return;
        }

        log::warn!(
            "[CrdtFs] DEBUG: update_crdt_for_file_internal RUNNING: path='{}', is_new_file={}, content_len={}",
            path_str,
            is_new_file,
            content.len()
        );
        log::trace!(
            "[CrdtFs] update_crdt_for_file_internal: path_str='{}', is_new_file={}, body_preview='{}'",
            path_str,
            is_new_file,
            frontmatter::extract_body(content)
                .chars()
                .take(50)
                .collect::<String>()
        );
        let mut metadata = self.extract_metadata(path, content).await;

        // Get or create doc_id for this path
        // In doc-ID mode, this finds existing doc_id or creates new UUID
        // In legacy mode, this just returns the path as the key
        let doc_key = self
            .path_to_doc_id(path, &metadata)
            .unwrap_or(path_str.clone());

        // Preserve existing attachment BinaryRef data from the CRDT.
        // When a file write triggers this update, frontmatter attachments are typically
        // stored as plain strings (e.g., "[name](path)") which parse into BinaryRef with
        // empty hash/mime_type/size. But the JS side may have already written a BinaryRef
        // with the correct hash via setFileMetadata(). Merging prevents overwriting rich
        // metadata with empty values.
        if let Some(existing) = self.workspace_crdt.get_file(&doc_key)
            && !existing.attachments.is_empty()
        {
            if metadata.attachments.is_empty() {
                // If this write didn't carry attachments frontmatter, keep existing
                // CRDT refs so we don't lose BinaryRef/hash metadata set by JS.
                metadata.attachments = existing.attachments.clone();
            } else {
                for attachment in &mut metadata.attachments {
                    if attachment.hash.is_empty() {
                        if let Some(existing_ref) = existing
                            .attachments
                            .iter()
                            .find(|r| r.path == attachment.path && !r.hash.is_empty())
                        {
                            attachment.hash = existing_ref.hash.clone();
                            attachment.mime_type = existing_ref.mime_type.clone();
                            attachment.size = existing_ref.size;
                            attachment.uploaded_at = existing_ref.uploaded_at;
                            attachment.source = existing_ref.source.clone();
                        }
                    }
                }
            }
        }

        // Update workspace CRDT with the doc_key (doc_id or path)
        log::warn!("[CrdtFs] DEBUG: BEFORE set_file: doc_key={}", doc_key);
        if let Err(e) = self.workspace_crdt.set_file(&doc_key, metadata.clone()) {
            log::warn!("[CrdtFs] set_file FAILED: {}: {}", doc_key, e);
        } else {
            log::warn!("[CrdtFs] DEBUG: set_file SUCCESS: doc_key={}", doc_key);
        }

        // Update body doc using the same key
        let body = frontmatter::extract_body(content);
        log::warn!(
            "[CrdtFs] DEBUG: body extracted, len={}, preview='{}'",
            body.len(),
            body.chars().take(50).collect::<String>()
        );

        // For new files, delete any stale storage and create a fresh doc
        // to prevent concatenation with old content from deleted files
        let body_doc = if is_new_file {
            // Delete stale storage first, then create fresh doc
            let _ = self.body_doc_manager.delete(&doc_key);
            self.body_doc_manager.create(&doc_key)
        } else {
            self.body_doc_manager.get_or_create(&doc_key)
        };

        let _ = body_doc.set_body(body);
    }

    /// Update parent's contents array when a child is moved or deleted.
    ///
    /// For rename/move: `new_path` is Some with the new path.
    /// For delete: `new_path` is None.
    fn update_parent_contents(&self, old_path: &str, new_path: Option<&str>) {
        if !self.is_enabled() {
            return;
        }

        let old_metadata = match self.workspace_crdt.get_file(old_path) {
            Some(m) => m,
            None => return,
        };

        if let Some(ref parent_path) = old_metadata.part_of
            && let Some(mut parent) = self.workspace_crdt.get_file(parent_path)
            && let Some(ref mut contents) = parent.contents
        {
            // Find old filename in contents
            let old_filename = std::path::Path::new(old_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(old_path);

            if let Some(idx) = contents
                .iter()
                .position(|e| e == old_filename || e == old_path)
            {
                match new_path {
                    Some(np) => {
                        // Rename: replace with new filename
                        let new_filename = std::path::Path::new(np)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(np);
                        contents[idx] = new_filename.to_string();
                    }
                    None => {
                        // Delete: remove from contents
                        contents.remove(idx);
                    }
                }
                parent.modified_at = chrono::Utc::now().timestamp_millis();
                let _ = self.workspace_crdt.set_file(parent_path, parent);
            }
        }
    }
}

// Implement Clone if the inner FS is Clone
impl<FS: AsyncFileSystem + Clone> Clone for CrdtFs<FS> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            workspace_crdt: Arc::clone(&self.workspace_crdt),
            body_doc_manager: Arc::clone(&self.body_doc_manager),
            enabled: Arc::clone(&self.enabled),
            local_writes_in_progress: RwLock::new(HashSet::new()),
            sync_writes_in_progress: RwLock::new(HashSet::new()),
        }
    }
}

// AsyncFileSystem implementation - delegates to inner with CRDT updates
#[cfg(not(target_arch = "wasm32"))]
impl<FS: AsyncFileSystem + Send + Sync> AsyncFileSystem for CrdtFs<FS> {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        self.inner.read_to_string(path)
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Mark local write in progress
            self.mark_local_write_start(path);

            // Write to inner filesystem
            let result = self.inner.write_file(path, content).await;

            // Update CRDT if write succeeded and enabled
            if result.is_ok() {
                self.update_crdt_for_file(path, content).await;
            }

            // Clear local write marker
            self.mark_local_write_end(path);

            result
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            log::info!(
                "[CrdtFs] create_new CALLED: path='{}', enabled={}, content_len={}",
                path.display(),
                self.is_enabled(),
                content.len()
            );

            // Mark local write in progress
            self.mark_local_write_start(path);

            // Create in inner filesystem
            let result = self.inner.create_new(path, content).await;

            log::info!(
                "[CrdtFs] create_new RESULT: path='{}', success={}, err={:?}",
                path.display(),
                result.is_ok(),
                result.as_ref().err()
            );

            // Update CRDT if creation succeeded and enabled
            // Use new file variant to clear any stale state from storage
            if result.is_ok() {
                log::info!(
                    "[CrdtFs] create_new calling update_crdt_for_new_file: path='{}'",
                    path.display()
                );
                self.update_crdt_for_new_file(path, content).await;
            }

            // Clear local write marker
            self.mark_local_write_end(path);

            result
        })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Mark local write in progress
            self.mark_local_write_start(path);

            // Delete from inner filesystem
            let result = self.inner.delete_file(path).await;

            // Mark as deleted in CRDT if deletion succeeded and enabled
            if result.is_ok() && self.is_enabled() {
                if self.is_sync_write_in_progress(path) || Self::is_temp_path(path) {
                    self.mark_local_write_end(path);
                    return result;
                }

                let path_str = Self::normalize_crdt_path(path);

                // Update parent's contents to remove the deleted file
                self.update_parent_contents(&path_str, None);

                if let Err(e) = self.workspace_crdt.delete_file(&path_str) {
                    log::warn!(
                        "Failed to mark file as deleted in CRDT for {}: {}",
                        path_str,
                        e
                    );
                }
            }

            // Clear local write marker
            self.mark_local_write_end(path);

            result
        })
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        self.inner.list_md_files(dir)
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        self.inner.exists(path)
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        self.inner.create_dir_all(path)
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        self.inner.is_dir(path)
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Mark both paths as local writes in progress
            self.mark_local_write_start(from);
            self.mark_local_write_start(to);

            // Perform the physical move
            let result = self.inner.move_file(from, to).await;

            // Update CRDT if move succeeded
            if result.is_ok() && self.is_enabled() {
                if self.is_sync_write_in_progress(from)
                    || self.is_sync_write_in_progress(to)
                    || Self::is_temp_path(from)
                    || Self::is_temp_path(to)
                {
                    self.mark_local_write_end(from);
                    self.mark_local_write_end(to);
                    return result;
                }

                let from_str = Self::normalize_crdt_path(from);
                let to_str = Self::normalize_crdt_path(to);

                // Find the doc_id for the file being moved
                if let Some(doc_id) = self.workspace_crdt.find_by_path(from) {
                    // Legacy path-key mode stores files under path keys (e.g. "notes/file.md")
                    // rather than stable doc IDs. In that mode, rename_file() only updates
                    // metadata.filename and keeps the old key, so body/doc routing diverges.
                    // Detect legacy keys and force delete+create semantics.
                    let legacy_path_key = doc_id.contains('/') || doc_id.ends_with(".md");
                    if legacy_path_key {
                        log::debug!(
                            "CrdtFs: Legacy path key '{}' detected, using delete+create move semantics",
                            doc_id
                        );

                        self.update_parent_contents(&from_str, Some(&to_str));

                        if let Err(e) = self.workspace_crdt.delete_file(&doc_id) {
                            log::warn!(
                                "Failed to mark legacy source as deleted in CRDT ({}): {}",
                                doc_id,
                                e
                            );
                        }

                        if let Ok(content) = self.inner.read_to_string(to).await {
                            // Destination key is logically new in legacy mode; clear stale body
                            // state so old updates don't merge into unrelated prior history.
                            self.update_crdt_for_new_file(to, &content).await;
                        }

                        self.mark_local_write_end(from);
                        self.mark_local_write_end(to);
                        return result;
                    }

                    let new_filename = to
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();

                    // Detect rename (same directory) vs move (different directory)
                    let from_parent = from.parent();
                    let to_parent = to.parent();
                    let is_rename = from_parent == to_parent;

                    if is_rename {
                        // Rename: Just update the filename property - doc_id stays stable
                        log::debug!(
                            "CrdtFs: Renaming doc_id={} from {:?} to {}",
                            doc_id,
                            from,
                            new_filename
                        );
                        if let Err(e) = self.workspace_crdt.rename_file(&doc_id, &new_filename) {
                            log::warn!("Failed to rename file in CRDT: {}", e);
                        }
                    } else {
                        // Move: Update the parent reference - doc_id stays stable
                        // Find the new parent's doc_id
                        let new_parent_id =
                            to_parent.and_then(|p| self.workspace_crdt.find_by_path(p));

                        log::debug!(
                            "CrdtFs: Moving doc_id={} to parent={:?}, new_filename={}",
                            doc_id,
                            new_parent_id,
                            new_filename
                        );

                        // Update parent reference
                        if let Err(e) = self
                            .workspace_crdt
                            .move_file(&doc_id, new_parent_id.as_deref())
                        {
                            log::warn!("Failed to move file in CRDT: {}", e);
                        }

                        // Also update filename if it changed
                        if let Some(meta) = self.workspace_crdt.get_file(&doc_id)
                            && meta.filename != new_filename
                            && let Err(e) = self.workspace_crdt.rename_file(&doc_id, &new_filename)
                        {
                            log::warn!("Failed to rename file during move in CRDT: {}", e);
                        }
                    }

                    // Update parent's contents list (replace old path with new path)
                    self.update_parent_contents(&from_str, Some(&to_str));
                } else {
                    // Fallback for legacy path-based entries: use old delete+create behavior
                    log::debug!(
                        "CrdtFs: No doc_id found for {:?}, using legacy move behavior",
                        from
                    );
                    self.update_parent_contents(&from_str, Some(&to_str));

                    if let Err(e) = self.workspace_crdt.delete_file(&from_str) {
                        log::warn!("Failed to mark old path as deleted in CRDT: {}", e);
                    }

                    if let Ok(content) = self.inner.read_to_string(to).await {
                        // Treat destination as new to avoid stale body-doc history reuse.
                        self.update_crdt_for_new_file(to, &content).await;
                    }
                }
            }

            // Clear local write markers
            self.mark_local_write_end(from);
            self.mark_local_write_end(to);

            result
        })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        self.inner.read_binary(path)
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        // Binary files are not tracked in the CRDT (they're attachments)
        self.inner.write_binary(path, content)
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        self.inner.list_files(dir)
    }

    fn get_modified_time<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        self.inner.get_modified_time(path)
    }

    // Override sync write markers to track which paths are being written from sync
    fn mark_sync_write_start(&self, path: &Path) {
        self.mark_sync_write_start_internal(path);
    }

    fn mark_sync_write_end(&self, path: &Path) {
        self.mark_sync_write_end_internal(path);
    }
}

// WASM implementation (without Send + Sync bounds)
#[cfg(target_arch = "wasm32")]
impl<FS: AsyncFileSystem> AsyncFileSystem for CrdtFs<FS> {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        self.inner.read_to_string(path)
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Mark local write in progress
            self.mark_local_write_start(path);

            // Write to inner filesystem
            let result = self.inner.write_file(path, content).await;

            // Update CRDT if write succeeded and enabled
            if result.is_ok() {
                self.update_crdt_for_file(path, content).await;
            }

            // Clear local write marker
            self.mark_local_write_end(path);

            result
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            log::info!(
                "[CrdtFs] create_new CALLED: path='{}', enabled={}, content_len={}",
                path.display(),
                self.is_enabled(),
                content.len()
            );

            // Mark local write in progress
            self.mark_local_write_start(path);

            // Create in inner filesystem
            let result = self.inner.create_new(path, content).await;

            log::info!(
                "[CrdtFs] create_new RESULT: path='{}', success={}, err={:?}",
                path.display(),
                result.is_ok(),
                result.as_ref().err()
            );

            // Update CRDT if creation succeeded and enabled
            // Use new file variant to clear any stale state from storage
            if result.is_ok() {
                log::info!(
                    "[CrdtFs] create_new calling update_crdt_for_new_file: path='{}'",
                    path.display()
                );
                self.update_crdt_for_new_file(path, content).await;
            }

            // Clear local write marker
            self.mark_local_write_end(path);

            result
        })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Mark local write in progress
            self.mark_local_write_start(path);

            // Delete from inner filesystem
            let result = self.inner.delete_file(path).await;

            // Mark as deleted in CRDT if deletion succeeded and enabled
            if result.is_ok() && self.is_enabled() {
                if self.is_sync_write_in_progress(path) || Self::is_temp_path(path) {
                    self.mark_local_write_end(path);
                    return result;
                }

                let path_str = Self::normalize_crdt_path(path);

                // Update parent's contents to remove the deleted file
                self.update_parent_contents(&path_str, None);

                if let Err(e) = self.workspace_crdt.delete_file(&path_str) {
                    log::warn!(
                        "Failed to mark file as deleted in CRDT for {}: {}",
                        path_str,
                        e
                    );
                }
            }

            // Clear local write marker
            self.mark_local_write_end(path);

            result
        })
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        self.inner.list_md_files(dir)
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        self.inner.exists(path)
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        self.inner.create_dir_all(path)
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        self.inner.is_dir(path)
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Mark both paths as local writes in progress
            self.mark_local_write_start(from);
            self.mark_local_write_start(to);

            // Perform the physical move
            let result = self.inner.move_file(from, to).await;

            // Update CRDT if move succeeded
            if result.is_ok() && self.is_enabled() {
                if self.is_sync_write_in_progress(from)
                    || self.is_sync_write_in_progress(to)
                    || Self::is_temp_path(from)
                    || Self::is_temp_path(to)
                {
                    self.mark_local_write_end(from);
                    self.mark_local_write_end(to);
                    return result;
                }

                let from_str = Self::normalize_crdt_path(from);
                let to_str = Self::normalize_crdt_path(to);

                // Find the doc_id for the file being moved
                if let Some(doc_id) = self.workspace_crdt.find_by_path(from) {
                    // Legacy path-key mode stores files under path keys (e.g. "notes/file.md")
                    // rather than stable doc IDs. In that mode, rename_file() only updates
                    // metadata.filename and keeps the old key, so body/doc routing diverges.
                    // Detect legacy keys and force delete+create semantics.
                    let legacy_path_key = doc_id.contains('/') || doc_id.ends_with(".md");
                    if legacy_path_key {
                        log::debug!(
                            "CrdtFs: Legacy path key '{}' detected, using delete+create move semantics",
                            doc_id
                        );

                        self.update_parent_contents(&from_str, Some(&to_str));

                        if let Err(e) = self.workspace_crdt.delete_file(&doc_id) {
                            log::warn!(
                                "Failed to mark legacy source as deleted in CRDT ({}): {}",
                                doc_id,
                                e
                            );
                        }

                        if let Ok(content) = self.inner.read_to_string(to).await {
                            // Destination key is logically new in legacy mode; clear stale body
                            // state so old updates don't merge into unrelated prior history.
                            self.update_crdt_for_new_file(to, &content).await;
                        }

                        self.mark_local_write_end(from);
                        self.mark_local_write_end(to);
                        return result;
                    }

                    let new_filename = to
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();

                    // Detect rename (same directory) vs move (different directory)
                    let from_parent = from.parent();
                    let to_parent = to.parent();
                    let is_rename = from_parent == to_parent;

                    if is_rename {
                        // Rename: Just update the filename property - doc_id stays stable
                        log::debug!(
                            "CrdtFs: Renaming doc_id={} from {:?} to {}",
                            doc_id,
                            from,
                            new_filename
                        );
                        if let Err(e) = self.workspace_crdt.rename_file(&doc_id, &new_filename) {
                            log::warn!("Failed to rename file in CRDT: {}", e);
                        }
                    } else {
                        // Move: Update the parent reference - doc_id stays stable
                        // Find the new parent's doc_id
                        let new_parent_id =
                            to_parent.and_then(|p| self.workspace_crdt.find_by_path(p));

                        log::debug!(
                            "CrdtFs: Moving doc_id={} to parent={:?}, new_filename={}",
                            doc_id,
                            new_parent_id,
                            new_filename
                        );

                        // Update parent reference
                        if let Err(e) = self
                            .workspace_crdt
                            .move_file(&doc_id, new_parent_id.as_deref())
                        {
                            log::warn!("Failed to move file in CRDT: {}", e);
                        }

                        // Also update filename if it changed
                        if let Some(meta) = self.workspace_crdt.get_file(&doc_id) {
                            if meta.filename != new_filename {
                                if let Err(e) =
                                    self.workspace_crdt.rename_file(&doc_id, &new_filename)
                                {
                                    log::warn!("Failed to rename file during move in CRDT: {}", e);
                                }
                            }
                        }
                    }

                    // Update parent's contents list (replace old path with new path)
                    self.update_parent_contents(&from_str, Some(&to_str));
                } else {
                    // Fallback for legacy path-based entries: use old delete+create behavior
                    log::debug!(
                        "CrdtFs: No doc_id found for {:?}, using legacy move behavior",
                        from
                    );
                    self.update_parent_contents(&from_str, Some(&to_str));

                    if let Err(e) = self.workspace_crdt.delete_file(&from_str) {
                        log::warn!("Failed to mark old path as deleted in CRDT: {}", e);
                    }

                    if let Ok(content) = self.inner.read_to_string(to).await {
                        // Treat destination as new to avoid stale body-doc history reuse.
                        self.update_crdt_for_new_file(to, &content).await;
                    }
                }
            }

            // Clear local write markers
            self.mark_local_write_end(from);
            self.mark_local_write_end(to);

            result
        })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        self.inner.read_binary(path)
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        // Binary files are not tracked in the CRDT (they're attachments)
        self.inner.write_binary(path, content)
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        self.inner.list_files(dir)
    }

    fn get_modified_time<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        self.inner.get_modified_time(path)
    }

    // Override sync write markers to track which paths are being written from sync
    fn mark_sync_write_start(&self, path: &Path) {
        self.mark_sync_write_start_internal(path);
    }

    fn mark_sync_write_end(&self, path: &Path) {
        self.mark_sync_write_end_internal(path);
    }
}

impl<FS: AsyncFileSystem> std::fmt::Debug for CrdtFs<FS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrdtFs")
            .field("enabled", &self.is_enabled())
            .field("workspace_crdt", &self.workspace_crdt)
            .field("body_doc_manager", &self.body_doc_manager)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::{BinaryRef, CrdtStorage, MemoryStorage};
    use crate::fs::{InMemoryFileSystem, SyncToAsyncFs};

    fn create_test_crdt_fs() -> CrdtFs<SyncToAsyncFs<InMemoryFileSystem>> {
        let inner = SyncToAsyncFs::new(InMemoryFileSystem::new());
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let workspace_crdt = Arc::new(WorkspaceCrdt::new(Arc::clone(&storage)));
        let body_manager = Arc::new(BodyDocManager::new(storage));
        let fs = CrdtFs::new(inner, workspace_crdt, body_manager);
        // Enable for tests — production code enables after sync handshake
        fs.set_enabled(true);
        fs
    }

    #[test]
    fn test_write_updates_crdt() {
        let fs = create_test_crdt_fs();
        let content = "---\ntitle: Test\npart_of: index.md\n---\nBody content";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), content).await.unwrap();
        });

        // Check CRDT was updated
        let metadata = fs.workspace_crdt.get_file("test.md").unwrap();
        assert_eq!(metadata.title, Some("Test".to_string()));
        assert_eq!(metadata.part_of, Some("index.md".to_string()));
    }

    #[test]
    fn test_write_parses_string_attachments_in_fallback_path() {
        let fs = create_test_crdt_fs();
        let content = "---\ntitle: Test\nattachments:\n  - \"[Image](/_attachments/a.png)\"\n---\nBody content";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), content).await.unwrap();
        });

        let metadata = fs.workspace_crdt.get_file("test.md").unwrap();
        assert_eq!(metadata.attachments.len(), 1);
        assert_eq!(metadata.attachments[0].path, "_attachments/a.png");
    }

    #[test]
    fn test_write_preserves_existing_binary_refs_when_frontmatter_has_no_attachments() {
        let fs = create_test_crdt_fs();
        let initial = "---\ntitle: Test\n---\nBody";
        let updated = "---\ntitle: Test\n---\nBody updated";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), initial).await.unwrap();
        });

        let mut metadata = fs.workspace_crdt.get_file("test.md").unwrap();
        metadata.attachments = vec![BinaryRef {
            path: "_attachments/a.png".to_string(),
            source: "local".to_string(),
            hash: "a".repeat(64),
            mime_type: "image/png".to_string(),
            size: 123,
            uploaded_at: Some(1),
            deleted: false,
        }];
        fs.workspace_crdt.set_file("test.md", metadata).unwrap();

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), updated).await.unwrap();
        });

        let after = fs.workspace_crdt.get_file("test.md").unwrap();
        assert_eq!(after.attachments.len(), 1);
        assert_eq!(after.attachments[0].path, "_attachments/a.png");
        assert_eq!(after.attachments[0].hash, "a".repeat(64));
    }

    #[test]
    fn test_delete_marks_deleted_in_crdt() {
        let fs = create_test_crdt_fs();
        let content = "---\ntitle: Test\n---\nBody";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), content).await.unwrap();
            fs.delete_file(Path::new("test.md")).await.unwrap();
        });

        // Check file is marked as deleted in CRDT
        let metadata = fs.workspace_crdt.get_file("test.md").unwrap();
        assert!(metadata.deleted);
    }

    #[test]
    fn test_disabled_skips_crdt_updates() {
        let fs = create_test_crdt_fs();
        fs.set_enabled(false);

        let content = "---\ntitle: Test\n---\nBody";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), content).await.unwrap();
        });

        // CRDT should not have the file
        assert!(fs.workspace_crdt.get_file("test.md").is_none());
    }

    #[test]
    fn test_toggle_enabled() {
        let fs = create_test_crdt_fs();

        assert!(fs.is_enabled());
        fs.set_enabled(false);
        assert!(!fs.is_enabled());
        fs.set_enabled(true);
        assert!(fs.is_enabled());
    }

    #[test]
    fn test_clone_shares_enabled_state() {
        let fs = create_test_crdt_fs();
        let clone = fs.clone();

        assert!(fs.is_enabled());
        assert!(clone.is_enabled());

        fs.set_enabled(false);
        assert!(!fs.is_enabled());
        assert!(!clone.is_enabled());

        clone.set_enabled(true);
        assert!(fs.is_enabled());
        assert!(clone.is_enabled());
    }

    #[test]
    fn test_local_write_tracking() {
        let fs = create_test_crdt_fs();

        assert!(!fs.is_local_write_in_progress(Path::new("test.md")));

        fs.mark_local_write_start(Path::new("test.md"));
        assert!(fs.is_local_write_in_progress(Path::new("test.md")));

        fs.mark_local_write_end(Path::new("test.md"));
        assert!(!fs.is_local_write_in_progress(Path::new("test.md")));
    }

    #[test]
    fn test_sync_write_tracking() {
        let fs = create_test_crdt_fs();

        assert!(!fs.is_sync_write_in_progress(Path::new("test.md")));

        fs.mark_sync_write_start(Path::new("test.md"));
        assert!(fs.is_sync_write_in_progress(Path::new("test.md")));

        fs.mark_sync_write_end(Path::new("test.md"));
        assert!(!fs.is_sync_write_in_progress(Path::new("test.md")));
    }

    #[test]
    fn test_sync_write_skips_crdt_update() {
        let fs = create_test_crdt_fs();
        let content = "---\ntitle: Sync Write Test\n---\nBody content";

        // First, write without sync marker - should update CRDT
        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test1.md"), content).await.unwrap();
        });
        assert!(fs.workspace_crdt.get_file("test1.md").is_some());

        // Now, mark sync write and write - should NOT update CRDT
        fs.mark_sync_write_start(Path::new("test2.md"));
        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test2.md"), content).await.unwrap();
        });
        fs.mark_sync_write_end(Path::new("test2.md"));

        // File should exist on disk but NOT in CRDT
        assert!(futures_lite::future::block_on(
            fs.exists(Path::new("test2.md"))
        ));
        assert!(
            fs.workspace_crdt.get_file("test2.md").is_none(),
            "CRDT should not have been updated for sync write"
        );
    }

    #[test]
    fn test_sync_safe_move_swap_does_not_mutate_crdt_path_state() {
        let fs = create_test_crdt_fs();
        let content = "---\ntitle: Swap Test\n---\nBody content";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), content).await.unwrap();
            fs.write_file(Path::new("test.md.tmp"), content)
                .await
                .unwrap();

            // Simulate metadata_writer swap during remote sync:
            // test.md -> test.md.bak -> test.md, while sync marker is active.
            fs.mark_sync_write_start(Path::new("test.md"));
            fs.move_file(Path::new("test.md"), Path::new("test.md.bak"))
                .await
                .unwrap();
            fs.move_file(Path::new("test.md.tmp"), Path::new("test.md"))
                .await
                .unwrap();
            fs.delete_file(Path::new("test.md.bak")).await.unwrap();
            fs.mark_sync_write_end(Path::new("test.md"));
        });

        let metadata = fs.workspace_crdt.get_file("test.md").unwrap();
        assert_eq!(metadata.filename, "test.md");
        assert!(!metadata.deleted);
        assert!(fs.workspace_crdt.get_file("test.md.bak").is_none());
    }

    #[test]
    fn test_sync_marked_delete_does_not_tombstone_crdt() {
        let fs = create_test_crdt_fs();
        let content = "---\ntitle: Delete Test\n---\nBody content";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test-delete.md"), content)
                .await
                .unwrap();
            fs.mark_sync_write_start(Path::new("test-delete.md"));
            fs.delete_file(Path::new("test-delete.md")).await.unwrap();
            fs.mark_sync_write_end(Path::new("test-delete.md"));
        });

        let metadata = fs.workspace_crdt.get_file("test-delete.md").unwrap();
        assert!(!metadata.deleted);
    }

    #[test]
    fn test_move_file_legacy_paths_emit_delete_create_keys() {
        let fs = create_test_crdt_fs();
        let content = "---\ntitle: New Entry\n---\nBody content";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("new-entry.md"), content)
                .await
                .unwrap();
            fs.move_file(Path::new("new-entry.md"), Path::new("wow.md"))
                .await
                .unwrap();
        });

        let old_meta = fs
            .workspace_crdt
            .get_file("new-entry.md")
            .expect("old path should remain as tombstone");
        assert!(old_meta.deleted, "old path should be tombstoned after move");

        let new_meta = fs
            .workspace_crdt
            .get_file("wow.md")
            .expect("new path should exist after move");
        assert!(!new_meta.deleted);
        assert_eq!(new_meta.filename, "wow.md");

        let active_paths: Vec<String> = fs
            .workspace_crdt
            .list_active_files()
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        assert!(active_paths.contains(&"wow.md".to_string()));
        assert!(!active_paths.contains(&"new-entry.md".to_string()));
    }

    // =========================================================================
    // Link Parser Integration Tests
    // =========================================================================

    #[test]
    fn test_markdown_link_part_of_converts_to_canonical() {
        let fs = create_test_crdt_fs();
        // Write a file with markdown link in part_of
        let content =
            "---\ntitle: Child\npart_of: \"[Parent Index](/Folder/parent.md)\"\n---\nContent";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("Folder/child.md"), content)
                .await
                .unwrap();
        });

        // Check CRDT stores canonical path (without leading /)
        let metadata = fs.workspace_crdt.get_file("Folder/child.md").unwrap();
        assert_eq!(metadata.part_of, Some("Folder/parent.md".to_string()));
    }

    #[test]
    fn test_relative_part_of_converts_to_canonical() {
        let fs = create_test_crdt_fs();
        // Write a file with relative path in part_of
        let content = "---\ntitle: Child\npart_of: ../index.md\n---\nContent";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("Folder/Sub/child.md"), content)
                .await
                .unwrap();
        });

        // Check CRDT stores canonical path
        let metadata = fs.workspace_crdt.get_file("Folder/Sub/child.md").unwrap();
        assert_eq!(metadata.part_of, Some("Folder/index.md".to_string()));
    }

    #[test]
    fn test_plain_part_of_at_root_stays_canonical() {
        let fs = create_test_crdt_fs();
        // Write a file at root with plain filename part_of
        let content = "---\ntitle: Child\npart_of: index.md\n---\nContent";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("child.md"), content).await.unwrap();
        });

        // Check CRDT stores canonical path
        let metadata = fs.workspace_crdt.get_file("child.md").unwrap();
        assert_eq!(metadata.part_of, Some("index.md".to_string()));
    }

    #[test]
    fn test_markdown_link_contents_converts_to_canonical() {
        let fs = create_test_crdt_fs();
        // Write a file with markdown links in contents
        let content = r#"---
title: Parent Index
contents:
  - "[Child 1](/Folder/child1.md)"
  - "[Child 2](/Folder/Sub/child2.md)"
---
Content"#;

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("Folder/index.md"), content)
                .await
                .unwrap();
        });

        // Check CRDT stores canonical paths (without leading /)
        let metadata = fs.workspace_crdt.get_file("Folder/index.md").unwrap();
        assert_eq!(
            metadata.contents,
            Some(vec![
                "Folder/child1.md".to_string(),
                "Folder/Sub/child2.md".to_string()
            ])
        );
    }

    #[test]
    fn test_relative_contents_converts_to_canonical() {
        let fs = create_test_crdt_fs();
        // Write a file with relative paths in contents
        let content = r#"---
title: Parent Index
contents:
  - child1.md
  - Sub/child2.md
---
Content"#;

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("Folder/index.md"), content)
                .await
                .unwrap();
        });

        // Check CRDT stores canonical paths
        let metadata = fs.workspace_crdt.get_file("Folder/index.md").unwrap();
        assert_eq!(
            metadata.contents,
            Some(vec![
                "Folder/child1.md".to_string(),
                "Folder/Sub/child2.md".to_string()
            ])
        );
    }

    #[test]
    fn test_mixed_format_links_all_convert_to_canonical() {
        let fs = create_test_crdt_fs();
        // Write a file with mixed link formats
        let content = r#"---
title: Parent Index
part_of: "[Root](/README.md)"
contents:
  - child1.md
  - "[Child 2](/Folder/Sub/child2.md)"
  - ../sibling.md
---
Content"#;

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("Folder/index.md"), content)
                .await
                .unwrap();
        });

        // Check CRDT stores all paths as canonical
        let metadata = fs.workspace_crdt.get_file("Folder/index.md").unwrap();
        assert_eq!(metadata.part_of, Some("README.md".to_string()));
        assert_eq!(
            metadata.contents,
            Some(vec![
                "Folder/child1.md".to_string(),
                "Folder/Sub/child2.md".to_string(),
                "sibling.md".to_string(),
            ])
        );
    }

    #[test]
    fn test_write_preserves_extra_frontmatter_fields_in_crdt() {
        let fs = create_test_crdt_fs();
        let content = "---\ntitle: My Journal\naudience:\n  - public\npublic_audience: public\ndescription: A workspace\n---\nBody";

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("README.md"), content)
                .await
                .unwrap();
        });

        let metadata = fs.workspace_crdt.get_file("README.md").unwrap();
        assert_eq!(metadata.title, Some("My Journal".to_string()));
        assert_eq!(metadata.audience, Some(vec!["public".to_string()]));
        assert!(
            metadata.extra.contains_key("public_audience"),
            "Expected 'public_audience' in extra, got keys: {:?}",
            metadata.extra.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            metadata
                .extra
                .get("public_audience")
                .and_then(|v| v.as_str()),
            Some("public")
        );
    }
}
