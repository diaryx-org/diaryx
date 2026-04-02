//! Attachment operation command handlers.

use std::path::{Path, PathBuf};

use serde_yaml::Value;

use crate::command::{ResolvedAttachmentRef, Response};
use crate::diaryx::Diaryx;
use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    fn attachment_note_filename(filename: &str) -> String {
        format!("{filename}.md")
    }

    fn attachment_note_canonical_path(&self, entry_path: &str, filename: &str) -> String {
        let note_filename = Self::attachment_note_filename(filename);
        let entry_parent = Path::new(entry_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        if entry_parent.is_empty() {
            format!("_attachments/{note_filename}")
        } else {
            format!("{entry_parent}/_attachments/{note_filename}")
        }
    }

    async fn resolve_attachment_note(
        &self,
        entry_path: &str,
        attachment_ref: &str,
    ) -> Result<(String, String, Option<String>)> {
        let entry_canonical = self.get_canonical_path(entry_path);
        let note_canonical = self.resolve_frontmatter_link_target(attachment_ref, &entry_canonical);
        let note_fs_path = self.resolve_fs_path(&note_canonical);
        let note = match self
            .workspace()
            .inner()
            .parse_index_with_hint(&note_fs_path, Some(self.link_format()))
            .await
        {
            Ok(note) => note,
            Err(_) => {
                // Body HTML/media embeds still point directly at binary assets.
                // Allow those refs for read/preview flows while frontmatter
                // `attachments[]` remains note-backed.
                if self.fs().exists(&note_fs_path).await {
                    return Ok((note_canonical.clone(), note_canonical, None));
                }
                return Err(DiaryxError::Validation(format!(
                    "Attachment note not found: {attachment_ref}"
                )));
            }
        };
        let binary_ref = note.frontmatter.attachment.clone().ok_or_else(|| {
            DiaryxError::Validation(format!(
                "Attachment note '{}' is missing its attachment property",
                note_canonical
            ))
        })?;
        let binary_canonical = self.resolve_attachment_link_target_with_hint(
            &binary_ref,
            &self.get_canonical_path(&note_canonical),
            Some(crate::link_parser::LinkFormat::PlainCanonical),
        );
        Ok((
            note_canonical,
            binary_canonical,
            note.frontmatter.title.clone(),
        ))
    }

    async fn ensure_attachment_note(
        &self,
        entry_path: &str,
        filename: &str,
    ) -> Result<(String, String)> {
        let entry_canonical = self.get_canonical_path(entry_path);
        let binary_canonical = if let Some(parent) = Path::new(&entry_canonical).parent() {
            let parent = parent.to_string_lossy();
            if parent.is_empty() || parent == "." {
                format!("_attachments/{filename}")
            } else {
                format!("{parent}/_attachments/{filename}")
            }
        } else {
            format!("_attachments/{filename}")
        };
        let note_canonical = self.attachment_note_canonical_path(entry_path, filename);
        let note_fs_path = self.resolve_fs_path(&note_canonical);

        if !self.fs().exists(&note_fs_path).await {
            let title = filename.to_string();
            let note_link = self.format_link_for_file(&note_canonical, &note_canonical);
            let attachment_link =
                self.format_attachment_link_for_file(&binary_canonical, &note_canonical);
            let content = format!(
                "---\ntitle: {title}\nlink: \"{note_link}\"\nattachment: \"{attachment_link}\"\n---\n"
            );
            self.fs()
                .write_file(&note_fs_path, &content)
                .await
                .map_err(|e| DiaryxError::FileWrite {
                    path: note_fs_path.clone(),
                    source: e,
                })?;
        }

        Ok((note_canonical, binary_canonical))
    }

    async fn upsert_attachment_backlink(&self, note_path: &str, source_path: &str) -> Result<()> {
        let note_canonical = self.get_canonical_path(note_path);
        let source_canonical = self.get_canonical_path(source_path);
        let existing = self
            .entry()
            .get_frontmatter_property(note_path, "attachment_of")
            .await?;
        let mut items = match existing {
            Some(Value::Sequence(items)) => items,
            _ => Vec::new(),
        };
        let exists = items.iter().any(|item| {
            item.as_str().is_some_and(|s| {
                self.resolve_frontmatter_link_target(s, &note_canonical) == source_canonical
            })
        });
        if !exists {
            let formatted = self.format_link_for_file(&source_canonical, &note_canonical);
            items.push(Value::String(formatted));
            self.entry()
                .set_frontmatter_property(note_path, "attachment_of", Value::Sequence(items))
                .await?;
        }
        Ok(())
    }

    async fn remove_attachment_backlink(
        &self,
        note_path: &str,
        source_path: &str,
    ) -> Result<usize> {
        let note_canonical = self.get_canonical_path(note_path);
        let source_canonical = self.get_canonical_path(source_path);
        let existing = self
            .entry()
            .get_frontmatter_property(note_path, "attachment_of")
            .await?;
        let Some(Value::Sequence(items)) = existing else {
            return Ok(0);
        };
        let filtered: Vec<Value> = items
            .into_iter()
            .filter(|item| {
                !item.as_str().is_some_and(|s| {
                    self.resolve_frontmatter_link_target(s, &note_canonical) == source_canonical
                })
            })
            .collect();
        let remaining = filtered.len();
        if remaining == 0 {
            self.entry()
                .remove_frontmatter_property(note_path, "attachment_of")
                .await?;
        } else {
            self.entry()
                .set_frontmatter_property(note_path, "attachment_of", Value::Sequence(filtered))
                .await?;
        }
        Ok(remaining)
    }

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
                let mut attachments = Vec::new();
                for note_ref in index.frontmatter.attachments_list() {
                    if let Ok((_, binary_canonical, note_title)) =
                        self.resolve_attachment_note(&path_str, note_ref).await
                    {
                        attachments.push(ResolvedAttachmentRef {
                            note_path: note_ref.clone(),
                            attachment_path: binary_canonical,
                            note_title,
                        });
                    }
                }

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
        let (note_canonical, binary_canonical) =
            self.ensure_attachment_note(&entry_path, &filename).await?;
        let entry_canonical = self.get_canonical_path(&entry_path);
        let link = self.format_link_for_file(&note_canonical, &entry_canonical);

        self.entry().add_attachment(&entry_path, &link).await?;
        self.upsert_attachment_backlink(&note_canonical, &entry_path)
            .await?;

        let storage_path = self
            .resolve_fs_path(&binary_canonical)
            .to_string_lossy()
            .into_owned();
        Ok(Response::Strings(vec![link, storage_path]))
    }

    pub(crate) async fn cmd_delete_attachment(
        &self,
        entry_path: String,
        attachment_path: String,
    ) -> Result<Response> {
        let (note_canonical, binary_canonical, _) = self
            .resolve_attachment_note(&entry_path, &attachment_path)
            .await?;
        self.entry()
            .remove_attachment(&entry_path, &attachment_path)
            .await?;
        let remaining = self
            .remove_attachment_backlink(&note_canonical, &entry_path)
            .await?;

        if remaining == 0 {
            let binary_full_path = self.resolve_fs_path(&binary_canonical);
            if self.fs().exists(&binary_full_path).await {
                self.fs()
                    .delete_file(&binary_full_path)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: binary_full_path,
                        source: e,
                    })?;
            }
            let note_full_path = self.resolve_fs_path(&note_canonical);
            if self.fs().exists(&note_full_path).await {
                self.fs().delete_file(&note_full_path).await.map_err(|e| {
                    DiaryxError::FileWrite {
                        path: note_full_path,
                        source: e,
                    }
                })?;
            }
        }

        Ok(Response::Ok)
    }

    pub(crate) async fn cmd_get_attachment_data(
        &self,
        entry_path: String,
        attachment_path: String,
    ) -> Result<Response> {
        let (_, binary_canonical, _) = self
            .resolve_attachment_note(&entry_path, &attachment_path)
            .await?;
        let full_path = self.resolve_fs_path(&binary_canonical);

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

    pub(crate) async fn cmd_resolve_attachment_path(
        &self,
        entry_path: String,
        attachment_path: String,
    ) -> Result<Response> {
        let (_, binary_canonical, _) = self
            .resolve_attachment_note(&entry_path, &attachment_path)
            .await?;
        let full_path = self.resolve_fs_path(&binary_canonical);
        Ok(Response::String(full_path.to_string_lossy().into_owned()))
    }

    pub(crate) async fn cmd_move_attachment(
        &self,
        source_entry_path: String,
        target_entry_path: String,
        attachment_path: String,
        new_filename: Option<String>,
    ) -> Result<Response> {
        let (note_canonical, binary_canonical, note_title) = self
            .resolve_attachment_note(&source_entry_path, &attachment_path)
            .await?;
        let source_attachment_path = self.resolve_fs_path(&binary_canonical);

        // Get the original filename
        let original_filename = Path::new(&binary_canonical)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: PathBuf::from(&binary_canonical),
                message: "Could not extract filename".to_string(),
            })?;

        // Determine final filename (use new_filename if provided, otherwise original)
        let final_filename = new_filename.as_deref().unwrap_or(original_filename);

        // Resolve target paths
        let target_entry = PathBuf::from(&target_entry_path);
        let target_dir = target_entry.parent().unwrap_or_else(|| Path::new("."));
        let target_attachments_dir = target_dir.join("_attachments");
        let target_binary_canonical = self
            .attachment_note_canonical_path(&target_entry_path, final_filename)
            .trim_end_matches(".md")
            .to_string();
        let target_attachment_path = self.resolve_fs_path(&target_binary_canonical);

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

        let note_full_path = self.resolve_fs_path(&note_canonical);
        let target_note_canonical =
            self.attachment_note_canonical_path(&target_entry_path, final_filename);
        let target_note_full_path = self.resolve_fs_path(&target_note_canonical);
        let backlinks = self
            .entry()
            .get_frontmatter_property(&note_canonical, "attachment_of")
            .await?
            .and_then(|v| v.as_sequence().cloned())
            .unwrap_or_default();
        let note_title = note_title.unwrap_or_else(|| final_filename.to_string());
        let target_note_link =
            self.format_link_for_file(&target_note_canonical, &target_note_canonical);
        let target_attachment_link =
            self.format_attachment_link_for_file(&target_binary_canonical, &target_note_canonical);
        let attachment_of_lines = if backlinks.is_empty() {
            String::new()
        } else {
            let mut s = String::from("attachment_of:\n");
            for backlink in &backlinks {
                if let Some(backlink) = backlink.as_str() {
                    s.push_str(&format!("  - \"{backlink}\"\n"));
                }
            }
            s
        };
        let note_content = format!(
            "---\ntitle: {note_title}\nlink: \"{target_note_link}\"\nattachment: \"{target_attachment_link}\"\n{attachment_of_lines}---\n"
        );
        self.fs()
            .write_file(&target_note_full_path, &note_content)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: target_note_full_path.clone(),
                source: e,
            })?;

        for backlink in backlinks.iter().filter_map(|v| v.as_str()) {
            let source_canonical =
                self.resolve_frontmatter_link_target(backlink, &target_note_canonical);
            let source_formatted =
                self.format_link_for_file(&target_note_canonical, &source_canonical);
            let old_formatted = self.format_link_for_file(&note_canonical, &source_canonical);
            let existing = self.entry().get_attachments(&source_canonical).await?;
            let rewritten: Vec<String> = existing
                .into_iter()
                .map(|item| {
                    let item_canonical =
                        self.resolve_frontmatter_link_target(&item, &source_canonical);
                    if item_canonical == note_canonical || item == old_formatted {
                        source_formatted.clone()
                    } else {
                        item
                    }
                })
                .collect();
            self.entry()
                .set_frontmatter_property(
                    &source_canonical,
                    "attachments",
                    Value::Sequence(rewritten.into_iter().map(Value::String).collect()),
                )
                .await?;
        }

        // Delete the original file
        self.fs()
            .delete_file(&source_attachment_path)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: source_attachment_path,
                source: e,
            })?;

        if self.fs().exists(&note_full_path).await {
            self.fs()
                .delete_file(&note_full_path)
                .await
                .map_err(|e| DiaryxError::FileWrite {
                    path: note_full_path,
                    source: e,
                })?;
        }

        Ok(Response::String(target_note_canonical))
    }
}
