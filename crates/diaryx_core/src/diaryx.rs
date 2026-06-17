//! Unified Diaryx API (async-first).
//!
//! This module provides the main entry point for all Diaryx operations.
//! The `Diaryx<FS>` struct wraps an async filesystem and provides access to
//! domain-specific operations through async sub-module accessors.
//!
//! # Example
//!
//! ```ignore
//! use diaryx_core::diaryx::Diaryx;
//! use diaryx_core::fs::SyncToAsyncFs;
//! use diaryx_native::RealFileSystem;
//!
//! let fs = SyncToAsyncFs::new(RealFileSystem);
//! let diaryx = Diaryx::new(fs);
//!
//! // Access entry operations
//! let content = diaryx.entry().get_content("path/to/file.md").await?;
//!
//! // Access workspace operations
//! let tree = diaryx.workspace().inner().get_tree("workspace/").await?;
//! ```

use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::date;
use crate::error::{DiaryxError, Result};
use crate::frontmatter;
use crate::fs::AsyncFileSystem;
use crate::link_parser;
use crate::plugin::PluginRegistry;
use crate::yaml;

/// The main Diaryx instance.
///
/// This struct provides a unified API for all Diaryx operations.
/// It wraps a filesystem and provides access to domain-specific
/// operations through sub-module accessors.
pub struct Diaryx<FS: AsyncFileSystem> {
    fs: FS,
    /// The workspace root directory (for computing canonical paths and link formatting).
    /// Uses RwLock for interior mutability since `set_workspace_root` takes `&self`.
    workspace_root: std::sync::RwLock<Option<PathBuf>>,
    /// Link format for `part_of`, `contents`, and `attachments` properties.
    link_format: crate::link_parser::LinkFormat,
    /// Plugin registry for dispatching events and commands to registered plugins.
    plugin_registry: PluginRegistry,
}

impl<FS: AsyncFileSystem> Diaryx<FS> {
    /// Create a new Diaryx instance with the given filesystem.
    pub fn new(fs: FS) -> Self {
        Self {
            fs,
            workspace_root: std::sync::RwLock::new(None),
            link_format: crate::link_parser::LinkFormat::default(),
            plugin_registry: PluginRegistry::new(),
        }
    }

    /// Set the link format for `part_of`, `contents`, and `attachments` properties.
    pub fn set_link_format(&mut self, format: crate::link_parser::LinkFormat) {
        self.link_format = format;
    }

    /// Get the workspace root directory.
    pub fn workspace_root(&self) -> Option<PathBuf> {
        self.workspace_root.read().unwrap().clone()
    }

    /// Get the link format.
    pub fn link_format(&self) -> crate::link_parser::LinkFormat {
        self.link_format
    }

    /// Get a reference to the underlying filesystem.
    pub fn fs(&self) -> &FS {
        &self.fs
    }

    /// Get a reference to the plugin registry.
    pub fn plugin_registry(&self) -> &PluginRegistry {
        &self.plugin_registry
    }

    /// Get a mutable reference to the plugin registry for registration.
    pub fn plugin_registry_mut(&mut self) -> &mut PluginRegistry {
        &mut self.plugin_registry
    }

    /// Initialize all registered plugins with the current instance state.
    ///
    /// This builds a [`PluginContext`] from the current workspace root and link format,
    /// then calls `init` on every registered plugin. Plugins that fail to init are
    /// marked as failed and skipped for subsequent dispatches. Returns a list of
    /// all failures (empty means all plugins initialized successfully).
    pub async fn init_plugins(&self) -> Vec<(crate::plugin::PluginId, crate::plugin::PluginError)> {
        let ctx = crate::plugin::PluginContext::new(self.workspace_root(), self.link_format);
        let failures = self.plugin_registry.init_all(&ctx).await;
        self.seed_plugin_configs().await;
        failures
    }

    /// Push each workspace plugin's declarative config from the workspace
    /// settings file (`plugins.<id>.config` in Config.md) into the plugin via
    /// `set_config`, so guests start with the workspace's stored configuration.
    ///
    /// Called after plugin init. Plugins with no stored config keep their
    /// defaults. No-op when no workspace is open or the root index can't be
    /// located. Read/seed failures are non-fatal and skipped per-plugin.
    pub async fn seed_plugin_configs(&self) {
        let Some(root) = self.workspace_root() else {
            return;
        };
        let ws = self.workspace();
        let inner = ws.inner();
        let Some(root_index) = inner.find_root_index_in_dir(&root).await.ok().flatten() else {
            return;
        };
        for wp in self.plugin_registry.workspace_plugins() {
            let id = wp.id().0;
            if let Ok(Some(config)) = inner.get_workspace_plugin_config(&root_index, &id).await {
                let _ = wp.set_config(config).await;
            }
        }
    }

    /// Like [`init_plugins`](Self::init_plugins), but invokes `on_progress`
    /// each time a plugin finishes init (in completion order). Plugins are
    /// driven concurrently, so a slow plugin no longer blocks others from
    /// being reported ready.
    pub async fn init_plugins_with_progress<F>(
        &self,
        on_progress: F,
    ) -> Vec<(crate::plugin::PluginId, crate::plugin::PluginError)>
    where
        // Use std::result::Result explicitly — `Result` is aliased to
        // `Result<T, DiaryxError>` at the top of this module.
        F: FnMut(&crate::plugin::PluginId, std::result::Result<(), &crate::plugin::PluginError>),
    {
        let ctx = crate::plugin::PluginContext::new(self.workspace_root(), self.link_format);
        let failures = self
            .plugin_registry
            .init_all_with_progress(&ctx, on_progress)
            .await;
        self.seed_plugin_configs().await;
        failures
    }

    /// Get entry operations accessor.
    ///
    /// This provides methods for reading/writing file content and frontmatter.
    pub fn entry(&self) -> EntryOps<'_, FS> {
        EntryOps { diaryx: self }
    }

    /// Get workspace operations accessor.
    ///
    /// This provides methods for traversing the workspace tree,
    /// managing files, and working with the index hierarchy.
    pub fn workspace(&self) -> WorkspaceOps<'_, FS> {
        WorkspaceOps { diaryx: self }
    }

    /// Set the workspace root directory.
    ///
    /// When set, canonical paths (e.g., "Archive/file.md") are resolved relative
    /// to this root. This is essential for Tauri/native apps where files should
    /// be written to a specific workspace directory.
    ///
    /// Call this after creating the Diaryx instance, typically in initialize_app().
    pub fn set_workspace_root(&self, root: std::path::PathBuf) {
        *self.workspace_root.write().unwrap() = Some(root);
    }
}

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    /// Get search operations accessor.
    ///
    /// Provides methods for searching workspace files by content or frontmatter.
    pub fn search(&self) -> SearchOps<'_, FS> {
        SearchOps { diaryx: self }
    }

    /// Get export operations accessor.
    ///
    /// Provides methods for exporting workspace files filtered by audience.
    pub fn export(&self) -> ExportOps<'_, FS> {
        ExportOps { diaryx: self }
    }

    /// Get validation operations accessor.
    ///
    /// Provides methods for validating workspace link integrity.
    pub fn validate(&self) -> ValidateOps<'_, FS> {
        ValidateOps { diaryx: self }
    }

    // execute() is implemented in command_handler.rs
}

// ============================================================================
// Entry Operations
// ============================================================================

/// Entry operations accessor.
///
/// Provides methods for reading/writing file content and frontmatter.
pub struct EntryOps<'a, FS: AsyncFileSystem> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: AsyncFileSystem> EntryOps<'a, FS> {
    // -------------------- Path Resolution --------------------

    /// Resolve a path relative to the workspace root (if set).
    ///
    /// If workspace_root is set, joins the path with the root.
    /// Otherwise, returns the path as-is.
    fn resolve_path(&self, path: &str) -> PathBuf {
        let wr = self.diaryx.workspace_root.read().unwrap();
        match &*wr {
            Some(root) => root.join(path),
            None => PathBuf::from(path),
        }
    }

    // -------------------- Frontmatter Methods --------------------

    /// Get all frontmatter properties for a file.
    ///
    /// Returns an empty map if no frontmatter exists.
    pub async fn get_frontmatter(&self, path: &str) -> Result<IndexMap<String, yaml::Value>> {
        let content = self.read_raw(path).await?;
        match frontmatter::parse(&content) {
            Ok(parsed) => Ok(parsed.frontmatter),
            Err(crate::frontmatter::FrontmatterError::NoFrontmatter) => Ok(IndexMap::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// Get a specific frontmatter property.
    ///
    /// Returns `Ok(None)` if the property doesn't exist or no frontmatter.
    pub async fn get_frontmatter_property(
        &self,
        path: &str,
        key: &str,
    ) -> Result<Option<yaml::Value>> {
        let frontmatter = self.get_frontmatter(path).await?;
        Ok(frontmatter.get(key).cloned())
    }

    /// Set a frontmatter property.
    ///
    /// Creates frontmatter if none exists.
    pub async fn set_frontmatter_property(
        &self,
        path: &str,
        key: &str,
        value: yaml::Value,
    ) -> Result<()> {
        let content = self.read_raw_or_empty(path).await?;
        let updated = frontmatter::set_property_in_text(&content, key, &value)?;
        self.write_raw(path, &updated).await
    }

    /// Remove a frontmatter property.
    pub async fn remove_frontmatter_property(&self, path: &str, key: &str) -> Result<()> {
        let content = match self.read_raw(path).await {
            Ok(c) => c,
            Err(_) => return Ok(()), // File doesn't exist, nothing to remove
        };
        let updated = frontmatter::remove_property_in_text(&content, key)?;
        if updated != content {
            self.write_raw(path, &updated).await?;
        }
        Ok(())
    }

    /// Reorder frontmatter keys to match the specified order.
    /// Keys not in the list are appended at the end in their original order.
    pub async fn reorder_frontmatter_keys(&self, path: &str, keys: &[String]) -> Result<()> {
        let content = match self.read_raw(path).await {
            Ok(c) => c,
            Err(_) => return Ok(()),
        };
        let updated = frontmatter::reorder_keys_in_text(&content, keys)?;
        if updated != content {
            self.write_raw(path, &updated).await?;
        }
        Ok(())
    }

    /// Move a frontmatter section to an external file, replacing it with a markdown link.
    pub async fn move_frontmatter_section_to_file(
        &self,
        source_path: &str,
        section_key: &str,
        target_path: &str,
        create_if_missing: bool,
    ) -> Result<()> {
        let source_content = self.read_raw(source_path).await?;
        let source_parsed = frontmatter::parse(&source_content)?;

        let section_value = source_parsed
            .frontmatter
            .get(section_key)
            .cloned()
            .ok_or_else(|| {
                DiaryxError::Validation(format!("Key '{}' not found in frontmatter", section_key))
            })?;

        // The target keys to write: a nested mapping is spread as top-level
        // frontmatter; a flat value becomes a single `section_key` property.
        let target_entries: Vec<(String, yaml::Value)> = match section_value {
            yaml::Value::Mapping(map) => map.into_iter().collect(),
            other => vec![(section_key.to_string(), other)],
        };

        // Write or update the target file by setting each key in place (a new
        // file gets a fresh frontmatter block; an existing one keeps comments).
        let target_resolved = self.resolve_path(target_path);
        let exists = self
            .diaryx
            .fs
            .try_exists(&target_resolved)
            .await
            .unwrap_or(false);
        if exists || create_if_missing {
            let mut target_content = if exists {
                self.read_raw_or_empty(target_path).await?
            } else {
                String::new()
            };
            for (k, v) in &target_entries {
                target_content = frontmatter::set_property_in_text(&target_content, k, v)?;
            }
            self.write_raw(target_path, &target_content).await?;
        } else {
            return Err(DiaryxError::FileRead {
                path: std::path::PathBuf::from(target_path),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Target file does not exist",
                ),
            });
        }

        // Replace the section's value in the source with a markdown link.
        let title = section_key.replace('_', " ");
        let title = title
            .split_whitespace()
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let link = format!("[{}]({})", title, target_path);
        let updated_source = frontmatter::set_property_in_text(
            &source_content,
            section_key,
            &yaml::Value::String(link),
        )?;
        self.write_raw(source_path, &updated_source).await
    }

    // -------------------- Content Methods --------------------

    /// Get the body content of a file, excluding frontmatter.
    pub async fn get_content(&self, path: &str) -> Result<String> {
        let content = self.read_raw_or_empty(path).await?;
        let parsed = frontmatter::parse_or_empty(&content)?;
        Ok(parsed.body)
    }

    /// Set the body content of a file, preserving frontmatter.
    ///
    /// Creates frontmatter if none exists.
    pub async fn set_content(&self, path: &str, body: &str) -> Result<()> {
        let content = self.read_raw_or_empty(path).await?;
        let updated = frontmatter::replace_body(&content, body);
        self.write_raw(path, &updated).await
    }

    /// Save content and update the 'updated' timestamp.
    ///
    /// This is a convenience method for the common save operation.
    pub async fn save_content(&self, path: &str, body: &str) -> Result<()> {
        self.save_content_with_options(path, body, true).await
    }

    /// Save content with explicit control over timestamp updating.
    pub async fn save_content_with_options(
        &self,
        path: &str,
        body: &str,
        auto_update_timestamp: bool,
    ) -> Result<()> {
        self.set_content(path, body).await?;
        if auto_update_timestamp {
            self.touch_updated(path).await?;
        }
        Ok(())
    }

    /// Update the 'updated' timestamp to the current time.
    pub async fn touch_updated(&self, path: &str) -> Result<()> {
        let timestamp = date::current_local_timestamp_rfc3339();
        self.set_frontmatter_property(path, "updated", yaml::Value::String(timestamp))
            .await
    }

    /// Append content to the end of a file's body.
    pub async fn append_content(&self, path: &str, content: &str) -> Result<()> {
        let raw = self.read_raw_or_empty(path).await?;
        let body = frontmatter::extract_body(&raw);

        let new_body = if body.is_empty() {
            content.to_string()
        } else if body.ends_with('\n') {
            format!("{}{}", body, content)
        } else {
            format!("{}\n{}", body, content)
        };

        let updated = frontmatter::replace_body(&raw, &new_body);
        self.write_raw(path, &updated).await
    }

    // -------------------- Raw I/O Methods --------------------

    /// Read the raw file content (including frontmatter).
    pub async fn read_raw(&self, path: &str) -> Result<String> {
        let resolved = self.resolve_path(path);
        self.diaryx
            .fs
            .read_to_string(&resolved)
            .await
            .map_err(|e| DiaryxError::FileRead {
                path: resolved,
                source: e,
            })
    }

    /// Read the raw file content, returning empty string if file doesn't exist.
    async fn read_raw_or_empty(&self, path: &str) -> Result<String> {
        let resolved = self.resolve_path(path);
        match self.diaryx.fs.read_to_string(&resolved).await {
            Ok(content) => Ok(content),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(DiaryxError::FileRead {
                path: resolved,
                source: e,
            }),
        }
    }

    /// Write raw file content to disk.
    async fn write_raw(&self, path: &str, content: &str) -> Result<()> {
        let resolved = self.resolve_path(path);
        self.diaryx
            .fs
            .write(&resolved, content.as_bytes())
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: resolved,
                source: e,
            })
    }

    // -------------------- Attachment Methods --------------------

    /// Get the list of attachments for a file.
    pub async fn get_attachments(&self, path: &str) -> Result<Vec<String>> {
        let frontmatter = self.get_frontmatter(path).await?;
        Ok(frontmatter::get_string_array(&frontmatter, "attachments"))
    }

    /// Add an attachment to a file's attachments list.
    pub async fn add_attachment(&self, path: &str, attachment_path: &str) -> Result<()> {
        let content = self.read_raw_or_empty(path).await?;
        let parsed_target = link_parser::parse_link(attachment_path);
        let target_canonical = link_parser::to_canonical(&parsed_target, Path::new(path));

        // Read the current list (read-only), append if absent, then write the
        // whole `attachments` value back in place (other keys' comments survive).
        let mut list = match frontmatter::parse_or_empty(&content)?
            .frontmatter
            .get("attachments")
        {
            Some(yaml::Value::Sequence(s)) => s.clone(),
            _ => Vec::new(),
        };
        let exists = list.iter().any(|item| {
            if let yaml::Value::String(existing) = item {
                let parsed_existing = link_parser::parse_link(existing);
                link_parser::to_canonical(&parsed_existing, Path::new(path)) == target_canonical
            } else {
                false
            }
        });
        if exists {
            return Ok(());
        }
        list.push(yaml::Value::String(attachment_path.to_string()));

        let updated = frontmatter::set_property_in_text(
            &content,
            "attachments",
            &yaml::Value::Sequence(list),
        )?;
        self.write_raw(path, &updated).await
    }

    /// Remove an attachment from a file's attachments list.
    pub async fn remove_attachment(&self, path: &str, attachment_path: &str) -> Result<()> {
        let content = match self.read_raw(path).await {
            Ok(c) => c,
            Err(_) => return Ok(()),
        };
        let parsed_target = link_parser::parse_link(attachment_path);
        let target_canonical = link_parser::to_canonical(&parsed_target, Path::new(path));

        let frontmatter = match frontmatter::parse(&content) {
            Ok(p) => p.frontmatter,
            Err(crate::frontmatter::FrontmatterError::NoFrontmatter) => return Ok(()),
            Err(e) => return Err(e.into()),
        };

        let mut list = match frontmatter.get("attachments") {
            Some(yaml::Value::Sequence(s)) => s.clone(),
            _ => return Ok(()),
        };
        let before = list.len();
        list.retain(|item| {
            if let yaml::Value::String(s) = item {
                let parsed_existing = link_parser::parse_link(s);
                link_parser::to_canonical(&parsed_existing, Path::new(path)) != target_canonical
            } else {
                true
            }
        });
        if list.len() == before {
            return Ok(()); // nothing matched; leave the file untouched
        }

        // Drop the key entirely when the last attachment is removed, else write
        // the trimmed list back in place.
        let updated = if list.is_empty() {
            frontmatter::remove_property_in_text(&content, "attachments")?
        } else {
            frontmatter::set_property_in_text(
                &content,
                "attachments",
                &yaml::Value::Sequence(list),
            )?
        };
        self.write_raw(path, &updated).await
    }

    // -------------------- Frontmatter Sorting --------------------

    /// Sort frontmatter keys according to a pattern.
    ///
    /// Pattern is comma-separated keys, with "*" meaning "rest alphabetically".
    /// Example: "title,description,*" puts title first, description second, rest alphabetically
    pub async fn sort_frontmatter(&self, path: &str, pattern: Option<&str>) -> Result<()> {
        let content = match self.read_raw(path).await {
            Ok(c) => c,
            Err(_) => return Ok(()),
        };

        let parsed = match frontmatter::parse(&content) {
            Ok(p) => p,
            Err(crate::frontmatter::FrontmatterError::NoFrontmatter) => return Ok(()),
            Err(e) => return Err(e.into()),
        };

        // Compute the sorted key order, then apply it in place (comment-preserving)
        // rather than reserializing the parsed map.
        let sorted = match pattern {
            Some(p) => frontmatter::sort_by_pattern(parsed.frontmatter, p),
            None => frontmatter::sort_alphabetically(parsed.frontmatter),
        };
        let order: Vec<String> = sorted.keys().cloned().collect();
        let updated = frontmatter::reorder_keys_in_text(&content, &order)?;
        if updated != content {
            self.write_raw(path, &updated).await?;
        }
        Ok(())
    }
}

// ============================================================================
// Workspace Operations (placeholder - delegates to existing Workspace)
// ============================================================================

/// Workspace operations accessor.
///
/// This provides methods for traversing the workspace tree,
/// managing files, and working with the index hierarchy.
pub struct WorkspaceOps<'a, FS: AsyncFileSystem> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: AsyncFileSystem> WorkspaceOps<'a, FS> {
    /// Get access to the underlying Workspace struct for full functionality.
    ///
    /// If a workspace root has been set (via `set_workspace_root`), the returned
    /// Workspace will have link formatting enabled with the configured link format.
    pub fn inner(&self) -> crate::workspace::Workspace<&'a FS> {
        if let Some(root) = self.diaryx.workspace_root() {
            crate::workspace::Workspace::with_link_format(
                &self.diaryx.fs,
                root,
                self.diaryx.link_format,
            )
        } else {
            crate::workspace::Workspace::new(&self.diaryx.fs)
        }
    }
}

// ============================================================================
// Search Operations (placeholder - delegates to existing Searcher)
// ============================================================================

/// Search operations accessor.
///
/// Provides methods for searching workspace files by content or frontmatter.
pub struct SearchOps<'a, FS: AsyncFileSystem> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: AsyncFileSystem + Clone> SearchOps<'a, FS> {
    /// Get access to the underlying Searcher struct for full functionality.
    pub fn inner(&self) -> crate::search::Searcher<FS> {
        crate::search::Searcher::new(self.diaryx.fs.clone())
    }

    /// Search the entire workspace for a pattern.
    pub async fn search_workspace(
        &self,
        workspace_root: &std::path::Path,
        query: &crate::search::SearchQuery,
    ) -> crate::error::Result<crate::search::SearchResults> {
        self.inner().search_workspace(workspace_root, query).await
    }

    /// Search a single file for a pattern.
    pub async fn search_file(
        &self,
        path: &std::path::Path,
        query: &crate::search::SearchQuery,
    ) -> crate::error::Result<Option<crate::search::FileSearchResult>> {
        self.inner().search_file(path, query).await
    }
}

// ============================================================================
// Export Operations (placeholder - delegates to existing Exporter)
// ============================================================================

/// Export operations accessor.
///
/// Provides methods for exporting workspace files filtered by audience.
pub struct ExportOps<'a, FS: AsyncFileSystem> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: AsyncFileSystem + Clone> ExportOps<'a, FS> {
    /// Get access to the underlying Exporter struct for full functionality.
    pub fn inner(&self) -> crate::export::Exporter<FS> {
        crate::export::Exporter::new(self.diaryx.fs.clone())
    }

    /// Plan an export operation without executing it.
    pub async fn plan_export(
        &self,
        workspace_root: &std::path::Path,
        audience: &str,
        destination: &std::path::Path,
        default_audience: Option<&str>,
    ) -> crate::error::Result<crate::export::ExportPlan> {
        self.inner()
            .plan_export(workspace_root, audience, destination, default_audience)
            .await
    }

    /// Execute an export plan.
    pub async fn execute_export(
        &self,
        plan: &crate::export::ExportPlan,
        options: &crate::export::ExportOptions,
    ) -> crate::error::Result<crate::export::ExportStats> {
        self.inner().execute_export(plan, options).await
    }
}

// ============================================================================
// Validate Operations (placeholder - delegates to existing Validator)
// ============================================================================

/// Validation operations accessor.
///
/// Provides methods for validating workspace link integrity.
pub struct ValidateOps<'a, FS: AsyncFileSystem> {
    diaryx: &'a Diaryx<FS>,
}

impl<'a, FS: AsyncFileSystem + Clone> ValidateOps<'a, FS> {
    /// Get access to the underlying Validator struct for full functionality.
    pub fn inner(&self) -> crate::validate::Validator<FS> {
        crate::validate::Validator::new(self.diaryx.fs.clone())
    }

    /// Validate all links starting from a workspace root index.
    ///
    /// # Arguments
    /// * `root_path` - Path to the root index file
    /// * `max_depth` - Maximum depth for orphan detection (None = unlimited, Some(2) matches tree view)
    pub async fn validate_workspace(
        &self,
        root_path: &std::path::Path,
        max_depth: Option<usize>,
    ) -> crate::error::Result<crate::validate::ValidationResult> {
        self.inner().validate_workspace(root_path, max_depth).await
    }

    /// Validate a single file's links.
    pub async fn validate_file(
        &self,
        file_path: &std::path::Path,
    ) -> crate::error::Result<crate::validate::ValidationResult> {
        self.inner().validate_file(file_path).await
    }

    /// Get a fixer for validation issues.
    pub fn fixer(&self) -> crate::validate::ValidationFixer<FS> {
        if let Some(root) = self.diaryx.workspace_root() {
            crate::validate::ValidationFixer::with_link_format(
                self.diaryx.fs.clone(),
                root,
                self.diaryx.link_format,
            )
        } else {
            crate::validate::ValidationFixer::new(self.diaryx.fs.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::SyncToAsyncFs;
    use crate::test_utils::MockFileSystem;

    #[test]
    fn test_entry_get_set_content() {
        let fs =
            MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\n\nOriginal content");

        let diaryx = Diaryx::new(SyncToAsyncFs::new(fs));

        // Get content
        let content = crate::fs::block_on_test(diaryx.entry().get_content("test.md")).unwrap();
        assert_eq!(content.trim(), "Original content");

        // Set content
        crate::fs::block_on_test(diaryx.entry().set_content("test.md", "\nNew content")).unwrap();

        let content = crate::fs::block_on_test(diaryx.entry().get_content("test.md")).unwrap();
        assert_eq!(content.trim(), "New content");
    }

    #[test]
    fn test_entry_get_frontmatter() {
        let fs = MockFileSystem::new()
            .with_file("test.md", "---\ntitle: My Title\nauthor: John\n---\n\nBody");

        let diaryx = Diaryx::new(SyncToAsyncFs::new(fs));

        let fm = crate::fs::block_on_test(diaryx.entry().get_frontmatter("test.md")).unwrap();
        assert_eq!(fm.get("title").unwrap().as_str().unwrap(), "My Title");
        assert_eq!(fm.get("author").unwrap().as_str().unwrap(), "John");
    }

    #[test]
    fn test_entry_set_frontmatter_property() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Original\n---\n\nBody");

        let diaryx = Diaryx::new(SyncToAsyncFs::new(fs));

        crate::fs::block_on_test(diaryx.entry().set_frontmatter_property(
            "test.md",
            "title",
            yaml::Value::String("Updated".to_string()),
        ))
        .unwrap();

        let fm = crate::fs::block_on_test(diaryx.entry().get_frontmatter("test.md")).unwrap();
        assert_eq!(fm.get("title").unwrap().as_str().unwrap(), "Updated");
    }
}
