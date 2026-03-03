//! Command execution handler.
//!
//! This module contains the implementation of the `execute()` method for `Diaryx`.
//! It handles all command types and returns appropriate responses.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde_yaml::Value;

use crate::command::{Command, EntryData, Response};
use crate::diaryx::{Diaryx, json_to_yaml, yaml_to_json};
use crate::error::{DiaryxError, Result};
use crate::frontmatter;
use crate::fs::AsyncFileSystem;
use crate::link_parser;
use crate::path_utils::{normalize_path, strip_workspace_root_prefix};
use crate::plugin::{FileCreatedEvent, FileDeletedEvent, FileMovedEvent, FileSavedEvent};

#[cfg(test)]
use std::path::Component;

/// Normalize a path by resolving a relative path against a base directory.
/// Handles `.` and `..` components without filesystem access.
/// Returns a forward-slash-separated path string suitable for CRDT keys.
/// Also handles corrupted absolute paths by stripping the workspace base path if found.
#[cfg(test)]
fn normalize_contents_path(base_dir: &Path, relative: &str, workspace_base: &Path) -> String {
    // First, check if this looks like a corrupted absolute path
    // (e.g., "Users/adamharris/Documents/journal/Archive/file.md" - absolute path with leading / stripped)
    let cleaned_relative = {
        // Try to find the workspace base path within the relative path and strip it
        let workspace_str = workspace_base.to_string_lossy();
        let workspace_without_root = workspace_str.trim_start_matches('/');

        if relative.starts_with(workspace_without_root) {
            // This is a corrupted absolute path - strip the workspace prefix
            let stripped = relative
                .strip_prefix(workspace_without_root)
                .unwrap_or(relative);
            stripped.trim_start_matches('/')
        } else if relative.starts_with(&*workspace_str) {
            // Full absolute path starting with /
            let stripped = relative.strip_prefix(&*workspace_str).unwrap_or(relative);
            stripped.trim_start_matches('/')
        } else {
            relative
        }
    };

    let joined = base_dir.join(cleaned_relative);
    let mut normalized: Vec<String> = Vec::new();
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            Component::Normal(s) => normalized.push(s.to_string_lossy().to_string()),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    normalized.join("/")
}

/// Resolve an attachment reference into a workspace-relative storage path.
///
/// Handles markdown links, root-relative links, plain relative paths, and
/// plain canonical paths that start with the current entry directory.
fn resolve_attachment_storage_path(entry_path: &str, attachment_path: &str) -> PathBuf {
    let entry = Path::new(entry_path);
    let parsed = link_parser::parse_link(attachment_path);
    let canonical = if parsed.path_type == link_parser::PathType::Ambiguous {
        let current_dir = entry
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
                entry,
                Some(link_parser::LinkFormat::PlainCanonical),
            )
        } else {
            link_parser::to_canonical(&parsed, entry)
        }
    } else {
        link_parser::to_canonical(&parsed, entry)
    };

    normalize_path(Path::new(&canonical))
}

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    // =========================================================================
    // Path Conversion Helpers (Phase 1)
    // =========================================================================

    /// Resolve a workspace-relative path against the workspace root (if set).
    ///
    /// This is needed for direct `self.fs()` calls that bypass `EntryOps`,
    /// e.g. attachment file operations.
    fn resolve_fs_path<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        match self.workspace_root() {
            Some(root) => root.join(path),
            None => path.as_ref().to_path_buf(),
        }
    }

    /// Recursively filter a tree to only include nodes visible to the given audience.
    ///
    /// Uses the same visibility rules as export: explicit audience takes priority,
    /// then inherited from parent, case-insensitive matching.
    async fn filter_tree_by_audience(
        &self,
        node: crate::workspace::TreeNode,
        audience: &str,
    ) -> crate::workspace::TreeNode {
        Box::pin(self.filter_tree_node(node, audience, None)).await
    }

    async fn filter_tree_node(
        &self,
        node: crate::workspace::TreeNode,
        audience: &str,
        inherited_audience: Option<Vec<String>>,
    ) -> crate::workspace::TreeNode {
        // Parse this node's frontmatter to get its audience
        let node_audience = match self.workspace().inner().parse_index(&node.path).await {
            Ok(index) => index.frontmatter.audience.clone(),
            Err(_) => None,
        };

        // Effective audience for children to inherit
        let effective_for_children = node_audience.clone().or(inherited_audience);

        // Recursively filter children
        let mut filtered_children = Vec::new();
        for child in node.children {
            // Check visibility of each child
            let child_audience = match self.workspace().inner().parse_index(&child.path).await {
                Ok(index) => index.frontmatter.audience.clone(),
                Err(_) => None,
            };

            let is_visible = if let Some(ref file_aud) = child_audience {
                file_aud
                    .iter()
                    .any(|a| a.trim().eq_ignore_ascii_case(audience))
            } else if let Some(ref parent_aud) = effective_for_children {
                parent_aud
                    .iter()
                    .any(|a| a.trim().eq_ignore_ascii_case(audience))
            } else {
                // No audience defined anywhere — exclude
                false
            };

            if is_visible {
                let filtered = Box::pin(self.filter_tree_node(
                    child,
                    audience,
                    child_audience.or_else(|| effective_for_children.clone()),
                ))
                .await;
                filtered_children.push(filtered);
            }
        }

        crate::workspace::TreeNode {
            name: node.name,
            description: node.description,
            path: node.path,
            children: filtered_children,
            properties: node.properties,
        }
    }

    /// Recursively collect all unique audience tags from a workspace tree.
    async fn collect_audiences_recursive<F: AsyncFileSystem>(
        ws: &crate::workspace::Workspace<F>,
        path: &Path,
        audiences: &mut std::collections::HashSet<String>,
        visited: &mut std::collections::HashSet<PathBuf>,
        workspace_root: &Path,
        link_format: Option<crate::link_parser::LinkFormat>,
    ) {
        if visited.contains(path) {
            return;
        }
        visited.insert(path.to_path_buf());

        if let Ok(index) = ws.parse_index_with_hint(path, link_format).await {
            if let Some(file_audiences) = &index.frontmatter.audience {
                for a in file_audiences {
                    let trimmed = a.trim();
                    if !trimmed.is_empty() {
                        audiences.insert(a.clone());
                    }
                }
            }

            if index.frontmatter.is_index() {
                for child_rel in index.frontmatter.contents_list() {
                    let child_path = index.resolve_path(child_rel);
                    let absolute_child_path = if child_path.is_absolute() {
                        child_path
                    } else {
                        workspace_root.join(&child_path)
                    };
                    if ws.fs_ref().exists(&absolute_child_path).await {
                        Box::pin(Self::collect_audiences_recursive(
                            ws,
                            &absolute_child_path,
                            audiences,
                            visited,
                            workspace_root,
                            link_format,
                        ))
                        .await;
                    }
                }
            }
        }
    }

    /// Strip the workspace root from a path if present, returning a workspace-relative path.
    ///
    /// On Tauri, entry paths from the frontend may be absolute OS paths (e.g.,
    /// `/Users/.../workspace/diaryx.md`). Functions like `resolve_attachment_storage_path`
    /// expect workspace-relative paths, so we strip the workspace root prefix first.
    fn to_workspace_relative(&self, path: &str) -> String {
        if let Some(root) = self.workspace_root()
            && let Some(relative) = strip_workspace_root_prefix(path, &root)
        {
            return relative;
        }
        path.to_string()
    }

    /// Get the canonical path from a storage path.
    ///
    /// Delegates to the plugin registry (e.g., SyncPlugin) if a plugin provides
    /// path mapping. Otherwise returns the path unchanged.
    fn get_canonical_path(&self, storage_path: &str) -> String {
        self.plugin_registry()
            .get_canonical_path(storage_path)
            .unwrap_or_else(|| storage_path.to_string())
    }

    /// Sync the first H1 heading in the body to the given title.
    /// If an H1 exists on the first non-blank line, replace its text.
    /// If not, prepend `# {title}\n\n` to the body.
    async fn sync_heading_to_title(&self, path: &str, title: &str) -> Result<()> {
        use crate::entry::sync_h1_in_body;
        let body = self.entry().get_content(path).await.unwrap_or_default();
        let new_body = sync_h1_in_body(&body, title);
        if new_body != body {
            self.entry().set_content(path, &new_body).await
        } else {
            Ok(())
        }
    }

    /// Resolve a template from a workspace config link value.
    /// The link is in the workspace's configured link_format (e.g., `[Template](/templates/note.md)`).
    /// Returns the file content as a Template, or None if resolution fails.
    async fn resolve_template_from_link(
        &self,
        link: &str,
        workspace_root_path: &Path,
    ) -> Option<crate::template::Template> {
        let parsed = link_parser::parse_link(link);
        // to_canonical expects the current file path (the root index) to resolve relative links
        let canonical = link_parser::to_canonical(&parsed, workspace_root_path);
        let workspace_dir = workspace_root_path
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let file_path = workspace_dir.join(&canonical);
        let content = self.fs().read_to_string(&file_path).await.ok()?;
        let name = Path::new(&canonical)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("template")
            .to_string();
        Some(crate::template::Template::new(name, content))
    }

    /// Notify plugins that the workspace was modified (for sync broadcast).
    async fn emit_workspace_sync(&self) {
        self.plugin_registry().notify_workspace_modified().await;
    }

    /// Format a canonical path as a link for frontmatter.
    ///
    /// Uses the configured link format (see [`link_format`]).
    ///
    /// # Arguments
    /// * `canonical_path` - The canonical path of the target file
    /// * `from_canonical_path` - The canonical path of the file containing this link
    ///
    /// The title is resolved from:
    /// 1. CRDT metadata (if available and has a title)
    /// 2. Fallback: generated from the filename using `path_to_title`
    fn format_link_for_file(&self, canonical_path: &str, from_canonical_path: &str) -> String {
        let title = self.resolve_title(canonical_path);
        let format = self.link_format();
        link_parser::format_link_with_format(canonical_path, &title, format, from_canonical_path)
    }

    /// Resolve a display title for an attachment link.
    ///
    /// Attachments should keep their filename (including extension) as the
    /// link title to avoid lossy title prettification (e.g. dropping `.png`).
    fn resolve_attachment_title(canonical_path: &str) -> String {
        Path::new(canonical_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(canonical_path)
            .to_string()
    }

    /// Format a canonical attachment path as a frontmatter link.
    fn format_attachment_link_for_file(
        &self,
        canonical_path: &str,
        from_canonical_path: &str,
    ) -> String {
        let title = Self::resolve_attachment_title(canonical_path);
        let format = self.link_format();
        link_parser::format_link_with_format(canonical_path, &title, format, from_canonical_path)
    }

    /// Format a canonical path as a link (simple version without source file context).
    ///
    /// For formats that require a source file (relative formats), this falls back
    /// to MarkdownRoot format.
    #[allow(dead_code)]
    fn format_link(&self, canonical_path: &str) -> String {
        let title = self.resolve_title(canonical_path);
        // For simple format_link, always use MarkdownRoot since we don't have context
        link_parser::format_link(canonical_path, &title)
    }

    /// Resolve a display title for a canonical path.
    ///
    /// Looks up the title from CRDT metadata if available,
    /// otherwise generates one from the filename.
    fn resolve_title(&self, canonical_path: &str) -> String {
        if let Some(title) = self.plugin_registry().get_file_title(canonical_path) {
            return title;
        }
        link_parser::path_to_title(canonical_path)
    }

    /// Resolve a frontmatter reference (`part_of`, `contents`, `attachments`) to
    /// canonical workspace path.
    ///
    /// Uses the configured workspace link format as a hint so PlainCanonical
    /// workspaces treat ambiguous plain paths as workspace-root references.
    fn resolve_frontmatter_link_target(&self, raw_link: &str, from_canonical_path: &str) -> String {
        let parsed = link_parser::parse_link(raw_link);
        link_parser::to_canonical_with_link_format(
            &parsed,
            Path::new(from_canonical_path),
            Some(self.link_format()),
        )
    }

    /// Resolve an attachment frontmatter reference to canonical workspace path.
    ///
    /// In addition to normal link parsing semantics, this treats ambiguous
    /// plain paths that start with the current entry directory as canonical
    /// workspace paths (for compatibility with persisted attachment refs).
    fn resolve_attachment_link_target_with_hint(
        &self,
        raw_link: &str,
        from_canonical_path: &str,
        source_format_hint: Option<link_parser::LinkFormat>,
    ) -> String {
        let parsed = link_parser::parse_link(raw_link);

        if parsed.path_type == link_parser::PathType::Ambiguous {
            let current_dir = Path::new(from_canonical_path)
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
                return link_parser::to_canonical_with_link_format(
                    &parsed,
                    Path::new(from_canonical_path),
                    Some(link_parser::LinkFormat::PlainCanonical),
                );
            }
        }

        link_parser::to_canonical_with_link_format(
            &parsed,
            Path::new(from_canonical_path),
            source_format_hint,
        )
    }

    // =========================================================================
    // Command Execution
    // =========================================================================

    /// Execute a command and return the response.
    ///
    /// This is the unified command interface that replaces individual method calls.
    /// All commands are async and return a `Result<Response>`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use diaryx_core::{Command, Response, Diaryx};
    ///
    /// let cmd = Command::GetEntry { path: "notes/hello.md".to_string() };
    /// let response = diaryx.execute(cmd).await?;
    ///
    /// if let Response::Entry(entry) = response {
    ///     println!("Title: {:?}", entry.title);
    /// }
    /// ```
    pub async fn execute(&self, mut command: Command) -> Result<Response> {
        command.normalize_paths(|p| self.to_workspace_relative(p));

        match command {
            // === Entry Operations ===
            Command::GetEntry { path } => {
                let content = self.entry().read_raw(&path).await?;
                let parsed = frontmatter::parse_or_empty(&content)?;
                let title = parsed
                    .frontmatter
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Convert serde_yaml::Value to serde_json::Value
                let fm: IndexMap<String, serde_json::Value> = parsed
                    .frontmatter
                    .into_iter()
                    .map(|(k, v)| (k, yaml_to_json(v)))
                    .collect();

                Ok(Response::Entry(EntryData {
                    path: PathBuf::from(&path),
                    title,
                    frontmatter: fm,
                    content: parsed.body,
                }))
            }

            Command::SaveEntry {
                path,
                content,
                root_index_path,
                detect_h1_title,
            } => {
                log::debug!(
                    "[CommandHandler] SaveEntry: input path='{}', content_preview='{}'",
                    path,
                    content.chars().take(50).collect::<String>()
                );

                // Read workspace config if root_index_path provided
                let ws_config = if let Some(ref rip) = root_index_path {
                    let ws = self.workspace().inner();
                    let resolved_root_index_path = self.resolve_fs_path(rip);
                    ws.get_workspace_config(&resolved_root_index_path)
                        .await
                        .ok()
                } else {
                    None
                };
                let auto_update = ws_config
                    .as_ref()
                    .map(|c| c.auto_update_timestamp)
                    .unwrap_or(true);

                // Save to filesystem (CrdtFs automatically updates body CRDT via its write hook)
                self.entry()
                    .save_content_with_options(&path, &content, auto_update)
                    .await?;

                // Track for echo detection and emit sync message if CRDT is enabled
                {
                    let canonical_path = self.get_canonical_path(&path);
                    log::debug!(
                        "[CommandHandler] SaveEntry: canonical_path='{}' (from input path='{}')",
                        canonical_path,
                        path
                    );

                    // Track for echo detection
                    self.plugin_registry()
                        .track_content_for_sync(&canonical_path, &content);

                    // Note: Body sync messages are now automatically emitted via the Yrs observer
                    // pattern when set_body() is called. No manual emit_body_update needed.

                    log::debug!(
                        "[CommandHandler] SaveEntry: completed for canonical_path='{}'",
                        canonical_path
                    );
                }

                // Emit file-saved event to file plugins
                self.plugin_registry()
                    .emit_file_saved(&FileSavedEvent { path: path.clone() })
                    .await;

                // H1→title sync: detect first-line H1 and sync to title + filename
                if detect_h1_title && let Some(ref ws_config) = ws_config {
                    use crate::entry::{apply_filename_style, extract_first_line_h1};

                    if let Some(h1_title) = extract_first_line_h1(&content) {
                        // Read current frontmatter title
                        let current_title = self
                            .entry()
                            .get_frontmatter_property(&path, "title")
                            .await
                            .ok()
                            .flatten()
                            .and_then(|v| {
                                if let serde_yaml::Value::String(s) = v {
                                    Some(s)
                                } else {
                                    None
                                }
                            });

                        if current_title.as_deref() != Some(&h1_title) {
                            // Set title in frontmatter
                            self.entry()
                                .set_frontmatter_property(
                                    &path,
                                    "title",
                                    serde_yaml::Value::String(h1_title.clone()),
                                )
                                .await?;

                            // Rename file to match new title
                            let new_stem =
                                apply_filename_style(&h1_title, &ws_config.filename_style);
                            let new_filename = format!("{}.md", new_stem);

                            let entry_path = self.resolve_fs_path(&path);
                            let ws = self.workspace().inner();
                            let is_index = ws.is_index_file(&entry_path).await;
                            let is_root = ws.is_root_index(&entry_path).await;

                            let current_comparable = if is_index && !is_root {
                                entry_path
                                    .parent()
                                    .and_then(|p| p.file_name())
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("")
                                    .to_string()
                            } else {
                                entry_path
                                    .file_stem()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("")
                                    .to_string()
                            };

                            if current_comparable != new_stem {
                                let new_path = ws.rename_entry(&entry_path, &new_filename).await?;
                                let new_path_str = new_path.to_string_lossy().to_string();

                                // Migrate body CRDT doc to new path
                                {
                                    let canonical_old = self.get_canonical_path(&path);
                                    let canonical_new = self.get_canonical_path(&new_path_str);
                                    if canonical_old != canonical_new {
                                        self.plugin_registry()
                                            .emit_body_doc_renamed(&canonical_old, &canonical_new)
                                            .await;
                                    }

                                    self.emit_workspace_sync().await;
                                }

                                return Ok(Response::String(new_path_str));
                            }

                            // Title changed but filename didn't need to change
                            self.emit_workspace_sync().await;

                            return Ok(Response::String(path));
                        }
                    }
                }

                Ok(Response::Ok)
            }

            Command::GetFrontmatter { path } => {
                let fm = self.entry().get_frontmatter(&path).await?;
                let json_fm: IndexMap<String, serde_json::Value> =
                    fm.into_iter().map(|(k, v)| (k, yaml_to_json(v))).collect();
                Ok(Response::Frontmatter(json_fm))
            }

            Command::SetFrontmatterProperty {
                path,
                key,
                value,
                root_index_path,
            } => {
                // Handle part_of/contents/attachments specially - normalize and
                // format links according to workspace settings.
                // CrdtFs.write_file extracts metadata from frontmatter automatically
                {
                    let canonical_path = self.get_canonical_path(&path);

                    if key == "part_of" {
                        // Parse the value, convert to canonical, format as markdown link
                        if let serde_json::Value::String(ref s) = value {
                            let canonical_target =
                                self.resolve_frontmatter_link_target(s, &canonical_path);

                            // Format as markdown link for file
                            let formatted =
                                self.format_link_for_file(&canonical_target, &canonical_path);

                            // Write formatted link to file - CrdtFs extracts metadata automatically
                            let yaml_value = Value::String(formatted);
                            self.entry()
                                .set_frontmatter_property(&path, &key, yaml_value)
                                .await?;

                            // Track for echo detection
                            self.plugin_registry()
                                .track_file_for_sync(&canonical_path)
                                .await;

                            // Emit workspace sync message
                            self.emit_workspace_sync().await;
                            return Ok(Response::Ok);
                        }
                    } else if key == "contents" {
                        // Handle contents array - format each item as markdown link
                        if let serde_json::Value::Array(ref arr) = value {
                            let mut formatted_links: Vec<Value> = Vec::new();

                            for item in arr {
                                if let serde_json::Value::String(s) = item {
                                    let canonical_target =
                                        self.resolve_frontmatter_link_target(s, &canonical_path);
                                    let formatted = self
                                        .format_link_for_file(&canonical_target, &canonical_path);
                                    formatted_links.push(Value::String(formatted));
                                }
                            }

                            // Write formatted links to file - CrdtFs extracts metadata automatically
                            let yaml_value = Value::Sequence(formatted_links);
                            self.entry()
                                .set_frontmatter_property(&path, &key, yaml_value)
                                .await?;

                            // Track for echo detection
                            self.plugin_registry()
                                .track_file_for_sync(&canonical_path)
                                .await;

                            // Emit workspace sync message
                            self.emit_workspace_sync().await;
                            return Ok(Response::Ok);
                        }
                    } else if key == "attachments" {
                        // Handle attachments array - format each item as a normalized link.
                        if let serde_json::Value::Array(ref arr) = value {
                            let mut formatted_links: Vec<Value> = Vec::new();

                            for item in arr {
                                if let serde_json::Value::String(s) = item {
                                    let canonical_target = self
                                        .resolve_attachment_link_target_with_hint(
                                            s,
                                            &canonical_path,
                                            Some(self.link_format()),
                                        );
                                    let formatted = self.format_attachment_link_for_file(
                                        &canonical_target,
                                        &canonical_path,
                                    );
                                    formatted_links.push(Value::String(formatted));
                                }
                            }

                            let yaml_value = Value::Sequence(formatted_links);
                            self.entry()
                                .set_frontmatter_property(&path, &key, yaml_value)
                                .await?;

                            // Track for echo detection
                            self.plugin_registry()
                                .track_file_for_sync(&canonical_path)
                                .await;

                            self.emit_workspace_sync().await;
                            return Ok(Response::Ok);
                        } else if let serde_json::Value::String(ref s) = value {
                            // Accept scalar attachment values for backwards compatibility.
                            let canonical_target = self.resolve_attachment_link_target_with_hint(
                                s,
                                &canonical_path,
                                Some(self.link_format()),
                            );
                            let formatted = self.format_attachment_link_for_file(
                                &canonical_target,
                                &canonical_path,
                            );
                            let yaml_value = Value::String(formatted);
                            self.entry()
                                .set_frontmatter_property(&path, &key, yaml_value)
                                .await?;

                            // Track for echo detection
                            self.plugin_registry()
                                .track_file_for_sync(&canonical_path)
                                .await;

                            self.emit_workspace_sync().await;
                            return Ok(Response::Ok);
                        }
                    }
                }

                // Auto-rename on title change + sync heading
                if key == "title"
                    && let Some(ref rip) = root_index_path
                    && let serde_json::Value::String(ref new_title) = value
                    && !new_title.trim().is_empty()
                {
                    use crate::entry::apply_filename_style;

                    let ws_config = self
                        .workspace()
                        .inner()
                        .get_workspace_config(&self.resolve_fs_path(rip))
                        .await
                        .unwrap_or_default();

                    let mut effective_path = path.clone();

                    // Write the title FIRST so that rename_entry's resolve_title
                    // reads the new title when formatting links in parent contents
                    // and children's part_of references.
                    let yaml_value = json_to_yaml(value.clone());
                    self.entry()
                        .set_frontmatter_property(&path, &key, yaml_value)
                        .await?;

                    // Always auto-rename file to match title
                    {
                        let new_stem = apply_filename_style(new_title, &ws_config.filename_style);
                        let new_filename = format!("{}.md", new_stem);

                        let entry_path = self.resolve_fs_path(&path);
                        let ws = self.workspace().inner();
                        let is_index = ws.is_index_file(&entry_path).await;
                        let is_root = ws.is_root_index(&entry_path).await;

                        // Compare current name:
                        // - Non-root index: dir name (index lives in dirname/dirname.md)
                        // - Root index or leaf: file stem
                        let current_comparable = if is_index && !is_root {
                            entry_path
                                .parent()
                                .and_then(|p| p.file_name())
                                .and_then(|n| n.to_str())
                                .unwrap_or("")
                                .to_string()
                        } else {
                            entry_path
                                .file_stem()
                                .and_then(|n| n.to_str())
                                .unwrap_or("")
                                .to_string()
                        };

                        if current_comparable != new_stem {
                            let new_path = ws.rename_entry(&entry_path, &new_filename).await?;
                            let new_path_str = new_path.to_string_lossy().to_string();

                            // Migrate body CRDT doc to new path
                            {
                                let canonical_old = self.get_canonical_path(&path);
                                let canonical_new = self.get_canonical_path(&new_path_str);
                                if canonical_old != canonical_new {
                                    self.plugin_registry()
                                        .emit_body_doc_renamed(&canonical_old, &canonical_new)
                                        .await;
                                }
                            }

                            effective_path = new_path_str;
                        }
                    }

                    // Always sync title to H1 heading
                    self.sync_heading_to_title(&effective_path, new_title)
                        .await?;

                    // Emit workspace sync (covers both rename + frontmatter update)
                    self.emit_workspace_sync().await;

                    // Return new path if rename happened, Ok otherwise
                    if effective_path != path {
                        return Ok(Response::String(effective_path));
                    } else {
                        return Ok(Response::Ok);
                    }
                }

                // Default: just set the property as-is (non-title keys, or title without root_index_path)
                let yaml_value = json_to_yaml(value.clone());
                self.entry()
                    .set_frontmatter_property(&path, &key, yaml_value)
                    .await?;

                Ok(Response::Ok)
            }

            Command::RemoveFrontmatterProperty { path, key } => {
                // Remove property from frontmatter - CrdtFs extracts metadata automatically
                self.entry()
                    .remove_frontmatter_property(&path, &key)
                    .await?;

                // CrdtFs handles CRDT updates automatically via write_file hook.
                // We only need to track for echo detection and emit sync.
                {
                    if key == "part_of" || key == "contents" || key == "attachments" {
                        let canonical_path = self.get_canonical_path(&path);

                        // Track for echo detection
                        self.plugin_registry()
                            .track_file_for_sync(&canonical_path)
                            .await;

                        // Emit workspace sync message
                        self.emit_workspace_sync().await;
                    }
                }

                Ok(Response::Ok)
            }

            // === Workspace Operations ===
            Command::FindRootIndex { directory } => {
                let ws = self.workspace().inner();
                match ws.find_root_index_in_dir(Path::new(&directory)).await? {
                    Some(path) => Ok(Response::String(path.to_string_lossy().to_string())),
                    None => Err(DiaryxError::WorkspaceNotFound(PathBuf::from(&directory))),
                }
            }

            Command::GetAvailableAudiences { path } => {
                let resolved = self.resolve_fs_path(&path);
                let ws = self.workspace().inner();
                let link_format = Some(self.link_format());
                let mut audiences = std::collections::HashSet::new();
                let mut visited = std::collections::HashSet::new();
                let workspace_root = resolved.parent().unwrap_or(Path::new(".")).to_path_buf();

                Self::collect_audiences_recursive(
                    &ws,
                    &resolved,
                    &mut audiences,
                    &mut visited,
                    &workspace_root,
                    link_format,
                )
                .await;

                let mut result: Vec<String> = audiences.into_iter().collect();
                result.sort();
                Ok(Response::Strings(result))
            }

            Command::GetWorkspaceTree {
                path,
                depth,
                audience,
            } => {
                let root_path = path.unwrap_or_else(|| "workspace/index.md".to_string());
                let resolved_root_path = self.resolve_fs_path(&root_path);
                log::info!(
                    "[CommandHandler] GetWorkspaceTree called: path={}, resolved_path={}, depth={:?}, audience={:?}",
                    root_path,
                    resolved_root_path.display(),
                    depth,
                    audience
                );
                let tree = self
                    .workspace()
                    .inner()
                    .build_tree_with_depth(
                        &resolved_root_path,
                        depth.map(|d| d as usize),
                        &mut std::collections::HashSet::new(),
                    )
                    .await?;

                // If an audience filter is specified, prune nodes not visible to that audience
                let tree = if let Some(ref audience) = audience {
                    self.filter_tree_by_audience(tree, audience).await
                } else {
                    tree
                };

                log::info!(
                    "[CommandHandler] GetWorkspaceTree result: name={}, children_count={}",
                    tree.name,
                    tree.children.len()
                );
                Ok(Response::Tree(tree))
            }

            Command::GetFilesystemTree {
                path,
                show_hidden,
                depth,
            } => {
                let root_path = path.unwrap_or_else(|| "workspace".to_string());
                let tree = self
                    .workspace()
                    .inner()
                    .build_filesystem_tree_with_depth(
                        Path::new(&root_path),
                        show_hidden,
                        depth.map(|d| d as usize),
                    )
                    .await?;
                Ok(Response::Tree(tree))
            }

            // === Validation Operations ===
            Command::ValidateWorkspace { path } => {
                let root_path = path.ok_or_else(|| DiaryxError::InvalidPath {
                    path: PathBuf::new(),
                    message: "ValidateWorkspace requires a root index path".to_string(),
                })?;
                let resolved_root_path = self.resolve_fs_path(&root_path);
                // Use depth limit of 2 to match tree view (TREE_INITIAL_DEPTH in App.svelte)
                // This significantly improves performance for large workspaces
                let result = self
                    .validate()
                    .validate_workspace(&resolved_root_path, Some(2))
                    .await?;
                // Include computed metadata for frontend display
                Ok(Response::ValidationResult(result.with_metadata()))
            }

            Command::ValidateFile { path } => {
                let resolved_path = self.resolve_fs_path(&path);
                let result = self.validate().validate_file(&resolved_path).await?;
                // Include computed metadata for frontend display
                Ok(Response::ValidationResult(result.with_metadata()))
            }

            Command::FixBrokenPartOf { path } => {
                let resolved_path = self.resolve_fs_path(&path);
                let result = self
                    .validate()
                    .fixer()
                    .fix_broken_part_of(&resolved_path)
                    .await;

                if result.success {
                    self.emit_workspace_sync().await;
                }

                Ok(Response::FixResult(result))
            }

            Command::FixBrokenContentsRef { index_path, target } => {
                let resolved_index_path = self.resolve_fs_path(&index_path);
                let result = self
                    .validate()
                    .fixer()
                    .fix_broken_contents_ref(&resolved_index_path, &target)
                    .await;

                if result.success {
                    self.emit_workspace_sync().await;
                }

                Ok(Response::FixResult(result))
            }

            // === Search Operations ===
            Command::SearchWorkspace { pattern, options } => {
                use crate::search::SearchQuery;

                let query = if options.search_frontmatter {
                    if let Some(prop) = options.property {
                        SearchQuery::property(&pattern, prop)
                    } else {
                        SearchQuery::frontmatter(&pattern)
                    }
                } else {
                    SearchQuery::content(&pattern)
                }
                .case_sensitive(options.case_sensitive);

                let workspace_path = options
                    .workspace_path
                    .unwrap_or_else(|| "workspace/index.md".to_string());
                let resolved_workspace_path = self.resolve_fs_path(&workspace_path);
                let results = self
                    .search()
                    .search_workspace(&resolved_workspace_path, &query)
                    .await?;
                Ok(Response::SearchResults(results))
            }

            // === File System Operations ===
            Command::FileExists { path } => {
                let resolved_path = self.resolve_fs_path(&path);
                let exists = self.fs().exists(&resolved_path).await;
                Ok(Response::Bool(exists))
            }

            Command::ReadFile { path } => {
                let content = self.entry().read_raw(&path).await?;
                Ok(Response::String(content))
            }

            Command::WriteFile { path, content } => {
                let resolved_path = self.resolve_fs_path(&path);
                self.fs()
                    .write_file(&resolved_path, &content)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: resolved_path.clone(),
                        source: e,
                    })?;
                Ok(Response::Ok)
            }

            Command::DeleteFile { path } => {
                let resolved_path = self.resolve_fs_path(&path);
                self.fs().delete_file(&resolved_path).await.map_err(|e| {
                    DiaryxError::FileWrite {
                        path: resolved_path,
                        source: e,
                    }
                })?;
                Ok(Response::Ok)
            }

            Command::ClearDirectory { path } => {
                let resolved_path = self.resolve_fs_path(&path);
                self.fs()
                    .clear_dir(&resolved_path)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: resolved_path,
                        source: e,
                    })?;
                Ok(Response::Ok)
            }

            Command::WriteFileWithMetadata {
                path,
                metadata,
                body,
            } => {
                let resolved_path = self.resolve_fs_path(&path);
                crate::metadata_writer::write_file_with_metadata(
                    self.fs(),
                    &resolved_path,
                    &metadata,
                    &body,
                )
                .await?;
                Ok(Response::Ok)
            }

            Command::UpdateFileMetadata {
                path,
                metadata,
                body,
            } => {
                let resolved_path = self.resolve_fs_path(&path);
                crate::metadata_writer::update_file_metadata(
                    self.fs(),
                    &resolved_path,
                    &metadata,
                    body.as_deref(),
                )
                .await?;
                Ok(Response::Ok)
            }

            // === Attachment Operations ===
            Command::GetAttachments { path } => {
                let attachments = self.entry().get_attachments(&path).await?;
                Ok(Response::Strings(attachments))
            }

            Command::GetAncestorAttachments { path } => {
                use crate::command::{AncestorAttachmentEntry, AncestorAttachmentsResult};
                use std::collections::HashSet;

                let ws = self.workspace().inner();
                let mut entries = Vec::new();
                let mut visited = HashSet::new();
                let mut current_path = self.resolve_fs_path(&path);

                // Get workspace root for resolving workspace-relative paths
                let workspace_root = self.workspace_root().unwrap_or_else(|| {
                    current_path
                        .parent()
                        .and_then(|p| p.parent())
                        .unwrap_or(Path::new("."))
                        .to_path_buf()
                });

                // Try to get link format from workspace config
                let link_format = ws
                    .get_workspace_config(&current_path)
                    .await
                    .map(|c| c.link_format)
                    .ok();

                // Maximum depth to prevent runaway traversal
                const MAX_DEPTH: usize = 100;

                // Traverse up the part_of chain
                for _ in 0..MAX_DEPTH {
                    let path_str = current_path.to_string_lossy().to_string();
                    if visited.contains(&path_str) {
                        break; // Circular reference protection
                    }
                    visited.insert(path_str.clone());

                    // Try to parse the file (with link format hint)
                    if let Ok(index) = ws.parse_index_with_hint(&current_path, link_format).await {
                        let attachments = index.frontmatter.attachments_list().to_vec();

                        // Only add if there are attachments
                        if !attachments.is_empty() {
                            entries.push(AncestorAttachmentEntry {
                                entry_path: path_str,
                                entry_title: index.frontmatter.title.clone(),
                                attachments,
                            });
                        }

                        // Move to parent via part_of
                        if let Some(part_of) = &index.frontmatter.part_of {
                            let parent_path = index.resolve_path(part_of);
                            // Make path absolute if needed
                            current_path = if parent_path.is_absolute() {
                                parent_path
                            } else {
                                workspace_root.join(&parent_path)
                            };
                        } else {
                            break; // Reached root
                        }
                    } else {
                        break; // File doesn't exist or can't be parsed
                    }
                }

                Ok(Response::AncestorAttachments(AncestorAttachmentsResult {
                    entries,
                }))
            }

            // === Entry Creation/Deletion Operations ===
            Command::CreateEntry { path, options } => {
                // Derive title from filename if not provided
                let path_buf = PathBuf::from(&path);
                let title = options.title.clone().unwrap_or_else(|| {
                    path_buf
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Untitled")
                        .to_string()
                });

                // Resolve template: workspace config link → built-in "note"
                let content = if let Some(ref rip) = options.root_index_path {
                    let workspace_root_path = self.resolve_fs_path(rip);
                    let ws_config = self
                        .workspace()
                        .inner()
                        .get_workspace_config(&workspace_root_path)
                        .await
                        .ok();
                    let tmpl = if let Some(ref cfg) = ws_config
                        && let Some(ref tmpl_link) = cfg.default_template
                    {
                        self.resolve_template_from_link(tmpl_link, &workspace_root_path)
                            .await
                    } else {
                        None
                    };
                    let tmpl = tmpl.unwrap_or_else(crate::template::Template::builtin_note);
                    let ctx = crate::template::TemplateContext::new()
                        .with_title(&title)
                        .with_filename(
                            path_buf
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("untitled"),
                        );
                    tmpl.render(&ctx)
                } else {
                    // No workspace context — use simple hardcoded template
                    format!("---\ntitle: {}\n---\n\n# {}\n\n", title, title)
                };

                // CrdtFs.create_new extracts metadata from frontmatter automatically
                let resolved_path = self.resolve_fs_path(&path);
                self.fs()
                    .create_new(&resolved_path, &content)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: resolved_path.clone(),
                        source: e,
                    })?;

                // Set part_of if provided - format based on configured link format
                // CrdtFs.write_file (via set_frontmatter_property) extracts updated metadata
                if let Some(ref parent) = options.part_of {
                    let _formatted_link = {
                        let canonical_path = self.get_canonical_path(&path);
                        let canonical_parent = self.get_canonical_path(parent);
                        self.format_link_for_file(&canonical_parent, &canonical_path)
                    };
                    let formatted_link = {
                        let canonical_path = &path;
                        self.format_link_for_file(parent, canonical_path)
                    };
                    self.entry()
                        .set_frontmatter_property(&path, "part_of", Value::String(formatted_link))
                        .await?;
                }

                // CrdtFs handles CRDT updates automatically via create_new and write_file hooks.
                // We only need to track for echo detection and emit sync.
                {
                    let canonical_path = self.get_canonical_path(&path);

                    // Track for echo detection
                    self.plugin_registry()
                        .track_file_for_sync(&canonical_path)
                        .await;

                    // Emit workspace sync message
                    self.emit_workspace_sync().await;

                    log::debug!(
                        "[CommandHandler] CreateEntry: created {} (CrdtFs handled CRDT)",
                        canonical_path
                    );
                }

                // Emit file-created event to file plugins
                self.plugin_registry()
                    .emit_file_created(&FileCreatedEvent { path: path.clone() })
                    .await;

                Ok(Response::String(path))
            }

            Command::DeleteEntry {
                path,
                hard_delete: _hard_delete,
            } => {
                // Use Workspace::delete_entry which handles contents cleanup
                // CrdtFs.delete_file marks as deleted and updates parent contents automatically
                let resolved_path = self.resolve_fs_path(&path);
                let ws = self.workspace().inner();
                ws.delete_entry(&resolved_path).await?;

                // Delete body doc CRDT and emit sync
                {
                    self.plugin_registry().emit_body_doc_deleted(&path).await;
                    self.emit_workspace_sync().await;

                    log::debug!(
                        "[CommandHandler] DeleteEntry: deleted {} (CrdtFs handled CRDT)",
                        path
                    );
                }

                // Emit file-deleted event to file plugins
                self.plugin_registry()
                    .emit_file_deleted(&FileDeletedEvent { path: path.clone() })
                    .await;

                Ok(Response::Ok)
            }

            Command::MoveEntry { from, to } => {
                if from == to {
                    return Ok(Response::String(to));
                }

                // Use Workspace::move_entry which handles contents/part_of updates
                let resolved_from = self.resolve_fs_path(&from);
                let resolved_to = self.resolve_fs_path(&to);
                let ws = self.workspace().inner();
                ws.move_entry(&resolved_from, &resolved_to).await?;

                // Migrate body doc CRDT to new path
                self.plugin_registry()
                    .emit_body_doc_renamed(&from, &to)
                    .await;

                // Emit file-moved event to file plugins
                self.plugin_registry()
                    .emit_file_moved(&FileMovedEvent {
                        old_path: from,
                        new_path: to.clone(),
                    })
                    .await;

                Ok(Response::String(to))
            }

            Command::SyncMoveMetadata { old_path, new_path } => {
                if old_path == new_path {
                    return Ok(Response::String(new_path));
                }

                // Use Workspace::sync_move_metadata — file is already at new_path,
                // only update contents/part_of metadata in parent indexes.
                let resolved_old = self.resolve_fs_path(&old_path);
                let resolved_new = self.resolve_fs_path(&new_path);
                let ws = self.workspace().inner();
                ws.sync_move_metadata(&resolved_old, &resolved_new).await?;

                // Migrate body doc CRDT to new path
                self.plugin_registry()
                    .emit_body_doc_renamed(&old_path, &new_path)
                    .await;

                Ok(Response::String(new_path))
            }

            Command::SyncCreateMetadata { path } => {
                let resolved_path = self.resolve_fs_path(&path);
                let ws = self.workspace().inner();
                ws.sync_create_metadata(&resolved_path).await?;

                {
                    self.emit_workspace_sync().await;

                    log::debug!(
                        "[CommandHandler] SyncCreateMetadata: added {} to hierarchy",
                        path
                    );
                }

                Ok(Response::Ok)
            }

            Command::SyncDeleteMetadata { path } => {
                let resolved_path = self.resolve_fs_path(&path);
                let ws = self.workspace().inner();
                ws.sync_delete_metadata(&resolved_path).await?;

                {
                    self.emit_workspace_sync().await;

                    log::debug!(
                        "[CommandHandler] SyncDeleteMetadata: removed {} from hierarchy",
                        path
                    );
                }

                Ok(Response::Ok)
            }

            Command::RenameEntry { path, new_filename } => {
                let from_path = self.resolve_fs_path(&path);

                // Use rename_entry which handles both leaf files and index files
                // (directory rename + children migration + part_of/contents updates)
                let ws = self.workspace().inner();
                let new_path = ws.rename_entry(&from_path, &new_filename).await?;
                let to_path_str = new_path.to_string_lossy().to_string();

                // Migrate body doc and emit sync
                {
                    let canonical_old = self.get_canonical_path(&path);
                    let canonical_new = self.get_canonical_path(&to_path_str);

                    if canonical_old != canonical_new {
                        self.plugin_registry()
                            .emit_body_doc_renamed(&canonical_old, &canonical_new)
                            .await;
                    }

                    self.emit_workspace_sync().await;

                    log::debug!(
                        "[CommandHandler] RenameEntry: renamed {} -> {} (CrdtFs handled CRDT)",
                        canonical_old,
                        canonical_new
                    );
                }

                // Sync title and H1 to match the new filename
                {
                    use crate::entry::prettify_filename;
                    let title = prettify_filename(new_filename.trim_end_matches(".md"));
                    self.entry()
                        .set_frontmatter_property(
                            &to_path_str,
                            "title",
                            serde_yaml::Value::String(title.clone()),
                        )
                        .await?;
                    self.sync_heading_to_title(&to_path_str, &title).await?;
                }

                // Emit file-moved event to file plugins
                self.plugin_registry()
                    .emit_file_moved(&FileMovedEvent {
                        old_path: path,
                        new_path: to_path_str.clone(),
                    })
                    .await;

                Ok(Response::String(to_path_str))
            }

            Command::DuplicateEntry { path } => {
                // workspace.duplicate_entry uses fs.write_file which goes through CrdtFs
                // CrdtFs extracts metadata from frontmatter automatically
                let resolved_path = self.resolve_fs_path(&path);
                let ws = self.workspace().inner();
                let new_path = ws.duplicate_entry(&resolved_path).await?;
                let new_path_str = new_path.to_string_lossy().to_string();

                // CrdtFs handles CRDT updates automatically via write_file hooks.
                // We only need to track for echo detection and emit sync.
                {
                    let canonical_path = self.get_canonical_path(&new_path_str);

                    // Track for echo detection
                    self.plugin_registry()
                        .track_file_for_sync(&canonical_path)
                        .await;

                    // Emit workspace sync message
                    self.emit_workspace_sync().await;

                    log::debug!(
                        "[CommandHandler] DuplicateEntry: duplicated {} (CrdtFs handled CRDT)",
                        canonical_path
                    );
                }

                Ok(Response::String(new_path_str))
            }

            // === Hierarchy Operations ===
            Command::ConvertToIndex { path } => {
                let fm = self.entry().get_frontmatter(&path).await?;

                // Check if already has contents
                if fm.contains_key("contents") {
                    return Ok(Response::String(path));
                }

                // Add empty contents array to frontmatter
                // CrdtFs.write_file extracts contents: [] from frontmatter automatically
                self.entry()
                    .set_frontmatter_property(&path, "contents", Value::Sequence(vec![]))
                    .await?;

                // CrdtFs handles CRDT updates automatically via write_file hook.
                // We only need to track for echo detection and emit sync.
                {
                    let canonical_path = self.get_canonical_path(&path);

                    // Track for echo detection
                    self.plugin_registry()
                        .track_file_for_sync(&canonical_path)
                        .await;

                    // Emit workspace sync for hierarchy change
                    self.emit_workspace_sync().await;
                }

                Ok(Response::String(path))
            }

            Command::ConvertToLeaf { path } => {
                // Remove contents property from frontmatter
                // CrdtFs.write_file detects absence of contents property automatically
                self.entry()
                    .remove_frontmatter_property(&path, "contents")
                    .await?;

                // CrdtFs handles CRDT updates automatically via write_file hook.
                // We only need to track for echo detection and emit sync.
                {
                    let canonical_path = self.get_canonical_path(&path);

                    // Track for echo detection
                    self.plugin_registry()
                        .track_file_for_sync(&canonical_path)
                        .await;

                    // Emit workspace sync for hierarchy change
                    self.emit_workspace_sync().await;
                }

                Ok(Response::String(path))
            }

            Command::CreateChildEntry { parent_path } => {
                let ws = self.workspace().inner();
                let resolved_parent_path = self.resolve_fs_path(&parent_path);
                // workspace.create_child_entry_with_result:
                // 1. Converts parent to index if needed (moves parent.md to parent/parent.md)
                // 2. Creates child file with frontmatter (title, part_of)
                // 3. Updates parent's contents array
                // 4. Returns detailed result with both child and (possibly new) parent paths
                // All file writes go through CrdtFs which extracts metadata from frontmatter
                let result = ws
                    .create_child_entry_with_result(&resolved_parent_path, None)
                    .await?;

                // CrdtFs handles CRDT updates automatically via create_new and write_file hooks.
                // We only need to track for echo detection and emit sync.
                {
                    let canonical_child = self.get_canonical_path(&result.child_path);

                    // Track for echo detection
                    self.plugin_registry()
                        .track_file_for_sync(&canonical_child)
                        .await;

                    // Emit workspace sync message
                    self.emit_workspace_sync().await;

                    log::debug!(
                        "[CommandHandler] CreateChildEntry: created {} (parent_converted={}, CrdtFs handled CRDT)",
                        canonical_child,
                        result.parent_converted
                    );
                }

                Ok(Response::CreateChildResult(result))
            }

            Command::AttachEntryToParent {
                entry_path,
                parent_path,
            } => {
                // workspace.attach_and_move_entry_to_parent uses move operations via CrdtFs
                // CrdtFs handles: marking old deleted, creating new entry, updating parent contents
                let resolved_entry_path = self.resolve_fs_path(&entry_path);
                let resolved_parent_path = self.resolve_fs_path(&parent_path);
                let ws = self.workspace().inner();
                let new_path = ws
                    .attach_and_move_entry_to_parent(&resolved_entry_path, &resolved_parent_path)
                    .await?;
                let new_path_str = new_path.to_string_lossy().to_string();

                // CrdtFs handles CRDT updates automatically via move_file hooks.
                // We only need to migrate body doc and emit sync.
                {
                    let canonical_old = self.get_canonical_path(&entry_path);
                    let canonical_new = self.get_canonical_path(&new_path_str);

                    // Migrate body doc CRDT to new path
                    if canonical_old != canonical_new {
                        self.plugin_registry()
                            .emit_body_doc_renamed(&canonical_old, &canonical_new)
                            .await;
                    }

                    // Emit workspace sync message
                    self.emit_workspace_sync().await;

                    log::debug!(
                        "[CommandHandler] AttachEntryToParent: moved {} -> {} (CrdtFs handled CRDT)",
                        canonical_old,
                        canonical_new
                    );
                }

                Ok(Response::String(new_path_str))
            }

            // === Workspace Operations ===
            Command::CreateWorkspace { path, name } => {
                let ws_path = path.unwrap_or_else(|| "workspace".to_string());
                let ws_name = name.as_deref();
                let ws = self.workspace().inner();
                let readme_path = ws
                    .init_workspace(Path::new(&ws_path), ws_name, None)
                    .await?;
                Ok(Response::String(readme_path.to_string_lossy().to_string()))
            }

            // === Validation Fix Operations ===
            Command::FixBrokenAttachment { path, attachment } => {
                let resolved_path = self.resolve_fs_path(&path);
                let result = self
                    .validate()
                    .fixer()
                    .fix_broken_attachment(&resolved_path, &attachment)
                    .await;

                if result.success {
                    self.emit_workspace_sync().await;
                }

                Ok(Response::FixResult(result))
            }

            Command::FixNonPortablePath {
                path,
                property,
                old_value,
                new_value,
            } => {
                let resolved_path = self.resolve_fs_path(&path);
                let result = self
                    .validate()
                    .fixer()
                    .fix_non_portable_path(&resolved_path, &property, &old_value, &new_value)
                    .await;

                if result.success {
                    self.emit_workspace_sync().await;
                }

                Ok(Response::FixResult(result))
            }

            Command::FixUnlistedFile {
                index_path,
                file_path,
            } => {
                let resolved_index_path = self.resolve_fs_path(&index_path);
                let resolved_file_path = self.resolve_fs_path(&file_path);
                let result = self
                    .validate()
                    .fixer()
                    .fix_unlisted_file(&resolved_index_path, &resolved_file_path)
                    .await;

                if result.success {
                    self.emit_workspace_sync().await;
                }

                Ok(Response::FixResult(result))
            }

            Command::FixOrphanBinaryFile {
                index_path,
                file_path,
            } => {
                let resolved_index_path = self.resolve_fs_path(&index_path);
                let resolved_file_path = self.resolve_fs_path(&file_path);
                let result = self
                    .validate()
                    .fixer()
                    .fix_orphan_binary_file(&resolved_index_path, &resolved_file_path)
                    .await;

                if result.success {
                    self.emit_workspace_sync().await;
                }

                Ok(Response::FixResult(result))
            }

            Command::FixMissingPartOf {
                file_path,
                index_path,
            } => {
                let resolved_file_path = self.resolve_fs_path(&file_path);
                let resolved_index_path = self.resolve_fs_path(&index_path);
                let result = self
                    .validate()
                    .fixer()
                    .fix_missing_part_of(&resolved_file_path, &resolved_index_path)
                    .await;

                if result.success {
                    self.emit_workspace_sync().await;
                }

                Ok(Response::FixResult(result))
            }

            Command::FixAll { validation_result } => {
                let fixer = self.validate().fixer();
                let (error_fixes, warning_fixes) = fixer.fix_all(&validation_result).await;

                let total_fixed = error_fixes.iter().filter(|r| r.success).count()
                    + warning_fixes.iter().filter(|r| r.success).count();
                let total_failed = error_fixes.iter().filter(|r| !r.success).count()
                    + warning_fixes.iter().filter(|r| !r.success).count();

                if total_fixed > 0 {
                    self.emit_workspace_sync().await;
                }

                Ok(Response::FixSummary(crate::command::FixSummary {
                    error_fixes,
                    warning_fixes,
                    total_fixed,
                    total_failed,
                }))
            }

            Command::FixCircularReference {
                file_path,
                part_of_value,
            } => {
                let resolved_file_path = self.resolve_fs_path(&file_path);
                let result = self
                    .validate()
                    .fixer()
                    .fix_circular_reference(&resolved_file_path, &part_of_value)
                    .await;

                if result.success {
                    self.emit_workspace_sync().await;
                }

                Ok(Response::FixResult(result))
            }

            Command::GetAvailableParentIndexes {
                file_path,
                workspace_root,
            } => {
                // Find all index files between the file and the workspace root
                let ws = self.workspace().inner();
                let resolved_file_path = self.resolve_fs_path(&file_path);
                let resolved_workspace_root = self.resolve_fs_path(&workspace_root);
                let file = resolved_file_path.as_path();
                let root_index = resolved_workspace_root.as_path();
                let root_dir = root_index.parent().unwrap_or(root_index);

                let mut parents = Vec::new();

                // Start from the file's directory and walk up to the workspace root
                let file_dir = file.parent().unwrap_or(Path::new("."));
                let mut current = file_dir.to_path_buf();

                loop {
                    // Look for index files in this directory
                    if let Ok(files) = ws.fs_ref().list_files(&current).await {
                        for file_path in files {
                            // Check if it's a markdown file
                            if file_path.extension().is_some_and(|ext| ext == "md")
                                && !ws.fs_ref().is_dir(&file_path).await
                            {
                                // Try to parse and check if it has contents (is an index)
                                if let Ok(index) = ws.parse_index(&file_path).await
                                    && index.frontmatter.is_index()
                                {
                                    parents.push(file_path.to_string_lossy().to_string());
                                }
                            }
                        }
                    }

                    // Stop if we've reached or passed the workspace root
                    if current == root_dir || !current.starts_with(root_dir) {
                        break;
                    }

                    // Go up one level
                    match current.parent() {
                        Some(parent) if parent != current => {
                            current = parent.to_path_buf();
                        }
                        _ => break,
                    }
                }

                // Always include the workspace root if not already present
                let root_str = root_index.to_string_lossy().to_string();
                if !parents.contains(&root_str) && ws.fs_ref().exists(root_index).await {
                    parents.push(root_str);
                }

                // Sort for consistent ordering
                parents.sort();
                Ok(Response::Strings(parents))
            }

            // === Attachment Operations ===
            Command::UploadAttachment {
                entry_path,
                filename,
                data_base64,
            } => {
                use base64::{Engine as _, engine::general_purpose::STANDARD};

                let entry = PathBuf::from(&entry_path);
                let entry_dir = entry.parent().unwrap_or_else(|| Path::new("."));
                let attachments_dir = entry_dir.join("_attachments");

                // Resolve against workspace root for direct fs operations
                let resolved_attachments_dir = self.resolve_fs_path(&attachments_dir);

                // Create _attachments directory if needed
                self.fs().create_dir_all(&resolved_attachments_dir).await?;

                // Decode base64 data
                let data = STANDARD.decode(&data_base64).map_err(|e| {
                    DiaryxError::Unsupported(format!("Failed to decode base64: {}", e))
                })?;

                // Write file
                let dest_path = resolved_attachments_dir.join(&filename);
                self.fs()
                    .write_binary(&dest_path, &data)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: dest_path.clone(),
                        source: e,
                    })?;

                // Build canonical path for the attachment (entry_dir + _attachments/filename)
                let attachment_rel_path = format!("_attachments/{}", filename);
                let entry_parent = Path::new(&entry_path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                let canonical_attachment = if entry_parent.is_empty() {
                    attachment_rel_path.clone()
                } else {
                    format!("{}/{}", entry_parent, attachment_rel_path)
                };

                // Format using the configured workspace link format.
                let entry_canonical = self.get_canonical_path(&entry_path);
                let link =
                    self.format_attachment_link_for_file(&canonical_attachment, &entry_canonical);

                self.entry().add_attachment(&entry_path, &link).await?;

                Ok(Response::String(link))
            }

            Command::DeleteAttachment {
                entry_path,
                attachment_path,
            } => {
                let rel_path = resolve_attachment_storage_path(&entry_path, &attachment_path);
                let full_path = self.resolve_fs_path(&rel_path);

                // Delete the file if it exists
                if self.fs().exists(&full_path).await {
                    self.fs().delete_file(&full_path).await.map_err(|e| {
                        DiaryxError::FileWrite {
                            path: full_path,
                            source: e,
                        }
                    })?;
                }

                // Remove from frontmatter
                self.entry()
                    .remove_attachment(&entry_path, &attachment_path)
                    .await?;

                Ok(Response::Ok)
            }

            Command::GetAttachmentData {
                entry_path,
                attachment_path,
            } => {
                let rel_path = resolve_attachment_storage_path(&entry_path, &attachment_path);
                let full_path = self.resolve_fs_path(&rel_path);

                let data =
                    self.fs()
                        .read_binary(&full_path)
                        .await
                        .map_err(|e| DiaryxError::FileRead {
                            path: full_path,
                            source: e,
                        })?;

                Ok(Response::Bytes(data))
            }

            Command::ResolveAttachmentPath {
                entry_path,
                attachment_path,
            } => {
                let rel_path = resolve_attachment_storage_path(&entry_path, &attachment_path);
                let full_path = self.resolve_fs_path(&rel_path);
                Ok(Response::String(full_path.to_string_lossy().into_owned()))
            }

            Command::MoveAttachment {
                source_entry_path,
                target_entry_path,
                attachment_path,
                new_filename,
            } => {
                // Resolve source attachment path from the link/path reference.
                let source_rel_path =
                    resolve_attachment_storage_path(&source_entry_path, &attachment_path);
                let source_attachment_path = self.resolve_fs_path(&source_rel_path);

                // Get the original filename
                let original_filename = source_rel_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| DiaryxError::InvalidPath {
                        path: source_rel_path.clone(),
                        message: "Could not extract filename".to_string(),
                    })?;

                // Determine final filename (use new_filename if provided, otherwise original)
                let final_filename = new_filename.as_deref().unwrap_or(original_filename);

                // Resolve target paths
                let target_entry = PathBuf::from(&target_entry_path);
                let target_dir = target_entry.parent().unwrap_or_else(|| Path::new("."));
                let target_attachments_dir = target_dir.join("_attachments");
                let target_attachment_path =
                    self.resolve_fs_path(target_attachments_dir.join(final_filename));

                // Check for collision at destination
                if self.fs().exists(&target_attachment_path).await {
                    return Err(DiaryxError::InvalidPath {
                        path: target_attachment_path,
                        message: "File already exists at destination".to_string(),
                    });
                }

                // Read the source file data
                let data = self
                    .fs()
                    .read_binary(&source_attachment_path)
                    .await
                    .map_err(|e| DiaryxError::FileRead {
                        path: source_attachment_path.clone(),
                        source: e,
                    })?;

                // Create target _attachments directory if needed
                let resolved_target_attachments_dir = self.resolve_fs_path(&target_attachments_dir);
                self.fs()
                    .create_dir_all(&resolved_target_attachments_dir)
                    .await?;

                // Write to target location
                self.fs()
                    .write_binary(&target_attachment_path, &data)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: target_attachment_path.clone(),
                        source: e,
                    })?;

                // Update frontmatter: remove from source, add to target
                self.entry()
                    .remove_attachment(&source_entry_path, &attachment_path)
                    .await?;
                let target_rel_path = format!("_attachments/{}", final_filename);
                let target_entry_canonical = self.get_canonical_path(&target_entry_path);
                let target_canonical_path =
                    self.resolve_frontmatter_link_target(&target_rel_path, &target_entry_canonical);
                let formatted_target = self.format_attachment_link_for_file(
                    &target_canonical_path,
                    &target_entry_canonical,
                );
                self.entry()
                    .add_attachment(&target_entry_path, &formatted_target)
                    .await?;

                // Delete the original file
                self.fs()
                    .delete_file(&source_attachment_path)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: source_attachment_path,
                        source: e,
                    })?;

                // Return canonical path so callers can match CRDT BinaryRef paths directly.
                Ok(Response::String(target_canonical_path))
            }

            // === Import Operations ===
            Command::ImportEntries {
                entries_json,
                folder,
                parent_path,
            } => {
                let entries: Vec<crate::import::ImportedEntry> =
                    serde_json::from_str(&entries_json).map_err(|e| DiaryxError::InvalidPath {
                        path: PathBuf::from("<import entries>"),
                        message: format!("Invalid ImportedEntry JSON: {e}"),
                    })?;

                let workspace_root = self.workspace_root().unwrap_or_else(|| PathBuf::from("."));
                let result = crate::import::orchestrate::write_entries(
                    self.fs(),
                    &workspace_root,
                    &folder,
                    &entries,
                    parent_path.as_deref(),
                )
                .await;

                Ok(Response::ImportResult(result))
            }

            Command::ImportDirectoryInPlace { path } => {
                let root = path
                    .map(|p| self.resolve_fs_path(&p))
                    .unwrap_or_else(|| PathBuf::from(""));
                let result =
                    crate::import::directory::import_directory_in_place(self.fs(), &root).await?;
                Ok(Response::ImportResult(result))
            }

            // === Storage Operations ===
            Command::GetStorageUsage => {
                // This requires knowledge of the workspace path which we don't have
                // Return basic info - clients can calculate usage themselves
                Ok(Response::StorageInfo(crate::command::StorageInfo {
                    used: 0,
                    limit: None,
                    attachment_limit: None,
                }))
            }

            // === Workspace Configuration Commands ===
            Command::GetLinkFormat { root_index_path } => {
                let ws = self.workspace().inner();
                let resolved_root_index_path = self.resolve_fs_path(&root_index_path);
                let format = ws.get_link_format(&resolved_root_index_path).await?;
                Ok(Response::LinkFormat(format))
            }

            Command::SetLinkFormat {
                root_index_path,
                format,
            } => {
                let link_format = match format.as_str() {
                    "markdown_root" => link_parser::LinkFormat::MarkdownRoot,
                    "markdown_relative" => link_parser::LinkFormat::MarkdownRelative,
                    "plain_relative" => link_parser::LinkFormat::PlainRelative,
                    "plain_canonical" => link_parser::LinkFormat::PlainCanonical,
                    _ => {
                        return Err(DiaryxError::InvalidPath {
                            path: PathBuf::from(&format),
                            message: format!(
                                "Invalid link format '{}'. Must be one of: markdown_root, markdown_relative, plain_relative, plain_canonical",
                                format
                            ),
                        });
                    }
                };

                let ws = self.workspace().inner();
                let resolved_root_index_path = self.resolve_fs_path(&root_index_path);
                ws.set_link_format(&resolved_root_index_path, link_format)
                    .await?;
                Ok(Response::Ok)
            }

            Command::GetWorkspaceConfig { root_index_path } => {
                let ws = self.workspace().inner();
                let resolved_root_index_path = self.resolve_fs_path(&root_index_path);
                let config = ws.get_workspace_config(&resolved_root_index_path).await?;
                Ok(Response::WorkspaceConfig(config))
            }

            Command::GenerateFilename {
                title,
                root_index_path,
            } => {
                use crate::entry::apply_filename_style;
                use crate::workspace::FilenameStyle;

                let style = if let Some(ref root_path) = root_index_path {
                    let ws = self.workspace().inner();
                    let resolved_root_path = self.resolve_fs_path(root_path);
                    let config = ws.get_workspace_config(&resolved_root_path).await?;
                    config.filename_style
                } else {
                    FilenameStyle::default()
                };
                let stem = apply_filename_style(&title, &style);
                Ok(Response::String(format!("{}.md", stem)))
            }

            Command::SetWorkspaceConfig {
                root_index_path,
                field,
                value,
            } => {
                let ws = self.workspace().inner();
                let resolved_root_index_path = self.resolve_fs_path(&root_index_path);
                ws.set_workspace_config_field(&resolved_root_index_path, &field, &value)
                    .await?;
                Ok(Response::Ok)
            }

            Command::ConvertLinks {
                root_index_path,
                format,
                path,
                dry_run,
            } => {
                let target_format = match format.as_str() {
                    "markdown_root" => link_parser::LinkFormat::MarkdownRoot,
                    "markdown_relative" => link_parser::LinkFormat::MarkdownRelative,
                    "plain_relative" => link_parser::LinkFormat::PlainRelative,
                    "plain_canonical" => link_parser::LinkFormat::PlainCanonical,
                    _ => {
                        return Err(DiaryxError::InvalidPath {
                            path: PathBuf::from(&format),
                            message: format!(
                                "Invalid link format '{}'. Must be one of: markdown_root, markdown_relative, plain_relative, plain_canonical",
                                format
                            ),
                        });
                    }
                };

                let resolved_root_index_path = self.resolve_fs_path(&root_index_path);
                let resolved_specific_path = path
                    .as_deref()
                    .map(|p| self.resolve_fs_path(p).to_string_lossy().to_string());
                let result = self
                    .convert_workspace_links(
                        &resolved_root_index_path,
                        target_format,
                        resolved_specific_path.as_deref(),
                        dry_run,
                    )
                    .await?;

                Ok(Response::ConvertLinksResult(result))
            }

            Command::LinkParser { operation } => {
                let result = match operation {
                    crate::command::LinkParserOperation::Parse { link } => {
                        let parsed = link_parser::parse_link(&link);
                        let path_type = match parsed.path_type {
                            link_parser::PathType::WorkspaceRoot => {
                                crate::command::LinkPathType::WorkspaceRoot
                            }
                            link_parser::PathType::Relative => {
                                crate::command::LinkPathType::Relative
                            }
                            link_parser::PathType::Ambiguous => {
                                crate::command::LinkPathType::Ambiguous
                            }
                        };
                        crate::command::LinkParserResult::Parsed(crate::command::ParsedLinkResult {
                            title: parsed.title,
                            path: parsed.path,
                            path_type,
                        })
                    }
                    crate::command::LinkParserOperation::ToCanonical {
                        link,
                        current_file_path,
                        link_format_hint,
                    } => {
                        let parsed = link_parser::parse_link(&link);
                        let canonical = link_parser::to_canonical_with_link_format(
                            &parsed,
                            Path::new(&current_file_path),
                            link_format_hint,
                        );
                        crate::command::LinkParserResult::String(canonical)
                    }
                    crate::command::LinkParserOperation::Format {
                        canonical_path,
                        title,
                        format,
                        from_canonical_path,
                    } => crate::command::LinkParserResult::String(
                        link_parser::format_link_with_format(
                            &canonical_path,
                            &title,
                            format,
                            &from_canonical_path,
                        ),
                    ),
                    crate::command::LinkParserOperation::Convert {
                        link,
                        target_format,
                        current_file_path,
                        source_format_hint,
                    } => crate::command::LinkParserResult::String(
                        link_parser::convert_link_with_hint(
                            &link,
                            target_format,
                            &current_file_path,
                            None,
                            source_format_hint,
                        ),
                    ),
                };

                Ok(Response::LinkParserResult(result))
            }

            // ── Naming / URL Validation ──────────────────────────────────
            Command::ValidateWorkspaceName {
                name,
                existing_local_names,
                existing_server_names,
            } => {
                use crate::utils::naming;
                naming::validate_workspace_name(
                    &name,
                    &existing_local_names,
                    existing_server_names.as_deref(),
                )
                .map(Response::String)
                .map_err(DiaryxError::Validation)
            }

            Command::ValidatePublishingSlug { slug } => {
                use crate::utils::naming;
                naming::validate_publishing_slug(&slug)
                    .map(|()| Response::Ok)
                    .map_err(DiaryxError::Validation)
            }

            Command::NormalizeServerUrl { url } => {
                use crate::utils::naming;
                Ok(Response::String(naming::normalize_server_url(&url)))
            }

            Command::ToWebSocketSyncUrl { url } => {
                use crate::utils::naming;
                Ok(Response::String(naming::to_websocket_sync_url(&url)))
            }

            // === Plugin Operations ===
            Command::PluginCommand {
                plugin,
                command,
                params,
            } => {
                let result = self
                    .plugin_registry()
                    .handle_plugin_command(&plugin, &command, params)
                    .await;
                match result {
                    Some(Ok(value)) => Ok(Response::PluginResult(value)),
                    Some(Err(e)) => Err(DiaryxError::Plugin(e.to_string())),
                    None => Err(DiaryxError::Plugin(format!(
                        "No plugin '{plugin}' handles command '{command}'"
                    ))),
                }
            }

            Command::GetPluginManifests => {
                let manifests = self.plugin_registry().get_all_manifests();
                Ok(Response::PluginManifests(manifests))
            }

            Command::GetPluginConfig { plugin } => {
                for wp in self.plugin_registry().workspace_plugins() {
                    if wp.id().0 == plugin {
                        let config = wp.get_config().await;
                        return Ok(Response::PluginResult(
                            config.unwrap_or(serde_json::Value::Null),
                        ));
                    }
                }
                Err(DiaryxError::Plugin(format!("Plugin '{plugin}' not found")))
            }

            Command::SetPluginConfig { plugin, config } => {
                for wp in self.plugin_registry().workspace_plugins() {
                    if wp.id().0 == plugin {
                        wp.set_config(config)
                            .await
                            .map_err(|e| DiaryxError::Plugin(e.to_string()))?;
                        return Ok(Response::Ok);
                    }
                }
                Err(DiaryxError::Plugin(format!("Plugin '{plugin}' not found")))
            }
        }
    }

    /// Convert all links in workspace files to a target format.
    ///
    /// This method scans all files in the workspace tree and rewrites
    /// `part_of`, `contents`, and `attachments` properties to use the
    /// specified format.
    async fn convert_workspace_links(
        &self,
        root_index_path: &Path,
        target_format: link_parser::LinkFormat,
        specific_path: Option<&str>,
        dry_run: bool,
    ) -> Result<crate::command::ConvertLinksResult> {
        use std::collections::HashSet;

        let ws = self.workspace().inner();
        let mut files_modified = 0;
        let mut links_converted = 0;
        let mut modified_files = Vec::new();
        let source_format_hint = ws
            .get_workspace_config(root_index_path)
            .await
            .map(|cfg| cfg.link_format)
            .ok();

        // Get workspace root directory (parent of root index file)
        let workspace_root = root_index_path.parent().unwrap_or_else(|| Path::new(""));

        // If a specific path is provided, only convert that file
        if let Some(file_path) = specific_path {
            let path = Path::new(file_path);
            // Compute workspace-relative path
            let relative_path = path
                .strip_prefix(workspace_root)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            let (file_links_converted, was_modified) = self
                .convert_file_links(
                    path,
                    &relative_path,
                    target_format,
                    source_format_hint,
                    dry_run,
                )
                .await?;

            if was_modified {
                files_modified = 1;
                links_converted = file_links_converted;
                modified_files.push(file_path.to_string());
            }
        } else {
            // Scan entire workspace tree
            let tree = ws
                .build_tree_with_depth(root_index_path, None, &mut HashSet::new())
                .await?;

            // Collect all file paths from tree
            let mut file_paths = Vec::new();
            self.collect_tree_paths(&tree, &mut file_paths);

            for file_path in file_paths {
                // Compute workspace-relative path for link conversion
                let relative_path = file_path
                    .strip_prefix(workspace_root)
                    .unwrap_or(&file_path)
                    .to_string_lossy()
                    .to_string();

                let (file_links_converted, was_modified) = self
                    .convert_file_links(
                        &file_path,
                        &relative_path,
                        target_format,
                        source_format_hint,
                        dry_run,
                    )
                    .await?;

                if was_modified {
                    files_modified += 1;
                    links_converted += file_links_converted;
                    modified_files.push(relative_path);
                }
            }
        }

        // Update the workspace config with the new link format (unless dry run)
        if !dry_run {
            ws.set_link_format(root_index_path, target_format).await?;
        }

        Ok(crate::command::ConvertLinksResult {
            files_modified,
            links_converted,
            modified_files,
            dry_run,
        })
    }

    /// Collect all file paths from a tree node recursively.
    fn collect_tree_paths(&self, node: &crate::workspace::TreeNode, paths: &mut Vec<PathBuf>) {
        paths.push(node.path.clone());
        for child in &node.children {
            self.collect_tree_paths(child, paths);
        }
    }

    /// Convert links in a single file to the target format.
    ///
    /// # Arguments
    /// * `file_path` - Absolute path to the file (for reading/writing)
    /// * `relative_path` - Workspace-relative path (for link conversion)
    /// * `target_format` - The target link format
    /// * `dry_run` - If true, don't write changes
    ///
    /// Returns (links_converted, was_modified).
    async fn convert_file_links(
        &self,
        file_path: &Path,
        relative_path: &str,
        target_format: link_parser::LinkFormat,
        source_format_hint: Option<link_parser::LinkFormat>,
        dry_run: bool,
    ) -> Result<(usize, bool)> {
        let file_path_str = file_path.to_string_lossy().to_string();
        let content = self.entry().read_raw(&file_path_str).await?;
        let parsed = frontmatter::parse_or_empty(&content)?;

        let mut links_converted = 0;
        let mut fm = parsed.frontmatter.clone();
        let mut modified = false;

        fn attachment_title(path: &str) -> String {
            Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path)
                .to_string()
        }

        // Convert part_of if present (can be string or array)
        if let Some(part_of_value) = fm.get("part_of") {
            if let Some(part_of_str) = part_of_value.as_str() {
                // Single string value
                let converted = link_parser::convert_link_with_hint(
                    part_of_str,
                    target_format,
                    relative_path,
                    None,
                    source_format_hint,
                );
                if converted != part_of_str {
                    fm.insert("part_of".to_string(), Value::String(converted));
                    links_converted += 1;
                    modified = true;
                }
            } else if let Some(part_of_seq) = part_of_value.as_sequence() {
                // Array of strings
                let mut new_part_of = Vec::new();
                let mut part_of_changed = false;

                for item in part_of_seq {
                    if let Some(item_str) = item.as_str() {
                        let converted = link_parser::convert_link_with_hint(
                            item_str,
                            target_format,
                            relative_path,
                            None,
                            source_format_hint,
                        );
                        if converted != item_str {
                            part_of_changed = true;
                            links_converted += 1;
                        }
                        new_part_of.push(Value::String(converted));
                    } else {
                        new_part_of.push(item.clone());
                    }
                }

                if part_of_changed {
                    fm.insert("part_of".to_string(), Value::Sequence(new_part_of));
                    modified = true;
                }
            }
        }

        // Convert contents if present
        if let Some(contents_value) = fm.get("contents")
            && let Some(contents_seq) = contents_value.as_sequence()
        {
            let mut new_contents = Vec::new();
            let mut contents_changed = false;

            for item in contents_seq {
                if let Some(item_str) = item.as_str() {
                    let converted = link_parser::convert_link_with_hint(
                        item_str,
                        target_format,
                        relative_path,
                        None,
                        source_format_hint,
                    );
                    if converted != item_str {
                        contents_changed = true;
                        links_converted += 1;
                    }
                    new_contents.push(Value::String(converted));
                } else {
                    new_contents.push(item.clone());
                }
            }

            if contents_changed {
                fm.insert("contents".to_string(), Value::Sequence(new_contents));
                modified = true;
            }
        }

        // Convert attachments if present
        if let Some(attachments_value) = fm.get("attachments")
            && let Some(attachments_seq) = attachments_value.as_sequence()
        {
            let mut new_attachments = Vec::new();
            let mut attachments_changed = false;

            for item in attachments_seq {
                if let Some(item_str) = item.as_str() {
                    let parsed = link_parser::parse_link(item_str);
                    let canonical_target = self.resolve_attachment_link_target_with_hint(
                        item_str,
                        relative_path,
                        source_format_hint,
                    );
                    let title = parsed
                        .title
                        .unwrap_or_else(|| attachment_title(&canonical_target));
                    let converted = link_parser::format_link_with_format(
                        &canonical_target,
                        &title,
                        target_format,
                        relative_path,
                    );
                    if converted != item_str {
                        attachments_changed = true;
                        links_converted += 1;
                    }
                    new_attachments.push(Value::String(converted));
                } else {
                    new_attachments.push(item.clone());
                }
            }

            if attachments_changed {
                fm.insert("attachments".to_string(), Value::Sequence(new_attachments));
                modified = true;
            }
        }

        // Write the file if modified and not dry run
        if modified && !dry_run {
            let new_content = frontmatter::serialize(&fm, &parsed.body)?;
            // Use write_file directly to write the full content (frontmatter + body)
            // Note: save_content only saves the body and preserves existing frontmatter
            self.fs().write_file(file_path, &new_content).await?;
        }

        Ok((links_converted, modified))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    use crate::fs::{InMemoryFileSystem, SyncToAsyncFs};
    use futures_lite::future::block_on;

    // =========================================================================
    // normalize_contents_path tests
    // =========================================================================

    #[test]
    fn test_normalize_contents_path_plain_relative() {
        let base_dir = Path::new("Archive");
        let workspace_base = Path::new("/Users/test/journal");

        let result = normalize_contents_path(base_dir, "./file.md", workspace_base);
        assert_eq!(result, "Archive/file.md");
    }

    #[test]
    fn test_normalize_contents_path_parent_relative() {
        let base_dir = Path::new("Archive/Sub");
        let workspace_base = Path::new("/Users/test/journal");

        let result = normalize_contents_path(base_dir, "../file.md", workspace_base);
        assert_eq!(result, "Archive/file.md");
    }

    #[test]
    fn test_normalize_contents_path_plain_filename() {
        let base_dir = Path::new("Archive");
        let workspace_base = Path::new("/Users/test/journal");

        let result = normalize_contents_path(base_dir, "child.md", workspace_base);
        assert_eq!(result, "Archive/child.md");
    }

    #[test]
    fn test_normalize_contents_path_with_spaces() {
        // This tests the case that was causing corruption
        // The path has spaces, which is fine for normalize_contents_path
        // as long as it receives the CLEAN path, not a markdown link
        let base_dir = Path::new("Archive");
        let workspace_base = Path::new("/Users/test/journal");

        let result = normalize_contents_path(base_dir, "Archived documents.md", workspace_base);
        assert_eq!(result, "Archive/Archived documents.md");
    }

    #[test]
    fn test_normalize_contents_path_strips_corrupted_absolute() {
        // Test that corrupted absolute paths are cleaned up
        let base_dir = Path::new("");
        let workspace_base = Path::new("/Users/test/journal");

        // This simulates a corrupted absolute path (without leading /)
        let result = normalize_contents_path(
            base_dir,
            "Users/test/journal/Archive/file.md",
            workspace_base,
        );
        assert_eq!(result, "Archive/file.md");
    }

    // =========================================================================
    // Integration test: parse_link + normalize_contents_path
    // =========================================================================
    // This tests the fix for the CRDT sync corruption bug where markdown links
    // in frontmatter contents were passed directly to normalize_contents_path,
    // corrupting the path.

    #[test]
    fn test_parse_then_normalize_markdown_link_with_spaces() {
        // This is the exact scenario that was causing corruption
        let raw_frontmatter_value = "[Archived documents](</Archive/Archived documents.md>)";
        let base_dir = Path::new("");
        let workspace_base = Path::new("/Users/test/journal");

        // Step 1: Parse the markdown link to extract the clean path
        let parsed = link_parser::parse_link(raw_frontmatter_value);
        assert_eq!(parsed.path, "Archive/Archived documents.md");

        // Step 2: Normalize the extracted path (not the raw markdown link!)
        let result = normalize_contents_path(base_dir, &parsed.path, workspace_base);
        assert_eq!(result, "Archive/Archived documents.md");

        // Verify the path is clean and usable
        let path = Path::new(&result);
        let components: Vec<_> = path.components().collect();
        assert_eq!(components.len(), 2);
    }

    #[test]
    fn test_parse_then_normalize_various_formats() {
        // Use root directory as base to test relative path resolution
        let base_dir = Path::new("");
        let workspace_base = Path::new("/Users/test/journal");

        // Test cases: (raw_value, expected_canonical_path)
        // Note: normalize_contents_path joins base_dir with the path, so we use
        // base_dir = "" to get the canonical path directly.
        let test_cases = [
            // Plain paths (relative to root)
            ("./child.md", "child.md"),
            ("Sub/file.md", "Sub/file.md"),
            // Markdown links with workspace-root paths
            ("[Child](/Archive/child.md)", "Archive/child.md"),
            // Markdown links with angle brackets (spaces in path)
            ("[My File](</Archive/My File.md>)", "Archive/My File.md"),
            (
                "[Creative Writing](</Creative Writing/index.md>)",
                "Creative Writing/index.md",
            ),
        ];

        for (raw_value, expected) in test_cases {
            let parsed = link_parser::parse_link(raw_value);
            let result = normalize_contents_path(base_dir, &parsed.path, workspace_base);
            assert_eq!(
                result, expected,
                "Failed for input '{}': got '{}', expected '{}'",
                raw_value, result, expected
            );
        }
    }

    #[test]
    fn test_parse_then_normalize_relative_paths_from_subdir() {
        // Test relative path resolution from a subdirectory
        let base_dir = Path::new("Archive");
        let workspace_base = Path::new("/Users/test/journal");

        let test_cases = [
            // Relative paths from Archive/
            ("./child.md", "Archive/child.md"),
            ("../sibling.md", "sibling.md"),
            ("Sub/file.md", "Archive/Sub/file.md"),
            // Markdown links with relative paths
            ("[Parent](../parent.md)", "parent.md"),
            ("[Child](<./child file.md>)", "Archive/child file.md"),
        ];

        for (raw_value, expected) in test_cases {
            let parsed = link_parser::parse_link(raw_value);
            let result = normalize_contents_path(base_dir, &parsed.path, workspace_base);
            assert_eq!(
                result, expected,
                "Failed for input '{}': got '{}', expected '{}'",
                raw_value, result, expected
            );
        }
    }

    #[test]
    fn test_bug_regression_markdown_link_not_passed_directly() {
        // This test explicitly verifies that the bug cannot happen:
        // If someone accidentally passes a markdown link directly to
        // normalize_contents_path, it would produce garbage output.
        // After the fix, we always parse first, so this should never happen.

        let base_dir = Path::new("");
        let workspace_base = Path::new("/Users/test/journal");

        // What the BUG did: pass markdown link directly
        // This produces WRONG output because Path splits at '/'
        let buggy_result = normalize_contents_path(
            base_dir,
            "[Archived documents](</Archive/Archived documents.md>)",
            workspace_base,
        );
        // The buggy result would include markdown syntax fragments
        assert!(
            buggy_result.contains('[') || buggy_result.contains('<'),
            "This test shows what the bug produced: {}",
            buggy_result
        );

        // What the FIX does: parse first, then normalize
        let parsed =
            link_parser::parse_link("[Archived documents](</Archive/Archived documents.md>)");
        let correct_result = normalize_contents_path(base_dir, &parsed.path, workspace_base);
        // The correct result is a clean path
        assert_eq!(correct_result, "Archive/Archived documents.md");
        assert!(!correct_result.contains('['));
        assert!(!correct_result.contains('<'));
    }

    #[test]
    fn test_get_workspace_tree_resolves_normalized_relative_path_with_workspace_root() {
        block_on(async {
            let fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
            let diaryx = Diaryx::new(fs);

            let workspace_root = PathBuf::from("/workspace");
            diaryx.set_workspace_root(workspace_root.clone());

            let root_path = workspace_root.join("diaryx.md");
            let child_path = workspace_root.join("child.md");

            diaryx
                .fs()
                .write_file(
                    &root_path,
                    "---\ntitle: Root\nlink_format: markdown_root\ncontents:\n  - \"[Child](/child.md)\"\n---\n\n# Root\n",
                )
                .await
                .unwrap();
            diaryx
                .fs()
                .write_file(&child_path, "---\ntitle: Child\n---\n\n# Child\n")
                .await
                .unwrap();

            let response = diaryx
                .execute(Command::GetWorkspaceTree {
                    path: Some(root_path.to_string_lossy().to_string()),
                    depth: Some(2),
                    audience: None,
                })
                .await
                .unwrap();

            match response {
                Response::Tree(tree) => {
                    assert_eq!(tree.path, root_path);
                    assert_eq!(tree.children.len(), 1);
                    assert_eq!(tree.children[0].path, child_path);
                }
                other => panic!("Expected Response::Tree, got {:?}", other),
            }
        });
    }

    #[test]
    fn test_to_workspace_relative_strips_corrupted_absolute_without_leading_slash() {
        let fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
        let diaryx = Diaryx::new(fs);
        diaryx.set_workspace_root(PathBuf::from("/Users/test/workspace"));

        let relative = diaryx.to_workspace_relative("Users/test/workspace/notes/day.md");
        assert_eq!(relative, "notes/day.md");
    }

    #[test]
    fn test_validate_workspace_resolves_normalized_relative_path_with_workspace_root() {
        block_on(async {
            let fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
            let diaryx = Diaryx::new(fs);

            let workspace_root = PathBuf::from("/workspace");
            diaryx.set_workspace_root(workspace_root.clone());

            let root_path = workspace_root.join("diaryx.md");
            let child_path = workspace_root.join("child.md");

            diaryx
                .fs()
                .write_file(
                    &root_path,
                    "---\ntitle: Root\nlink_format: markdown_root\ncontents:\n  - \"[Child](/child.md)\"\n---\n\n# Root\n",
                )
                .await
                .unwrap();
            diaryx
                .fs()
                .write_file(
                    &child_path,
                    "---\ntitle: Child\npart_of: \"[Root](/diaryx.md)\"\n---\n\n# Child\n",
                )
                .await
                .unwrap();

            let response = diaryx
                .execute(Command::ValidateWorkspace {
                    path: Some(root_path.to_string_lossy().to_string()),
                })
                .await
                .unwrap();

            match response {
                Response::ValidationResult(result) => {
                    assert!(result.errors.is_empty());
                }
                other => panic!("Expected Response::ValidationResult, got {:?}", other),
            }
        });
    }

    #[test]
    fn test_set_frontmatter_property_normalizes_attachments_to_link_format() {
        block_on(async {
            let fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
            let mut diaryx = Diaryx::new(fs);
            diaryx.set_link_format(link_parser::LinkFormat::PlainRelative);

            diaryx
                .fs()
                .create_dir_all(Path::new("notes"))
                .await
                .unwrap();
            diaryx
                .fs()
                .write_file(Path::new("notes/day.md"), "---\ntitle: Day\n---\n\n# Day\n")
                .await
                .unwrap();

            diaryx
                .execute(Command::SetFrontmatterProperty {
                    path: "notes/day.md".to_string(),
                    key: "attachments".to_string(),
                    value: serde_json::json!([
                        "notes/_attachments/a.png",
                        "[Doc](/notes/_attachments/report.pdf)"
                    ]),
                    root_index_path: None,
                })
                .await
                .unwrap();

            let updated = diaryx
                .fs()
                .read_to_string(Path::new("notes/day.md"))
                .await
                .unwrap();
            let parsed = crate::frontmatter::parse_or_empty(&updated).unwrap();
            let attachments: Vec<String> = parsed
                .frontmatter
                .get("attachments")
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            assert_eq!(
                attachments,
                vec![
                    "_attachments/a.png".to_string(),
                    "_attachments/report.pdf".to_string()
                ]
            );
        });
    }

    #[test]
    fn test_convert_links_converts_attachment_values() {
        block_on(async {
            let fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
            let mut diaryx = Diaryx::new(fs);
            diaryx.set_link_format(link_parser::LinkFormat::MarkdownRoot);

            diaryx
                .fs()
                .create_dir_all(Path::new("notes"))
                .await
                .unwrap();
            diaryx
                .fs()
                .write_file(
                    Path::new("README.md"),
                    "---\ntitle: Root\nlink_format: markdown_root\ncontents:\n  - \"[Day](/notes/day.md)\"\n---\n\n# Root\n",
                )
                .await
                .unwrap();
            diaryx
                .fs()
                .write_file(
                    Path::new("notes/day.md"),
                    "---\ntitle: Day\npart_of: \"[Root](/README.md)\"\nattachments:\n  - \"[Image](/notes/_attachments/a.png)\"\n  - \"/notes/_attachments/report.pdf\"\n---\n\n# Day\n",
                )
                .await
                .unwrap();

            diaryx
                .execute(Command::ConvertLinks {
                    root_index_path: "README.md".to_string(),
                    format: "plain_relative".to_string(),
                    path: Some("notes/day.md".to_string()),
                    dry_run: false,
                })
                .await
                .unwrap();

            let updated = diaryx
                .fs()
                .read_to_string(Path::new("notes/day.md"))
                .await
                .unwrap();
            let parsed = crate::frontmatter::parse_or_empty(&updated).unwrap();
            let attachments: Vec<String> = parsed
                .frontmatter
                .get("attachments")
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            assert_eq!(
                attachments,
                vec![
                    "_attachments/a.png".to_string(),
                    "_attachments/report.pdf".to_string()
                ]
            );
        });
    }

    #[test]
    fn test_resolve_attachment_storage_path_relative_nested_entry() {
        let resolved = resolve_attachment_storage_path("notes/day.md", "_attachments/a.png");
        assert_eq!(
            resolved.to_string_lossy().replace('\\', "/"),
            "notes/_attachments/a.png"
        );
    }

    #[test]
    fn test_resolve_attachment_storage_path_plain_canonical_nested_entry() {
        let resolved = resolve_attachment_storage_path("notes/day.md", "notes/_attachments/a.png");
        assert_eq!(
            resolved.to_string_lossy().replace('\\', "/"),
            "notes/_attachments/a.png"
        );
    }
}
