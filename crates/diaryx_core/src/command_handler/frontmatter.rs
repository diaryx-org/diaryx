//! Frontmatter operation command handlers.

use serde_yaml::Value;

use crate::command::Response;
use crate::diaryx::{Diaryx, json_to_yaml, yaml_to_json};
use crate::error::Result;
use crate::fs::AsyncFileSystem;
use indexmap::IndexMap;

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    pub(crate) async fn cmd_get_frontmatter(&self, path: String) -> Result<Response> {
        let fm = self.entry().get_frontmatter(&path).await?;
        let json_fm: IndexMap<String, serde_json::Value> =
            fm.into_iter().map(|(k, v)| (k, yaml_to_json(v))).collect();
        Ok(Response::Frontmatter(json_fm))
    }

    pub(crate) async fn cmd_set_frontmatter_property(
        &self,
        path: String,
        key: String,
        value: serde_json::Value,
        root_index_path: Option<String>,
    ) -> Result<Response> {
        // Handle part_of/contents/attachments specially - normalize and
        // format links according to workspace settings.
        // CrdtFs.write_file extracts metadata from frontmatter automatically
        {
            let canonical_path = self.get_canonical_path(&path);

            if key == "part_of" {
                // Parse the value, convert to canonical, format as markdown link
                if let serde_json::Value::String(ref s) = value {
                    let canonical_target = self.resolve_frontmatter_link_target(s, &canonical_path);

                    // Format as markdown link for file
                    let formatted = self.format_link_for_file(&canonical_target, &canonical_path);

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
                            let formatted =
                                self.format_link_for_file(&canonical_target, &canonical_path);
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
                            let canonical_target = self.resolve_attachment_link_target_with_hint(
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
                    let formatted =
                        self.format_attachment_link_for_file(&canonical_target, &canonical_path);
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

    pub(crate) async fn cmd_remove_frontmatter_property(
        &self,
        path: String,
        key: String,
    ) -> Result<Response> {
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

    pub(crate) async fn cmd_reorder_frontmatter_keys(
        &self,
        path: String,
        keys: Vec<String>,
    ) -> Result<Response> {
        self.entry().reorder_frontmatter_keys(&path, &keys).await?;
        Ok(Response::Ok)
    }

    pub(crate) async fn cmd_move_frontmatter_section_to_file(
        &self,
        source_path: String,
        section_key: String,
        target_path: String,
        create_if_missing: bool,
    ) -> Result<Response> {
        self.entry()
            .move_frontmatter_section_to_file(
                &source_path,
                &section_key,
                &target_path,
                create_if_missing,
            )
            .await?;
        Ok(Response::Ok)
    }
}
