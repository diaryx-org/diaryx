//! Attachment operation command handlers.

use std::path::{Path, PathBuf};

use crate::command::Response;
use crate::diaryx::Diaryx;
use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    pub(crate) async fn cmd_get_attachments(&self, path: String) -> Result<Response> {
        let attachments = self.entry().get_attachments(&path).await?;
        Ok(Response::Strings(attachments))
    }

    pub(crate) async fn cmd_get_ancestor_attachments(&self, path: String) -> Result<Response> {
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

    pub(crate) async fn cmd_register_attachment(
        &self,
        entry_path: String,
        filename: String,
    ) -> Result<Response> {
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
        let link = self.format_attachment_link_for_file(&canonical_attachment, &entry_canonical);

        self.entry().add_attachment(&entry_path, &link).await?;

        Ok(Response::String(link))
    }

    pub(crate) async fn cmd_delete_attachment(
        &self,
        entry_path: String,
        attachment_path: String,
    ) -> Result<Response> {
        let rel_path = super::resolve_attachment_storage_path(&entry_path, &attachment_path);
        let full_path = self.resolve_fs_path(&rel_path);

        // Delete the file if it exists
        if self.fs().exists(&full_path).await {
            self.fs()
                .delete_file(&full_path)
                .await
                .map_err(|e| DiaryxError::FileWrite {
                    path: full_path,
                    source: e,
                })?;
        }

        // Remove from frontmatter
        self.entry()
            .remove_attachment(&entry_path, &attachment_path)
            .await?;

        Ok(Response::Ok)
    }

    pub(crate) async fn cmd_get_attachment_data(
        &self,
        entry_path: String,
        attachment_path: String,
    ) -> Result<Response> {
        let rel_path = super::resolve_attachment_storage_path(&entry_path, &attachment_path);
        let full_path = self.resolve_fs_path(&rel_path);

        let data = self
            .fs()
            .read_binary(&full_path)
            .await
            .map_err(|e| DiaryxError::FileRead {
                path: full_path,
                source: e,
            })?;

        Ok(Response::Bytes(data))
    }

    pub(crate) fn cmd_resolve_attachment_path(
        &self,
        entry_path: String,
        attachment_path: String,
    ) -> Result<Response> {
        let rel_path = super::resolve_attachment_storage_path(&entry_path, &attachment_path);
        let full_path = self.resolve_fs_path(&rel_path);
        Ok(Response::String(full_path.to_string_lossy().into_owned()))
    }

    pub(crate) async fn cmd_move_attachment(
        &self,
        source_entry_path: String,
        target_entry_path: String,
        attachment_path: String,
        new_filename: Option<String>,
    ) -> Result<Response> {
        // Resolve source attachment path from the link/path reference.
        let source_rel_path =
            super::resolve_attachment_storage_path(&source_entry_path, &attachment_path);
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
        let formatted_target =
            self.format_attachment_link_for_file(&target_canonical_path, &target_entry_canonical);
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
}
