//! Entry operation command handlers.

use std::path::PathBuf;

use indexmap::IndexMap;

use crate::yaml_value::YamlValue;

use crate::command::{EntryData, Response};
use crate::diaryx::{Diaryx, yaml_to_json};
use crate::error::Result;
use crate::frontmatter;
use crate::fs::AsyncFileSystem;
use crate::plugin::{FileCreatedEvent, FileDeletedEvent, FileMovedEvent, FileSavedEvent};

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    pub(crate) async fn cmd_get_entry(&self, path: String) -> Result<Response> {
        let content = self.entry().read_raw(&path).await?;
        let parsed = frontmatter::parse_or_empty(&content)?;
        let title = parsed
            .frontmatter
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

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

    pub(crate) async fn cmd_save_entry(
        &self,
        path: String,
        content: String,
        root_index_path: Option<String>,
        detect_h1_title: bool,
    ) -> Result<Response> {
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
                .track_content_for_sync(&canonical_path, frontmatter::extract_body(&content));

            log::debug!(
                "[CommandHandler] SaveEntry: completed for canonical_path='{}'",
                canonical_path
            );
        }

        // Emit file-saved event to file plugins
        self.plugin_registry()
            .emit_file_saved(&FileSavedEvent { path: path.clone() })
            .await;

        // H1->title sync: detect first-line H1 and sync to title + filename
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
                        if let YamlValue::String(s) = v {
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
                            YamlValue::String(h1_title.clone()),
                        )
                        .await?;

                    // Rename file to match new title
                    let new_stem = apply_filename_style(&h1_title, &ws_config.filename_style);
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

    pub(crate) async fn cmd_create_entry(
        &self,
        path: String,
        options: crate::command::CreateEntryOptions,
    ) -> Result<Response> {
        use crate::link_parser;

        // Derive title from filename if not provided
        let path_buf = PathBuf::from(&path);
        let title = options.title.clone().unwrap_or_else(|| {
            path_buf
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .to_string()
        });

        // Resolve template via the templating plugin, falling back to
        // a simple hardcoded template when the plugin isn't available.
        let content = 'tmpl: {
            if let Some(ref rip) = options.root_index_path {
                // Try to resolve the workspace's default_template name
                let workspace_root_path = self.resolve_fs_path(rip);
                let ws_config = self
                    .workspace()
                    .inner()
                    .get_workspace_config(&workspace_root_path)
                    .await
                    .ok();

                // Extract template name from config (may be a link or a plain name)
                let template_name = ws_config
                    .as_ref()
                    .and_then(|cfg| cfg.default_template.as_ref())
                    .map(|link| {
                        // Support legacy link format: extract filename stem
                        let parsed = link_parser::parse_link(link);
                        if !parsed.path.is_empty() {
                            std::path::Path::new(&parsed.path)
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or(link)
                                .to_string()
                        } else {
                            link.clone()
                        }
                    });

                let filename = path_buf
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("untitled");

                let params = serde_json::json!({
                    "template": template_name.as_deref().unwrap_or("note"),
                    "title": title,
                    "filename": filename,
                });

                if let Some(Ok(result)) = self
                    .plugin_registry()
                    .handle_plugin_command("diaryx.templating", "RenderCreationTemplate", params)
                    .await
                    && let Some(content) = result.as_str()
                {
                    break 'tmpl content.to_string();
                }
            }

            // Fallback: simple hardcoded template
            format!("---\ntitle: {}\n---\n\n# {}\n\n", title, title)
        };

        // CrdtFs.create_new extracts metadata from frontmatter automatically
        let resolved_path = self.resolve_fs_path(&path);
        self.fs()
            .create_new(&resolved_path, &content)
            .await
            .map_err(|e| crate::error::DiaryxError::FileWrite {
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
                .set_frontmatter_property(&path, "part_of", YamlValue::String(formatted_link))
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

    pub(crate) async fn cmd_delete_entry(
        &self,
        path: String,
        _hard_delete: bool,
    ) -> Result<Response> {
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

    pub(crate) async fn cmd_move_entry(&self, from: String, to: String) -> Result<Response> {
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

    pub(crate) async fn cmd_rename_entry(
        &self,
        path: String,
        new_filename: String,
    ) -> Result<Response> {
        let from_path = self.resolve_fs_path(&path);

        // Write the title FIRST so that rename_entry's resolve_title
        // reads the new title when formatting links in parent contents.
        use crate::entry::prettify_filename;
        let title = prettify_filename(new_filename.trim_end_matches(".md"));
        self.entry()
            .set_frontmatter_property(&path, "title", YamlValue::String(title.clone()))
            .await?;

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

        // Sync H1 heading to match the new title
        self.sync_heading_to_title(&to_path_str, &title).await?;

        // Emit file-moved event to file plugins
        self.plugin_registry()
            .emit_file_moved(&FileMovedEvent {
                old_path: path,
                new_path: to_path_str.clone(),
            })
            .await;

        Ok(Response::String(to_path_str))
    }

    pub(crate) async fn cmd_duplicate_entry(&self, path: String) -> Result<Response> {
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

    pub(crate) async fn cmd_convert_to_index(&self, path: String) -> Result<Response> {
        let fm = self.entry().get_frontmatter(&path).await?;

        // Check if already has contents
        if fm.contains_key("contents") {
            return Ok(Response::String(path));
        }

        // Add empty contents array to frontmatter
        // CrdtFs.write_file extracts contents: [] from frontmatter automatically
        self.entry()
            .set_frontmatter_property(&path, "contents", YamlValue::Sequence(vec![]))
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

    pub(crate) async fn cmd_convert_to_leaf(&self, path: String) -> Result<Response> {
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

    pub(crate) async fn cmd_create_child_entry(&self, parent_path: String) -> Result<Response> {
        let ws = self.workspace().inner();
        let resolved_parent_path = self.resolve_fs_path(&parent_path);
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

    pub(crate) async fn cmd_attach_entry_to_parent(
        &self,
        entry_path: String,
        parent_path: String,
    ) -> Result<Response> {
        // workspace.attach_and_move_entry_to_parent uses move operations via CrdtFs
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

    pub(crate) async fn cmd_sync_move_metadata(
        &self,
        old_path: String,
        new_path: String,
    ) -> Result<Response> {
        if old_path == new_path {
            return Ok(Response::String(new_path));
        }

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

    pub(crate) async fn cmd_sync_create_metadata(&self, path: String) -> Result<Response> {
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

    pub(crate) async fn cmd_sync_delete_metadata(&self, path: String) -> Result<Response> {
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
}
