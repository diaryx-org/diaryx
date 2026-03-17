//! Command execution handler.
//!
//! This module contains the implementation of the `execute()` method for `Diaryx`.
//! It handles all command types and returns appropriate responses.
//!
//! Command handlers are split into domain submodules to produce smaller async
//! state machines and reduce WASM binary size.

mod attachment;
mod config;
mod entry;
mod filesystem;
mod frontmatter;
mod plugin;
mod util;
mod validation;
mod workspace;

use std::path::{Path, PathBuf};

use serde_yaml::Value;

use crate::command::{Command, Response};
use crate::diaryx::Diaryx;
use crate::error::Result;
use crate::fs::AsyncFileSystem;
use crate::link_parser;
use crate::path_utils::normalize_path;
use crate::path_utils::strip_workspace_root_prefix;

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
    // Path Conversion Helpers
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
            is_index: node.is_index,
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
            Command::GetEntry { path } => self.cmd_get_entry(path).await,
            Command::SaveEntry {
                path,
                content,
                root_index_path,
                detect_h1_title,
            } => {
                self.cmd_save_entry(path, content, root_index_path, detect_h1_title)
                    .await
            }
            Command::CreateEntry { path, options } => self.cmd_create_entry(path, options).await,
            Command::DeleteEntry { path, hard_delete } => {
                self.cmd_delete_entry(path, hard_delete).await
            }
            Command::MoveEntry { from, to } => self.cmd_move_entry(from, to).await,
            Command::RenameEntry { path, new_filename } => {
                self.cmd_rename_entry(path, new_filename).await
            }
            Command::DuplicateEntry { path } => self.cmd_duplicate_entry(path).await,
            Command::ConvertToIndex { path } => self.cmd_convert_to_index(path).await,
            Command::ConvertToLeaf { path } => self.cmd_convert_to_leaf(path).await,
            Command::CreateChildEntry { parent_path } => {
                self.cmd_create_child_entry(parent_path).await
            }
            Command::AttachEntryToParent {
                entry_path,
                parent_path,
            } => {
                self.cmd_attach_entry_to_parent(entry_path, parent_path)
                    .await
            }
            Command::SyncMoveMetadata { old_path, new_path } => {
                self.cmd_sync_move_metadata(old_path, new_path).await
            }
            Command::SyncCreateMetadata { path } => self.cmd_sync_create_metadata(path).await,
            Command::SyncDeleteMetadata { path } => self.cmd_sync_delete_metadata(path).await,

            // === Frontmatter Operations ===
            Command::GetFrontmatter { path } => self.cmd_get_frontmatter(path).await,
            Command::SetFrontmatterProperty {
                path,
                key,
                value,
                root_index_path,
            } => {
                self.cmd_set_frontmatter_property(path, key, value, root_index_path)
                    .await
            }
            Command::RemoveFrontmatterProperty { path, key } => {
                self.cmd_remove_frontmatter_property(path, key).await
            }
            Command::ReorderFrontmatterKeys { path, keys } => {
                self.cmd_reorder_frontmatter_keys(path, keys).await
            }
            Command::MoveFrontmatterSectionToFile {
                source_path,
                section_key,
                target_path,
                create_if_missing,
            } => {
                self.cmd_move_frontmatter_section_to_file(
                    source_path,
                    section_key,
                    target_path,
                    create_if_missing,
                )
                .await
            }

            // === Workspace Operations ===
            Command::FindRootIndex { directory } => self.cmd_find_root_index(directory).await,
            Command::GetAvailableAudiences { path } => self.cmd_get_available_audiences(path).await,
            Command::GetEffectiveAudience { path } => self.cmd_get_effective_audience(path).await,
            Command::GetWorkspaceTree {
                path,
                depth,
                audience,
            } => self.cmd_get_workspace_tree(path, depth, audience).await,
            Command::GetFilesystemTree {
                path,
                show_hidden,
                depth,
            } => self.cmd_get_filesystem_tree(path, show_hidden, depth).await,
            Command::CreateWorkspace { path, name } => self.cmd_create_workspace(path, name).await,
            Command::GetAvailableParentIndexes {
                file_path,
                workspace_root,
            } => {
                self.cmd_get_available_parent_indexes(file_path, workspace_root)
                    .await
            }
            Command::SearchWorkspace { pattern, options } => {
                self.cmd_search_workspace(pattern, options).await
            }

            // === Validation Operations ===
            Command::ValidateWorkspace { path } => self.cmd_validate_workspace(path).await,
            Command::ValidateFile { path } => self.cmd_validate_file(path).await,
            Command::FixBrokenPartOf { path } => self.cmd_fix_broken_part_of(path).await,
            Command::FixBrokenContentsRef { index_path, target } => {
                self.cmd_fix_broken_contents_ref(index_path, target).await
            }
            Command::FixBrokenAttachment { path, attachment } => {
                self.cmd_fix_broken_attachment(path, attachment).await
            }
            Command::FixNonPortablePath {
                path,
                property,
                old_value,
                new_value,
            } => {
                self.cmd_fix_non_portable_path(path, property, old_value, new_value)
                    .await
            }
            Command::FixUnlistedFile {
                index_path,
                file_path,
            } => self.cmd_fix_unlisted_file(index_path, file_path).await,
            Command::FixOrphanBinaryFile {
                index_path,
                file_path,
            } => self.cmd_fix_orphan_binary_file(index_path, file_path).await,
            Command::FixMissingPartOf {
                file_path,
                index_path,
            } => self.cmd_fix_missing_part_of(file_path, index_path).await,
            Command::FixAll { validation_result } => self.cmd_fix_all(validation_result).await,
            Command::FixCircularReference {
                file_path,
                part_of_value,
            } => {
                self.cmd_fix_circular_reference(file_path, part_of_value)
                    .await
            }

            // === Attachment Operations ===
            Command::GetAttachments { path } => self.cmd_get_attachments(path).await,
            Command::GetAncestorAttachments { path } => {
                self.cmd_get_ancestor_attachments(path).await
            }
            Command::RegisterAttachment {
                entry_path,
                filename,
            } => self.cmd_register_attachment(entry_path, filename).await,
            Command::DeleteAttachment {
                entry_path,
                attachment_path,
            } => {
                self.cmd_delete_attachment(entry_path, attachment_path)
                    .await
            }
            Command::GetAttachmentData {
                entry_path,
                attachment_path,
            } => {
                self.cmd_get_attachment_data(entry_path, attachment_path)
                    .await
            }
            Command::ResolveAttachmentPath {
                entry_path,
                attachment_path,
            } => self.cmd_resolve_attachment_path(entry_path, attachment_path),
            Command::MoveAttachment {
                source_entry_path,
                target_entry_path,
                attachment_path,
                new_filename,
            } => {
                self.cmd_move_attachment(
                    source_entry_path,
                    target_entry_path,
                    attachment_path,
                    new_filename,
                )
                .await
            }

            // === Filesystem Operations ===
            Command::FileExists { path } => self.cmd_file_exists(path).await,
            Command::ReadFile { path } => self.cmd_read_file(path).await,
            Command::WriteFile { path, content } => self.cmd_write_file(path, content).await,
            Command::DeleteFile { path } => self.cmd_delete_file(path).await,
            Command::ClearDirectory { path } => self.cmd_clear_directory(path).await,
            Command::WriteFileWithMetadata {
                path,
                metadata,
                body,
            } => {
                self.cmd_write_file_with_metadata(path, metadata, body)
                    .await
            }
            Command::UpdateFileMetadata {
                path,
                metadata,
                body,
            } => self.cmd_update_file_metadata(path, metadata, body).await,

            // === Config Operations ===
            Command::GetLinkFormat { root_index_path } => {
                self.cmd_get_link_format(root_index_path).await
            }
            Command::SetLinkFormat {
                root_index_path,
                format,
            } => self.cmd_set_link_format(root_index_path, format).await,
            Command::GetWorkspaceConfig { root_index_path } => {
                self.cmd_get_workspace_config(root_index_path).await
            }
            Command::GenerateFilename {
                title,
                root_index_path,
            } => self.cmd_generate_filename(title, root_index_path).await,
            Command::SetWorkspaceConfig {
                root_index_path,
                field,
                value,
            } => {
                self.cmd_set_workspace_config(root_index_path, field, value)
                    .await
            }
            Command::ConvertLinks {
                root_index_path,
                format,
                path,
                dry_run,
            } => {
                self.cmd_convert_links(root_index_path, format, path, dry_run)
                    .await
            }

            // === Sync utility (no awaits) ===
            Command::LinkParser { operation } => self.cmd_link_parser(operation),

            // === Naming / URL Validation (no awaits) ===
            Command::ValidateWorkspaceName {
                name,
                existing_local_names,
                existing_server_names,
            } => {
                self.cmd_validate_workspace_name(name, existing_local_names, existing_server_names)
            }
            Command::ValidatePublishingSlug { slug } => self.cmd_validate_publishing_slug(slug),
            Command::NormalizeServerUrl { url } => self.cmd_normalize_server_url(url),

            // === Storage (no awaits) ===
            Command::GetStorageUsage => self.cmd_get_storage_usage(),

            // === Plugin Operations ===
            Command::PluginCommand {
                plugin,
                command,
                params,
            } => self.cmd_plugin_command(plugin, command, params).await,
            Command::GetPluginManifests => self.cmd_get_plugin_manifests(),
            Command::GetPluginConfig { plugin } => self.cmd_get_plugin_config(plugin).await,
            Command::SetPluginConfig { plugin, config } => {
                self.cmd_set_plugin_config(plugin, config).await
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
        use crate::frontmatter;

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
