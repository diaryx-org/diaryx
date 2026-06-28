//! Entry lifecycle operations: attach, move, rename, delete, duplicate, and
//! index/leaf conversion, plus external metadata-sync hooks.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;
use crate::link_parser;
use crate::yaml;

use super::*;

impl<FS: AsyncFileSystem> Workspace<FS> {
    // ==================== Entry Management Methods ====================

    /// Attach an entry to a parent index, creating bidirectional links.
    ///
    /// This method:
    /// - Adds the entry to the parent index's `contents` list (relative to parent's directory)
    /// - Sets the entry's `part_of` property to point to the parent index (relative to entry)
    ///
    /// Both paths must exist.
    pub async fn attach_entry_to_parent(
        &self,
        entry_path: &Path,
        parent_index_path: &Path,
    ) -> Result<()> {
        use crate::path_utils::relative_path_from_file_to_target;

        // Validate both paths exist
        if !self.fs.try_exists(entry_path).await.unwrap_or(false) {
            return Err(DiaryxError::FileRead {
                path: entry_path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "Entry does not exist"),
            });
        }
        if !self.fs.try_exists(parent_index_path).await.unwrap_or(false) {
            return Err(DiaryxError::FileRead {
                path: parent_index_path.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Parent index does not exist",
                ),
            });
        }

        // Resolve old parent before making any changes, so we can remove the
        // entry from its old parent's contents after attaching it to the new one.
        let entry_dir = entry_path.parent();
        let old_index_path = self
            .resolve_part_of_from_dir(entry_path, entry_dir, entry_dir.unwrap_or(Path::new("")))
            .await;

        // Add entry to parent's contents with proper formatting
        let entry_canonical = self.get_canonical_path(entry_path);
        let title = self.resolve_title(&entry_canonical).await;
        self.add_to_index_contents_canonical(parent_index_path, &entry_canonical, &title)
            .await?;

        // Remove entry from old parent's contents (if it had one and it differs from new parent).
        if let Some(old_index_path) = old_index_path.as_ref()
            && old_index_path != parent_index_path
            && let Err(e) = self
                .remove_from_index_contents_canonical(old_index_path, &entry_canonical)
                .await
        {
            log::warn!(
                "attach_entry_to_parent: failed to remove old contents reference '{}' from '{}': {}",
                entry_canonical,
                old_index_path.display(),
                e
            );
        }

        // Set entry's part_of with proper formatting
        let part_of = if self.root_path.is_some() {
            let parent_canonical = self.get_canonical_path(parent_index_path);
            let parent_title = self.resolve_title(&parent_canonical).await;
            self.format_link_sync(&parent_canonical, &parent_title, &entry_canonical)
        } else {
            relative_path_from_file_to_target(entry_path, parent_index_path)
        };
        self.set_frontmatter_property(entry_path, "part_of", yaml::Value::String(part_of))
            .await?;

        Ok(())
    }

    /// Move every file under `old_dir` into `new_dir`, preserving subdirectory
    /// structure. Works across every `AsyncFileSystem` backend by only ever
    /// calling `move_file` on leaf files — backends like OPFS/FSA don't
    /// implement directory moves, so we iterate rather than rely on a single
    /// directory rename.
    pub(crate) async fn move_dir_contents_recursive(
        &self,
        old_dir: &Path,
        new_dir: &Path,
    ) -> Result<()> {
        self.fs
            .create_dir_all(new_dir)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: new_dir.to_path_buf(),
                source: e,
            })?;

        let entries = self
            .fs
            .read_dir(old_dir)
            .await
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|e| e.path().to_path_buf())
                    .collect::<Vec<_>>()
            })
            .map_err(|e| DiaryxError::FileRead {
                path: old_dir.to_path_buf(),
                source: e,
            })?;

        for entry in entries {
            let name = match entry.file_name() {
                Some(n) => n,
                None => continue,
            };
            let target = new_dir.join(name);

            if self
                .fs
                .metadata(&entry)
                .await
                .map(|m| m.is_dir())
                .unwrap_or(false)
            {
                Box::pin(self.move_dir_contents_recursive(&entry, &target)).await?;
            } else {
                self.fs
                    .rename(&entry, &target)
                    .await
                    .map_err(|e| DiaryxError::FileWrite {
                        path: target,
                        source: e,
                    })?;
            }
        }

        Ok(())
    }

    /// Walk every markdown file under `tree_root_dir` and rewrite each file's
    /// `part_of` to point at its directory-nearest ancestor index, using the
    /// workspace's link format for the current file location.
    ///
    /// This is how we heal descendants after a folder move: the moved tree
    /// has shifted to a new absolute path, so any `part_of` in `markdown_root`
    /// or other absolute formats still points into the old location. By
    /// re-snapping each descendant to `find_nearest_index`, every
    /// grandchild gets a freshly-formatted link against its (also-moved)
    /// nearest index.
    ///
    /// Only files that already have a `part_of` value are touched — this
    /// preserves detached files and avoids auto-attaching anything that
    /// wasn't already in the hierarchy.
    pub(crate) async fn rewrite_descendants_part_of_in_dir(
        &self,
        tree_root_dir: &Path,
        skip_path: Option<&Path>,
    ) -> Result<()> {
        use crate::path_utils::relative_path_from_file_to_target;

        async fn list_md_recursive<FS: AsyncFileSystem>(
            fs: &FS,
            dir: &Path,
        ) -> std::io::Result<Vec<PathBuf>> {
            let mut all: Vec<PathBuf> = fs
                .read_dir(dir)
                .await?
                .into_iter()
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
                .map(|e| e.path().to_path_buf())
                .collect();
            if let Ok(entries) = fs.read_dir(dir).await {
                for entry in entries {
                    if entry.file_type()?.is_dir()
                        && let Ok(sub) = Box::pin(list_md_recursive(fs, entry.path())).await
                    {
                        all.extend(sub);
                    }
                }
            }
            Ok(all)
        }

        let md_files = match list_md_recursive(&self.fs, tree_root_dir).await {
            Ok(f) => f,
            Err(e) => {
                log::warn!(
                    "rewrite_descendants_part_of_in_dir: list failed for '{}': {}",
                    tree_root_dir.display(),
                    e
                );
                return Ok(());
            }
        };

        for file in md_files {
            if let Some(skip) = skip_path
                && file == skip
            {
                continue;
            }

            // Only rewrite files that already have an explicit part_of — we
            // don't want to auto-attach previously-detached files.
            let has_part_of = matches!(
                self.get_frontmatter_property(&file, "part_of").await,
                Ok(Some(_))
            );
            if !has_part_of {
                continue;
            }

            let nearest = match self.find_nearest_index(&file).await {
                Ok(Some(p)) => p,
                _ => continue,
            };
            if nearest == file {
                continue;
            }

            let part_of_value = if self.root_path.is_some() {
                let nearest_canonical = self.get_canonical_path(&nearest);
                let title = self.resolve_title(&nearest_canonical).await;
                let file_canonical = self.get_canonical_path(&file);
                self.format_link_sync(&nearest_canonical, &title, &file_canonical)
            } else {
                relative_path_from_file_to_target(&file, &nearest)
            };

            if let Err(e) = self
                .set_frontmatter_property(&file, "part_of", yaml::Value::String(part_of_value))
                .await
            {
                log::warn!(
                    "rewrite_descendants_part_of_in_dir: failed to update '{}': {}",
                    file.display(),
                    e
                );
            }
        }

        Ok(())
    }

    /// Move/rename an entry while updating workspace index references.
    ///
    /// This method:
    /// - Moves the file from `from_path` to `to_path`
    /// - Removes the entry from old parent's `contents` (if parent index exists)
    /// - Adds the entry to new parent's `contents` (if parent index exists)
    /// - Updates the moved file's `part_of` to point to new parent index
    ///
    /// Returns `Ok(())` if successful. Does nothing if source equals destination.
    pub async fn move_entry(&self, from_path: &Path, to_path: &Path) -> Result<()> {
        // No-op if same path
        if from_path == to_path {
            return Ok(());
        }

        // Validate destination has a valid filename
        to_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: to_path.to_path_buf(),
                message: "Invalid destination file name".to_string(),
            })?;

        // Move the file
        self.fs
            .rename(from_path, to_path)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: to_path.to_path_buf(),
                source: e,
            })?;

        // Update hierarchy metadata (contents/part_of in parent indexes)
        self.sync_move_metadata(from_path, to_path).await
    }

    /// Update workspace hierarchy metadata after a file has been moved.
    ///
    /// Unlike `move_entry`, this does NOT move the file on the filesystem.
    /// The file must already exist at `new_path`. This is useful when an
    /// external tool (e.g., Obsidian, VS Code) has already performed the
    /// move and you need to fix up the `contents`/`part_of` frontmatter.
    ///
    /// This method:
    /// 1. Resolves the old parent index from the file's `part_of` at `new_path`,
    ///    using `old_path`'s directory as context for relative link resolution
    /// 2. Finds the new parent index in `new_path`'s directory
    /// 3. Adds the entry to the new parent's `contents`
    /// 4. Updates the entry's `part_of` if the parent changed
    /// 5. Removes the entry from the old parent's `contents`
    pub async fn sync_move_metadata(&self, old_path: &Path, new_path: &Path) -> Result<()> {
        use crate::path_utils::relative_path_from_file_to_target;

        if old_path == new_path {
            return Ok(());
        }

        let old_parent = old_path.parent().ok_or_else(|| DiaryxError::InvalidPath {
            path: old_path.to_path_buf(),
            message: "No parent directory for source path".to_string(),
        })?;
        new_path.parent().ok_or_else(|| DiaryxError::InvalidPath {
            path: new_path.to_path_buf(),
            message: "No parent directory for destination path".to_string(),
        })?;

        // Find old parent index by following part_of (the moved file still has its
        // original part_of at this point). Resolve relative links from the OLD
        // directory, since the part_of was written for the old location.
        let old_index_path = self
            .resolve_part_of_from_dir(new_path, Some(old_parent), old_parent)
            .await;
        // Use nearest index (not just the immediate directory), matching
        // sync_create_metadata/sync_delete_metadata semantics.
        let new_index_path = self.find_nearest_index(new_path).await?;

        // Add to new parent's contents first. This avoids "disappearing entry" states
        // if a transient write error occurs during index updates.
        if let Some(new_index_path) = new_index_path.as_ref() {
            // Add with proper formatting
            let new_path_canonical = self.get_canonical_path(new_path);
            let title = self.resolve_title(&new_path_canonical).await;
            self.add_to_index_contents_canonical(new_index_path, &new_path_canonical, &title)
                .await?;

            // Always recompute part_of from the new file location. Even when the
            // parent index is unchanged, relative formats can change across moves.
            let part_of_value = if self.root_path.is_some() {
                let new_index_canonical = self.get_canonical_path(new_index_path);
                let parent_title = self.resolve_title(&new_index_canonical).await;
                self.format_link_sync(&new_index_canonical, &parent_title, &new_path_canonical)
            } else {
                relative_path_from_file_to_target(new_path, new_index_path)
            };
            self.set_frontmatter_property(new_path, "part_of", yaml::Value::String(part_of_value))
                .await?;
        } else {
            // No reachable parent index in the destination path. Remove stale
            // part_of to avoid broken references after external moves.
            self.remove_frontmatter_property(new_path, "part_of")
                .await?;
        }

        // Remove from old parent's contents after successful add to avoid lossy updates.
        if let Some(old_index_path) = old_index_path.as_ref() {
            let from_canonical = self.get_canonical_path(old_path);
            if let Err(e) = self
                .remove_from_index_contents_canonical(old_index_path, &from_canonical)
                .await
            {
                log::warn!(
                    "sync_move_metadata: failed to remove old contents reference '{}' from '{}': {}",
                    from_canonical,
                    old_index_path.display(),
                    e
                );
            }
        }

        // If the moved file is an index whose containing directory changed,
        // any descendants living in the new directory still have `part_of`
        // values written against the old location. Heal them by re-snapping
        // each one to its directory-nearest index at the new location.
        if old_path.parent() != new_path.parent()
            && self.is_index_file(new_path).await
            && let Some(new_dir) = new_path.parent()
        {
            self.rewrite_descendants_part_of_in_dir(new_dir, Some(new_path))
                .await?;
        }

        Ok(())
    }

    /// Update workspace hierarchy metadata after an external file creation.
    ///
    /// The file must already exist at `path`. This finds the nearest parent
    /// index and adds the file to its `contents`, then sets the file's
    /// `part_of` to point back to that index.
    ///
    /// Skips files that already have `part_of` (already attached) or
    /// `contents` (index files should not be auto-attached).
    pub async fn sync_create_metadata(&self, path: &Path) -> Result<()> {
        use crate::path_utils::relative_path_from_file_to_target;

        // Skip if file already has part_of (already in hierarchy)
        if let Ok(Some(_)) = self.get_frontmatter_property(path, "part_of").await {
            return Ok(());
        }

        // Skip if file is an index (has contents property)
        if self.is_index_file(path).await {
            return Ok(());
        }

        // Find nearest parent index
        let parent_index = match self.find_nearest_index(path).await? {
            Some(idx) => idx,
            None => return Ok(()), // No index found, nothing to do
        };

        // Add to parent index's contents
        let entry_canonical = self.get_canonical_path(path);
        let title = self.resolve_title(&entry_canonical).await;
        self.add_to_index_contents_canonical(&parent_index, &entry_canonical, &title)
            .await?;

        // Set file's part_of
        let part_of = if self.root_path.is_some() {
            let parent_canonical = self.get_canonical_path(&parent_index);
            let parent_title = self.resolve_title(&parent_canonical).await;
            self.format_link_sync(&parent_canonical, &parent_title, &entry_canonical)
        } else {
            relative_path_from_file_to_target(path, &parent_index)
        };
        self.set_frontmatter_property(path, "part_of", yaml::Value::String(part_of))
            .await?;

        Ok(())
    }

    /// Update workspace hierarchy metadata after an external file deletion.
    ///
    /// The file at `path` no longer exists on disk. This finds the nearest
    /// parent index (by walking up directories) and removes the file from
    /// its `contents` list.
    pub async fn sync_delete_metadata(&self, path: &Path) -> Result<()> {
        // Find nearest parent index by walking up from the deleted file's directory
        let parent_index = match self.find_nearest_index(path).await? {
            Some(idx) => idx,
            None => return Ok(()), // No index found, nothing to do
        };

        // Remove from parent index's contents
        let entry_canonical = self.get_canonical_path(path);
        let _ = self
            .remove_from_index_contents_canonical(&parent_index, &entry_canonical)
            .await;

        Ok(())
    }

    /// Delete an entry while updating workspace index references.
    ///
    /// This method:
    /// - Fails if the entry is an index with non-empty `contents` (has children)
    /// - Removes the entry from parent's `contents` (if parent index exists)
    /// - Deletes the file
    ///
    /// For index files with directories, only the file is deleted (not the directory).
    pub async fn delete_entry(&self, path: &Path) -> Result<()> {
        // Check if this is an index file with children
        if let Ok(index) = self.parse_index(path).await {
            let contents = index.frontmatter.contents_list();
            if !contents.is_empty() {
                return Err(DiaryxError::InvalidPath {
                    path: path.to_path_buf(),
                    message: format!(
                        "Cannot delete index with {} children. Delete children first.",
                        contents.len()
                    ),
                });
            }
        }

        // Get the filename and parent directory
        let parent = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
            path: path.to_path_buf(),
            message: "No parent directory".to_string(),
        })?;
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "Invalid file name".to_string(),
            })?
            .to_string();

        // Remove from parent's contents by following the part_of link.
        // This correctly handles both leaf files (parent index in same dir)
        // and index files (parent index in grandparent dir).
        if let Ok(Some(yaml::Value::String(part_of))) =
            self.get_frontmatter_property(path, "part_of").await
        {
            use crate::path_utils::normalize_path;

            let parsed = link_parser::parse_link(&part_of);
            let parent_index = match parsed.path_type {
                link_parser::PathType::WorkspaceRoot => {
                    if let Some(ref root) = self.root_path {
                        normalize_path(&root.join(&parsed.path))
                    } else {
                        PathBuf::from(&parsed.path)
                    }
                }
                link_parser::PathType::Relative | link_parser::PathType::Ambiguous => {
                    normalize_path(&parent.join(&parsed.path))
                }
            };

            let entry_canonical = self.get_canonical_path(path);
            let _ = self
                .remove_from_index_contents_canonical(&parent_index, &entry_canonical)
                .await;
        } else if let Ok(Some(index_path)) = self.find_any_index_in_dir(parent).await {
            // No part_of: fall back to finding an index in same directory
            let _ = self
                .remove_from_index_contents(&index_path, &file_name)
                .await;
        }

        // Delete the file
        self.fs
            .remove_file(path)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            })?;

        Ok(())
    }

    /// Generate a unique filename for a new child entry in the given directory.
    ///
    /// Returns filenames like "new-entry.md", "new-entry-1.md", "new-entry-2.md", etc.
    pub async fn generate_unique_child_name(&self, parent_dir: &Path) -> String {
        let base_name = "new-entry";
        let mut candidate = format!("{}.md", base_name);
        let mut counter = 1;

        while self
            .fs
            .try_exists(&parent_dir.join(&candidate))
            .await
            .unwrap_or(false)
        {
            candidate = format!("{}-{}.md", base_name, counter);
            counter += 1;
        }

        candidate
    }

    /// Write a brand-new entry file to disk, minting and injecting its ARK
    /// file id (the `id` frontmatter property) unless `content` already carries
    /// a valid blade that is unused in this workspace.
    ///
    /// This is the single chokepoint for creating entry files. Every creation
    /// path — top-level entries, child entries, future importers — funnels
    /// through here so the ARK `id` invariant lives in exactly one place and a
    /// new creation path cannot silently forget it. Only the file blade is
    /// stored; the full ARK is composed with the workspace blade at
    /// publish/resolve time.
    ///
    /// Minting draws entropy from v4 UUIDs, so it is active only when the
    /// `uuid` feature is enabled. Builds without it (the dumb server, isolated
    /// plugin builds) never create entries at runtime, so they write `content`
    /// unchanged.
    pub async fn write_new_entry(&self, path: &Path, content: &str) -> Result<()> {
        #[cfg(feature = "uuid")]
        let content = {
            // Scope blade-uniqueness to the workspace root when one is set,
            // otherwise to the new file's own directory (loose files, or a
            // command that supplies its root per-call rather than via an open
            // workspace).
            let scope = self
                .root_path
                .clone()
                .or_else(|| path.parent().map(|p| p.to_path_buf()));
            let existing: std::collections::HashSet<String> = match scope {
                Some(ref dir) => self.collect_file_blades(dir).await,
                None => std::collections::HashSet::new(),
            };

            let parsed = crate::frontmatter::parse_or_empty(content)?;
            let preset_ok = crate::frontmatter::get_string(&parsed.frontmatter, "id")
                .map(|id| diaryx_ark::validate_file_blade(id).is_ok() && !existing.contains(id))
                .unwrap_or(false);

            if preset_ok {
                content.to_string()
            } else {
                let blade = crate::mint::mint_file_blade(&existing);
                crate::frontmatter::set_property_in_text(
                    content,
                    "id",
                    &yaml::Value::String(blade),
                )?
            }
        };
        #[cfg(not(feature = "uuid"))]
        let content = content.to_string();

        self.fs
            .create_new(path, content.as_bytes())
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            })?;

        Ok(())
    }

    /// Create a new child entry under a parent index.
    ///
    /// This method:
    /// - Generates a unique filename if not provided
    /// - Creates the child file with basic frontmatter
    /// - Adds the child to the parent's `contents`
    /// - Sets the child's `part_of` to point to the parent
    ///
    /// Returns the path to the new child entry.
    ///
    /// Thin wrapper over [`create_child_entry_with_result`](Self::create_child_entry_with_result)
    /// for callers that only need the new child's path and not the
    /// parent-conversion details.
    pub async fn create_child_entry(
        &self,
        parent_index_path: &Path,
        title: Option<&str>,
    ) -> Result<PathBuf> {
        let result = self
            .create_child_entry_with_result(parent_index_path, title)
            .await?;
        Ok(PathBuf::from(result.child_path))
    }

    /// Create a new child entry under a parent index, returning detailed result.
    ///
    /// This method provides more information than `create_child_entry`, including
    /// whether the parent was converted to an index and what the new parent path is.
    /// This is essential for the frontend to correctly update the tree when a leaf
    /// is converted to an index (which changes its path).
    ///
    /// Returns a [`CreateChildResult`] with:
    /// - `child_path`: The path to the newly created child
    /// - `parent_path`: The current parent path (may differ from input if converted)
    /// - `parent_converted`: Whether the parent was converted from leaf to index
    /// - `original_parent_path`: The original parent path if conversion occurred
    pub async fn create_child_entry_with_result(
        &self,
        parent_index_path: &Path,
        title: Option<&str>,
    ) -> Result<crate::command::CreateChildResult> {
        use crate::path_utils::relative_path_from_file_to_target;

        let original_parent_str = parent_index_path.to_string_lossy().to_string();

        // Parse parent - if it's a leaf (not an index), convert it to an index first
        let (effective_parent, was_converted) = if let Ok(parent_index) =
            self.parse_index(parent_index_path).await
        {
            if parent_index.frontmatter.is_index() {
                (parent_index_path.to_path_buf(), false)
            } else {
                // Parent is a leaf file - convert to index first
                let new_path = self.convert_to_index(parent_index_path).await?;
                (new_path, true)
            }
        } else {
            // Parent doesn't exist or couldn't be parsed
            return Err(DiaryxError::FileRead {
                path: parent_index_path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "Parent file not found"),
            });
        };

        // Determine parent directory (from effective parent, which may have moved)
        let parent_dir = effective_parent
            .parent()
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: effective_parent.clone(),
                message: "Parent index has no directory".to_string(),
            })?;

        // Generate unique filename
        let child_filename = self.generate_unique_child_name(parent_dir).await;
        let child_path = parent_dir.join(&child_filename);

        // Format part_of link based on configured format
        let display_title = title.unwrap_or("New Entry");
        let part_of_value = if self.root_path.is_some() {
            // Use link formatting - resolve parent's title for the link display
            let child_canonical = self.get_canonical_path(&child_path);
            let parent_canonical = self.get_canonical_path(&effective_parent);
            let parent_title = self.resolve_title(&parent_canonical).await;
            self.format_link_sync(&parent_canonical, &parent_title, &child_canonical)
        } else {
            // Fallback: use relative path
            relative_path_from_file_to_target(&child_path, &effective_parent)
        };

        // Create child file with frontmatter, minting its ARK id via the
        // shared entry-creation chokepoint.
        let content = format!(
            "---\ntitle: \"{}\"\npart_of: \"{}\"\n---\n\n# {}\n\n",
            display_title, part_of_value, display_title
        );

        self.write_new_entry(&child_path, &content).await?;

        // Add to parent's contents (using formatted link)
        let child_canonical = self.get_canonical_path(&child_path);
        self.add_to_index_contents_canonical(&effective_parent, &child_canonical, display_title)
            .await?;

        Ok(crate::command::CreateChildResult {
            child_path: child_path.to_string_lossy().to_string(),
            parent_path: effective_parent.to_string_lossy().to_string(),
            parent_converted: was_converted,
            original_parent_path: if was_converted {
                Some(original_parent_str)
            } else {
                None
            },
        })
    }

    /// Rename an entry file by giving it a new filename.
    ///
    /// This method handles both leaf files and index files:
    /// - Leaf files: renames the file directly and updates parent `contents`
    /// - Root index files: renames the file in place and updates children's `part_of`
    /// - Index files: renames the containing directory AND the file itself, updates grandparent `contents`
    ///
    /// Returns the new path to the renamed file.
    pub async fn rename_entry(&self, path: &Path, new_filename: &str) -> Result<PathBuf> {
        let is_index = self.is_index_file(path).await;
        let is_root = self.is_root_index(path).await;

        if is_index && is_root {
            let children_paths = self.collect_index_content_children(path).await;

            // Root index files live at the workspace root (e.g. README.md).
            // There is no containing subdirectory to rename, so just rename the file in place.
            let parent = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "File has no parent directory".to_string(),
            })?;

            let new_path = parent.join(new_filename);

            if new_path == path {
                return Ok(path.to_path_buf());
            }

            if self.fs.try_exists(&new_path).await.unwrap_or(false) {
                return Err(DiaryxError::InvalidPath {
                    path: new_path,
                    message: "Target file already exists".to_string(),
                });
            }

            self.fs.rename(path, &new_path).await?;

            // Update children's part_of to point to the renamed root index
            let new_path_canonical = self.get_canonical_path(&new_path);
            let new_title = self.resolve_title(&new_path_canonical).await;
            for child_path in &children_paths {
                if child_path == &new_path || !self.fs.try_exists(child_path).await.unwrap_or(false)
                {
                    continue;
                }

                let part_of_value = if self.root_path.is_some() {
                    let child_canonical = self.get_canonical_path(child_path);
                    self.format_link_sync(&new_path_canonical, &new_title, &child_canonical)
                } else {
                    use crate::path_utils::relative_path_from_file_to_target;
                    relative_path_from_file_to_target(child_path, &new_path)
                };
                if let Err(e) = self
                    .set_frontmatter_property(
                        child_path,
                        "part_of",
                        yaml::Value::String(part_of_value),
                    )
                    .await
                {
                    log::warn!(
                        "rename_entry: failed to update child part_of for '{}': {}",
                        child_path.display(),
                        e
                    );
                }
            }

            // Update contents references to use new self-path
            // (contents entries reference children, not self, so no update needed)

            Ok(new_path)
        } else if is_index {
            // For index files, we rename the containing directory AND the file
            let current_dir = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "Index file has no parent directory".to_string(),
            })?;

            let parent_of_dir = current_dir
                .parent()
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: path.to_path_buf(),
                    message: "Directory has no parent".to_string(),
                })?;
            let children_paths_before_rename = self.collect_index_content_children(path).await;

            // Get new directory name from the filename (strip .md extension)
            let new_dir_name = new_filename.trim_end_matches(".md");
            let new_dir_path = parent_of_dir.join(new_dir_name);
            // New file will be named {dirname}.md
            let new_file_path = new_dir_path.join(new_filename);

            // Don't rename if same path
            if new_dir_path == current_dir {
                return Ok(path.to_path_buf());
            }

            // Check if target directory already exists
            if self.fs.try_exists(&new_dir_path).await.unwrap_or(false) {
                return Err(DiaryxError::InvalidPath {
                    path: new_dir_path,
                    message: "Target directory already exists".to_string(),
                });
            }

            // Create new directory
            self.fs.create_dir_all(&new_dir_path).await?;

            // Move all files/directories from old directory to new directory.
            if let Ok(files) = self.fs.read_dir(current_dir).await.map(|entries| {
                entries
                    .into_iter()
                    .map(|e| e.path().to_path_buf())
                    .collect::<Vec<_>>()
            }) {
                for file in files {
                    let file_name = file.file_name().unwrap_or_default();
                    let new_path = new_dir_path.join(file_name);

                    // If this is the index file itself, use the new filename
                    if file == path {
                        self.fs.rename(&file, &new_file_path).await?;
                    } else {
                        self.fs.rename(&file, &new_path).await?;
                    }
                }
            }

            // Update part_of for all listed children, including nested entries.
            let new_file_canonical = self.get_canonical_path(&new_file_path);
            let new_file_title = self.resolve_title(&new_file_canonical).await;
            let mut rewritten_child_paths = HashSet::new();
            for child_path_before in &children_paths_before_rename {
                let rewritten_child_path = if child_path_before.starts_with(current_dir) {
                    match child_path_before.strip_prefix(current_dir) {
                        Ok(relative) => new_dir_path.join(relative),
                        Err(_) => child_path_before.clone(),
                    }
                } else {
                    child_path_before.clone()
                };

                if !rewritten_child_paths.insert(rewritten_child_path.clone()) {
                    continue;
                }
                if rewritten_child_path == new_file_path
                    || !self
                        .fs
                        .try_exists(&rewritten_child_path)
                        .await
                        .unwrap_or(false)
                {
                    continue;
                }

                let part_of_value = if self.root_path.is_some() {
                    let child_canonical = self.get_canonical_path(&rewritten_child_path);
                    self.format_link_sync(&new_file_canonical, &new_file_title, &child_canonical)
                } else {
                    use crate::path_utils::relative_path_from_file_to_target;
                    relative_path_from_file_to_target(&rewritten_child_path, &new_file_path)
                };
                if let Err(e) = self
                    .set_frontmatter_property(
                        &rewritten_child_path,
                        "part_of",
                        yaml::Value::String(part_of_value),
                    )
                    .await
                {
                    log::warn!(
                        "rename_entry: failed to update child part_of for '{}': {}",
                        rewritten_child_path.display(),
                        e
                    );
                }
            }

            // Update parent's contents via part_of (fallback: grandparent directory)
            if let Some(parent_index) = self
                .resolve_part_of_to_path(&new_file_path, parent_of_dir)
                .await
            {
                let old_canonical = self.get_canonical_path(path);
                // Add new entry first to avoid transient "missing child" states.
                self.add_to_index_contents_canonical(
                    &parent_index,
                    &new_file_canonical,
                    &new_file_title,
                )
                .await?;

                if let Err(e) = self
                    .remove_from_index_contents_canonical(&parent_index, &old_canonical)
                    .await
                {
                    log::warn!(
                        "rename_entry: failed to remove old parent contents reference '{}' from '{}': {}",
                        old_canonical,
                        parent_index.display(),
                        e
                    );
                }
            }

            Ok(new_file_path)
        } else {
            // For leaf files, simple rename within the same directory
            let parent = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "File has no parent directory".to_string(),
            })?;

            let new_path = parent.join(new_filename);

            // Don't rename if same path
            if new_path == path {
                return Ok(path.to_path_buf());
            }

            // Check if target already exists
            if self.fs.try_exists(&new_path).await.unwrap_or(false) {
                if !self.fs.try_exists(path).await.unwrap_or(false) {
                    // File was already renamed on disk (e.g. by OS or sync tool)
                    // but parent's contents still references the old name.
                    // Just update the parent's contents reference.
                    if let Some(parent_index) =
                        self.resolve_part_of_to_path(&new_path, parent).await
                    {
                        let new_path_canonical = self.get_canonical_path(&new_path);
                        let old_canonical = self.get_canonical_path(path);
                        let title = self.resolve_title(&new_path_canonical).await;
                        self.add_to_index_contents_canonical(
                            &parent_index,
                            &new_path_canonical,
                            &title,
                        )
                        .await?;

                        if let Err(e) = self
                            .remove_from_index_contents_canonical(&parent_index, &old_canonical)
                            .await
                        {
                            log::warn!(
                                "rename_entry: failed to remove old parent contents reference '{}' from '{}': {}",
                                old_canonical,
                                parent_index.display(),
                                e
                            );
                        }
                    }
                    return Ok(new_path);
                }
                return Err(DiaryxError::InvalidPath {
                    path: new_path,
                    message: "Target file already exists".to_string(),
                });
            }

            // Move the file
            self.fs.rename(path, &new_path).await?;

            // Update parent's contents via part_of (fallback: same directory)
            if let Some(parent_index) = self.resolve_part_of_to_path(&new_path, parent).await {
                // Add new entry first to avoid transient "missing child" states.
                let new_path_canonical = self.get_canonical_path(&new_path);
                let old_canonical = self.get_canonical_path(path);
                let title = self.resolve_title(&new_path_canonical).await;
                self.add_to_index_contents_canonical(&parent_index, &new_path_canonical, &title)
                    .await?;

                if let Err(e) = self
                    .remove_from_index_contents_canonical(&parent_index, &old_canonical)
                    .await
                {
                    log::warn!(
                        "rename_entry: failed to remove old parent contents reference '{}' from '{}': {}",
                        old_canonical,
                        parent_index.display(),
                        e
                    );
                }
            }

            Ok(new_path)
        }
    }

    /// Duplicate an entry, creating a copy with a unique name.
    ///
    /// This method:
    /// - For leaf files: copies the file with a "-copy" suffix (or "-copy-N" if exists)
    /// - For index files: copies the entire directory structure recursively
    /// - Updates the copy's `part_of` to point to the same parent
    /// - Adds the copy to the parent's `contents`
    ///
    /// Returns the path to the new duplicated entry.
    pub async fn duplicate_entry(&self, source_path: &Path) -> Result<PathBuf> {
        use crate::path_utils::relative_path_from_file_to_target;

        let is_index = self.is_index_file(source_path).await;

        if is_index {
            // For index files, we duplicate the entire directory
            let source_dir = source_path
                .parent()
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: source_path.to_path_buf(),
                    message: "Index file has no parent directory".to_string(),
                })?;

            let parent_of_dir = source_dir
                .parent()
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: source_path.to_path_buf(),
                    message: "Directory has no parent".to_string(),
                })?;

            // Get source directory name and generate unique copy name
            let source_dir_name =
                source_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| DiaryxError::InvalidPath {
                        path: source_path.to_path_buf(),
                        message: "Invalid directory name".to_string(),
                    })?;

            let new_dir_name = self
                .generate_unique_copy_name(parent_of_dir, source_dir_name, false)
                .await;
            let new_dir_path = parent_of_dir.join(&new_dir_name);
            let new_index_path = new_dir_path.join(format!("{}.md", new_dir_name));

            // Create new directory
            self.fs.create_dir_all(&new_dir_path).await?;

            // Copy all files from source directory to new directory
            if let Ok(files) = self.fs.read_dir(source_dir).await.map(|entries| {
                entries
                    .into_iter()
                    .map(|e| e.path().to_path_buf())
                    .collect::<Vec<_>>()
            }) {
                for file in files {
                    let file_name = file
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default();

                    // For the index file, use the new directory name
                    let new_path = if file == source_path {
                        new_index_path.clone()
                    } else {
                        new_dir_path.join(file_name)
                    };

                    // Copy file content
                    let content =
                        self.fs
                            .read_to_string(&file)
                            .await
                            .map_err(|e| DiaryxError::FileRead {
                                path: file.clone(),
                                source: e,
                            })?;
                    self.fs
                        .write(&new_path, content.as_bytes())
                        .await
                        .map_err(|e| DiaryxError::FileWrite {
                            path: new_path.clone(),
                            source: e,
                        })?;

                    // Update part_of for child files to point to new index
                    if new_path != new_index_path {
                        let new_part_of =
                            relative_path_from_file_to_target(&new_path, &new_index_path);
                        let _ = self
                            .set_frontmatter_property(
                                &new_path,
                                "part_of",
                                yaml::Value::String(new_part_of),
                            )
                            .await;
                    }
                }
            }

            // Update the copied index's part_of to point to grandparent (same as source)
            if let Ok(Some(grandparent_index)) = self.find_any_index_in_dir(parent_of_dir).await {
                let new_part_of =
                    relative_path_from_file_to_target(&new_index_path, &grandparent_index);
                let _ = self
                    .set_frontmatter_property(
                        &new_index_path,
                        "part_of",
                        yaml::Value::String(new_part_of),
                    )
                    .await;

                // Add to grandparent's contents
                let rel_path = format!("{}/{}.md", new_dir_name, new_dir_name);
                let _ = self
                    .add_to_index_contents(&grandparent_index, &rel_path)
                    .await;
            }

            Ok(new_index_path)
        } else {
            // For leaf files, simple copy in same directory
            let parent = source_path
                .parent()
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: source_path.to_path_buf(),
                    message: "File has no parent directory".to_string(),
                })?;

            let source_filename = source_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: source_path.to_path_buf(),
                    message: "Invalid file name".to_string(),
                })?;

            // Generate unique copy name
            let new_filename = self
                .generate_unique_copy_name(parent, source_filename, true)
                .await;
            let new_path = parent.join(&new_filename);

            // Copy file content
            let content =
                self.fs
                    .read_to_string(source_path)
                    .await
                    .map_err(|e| DiaryxError::FileRead {
                        path: source_path.to_path_buf(),
                        source: e,
                    })?;
            self.fs
                .write(&new_path, content.as_bytes())
                .await
                .map_err(|e| DiaryxError::FileWrite {
                    path: new_path.clone(),
                    source: e,
                })?;

            // Update parent's contents if it exists
            if let Ok(Some(parent_index)) = self.find_any_index_in_dir(parent).await {
                // Update part_of to point to parent
                let new_part_of = relative_path_from_file_to_target(&new_path, &parent_index);
                let _ = self
                    .set_frontmatter_property(
                        &new_path,
                        "part_of",
                        yaml::Value::String(new_part_of),
                    )
                    .await;

                // Add to parent's contents
                let _ = self
                    .add_to_index_contents(&parent_index, &new_filename)
                    .await;
            }

            Ok(new_path)
        }
    }

    /// Generate a unique copy name for a file or directory.
    ///
    /// For files: "name.md" → "name-copy.md", "name-copy-2.md", etc.
    /// For directories: "name" → "name-copy", "name-copy-2", etc.
    pub(crate) async fn generate_unique_copy_name(
        &self,
        parent_dir: &Path,
        original_name: &str,
        is_file: bool,
    ) -> String {
        let (base_name, extension) = if is_file {
            // Strip .md extension for files
            let base = original_name.trim_end_matches(".md");
            (base.to_string(), ".md".to_string())
        } else {
            (original_name.to_string(), String::new())
        };

        // Try "name-copy" first
        let mut candidate = format!("{}-copy{}", base_name, extension);
        let mut counter = 2;

        while self
            .fs
            .try_exists(&parent_dir.join(&candidate))
            .await
            .unwrap_or(false)
        {
            candidate = format!("{}-copy-{}{}", base_name, counter, extension);
            counter += 1;
        }

        candidate
    }

    /// Convert a leaf file into an index file with a directory.
    ///
    /// This method:
    /// - Creates a directory with the same name as the file (without .md)
    /// - Moves the file into the directory as `{dirname}.md`
    /// - Adds empty `contents` property to the file
    ///
    /// Example: `journal/my-note.md` → `journal/my-note/my-note.md`
    ///
    /// Returns the new path to the index file.
    pub async fn convert_to_index(&self, path: &Path) -> Result<PathBuf> {
        // Check if already an index
        if self.is_index_file(path).await {
            return Err(DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "File is already an index".to_string(),
            });
        }

        let parent = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
            path: path.to_path_buf(),
            message: "File has no parent directory".to_string(),
        })?;

        let file_stem =
            path.file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: path.to_path_buf(),
                    message: "Invalid file name".to_string(),
                })?;

        // Create new directory and file paths
        let new_dir = parent.join(file_stem);
        let new_filename = format!("{}.md", file_stem);
        let new_path = new_dir.join(&new_filename);

        // Create directory
        self.fs.create_dir_all(&new_dir).await?;

        // Move file into directory
        self.fs.rename(path, &new_path).await?;

        // Add contents property
        self.set_frontmatter_property(&new_path, "contents", yaml::Value::Sequence(vec![]))
            .await?;

        // Update part_of path since file moved one level deeper
        if let Ok(Some(yaml::Value::String(old_part_of))) =
            self.get_frontmatter_property(&new_path, "part_of").await
        {
            use crate::path_utils::{normalize_path, relative_path_from_file_to_target};

            // Parse the markdown link to get the path and type
            let parsed = link_parser::parse_link(&old_part_of);

            // Resolve target path based on path type
            let target_path = match parsed.path_type {
                link_parser::PathType::WorkspaceRoot => {
                    // Workspace-root path: canonical is already workspace-relative
                    if let Some(ref root) = self.root_path {
                        normalize_path(&root.join(&parsed.path))
                    } else {
                        // No workspace root: path is already workspace-relative
                        PathBuf::from(&parsed.path)
                    }
                }
                link_parser::PathType::Relative | link_parser::PathType::Ambiguous => {
                    // Relative path: resolve against old file's parent directory
                    normalize_path(&parent.join(&parsed.path))
                }
            };

            // Format the new part_of link
            let new_part_of = if self.root_path.is_some() {
                // Use markdown link format
                let target_canonical = self.get_canonical_path(&target_path);
                let new_path_canonical = self.get_canonical_path(&new_path);
                let target_title = self.resolve_title(&target_canonical).await;
                self.format_link_sync(&target_canonical, &target_title, &new_path_canonical)
            } else {
                // Fallback: plain relative path
                relative_path_from_file_to_target(&new_path, &target_path)
            };

            let _ = self
                .set_frontmatter_property(&new_path, "part_of", yaml::Value::String(new_part_of))
                .await;
        }

        // Update parent's contents via part_of (fallback: original parent directory)
        if let Some(parent_index) = self.resolve_part_of_to_path(&new_path, parent).await {
            let old_canonical = self.get_canonical_path(path);
            let _ = self
                .remove_from_index_contents_canonical(&parent_index, &old_canonical)
                .await;

            // Add new entry with proper formatting
            let new_path_canonical = self.get_canonical_path(&new_path);
            let title = self.resolve_title(&new_path_canonical).await;
            let _ = self
                .add_to_index_contents_canonical(&parent_index, &new_path_canonical, &title)
                .await;
        }

        Ok(new_path)
    }

    /// Convert an empty index file back to a leaf file.
    ///
    /// This method:
    /// - Fails if the index has non-empty `contents`
    /// - Moves `dir/{name}.md` → `parent/dir.md`
    /// - Removes the now-empty directory
    /// - Removes the `contents` property
    ///
    /// Example: `journal/my-note/my-note.md` → `journal/my-note.md`
    ///
    /// Returns the new path to the leaf file.
    pub async fn convert_to_leaf(&self, path: &Path) -> Result<PathBuf> {
        // Check if this is an index with empty contents
        let index = self.parse_index(path).await?;
        let contents = index.frontmatter.contents_list();
        if !contents.is_empty() {
            return Err(DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: format!(
                    "Cannot convert index with {} children to leaf",
                    contents.len()
                ),
            });
        }

        let current_dir = path.parent().ok_or_else(|| DiaryxError::InvalidPath {
            path: path.to_path_buf(),
            message: "File has no parent directory".to_string(),
        })?;

        let parent_of_dir = current_dir
            .parent()
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: path.to_path_buf(),
                message: "Directory has no parent".to_string(),
            })?;

        let dir_name = current_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: current_dir.to_path_buf(),
                message: "Invalid directory name".to_string(),
            })?;

        let new_filename = format!("{}.md", dir_name);
        let new_path = parent_of_dir.join(&new_filename);

        // Check if target already exists
        if self.fs.try_exists(&new_path).await.unwrap_or(false) {
            return Err(DiaryxError::InvalidPath {
                path: new_path,
                message: "Target file already exists".to_string(),
            });
        }

        // Move file out of directory
        self.fs.rename(path, &new_path).await?;

        // Remove contents property
        let _ = self
            .remove_frontmatter_property(&new_path, "contents")
            .await;

        // Update part_of path since file moved one level up
        if let Ok(Some(yaml::Value::String(old_part_of))) =
            self.get_frontmatter_property(&new_path, "part_of").await
        {
            use crate::path_utils::{normalize_path, relative_path_from_file_to_target};

            // Parse the markdown link to get the path and type
            let parsed = link_parser::parse_link(&old_part_of);

            // Resolve target path based on path type
            let target_path = match parsed.path_type {
                link_parser::PathType::WorkspaceRoot => {
                    // Workspace-root path: canonical is already workspace-relative
                    if let Some(ref root) = self.root_path {
                        normalize_path(&root.join(&parsed.path))
                    } else {
                        // No workspace root: path is already workspace-relative
                        PathBuf::from(&parsed.path)
                    }
                }
                link_parser::PathType::Relative | link_parser::PathType::Ambiguous => {
                    // Relative path: resolve against old file's directory
                    normalize_path(&current_dir.join(&parsed.path))
                }
            };

            // Format the new part_of link
            let new_part_of = if self.root_path.is_some() {
                // Use markdown link format
                let target_canonical = self.get_canonical_path(&target_path);
                let new_path_canonical = self.get_canonical_path(&new_path);
                let target_title = self.resolve_title(&target_canonical).await;
                self.format_link_sync(&target_canonical, &target_title, &new_path_canonical)
            } else {
                // Fallback: plain relative path
                relative_path_from_file_to_target(&new_path, &target_path)
            };

            let _ = self
                .set_frontmatter_property(&new_path, "part_of", yaml::Value::String(new_part_of))
                .await;
        }

        // Update parent's contents via part_of (fallback: grandparent directory)
        if let Some(parent_index) = self.resolve_part_of_to_path(&new_path, parent_of_dir).await {
            let old_canonical = self.get_canonical_path(path);
            let _ = self
                .remove_from_index_contents_canonical(&parent_index, &old_canonical)
                .await;

            // Add new entry with proper formatting
            let new_path_canonical = self.get_canonical_path(&new_path);
            let title = self.resolve_title(&new_path_canonical).await;
            let _ = self
                .add_to_index_contents_canonical(&parent_index, &new_path_canonical, &title)
                .await;
        }

        Ok(new_path)
    }

    /// Attach an entry to a parent, converting the parent to an index if needed,
    /// and moving the entry file into the parent's directory.
    ///
    /// This is a higher-level operation that combines:
    /// 1. Convert parent to index if it's a leaf
    /// 2. Move entry into parent's directory
    /// 3. Create bidirectional links (contents and part_of)
    ///
    /// Returns the new path to the entry after any moves.
    pub async fn attach_and_move_entry_to_parent(
        &self,
        entry: &Path,
        parent: &Path,
    ) -> Result<PathBuf> {
        // Check if parent needs to be converted to index
        let parent_is_index = self.is_index_file(parent).await;

        let effective_parent = if parent_is_index {
            parent.to_path_buf()
        } else {
            // Convert parent to index first
            self.convert_to_index(parent).await?
        };

        // Get parent directory
        let parent_dir = effective_parent
            .parent()
            .ok_or_else(|| DiaryxError::InvalidPath {
                path: effective_parent.clone(),
                message: "Parent index has no directory".to_string(),
            })?;

        // If the entry being moved is itself a non-root index (i.e. a folder
        // represented by a directory + its index file), we need to move the
        // entire containing directory — not just the .md file. Moving just
        // the index would strand every descendant at the old path with stale
        // `part_of` links. The root index lives at the workspace root and has
        // no containing subfolder to move, so it falls through to leaf-style
        // handling below.
        let entry_is_index = self.is_index_file(entry).await;
        let entry_is_root = entry_is_index && self.is_root_index(entry).await;
        if entry_is_index && !entry_is_root {
            let old_dir = entry.parent().ok_or_else(|| DiaryxError::InvalidPath {
                path: entry.to_path_buf(),
                message: "Index file has no parent directory".to_string(),
            })?;

            // If the folder is already directly inside the new parent's
            // directory, there's nothing to move — just make sure the index
            // is attached to the correct parent.
            if old_dir.parent() == Some(parent_dir) {
                self.attach_entry_to_parent(entry, &effective_parent)
                    .await?;
                return Ok(entry.to_path_buf());
            }

            let dir_name = old_dir
                .file_name()
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: old_dir.to_path_buf(),
                    message: "Index directory has no name".to_string(),
                })?;
            let new_dir = parent_dir.join(dir_name);

            // Refuse to move a folder into itself or into one of its own
            // descendants — that would create a cycle on disk.
            if new_dir == old_dir || new_dir.starts_with(old_dir) {
                return Err(DiaryxError::InvalidPath {
                    path: new_dir,
                    message: "Cannot move a folder into itself or its descendants".to_string(),
                });
            }

            if self.fs.try_exists(&new_dir).await.unwrap_or(false) {
                return Err(DiaryxError::InvalidPath {
                    path: new_dir,
                    message: "Target directory already exists".to_string(),
                });
            }

            let entry_filename = entry.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
                DiaryxError::InvalidPath {
                    path: entry.to_path_buf(),
                    message: "Invalid entry filename".to_string(),
                }
            })?;
            let new_entry_path = new_dir.join(entry_filename);

            // Physically move the whole directory tree.
            self.move_dir_contents_recursive(old_dir, &new_dir).await?;

            // sync_move_metadata updates the moved index's own part_of and,
            // because it's an index whose directory changed, also walks the
            // new subtree and rewrites every descendant's part_of against
            // its directory-nearest index at the new location.
            self.sync_move_metadata(entry, &new_entry_path).await?;

            return Ok(new_entry_path);
        }

        // Get entry filename
        let entry_filename =
            entry
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| DiaryxError::InvalidPath {
                    path: entry.to_path_buf(),
                    message: "Invalid entry filename".to_string(),
                })?;

        // Calculate new path for entry
        let new_entry_path = parent_dir.join(entry_filename);

        // Move entry if not already in parent directory.
        // move_entry already updates contents and part_of when it discovers the
        // target directory's index, so we only need attach_entry_to_parent when
        // the file didn't actually move.
        if entry.parent() != Some(parent_dir) {
            self.move_entry(entry, &new_entry_path).await?;
        } else {
            self.attach_entry_to_parent(&new_entry_path, &effective_parent)
                .await?;
        }

        Ok(new_entry_path)
    }
}
