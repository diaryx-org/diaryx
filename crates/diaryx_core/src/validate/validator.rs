//! Async workspace validator.
//!
//! Walks the `contents` hierarchy rooted at a workspace's root index, emitting
//! [`super::types::ValidationError`] and [`super::types::ValidationWarning`]
//! values for broken references, orphan files, cycles, non-portable paths,
//! and backlink inconsistencies. The entry points are
//! [`Validator::validate_workspace`] and [`Validator::validate_file`].

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::entry::{has_non_portable_chars, sanitize_filename};
use crate::error::Result;
use crate::fs::{AsyncFileSystem, is_temp_file};
use crate::link_parser::{self, LinkFormat};
use crate::path_utils::normalize_sync_path;
use crate::utils::{is_workspace_skip_dir, matches_glob_pattern};
use crate::workspace::Workspace;

use super::check::{
    check_duplicate_lists, check_non_portable_path, compute_suggested_portable_path,
    expected_self_link, find_index_in_directory, is_clearly_non_portable_path,
    list_contains_canonical_link, normalize_path, workspace_relative_canonical_path,
};
use super::types::{
    InvalidAttachmentRefKind, ValidationError, ValidationResult, ValidationWarning,
};

/// Validator for checking workspace link integrity (async-first).
pub struct Validator<FS: AsyncFileSystem> {
    ws: Workspace<FS>,
}

/// Context for recursive validation.
struct ValidationContext<'a> {
    result: &'a mut ValidationResult,
    visited: &'a mut HashSet<PathBuf>,
    link_format: Option<LinkFormat>,
    workspace_root: &'a Path,
}

#[derive(Default)]
struct ValidationScanTrace {
    explored_dirs: BTreeSet<String>,
    pruned_hidden_dirs: BTreeSet<String>,
    pruned_excluded_dirs: BTreeSet<String>,
    pruned_skip_dirs: BTreeSet<String>,
}

impl ValidationScanTrace {
    fn record_explored(&mut self, path: &Path) {
        self.explored_dirs
            .insert(path.to_string_lossy().to_string());
    }

    fn record_pruned_excluded(&mut self, path: &Path) {
        self.pruned_excluded_dirs
            .insert(path.to_string_lossy().to_string());
    }

    fn record_pruned_hidden(&mut self, path: &Path) {
        self.pruned_hidden_dirs
            .insert(path.to_string_lossy().to_string());
    }

    fn record_pruned_skip(&mut self, path: &Path) {
        self.pruned_skip_dirs
            .insert(path.to_string_lossy().to_string());
    }
}

impl<FS: AsyncFileSystem> Validator<FS> {
    /// Create a new validator.
    pub fn new(fs: FS) -> Self {
        Self {
            ws: Workspace::new(fs),
        }
    }

    /// Collect exclude patterns from an index and all its ancestors via part_of chain.
    /// Returns patterns that should apply to files in the index's directory.
    async fn collect_exclude_patterns(&self, index_path: &Path) -> Vec<String> {
        let mut patterns = Vec::new();
        let mut visited = HashSet::new();
        let mut current_path = index_path.to_path_buf();

        // Traverse up the part_of chain
        while !visited.contains(&current_path) {
            visited.insert(current_path.clone());

            if let Ok(index) = self.ws.parse_index(&current_path).await {
                // Add this index's exclude patterns
                patterns.extend(index.frontmatter.exclude_list().iter().cloned());

                // Follow part_of to parent
                if let Some(ref part_of) = index.frontmatter.part_of {
                    let parent_path = index.resolve_path(part_of);
                    current_path = parent_path;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        patterns
    }

    fn workspace_relative_path(&self, workspace_root: &Path, path: &Path) -> String {
        let relative = path.strip_prefix(workspace_root).unwrap_or(path);
        normalize_sync_path(&relative.to_string_lossy().replace('\\', "/"))
    }

    fn path_matches_exclude(
        &self,
        pattern: &str,
        workspace_root: &Path,
        path: &Path,
        file_name: &str,
    ) -> bool {
        let relative_path = self.workspace_relative_path(workspace_root, path);
        matches_glob_pattern(pattern, file_name) || matches_glob_pattern(pattern, &relative_path)
    }

    fn should_skip_workspace_dir(&self, path: &Path) -> bool {
        is_workspace_skip_dir(path)
    }

    async fn exclude_patterns_for_dir(&self, dir: &Path, workspace_root: &Path) -> Vec<String> {
        let mut current = Some(dir);
        while let Some(candidate) = current {
            if !candidate.starts_with(workspace_root) {
                break;
            }

            if let Ok(Some(index)) = self.ws.find_any_index_in_dir(candidate).await {
                return self.collect_exclude_patterns(&index).await;
            }

            current = candidate.parent();
        }

        Vec::new()
    }

    /// Validate all links starting from a workspace root index.
    ///
    /// Checks:
    /// - All `contents` references point to existing files
    /// - All `part_of` references point to existing files
    /// - Detects unlinked files/directories (not reachable via contents references)
    ///
    /// # Arguments
    /// * `root_path` - Path to the root index file
    /// * `max_depth` - Maximum depth for orphan detection (None = unlimited, Some(2) matches tree view)
    pub async fn validate_workspace(
        &self,
        root_path: &Path,
        max_depth: Option<usize>,
    ) -> Result<ValidationResult> {
        let mut result = ValidationResult::default();
        let mut visited: HashSet<PathBuf> = HashSet::new();

        // Get link format from workspace config
        let link_format = self
            .ws
            .get_workspace_config(root_path)
            .await
            .map(|c| c.link_format)
            .ok();

        // Get the workspace root directory (parent of root index file)
        // This is needed to resolve workspace-relative paths correctly
        let workspace_root = root_path.parent().unwrap_or(Path::new(".")).to_path_buf();

        let mut ctx = ValidationContext {
            result: &mut result,
            visited: &mut visited,
            link_format,
            workspace_root: &workspace_root,
        };

        self.validate_recursive(root_path, &mut ctx, None, None)
            .await?;

        // Find unlinked entries: files/dirs in workspace not visited during traversal
        // Scan with depth limit to match tree view behavior and improve performance
        let workspace_root = root_path.parent().unwrap_or(Path::new("."));
        let root_exclude_patterns = self
            .exclude_patterns_for_dir(workspace_root, workspace_root)
            .await;
        let mut scan_trace = ValidationScanTrace::default();
        let all_entries = self
            .list_files_with_depth(
                workspace_root,
                workspace_root,
                0,
                max_depth,
                &root_exclude_patterns,
                &mut scan_trace,
            )
            .await;

        log::info!(
            "[Validator] Orphan scan explored {} directories, pruned {} hidden dirs, {} excluded dirs and {} built-in skip dirs",
            scan_trace.explored_dirs.len(),
            scan_trace.pruned_hidden_dirs.len(),
            scan_trace.pruned_excluded_dirs.len(),
            scan_trace.pruned_skip_dirs.len(),
        );
        log::debug!(
            "[Validator] Orphan scan explored directories: {:?}",
            scan_trace.explored_dirs
        );
        log::debug!(
            "[Validator] Orphan scan pruned hidden directories: {:?}",
            scan_trace.pruned_hidden_dirs
        );
        log::debug!(
            "[Validator] Orphan scan pruned excluded directories: {:?}",
            scan_trace.pruned_excluded_dirs
        );
        log::debug!(
            "[Validator] Orphan scan pruned built-in skip directories: {:?}",
            scan_trace.pruned_skip_dirs
        );

        if !all_entries.is_empty() {
            // Normalize visited paths for comparison using path normalization
            // This is more reliable than canonicalize() which can fail on WASM
            let visited_normalized: HashSet<PathBuf> =
                visited.iter().map(|p| normalize_path(p)).collect();

            // Build a map of directory -> index file path from visited files.
            // Only actual index files should participate here; using arbitrary
            // markdown files can cause orphan binary warnings to inherit exclude
            // patterns from a sibling leaf note instead of the directory index.
            // This allows us to find the nearest parent index for orphan files
            let mut dir_to_index: std::collections::HashMap<PathBuf, PathBuf> =
                std::collections::HashMap::new();
            for visited_path in &visited {
                if visited_path.extension().is_some_and(|ext| ext == "md")
                    && let Some(parent) = visited_path.parent()
                {
                    let is_index = self
                        .ws
                        .parse_index(visited_path)
                        .await
                        .map(|index| index.frontmatter.is_index())
                        .unwrap_or(false);
                    if is_index {
                        dir_to_index
                            .entry(parent.to_path_buf())
                            .or_insert_with(|| visited_path.clone());
                    }
                }
            }

            // Helper to find nearest parent index for a given path
            let find_nearest_index = |path: &Path| -> Option<PathBuf> {
                let mut current = path.parent();
                while let Some(dir) = current {
                    if let Some(index) = dir_to_index.get(dir) {
                        return Some(index.clone());
                    }
                    current = dir.parent();
                }
                None
            };

            for entry in all_entries {
                // Skip entries that are in hidden directories or are hidden files
                // Check all path components, not just the filename
                let in_hidden_dir = entry.components().any(|c| {
                    if let std::path::Component::Normal(name) = c {
                        name.to_str().is_some_and(|s| s.starts_with('.'))
                    } else {
                        false
                    }
                });

                if in_hidden_dir {
                    continue;
                }

                // Skip entries in common non-workspace directories
                if self.should_skip_workspace_dir(&entry) {
                    continue;
                }

                let entry_normalized = normalize_path(&entry);
                if !visited_normalized.contains(&entry_normalized) {
                    // Skip directories - we don't emit warnings for them.
                    // If a directory has an unlinked index file, OrphanFile covers it.
                    // If a directory has no index file, it's just a regular folder.
                    if self.ws.fs_ref().is_dir(&entry).await {
                        continue;
                    }

                    // Skip symlinks - they're filesystem implementation details.
                    // The real file is what matters for the hierarchy.
                    if self.ws.fs_ref().is_symlink(&entry).await {
                        continue;
                    }

                    let suggested_index = find_nearest_index(&entry);
                    let extension = entry.extension().and_then(|e| e.to_str());

                    // Collect exclude patterns from the nearest index and its ancestors
                    let local_exclude_patterns = if let Some(ref idx) = suggested_index {
                        self.collect_exclude_patterns(idx).await
                    } else {
                        Vec::new()
                    };

                    let filename = entry.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    let is_excluded = local_exclude_patterns.iter().any(|pattern| {
                        self.path_matches_exclude(pattern, workspace_root, &entry, filename)
                    });

                    if extension == Some("md") {
                        // Markdown file not in hierarchy
                        if !is_excluded {
                            // Attachment notes (files with the `attachment` property) are
                            // managed via `attachments` lists, not `contents`/`part_of`.
                            // Skip contents/part_of validation for them.
                            let is_attachment_note =
                                if let Ok(idx) = self.ws.parse_index(&entry).await {
                                    idx.frontmatter.attachment.is_some()
                                } else {
                                    false
                                };

                            if !is_attachment_note {
                                result.warnings.push(ValidationWarning::OrphanFile {
                                    file: entry.clone(),
                                    suggested_index: suggested_index.clone(),
                                });

                                // Also check if this orphan file is missing part_of
                                if let Ok(index) = self.ws.parse_index(&entry).await
                                    && !index.frontmatter.is_index()
                                    && index.frontmatter.part_of.is_none()
                                {
                                    result.warnings.push(ValidationWarning::MissingPartOf {
                                        file: entry.clone(),
                                        suggested_index,
                                    });
                                }
                            }
                        }
                    } else if extension.is_some() && !is_excluded {
                        // Binary file not referenced by any attachments
                        result.warnings.push(ValidationWarning::OrphanBinaryFile {
                            file: entry.clone(),
                            suggested_index,
                        });
                    }
                }
            }
        }

        Ok(result)
    }

    /// List files and directories with depth limiting.
    /// Returns all entries up to the specified depth from the starting directory.
    async fn list_files_with_depth(
        &self,
        dir: &Path,
        workspace_root: &Path,
        current_depth: usize,
        max_depth: Option<usize>,
        exclude_patterns: &[String],
        trace: &mut ValidationScanTrace,
    ) -> Vec<PathBuf> {
        // Check if we've exceeded max depth
        if let Some(max) = max_depth
            && current_depth >= max
        {
            return Vec::new();
        }

        let mut all_entries = Vec::new();
        trace.record_explored(dir);

        if let Ok(entries) = self.ws.fs_ref().list_files(dir).await {
            for entry in entries {
                // Skip symlinks entirely - they're filesystem implementation details.
                // Also avoids potential infinite loops from circular symlinks.
                if self.ws.fs_ref().is_symlink(&entry).await {
                    continue;
                }

                if let Some(name) = entry.file_name().and_then(|n| n.to_str())
                    && name.starts_with('.')
                {
                    if self.ws.fs_ref().is_dir(&entry).await {
                        trace.record_pruned_hidden(&entry);
                    }
                    continue;
                }

                // Skip temporary files (.tmp, .bak, .swap) from atomic write operations
                if let Some(name) = entry.file_name().and_then(|n| n.to_str())
                    && is_temp_file(name)
                {
                    continue;
                }

                let file_name = entry.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if exclude_patterns.iter().any(|pattern| {
                    self.path_matches_exclude(pattern, workspace_root, &entry, file_name)
                }) {
                    if self.ws.fs_ref().is_dir(&entry).await {
                        trace.record_pruned_excluded(&entry);
                    }
                    continue;
                }

                // Recurse into subdirectories
                if self.ws.fs_ref().is_dir(&entry).await {
                    if self.should_skip_workspace_dir(&entry) {
                        trace.record_pruned_skip(&entry);
                        continue;
                    }

                    all_entries.push(entry.clone());

                    let child_exclude_patterns =
                        self.exclude_patterns_for_dir(&entry, workspace_root).await;
                    let sub_entries = Box::pin(self.list_files_with_depth(
                        &entry,
                        workspace_root,
                        current_depth + 1,
                        max_depth,
                        &child_exclude_patterns,
                        trace,
                    ))
                    .await;
                    all_entries.extend(sub_entries);
                } else {
                    all_entries.push(entry.clone());
                }
            }
        }

        all_entries
    }

    /// Recursively validate from a given path.
    /// `from_parent` tracks which file led us here (for cycle detection).
    /// `contents_ref` is the reference string used in the parent's contents (for cycle fix suggestions).
    async fn validate_recursive(
        &self,
        path: &Path,
        ctx: &mut ValidationContext<'_>,
        from_parent: Option<&Path>,
        contents_ref: Option<&str>,
    ) -> Result<()> {
        // Skip symlinks - they're filesystem implementation details.
        // The real file (the symlink target) is what matters for the hierarchy.
        if self.ws.fs_ref().is_symlink(path).await {
            return Ok(());
        }

        // Avoid cycles - use normalize_path for consistent path comparison
        let normalized = normalize_path(path);
        if ctx.visited.contains(&normalized) {
            // Cycle detected! Suggest removing the contents reference from the parent
            ctx.result
                .warnings
                .push(ValidationWarning::CircularReference {
                    files: vec![path.to_path_buf()],
                    // Suggest editing the parent file that led us here
                    suggested_file: from_parent.map(|p| p.to_path_buf()),
                    // The contents ref to remove would be in the parent, but we suggest
                    // removing part_of from the target file as that's often cleaner
                    suggested_remove_part_of: contents_ref.map(|s| s.to_string()),
                });
            return Ok(());
        }
        ctx.visited.insert(normalized);
        ctx.result.files_checked += 1;

        // Check filename for non-portable characters
        if let Some(filename) = path.file_name().and_then(|n| n.to_str())
            && let Some(reason) = has_non_portable_chars(filename)
        {
            ctx.result
                .warnings
                .push(ValidationWarning::NonPortableFilename {
                    file: path.to_path_buf(),
                    reason,
                    suggested_filename: sanitize_filename(filename),
                });
        }

        // Try to parse as index (with link format hint for proper path resolution)
        if let Ok(index) = self.ws.parse_index_with_hint(path, ctx.link_format).await {
            let dir = index.directory().unwrap_or_else(|| Path::new(""));
            let file_canonical = workspace_relative_canonical_path(path, ctx.workspace_root);

            // Flag duplicate entries in any link-bearing list.
            check_duplicate_lists(
                ctx.result,
                path,
                &index.frontmatter,
                &file_canonical,
                ctx.link_format,
            );

            if let Some(link) = index.frontmatter.link.as_deref() {
                let parsed = link_parser::parse_link(link);
                let resolved = link_parser::to_canonical_with_link_format(
                    &parsed,
                    Path::new(&file_canonical),
                    ctx.link_format,
                );
                if resolved != file_canonical {
                    ctx.result
                        .warnings
                        .push(ValidationWarning::InvalidSelfLink {
                            file: path.to_path_buf(),
                            value: link.to_string(),
                            suggested: expected_self_link(
                                &file_canonical,
                                index.frontmatter.title.as_deref(),
                                ctx.link_format,
                            ),
                        });
                }
            }

            for link_ref in index.frontmatter.links_list() {
                let target_path = index.resolve_path(link_ref);
                let absolute_target_path = if target_path.is_absolute() {
                    target_path.clone()
                } else {
                    ctx.workspace_root.join(&target_path)
                };

                if !self.ws.fs_ref().exists(&absolute_target_path).await {
                    ctx.result.errors.push(ValidationError::BrokenLinkRef {
                        file: path.to_path_buf(),
                        target: link_ref.clone(),
                    });
                    continue;
                }

                if let Ok(target_index) = self
                    .ws
                    .parse_index_with_hint(&absolute_target_path, ctx.link_format)
                    .await
                {
                    let target_canonical = workspace_relative_canonical_path(
                        &absolute_target_path,
                        ctx.workspace_root,
                    );
                    if !list_contains_canonical_link(
                        target_index.frontmatter.link_of_list(),
                        &file_canonical,
                        &target_canonical,
                        ctx.link_format,
                    ) {
                        ctx.result
                            .warnings
                            .push(ValidationWarning::MissingBacklink {
                                file: absolute_target_path.clone(),
                                source: file_canonical.clone(),
                                suggested: expected_self_link(
                                    &file_canonical,
                                    index.frontmatter.title.as_deref(),
                                    ctx.link_format,
                                ),
                            });
                    }
                }
            }

            for backlink in index.frontmatter.link_of_list() {
                let source_path = index.resolve_path(backlink);
                let absolute_source_path = if source_path.is_absolute() {
                    source_path.clone()
                } else {
                    ctx.workspace_root.join(&source_path)
                };

                let stale = if !self.ws.fs_ref().exists(&absolute_source_path).await {
                    true
                } else if let Ok(source_index) = self
                    .ws
                    .parse_index_with_hint(&absolute_source_path, ctx.link_format)
                    .await
                {
                    let source_canonical = workspace_relative_canonical_path(
                        &absolute_source_path,
                        ctx.workspace_root,
                    );
                    !list_contains_canonical_link(
                        source_index.frontmatter.links_list(),
                        &file_canonical,
                        &source_canonical,
                        ctx.link_format,
                    )
                } else {
                    true
                };

                if stale {
                    ctx.result.warnings.push(ValidationWarning::StaleBacklink {
                        file: path.to_path_buf(),
                        value: backlink.clone(),
                    });
                }
            }

            // Check all contents references
            for child_ref in index.frontmatter.contents_list() {
                // Use index.resolve_path which handles markdown links and relative paths
                let child_path = index.resolve_path(child_ref);

                // Make path absolute if needed by joining with workspace root
                // This handles the case where resolve_path returns workspace-relative paths
                // but we need absolute paths for the real filesystem
                let absolute_child_path = if child_path.is_absolute() {
                    child_path.clone()
                } else {
                    ctx.workspace_root.join(&child_path)
                };

                // Flag non-portable relative dot-component paths (e.g.
                // `../foo.md`). Absolute-path portability is handled
                // separately below for `part_of`.
                if let Some(warning) = check_non_portable_path(path, "contents", child_ref, dir) {
                    ctx.result.warnings.push(warning);
                }

                if !self.ws.fs_ref().exists(&absolute_child_path).await {
                    ctx.result.errors.push(ValidationError::BrokenContentsRef {
                        index: path.to_path_buf(),
                        target: child_ref.clone(),
                    });
                } else if child_path.extension().is_none_or(|ext| ext != "md") {
                    // Non-markdown file in contents - contents entries must be
                    // markdown. Binary assets belong in `attachments`, wrapped
                    // by a markdown attachment note.
                    ctx.result
                        .warnings
                        .push(ValidationWarning::InvalidContentsRef {
                            index: path.to_path_buf(),
                            target: child_ref.clone(),
                        });
                } else {
                    // Recurse into child, tracking parent info for cycle detection
                    Box::pin(self.validate_recursive(
                        &absolute_child_path,
                        ctx,
                        Some(path),
                        Some(child_ref),
                    ))
                    .await?;
                }
            }

            // Check part_of if present
            if let Some(ref part_of) = index.frontmatter.part_of {
                if is_clearly_non_portable_path(part_of) {
                    // Non-portable absolute path - add warning, skip exists() check
                    ctx.result
                        .warnings
                        .push(ValidationWarning::NonPortablePath {
                            file: path.to_path_buf(),
                            property: "part_of".to_string(),
                            value: part_of.clone(),
                            suggested: compute_suggested_portable_path(part_of, dir),
                        });
                } else {
                    // Flag non-portable relative dot-component paths
                    if let Some(warning) = check_non_portable_path(path, "part_of", part_of, dir) {
                        ctx.result.warnings.push(warning);
                    }

                    // Use index.resolve_path which handles markdown links and relative paths
                    let parent_path = index.resolve_path(part_of);
                    // Make path absolute if needed
                    let absolute_parent_path = if parent_path.is_absolute() {
                        parent_path
                    } else {
                        ctx.workspace_root.join(&parent_path)
                    };
                    if !self.ws.fs_ref().exists(&absolute_parent_path).await {
                        ctx.result.errors.push(ValidationError::BrokenPartOf {
                            file: path.to_path_buf(),
                            target: part_of.clone(),
                        });
                    }
                }
            } else if from_parent.is_some() {
                // File has no part_of but was reached from a parent's contents.
                // Non-index files should have part_of to maintain hierarchy links.
                // Index files without part_of could be sub-roots, which is allowed.
                // Attachment notes are managed via `attachments`, not `contents`/`part_of`.
                let is_attachment_note = index.frontmatter.attachment.is_some();

                if !index.frontmatter.is_index() && !is_attachment_note {
                    let suggested_index = find_index_in_directory(&self.ws, dir, Some(path)).await;
                    ctx.result.warnings.push(ValidationWarning::MissingPartOf {
                        file: path.to_path_buf(),
                        suggested_index,
                    });
                }
            }

            // Mark attachments as visited so they're not reported as orphans.
            //
            // An attachments entry is a markdown "attachment note" that wraps a
            // binary asset. We mark both the note and the binary it points to
            // as visited so neither is flagged as an orphan at workspace scan
            // time. If the attachment target is missing, we still emit a
            // BrokenAttachment error.
            for attachment in index.frontmatter.attachments_list() {
                // Flag non-portable relative dot-component paths
                if let Some(warning) = check_non_portable_path(path, "attachments", attachment, dir)
                {
                    ctx.result.warnings.push(warning);
                }

                let attachment_path = index.resolve_path(attachment);
                let absolute_attachment_path = if attachment_path.is_absolute() {
                    attachment_path
                } else {
                    ctx.workspace_root.join(&attachment_path)
                };
                if !self.ws.fs_ref().exists(&absolute_attachment_path).await {
                    ctx.result.errors.push(ValidationError::BrokenAttachment {
                        file: path.to_path_buf(),
                        attachment: attachment.clone(),
                    });
                    continue;
                }
                ctx.visited.insert(absolute_attachment_path.clone());

                // An attachments entry must be a markdown attachment note
                // (`.md` with an `attachment:` frontmatter property). Anything
                // else is the legacy flat format and we flag it so the user
                // can migrate.
                if let Some((reason, kind)) = self
                    .attachment_entry_invalid_reason(&absolute_attachment_path)
                    .await
                {
                    ctx.result
                        .warnings
                        .push(ValidationWarning::InvalidAttachmentRef {
                            file: path.to_path_buf(),
                            target: attachment.clone(),
                            reason,
                            kind,
                        });
                    continue;
                }

                // If this is an attachment note, also mark the binary it
                // wraps as visited so orphan scanning ignores it.
                self.mark_attachment_binary_visited(
                    &absolute_attachment_path,
                    ctx.link_format,
                    ctx.workspace_root,
                    ctx.visited,
                )
                .await;

                // Verify the attachment note has a matching `attachment_of`
                // backlink to this index. This parallels the links/link_of
                // backlink check above.
                if let Ok(note) = self
                    .ws
                    .parse_index_with_hint(&absolute_attachment_path, ctx.link_format)
                    .await
                {
                    let note_canonical = workspace_relative_canonical_path(
                        &absolute_attachment_path,
                        ctx.workspace_root,
                    );
                    if !list_contains_canonical_link(
                        note.frontmatter.attachment_of_list(),
                        &file_canonical,
                        &note_canonical,
                        ctx.link_format,
                    ) {
                        ctx.result
                            .warnings
                            .push(ValidationWarning::MissingAttachmentBacklink {
                                file: absolute_attachment_path.clone(),
                                source: file_canonical.clone(),
                                suggested: expected_self_link(
                                    &file_canonical,
                                    index.frontmatter.title.as_deref(),
                                    ctx.link_format,
                                ),
                            });
                    }
                }
            }

            // Check this file's own `attachment_of` entries for stale refs:
            // the source must exist AND its `attachments` list must still
            // reference this file.
            for backlink in index.frontmatter.attachment_of_list() {
                let source_path = index.resolve_path(backlink);
                let absolute_source_path = if source_path.is_absolute() {
                    source_path.clone()
                } else {
                    ctx.workspace_root.join(&source_path)
                };

                let stale = if !self.ws.fs_ref().exists(&absolute_source_path).await {
                    true
                } else if let Ok(source_index) = self
                    .ws
                    .parse_index_with_hint(&absolute_source_path, ctx.link_format)
                    .await
                {
                    let source_canonical = workspace_relative_canonical_path(
                        &absolute_source_path,
                        ctx.workspace_root,
                    );
                    !list_contains_canonical_link(
                        source_index.frontmatter.attachments_list(),
                        &file_canonical,
                        &source_canonical,
                        ctx.link_format,
                    )
                } else {
                    true
                };

                if stale {
                    ctx.result
                        .warnings
                        .push(ValidationWarning::StaleAttachmentBacklink {
                            file: path.to_path_buf(),
                            value: backlink.clone(),
                        });
                }
            }
        }

        Ok(())
    }

    /// Decide whether a resolved `attachments[]` entry is a valid markdown
    /// attachment note. Returns `None` if it is, or `Some((reason, kind))`
    /// with a short human-readable explanation and a structured classification
    /// used by the autofixer if it is not.
    ///
    /// Rules:
    /// - Extension must be `.md` (case-insensitive).
    /// - The parsed file must expose an `attachment:` frontmatter property.
    async fn attachment_entry_invalid_reason(
        &self,
        entry: &Path,
    ) -> Option<(String, InvalidAttachmentRefKind)> {
        let is_markdown = entry
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
        if !is_markdown {
            return Some((
                "not a markdown attachment note (wrap the binary in a `.md` note with an `attachment:` property)"
                    .to_string(),
                InvalidAttachmentRefKind::LegacyBinary {
                    binary_path: entry.to_path_buf(),
                },
            ));
        }

        match self.ws.parse_index(entry).await {
            Ok(note) if note.frontmatter.attachment.is_some() => None,
            Ok(_) => Some((
                "markdown file is not an attachment note (missing `attachment:` frontmatter property)"
                    .to_string(),
                InvalidAttachmentRefKind::NotAttachmentNote,
            )),
            Err(_) => Some((
                "could not parse file as an attachment note".to_string(),
                InvalidAttachmentRefKind::UnparseableNote,
            )),
        }
    }

    /// Parse a markdown file as a potential attachment note and, if it has an
    /// `attachment:` property pointing at a binary asset, mark that binary as
    /// visited so the orphan scanner doesn't flag it.
    async fn mark_attachment_binary_visited(
        &self,
        note_path: &Path,
        link_format: Option<LinkFormat>,
        workspace_root: &Path,
        visited: &mut HashSet<PathBuf>,
    ) {
        let Ok(note) = self.ws.parse_index_with_hint(note_path, link_format).await else {
            return;
        };
        let Some(ref attachment_ref) = note.frontmatter.attachment else {
            return;
        };
        let binary_path = note.resolve_path(attachment_ref);
        let absolute_binary_path = if binary_path.is_absolute() {
            binary_path
        } else {
            workspace_root.join(&binary_path)
        };
        if self.ws.fs_ref().exists(&absolute_binary_path).await {
            visited.insert(absolute_binary_path);
        }
    }

    /// Try to find the workspace root by searching for a root index file in parent directories.
    ///
    /// Walks up from `start_path` asking `Workspace::find_any_index_in_dir`
    /// at each level. The first directory whose index has no `part_of` is the
    /// root. Returns `None` if no root index is found within 10 levels.
    async fn find_workspace_root(&self, start_path: &Path) -> Option<PathBuf> {
        let mut current = start_path.parent()?;

        for _ in 0..10 {
            if let Ok(Some(candidate)) = self.ws.find_any_index_in_dir(current).await
                && let Ok(index) = self.ws.parse_index(&candidate).await
                && index.frontmatter.is_root()
            {
                return Some(current.to_path_buf());
            }

            current = current.parent()?;
        }

        None
    }

    /// Validate a single file's links.
    ///
    /// Checks:
    /// - The file's `part_of` reference points to an existing file
    /// - All `contents` references (if any) point to existing files
    /// - Markdown files in the same directory that aren't listed in `contents`
    /// - Sub-indexes (files with a `contents` property) inside immediate
    ///   subdirectories that aren't listed in this index's `contents`
    ///
    /// Does not recursively validate the entire workspace, just the specified file.
    pub async fn validate_file(&self, file_path: &Path) -> Result<ValidationResult> {
        let mut result = ValidationResult::default();

        // Strip `.`/`..` components without touching the filesystem. We
        // deliberately avoid `canonicalize()`: it fails on WASM and on the
        // in-memory filesystem, and follows symlinks which we don't want.
        //
        // We also don't force the path to be absolute — each filesystem
        // backend (real, in-memory, WASM) handles relative paths according
        // to its own conventions, and the validator is agnostic.
        let path = normalize_path(file_path);

        if !self.ws.fs_ref().exists(&path).await {
            return Err(crate::error::DiaryxError::InvalidPath {
                path: path.clone(),
                message: "File not found".to_string(),
            });
        }

        result.files_checked = 1;

        // Check filename for non-portable characters
        if let Some(filename) = path.file_name().and_then(|n| n.to_str())
            && let Some(reason) = has_non_portable_chars(filename)
        {
            result
                .warnings
                .push(ValidationWarning::NonPortableFilename {
                    file: path.clone(),
                    reason,
                    suggested_filename: sanitize_filename(filename),
                });
        }

        // Try to find the workspace root by looking for a root index in parent directories
        // Fall back to the file's parent directory if not found
        let workspace_root = self
            .find_workspace_root(&path)
            .await
            .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")).to_path_buf());

        // Get link format from workspace config
        let link_format = self
            .ws
            .get_workspace_config(&workspace_root.join("README.md"))
            .await
            .map(|c| c.link_format)
            .ok();

        // Try to parse and validate (with link format hint)
        if let Ok(index) = self.ws.parse_index_with_hint(&path, link_format).await {
            let dir = index.directory().unwrap_or_else(|| Path::new(""));
            let file_canonical = workspace_relative_canonical_path(&path, &workspace_root);

            // Flag duplicate entries in any link-bearing list.
            check_duplicate_lists(
                &mut result,
                &path,
                &index.frontmatter,
                &file_canonical,
                link_format,
            );

            if let Some(link) = index.frontmatter.link.as_deref() {
                let parsed = link_parser::parse_link(link);
                let resolved = link_parser::to_canonical_with_link_format(
                    &parsed,
                    Path::new(&file_canonical),
                    link_format,
                );
                if resolved != file_canonical {
                    result.warnings.push(ValidationWarning::InvalidSelfLink {
                        file: path.clone(),
                        value: link.to_string(),
                        suggested: expected_self_link(
                            &file_canonical,
                            index.frontmatter.title.as_deref(),
                            link_format,
                        ),
                    });
                }
            }

            for link_ref in index.frontmatter.links_list() {
                let target_path = index.resolve_path(link_ref);
                let absolute_target_path = if target_path.is_absolute() {
                    target_path.clone()
                } else {
                    workspace_root.join(&target_path)
                };

                if !self.ws.fs_ref().exists(&absolute_target_path).await {
                    result.errors.push(ValidationError::BrokenLinkRef {
                        file: path.clone(),
                        target: link_ref.clone(),
                    });
                    continue;
                }

                if let Ok(target_index) = self
                    .ws
                    .parse_index_with_hint(&absolute_target_path, link_format)
                    .await
                {
                    let target_canonical =
                        workspace_relative_canonical_path(&absolute_target_path, &workspace_root);
                    if !list_contains_canonical_link(
                        target_index.frontmatter.link_of_list(),
                        &file_canonical,
                        &target_canonical,
                        link_format,
                    ) {
                        result.warnings.push(ValidationWarning::MissingBacklink {
                            file: absolute_target_path.clone(),
                            source: file_canonical.clone(),
                            suggested: expected_self_link(
                                &file_canonical,
                                index.frontmatter.title.as_deref(),
                                link_format,
                            ),
                        });
                    }
                }
            }

            for backlink in index.frontmatter.link_of_list() {
                let source_path = index.resolve_path(backlink);
                let absolute_source_path = if source_path.is_absolute() {
                    source_path.clone()
                } else {
                    workspace_root.join(&source_path)
                };

                let stale = if !self.ws.fs_ref().exists(&absolute_source_path).await {
                    true
                } else if let Ok(source_index) = self
                    .ws
                    .parse_index_with_hint(&absolute_source_path, link_format)
                    .await
                {
                    let source_canonical =
                        workspace_relative_canonical_path(&absolute_source_path, &workspace_root);
                    !list_contains_canonical_link(
                        source_index.frontmatter.links_list(),
                        &file_canonical,
                        &source_canonical,
                        link_format,
                    )
                } else {
                    true
                };

                if stale {
                    result.warnings.push(ValidationWarning::StaleBacklink {
                        file: path.clone(),
                        value: backlink.clone(),
                    });
                }
            }

            for backlink in index.frontmatter.attachment_of_list() {
                let source_path = index.resolve_path(backlink);
                let absolute_source_path = if source_path.is_absolute() {
                    source_path.clone()
                } else {
                    workspace_root.join(&source_path)
                };

                let stale = if !self.ws.fs_ref().exists(&absolute_source_path).await {
                    true
                } else if let Ok(source_index) = self
                    .ws
                    .parse_index_with_hint(&absolute_source_path, link_format)
                    .await
                {
                    let source_canonical =
                        workspace_relative_canonical_path(&absolute_source_path, &workspace_root);
                    !list_contains_canonical_link(
                        source_index.frontmatter.attachments_list(),
                        &file_canonical,
                        &source_canonical,
                        link_format,
                    )
                } else {
                    true
                };

                if stale {
                    result
                        .warnings
                        .push(ValidationWarning::StaleAttachmentBacklink {
                            file: path.clone(),
                            value: backlink.clone(),
                        });
                }
            }

            // Collect listed contents entries as normalized absolute PathBufs
            // so we can compare directory entries against them without losing
            // directory information (filename-only matching would collide on
            // siblings with the same basename across subdirectories).
            let contents_list = index.frontmatter.contents_list();
            let listed_files: HashSet<PathBuf> = contents_list
                .iter()
                .map(|child_ref| {
                    let resolved = index.resolve_path(child_ref);
                    let absolute = if resolved.is_absolute() {
                        resolved
                    } else {
                        workspace_root.join(&resolved)
                    };
                    normalize_path(&absolute)
                })
                .collect();

            // Check all contents references
            for child_ref in contents_list {
                // Use index.resolve_path which handles markdown links and relative paths
                let child_path = index.resolve_path(child_ref);

                // Make path absolute if needed by joining with workspace root
                let absolute_child_path = if child_path.is_absolute() {
                    child_path.clone()
                } else {
                    workspace_root.join(&child_path)
                };

                if !self.ws.fs_ref().exists(&absolute_child_path).await {
                    result.errors.push(ValidationError::BrokenContentsRef {
                        index: path.clone(),
                        target: child_ref.clone(),
                    });
                } else if child_path.extension().is_none_or(|ext| ext != "md") {
                    // Non-markdown file in contents - contents entries must be
                    // markdown. Binary assets belong in `attachments`, wrapped
                    // by a markdown attachment note.
                    result.warnings.push(ValidationWarning::InvalidContentsRef {
                        index: path.clone(),
                        target: child_ref.clone(),
                    });
                }
            }

            // Check part_of if present
            if let Some(ref part_of) = index.frontmatter.part_of {
                if is_clearly_non_portable_path(part_of) {
                    // Non-portable path - add warning, skip exists() check
                    result.warnings.push(ValidationWarning::NonPortablePath {
                        file: path.clone(),
                        property: "part_of".to_string(),
                        value: part_of.clone(),
                        suggested: compute_suggested_portable_path(part_of, dir),
                    });
                } else {
                    // Use index.resolve_path which handles markdown links and relative paths
                    let parent_path = index.resolve_path(part_of);
                    // Make path absolute if needed
                    let absolute_parent_path = if parent_path.is_absolute() {
                        parent_path
                    } else {
                        workspace_root.join(&parent_path)
                    };
                    if !self.ws.fs_ref().exists(&absolute_parent_path).await {
                        result.errors.push(ValidationError::BrokenPartOf {
                            file: path.clone(),
                            target: part_of.clone(),
                        });
                    }
                    // Also check for . or .. in non-absolute paths
                    if let Some(warning) = check_non_portable_path(&path, "part_of", part_of, dir) {
                        result.warnings.push(warning);
                    }
                }
            } else {
                // File has no part_of - check if it's a root index
                // Non-index files (files without contents) should have part_of
                // Index files without part_of are potential root indexes, which is allowed
                // Attachment notes are managed via `attachments`, not `contents`/`part_of`
                // But if it has no contents AND no part_of, it's definitely orphaned
                let is_attachment_note = index.frontmatter.attachment.is_some();

                if !index.frontmatter.is_index() && !is_attachment_note {
                    // Regular file with no part_of = orphan
                    // Try to find an index in the same directory to suggest
                    let suggested_index = find_index_in_directory(&self.ws, dir, Some(&path)).await;
                    result.warnings.push(ValidationWarning::MissingPartOf {
                        file: path.clone(),
                        suggested_index,
                    });
                }
            }

            // Check contents entries for non-portable paths
            for child_ref in index.frontmatter.contents_list() {
                if let Some(warning) = check_non_portable_path(&path, "contents", child_ref, dir) {
                    result.warnings.push(warning);
                }
            }

            // Check attachments if present
            for attachment in index.frontmatter.attachments_list() {
                // Use index.resolve_path which handles markdown links and relative paths
                let attachment_path = index.resolve_path(attachment);

                // Make path absolute if needed
                let absolute_attachment_path = if attachment_path.is_absolute() {
                    attachment_path
                } else {
                    workspace_root.join(&attachment_path)
                };

                // Check if attachment path is non-portable
                if let Some(warning) =
                    check_non_portable_path(&path, "attachments", attachment, dir)
                {
                    result.warnings.push(warning);
                }

                // Check if attachment exists
                if !self.ws.fs_ref().exists(&absolute_attachment_path).await {
                    result.errors.push(ValidationError::BrokenAttachment {
                        file: path.clone(),
                        attachment: attachment.clone(),
                    });
                    continue;
                }

                // Verify the entry points at a valid markdown attachment note.
                if let Some((reason, kind)) = self
                    .attachment_entry_invalid_reason(&absolute_attachment_path)
                    .await
                {
                    result
                        .warnings
                        .push(ValidationWarning::InvalidAttachmentRef {
                            file: path.clone(),
                            target: attachment.clone(),
                            reason,
                            kind,
                        });
                }
            }

            // Check for unlisted .md files in the same directory
            // Only if this file has contents (is an index)
            if index.frontmatter.is_index()
                && let Ok(entries) = self.ws.fs_ref().list_files(dir).await
            {
                let this_filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                // Collect exclude patterns from this index and all ancestors
                let inherited_exclude_patterns = self.collect_exclude_patterns(&path).await;

                // Collect all attachments referenced by this index as
                // normalized absolute paths, so we can compare directory
                // entries against them without losing directory information.
                //
                // Note: under the current attachment model, `attachments`
                // entries point at markdown attachment *notes* (not binaries
                // directly). A binary sitting in the same directory as this
                // index is therefore correctly reported as an orphan — the
                // expected layout is `_attachments/<file>.<ext>` wrapped by
                // `_attachments/<file>.<ext>.md`.
                let referenced_attachments: HashSet<PathBuf> = index
                    .frontmatter
                    .attachments_list()
                    .iter()
                    .map(|attachment_ref| {
                        let resolved = index.resolve_path(attachment_ref);
                        let absolute = if resolved.is_absolute() {
                            resolved
                        } else {
                            workspace_root.join(&resolved)
                        };
                        normalize_path(&absolute)
                    })
                    .collect();

                // Collect other index files in this directory
                let mut other_indexes: Vec<PathBuf> = Vec::new();

                // Collect immediate subdirectories so we can also look 1 level
                // deep for orphaned sub-indexes (files with a `contents`
                // property that the current index doesn't reference).
                let mut subdirs_to_check: Vec<PathBuf> = Vec::new();

                for entry_path in entries {
                    // Skip symlinks - they're filesystem implementation details
                    if self.ws.fs_ref().is_symlink(&entry_path).await {
                        continue;
                    }

                    // Skip hidden files (starting with .)
                    if entry_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|s| s.starts_with('.'))
                    {
                        continue;
                    }

                    // Skip temporary files (.tmp, .bak, .swap) from atomic write operations
                    if entry_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(is_temp_file)
                    {
                        continue;
                    }

                    if self.ws.fs_ref().is_dir(&entry_path).await {
                        if !self.should_skip_workspace_dir(&entry_path) {
                            subdirs_to_check.push(entry_path);
                        }
                        continue;
                    }

                    let extension = entry_path.extension().and_then(|e| e.to_str());
                    let filename = entry_path.file_name().and_then(|n| n.to_str());

                    match extension {
                        Some("md") => {
                            if let Some(fname) = filename {
                                // Skip the current file
                                if fname == this_filename {
                                    continue;
                                }

                                // Parse the entry once to answer both "is this
                                // another index in the same directory?" and
                                // "is this an attachment note?". A file is an
                                // index iff its frontmatter has a `contents`
                                // property — filename is irrelevant.
                                let parsed_entry = self.ws.parse_index(&entry_path).await.ok();
                                let is_other_index = parsed_entry
                                    .as_ref()
                                    .is_some_and(|idx| idx.frontmatter.is_index());
                                let is_attachment_note = parsed_entry
                                    .as_ref()
                                    .is_some_and(|idx| idx.frontmatter.attachment.is_some());

                                if is_other_index {
                                    other_indexes.push(entry_path.clone());
                                }
                                // Check if this markdown file is in contents
                                let entry_normalized = normalize_path(&entry_path);
                                if !listed_files.contains(&entry_normalized) {
                                    // Check if file matches any exclude pattern (inherited from ancestors)
                                    let is_excluded =
                                        inherited_exclude_patterns.iter().any(|pattern| {
                                            self.path_matches_exclude(
                                                pattern,
                                                &workspace_root,
                                                &entry_path,
                                                fname,
                                            )
                                        });

                                    if !is_excluded && !is_attachment_note {
                                        result.warnings.push(ValidationWarning::OrphanFile {
                                            file: entry_path,
                                            suggested_index: Some(path.clone()),
                                        });
                                    }
                                }
                            }
                        }
                        Some(ext) if !ext.eq_ignore_ascii_case("md") => {
                            // Binary file - check if it's referenced by attachments
                            let entry_normalized = normalize_path(&entry_path);
                            if let Some(fname) = filename
                                && !referenced_attachments.contains(&entry_normalized)
                            {
                                // Check if file matches any exclude pattern (inherited from ancestors)
                                let is_excluded =
                                    inherited_exclude_patterns.iter().any(|pattern| {
                                        self.path_matches_exclude(
                                            pattern,
                                            &workspace_root,
                                            &entry_path,
                                            fname,
                                        )
                                    });

                                if !is_excluded {
                                    result.warnings.push(ValidationWarning::OrphanBinaryFile {
                                        file: entry_path,
                                        // We can suggest connecting to the current index
                                        suggested_index: Some(path.clone()),
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // Report multiple indexes if found
                if !other_indexes.is_empty() {
                    let mut all_indexes = other_indexes;
                    all_indexes.push(path.clone());
                    all_indexes.sort();
                    result.warnings.push(ValidationWarning::MultipleIndexes {
                        directory: dir.to_path_buf(),
                        indexes: all_indexes,
                    });
                }

                // Look one level deep into each immediate subdirectory for
                // orphaned sub-indexes. A sub-index is any file with a
                // `contents` property. If the current index doesn't reference
                // it, we flag the sub-index itself as an OrphanFile (we don't
                // descend further — the sub-index owns its own children).
                //
                // Note: this deliberately doesn't warn about subdirs that have
                // no index file at all; those are caught by workspace-level
                // validation's orphan scan.
                for subdir in subdirs_to_check {
                    let Ok(sub_entries) = self.ws.fs_ref().list_files(&subdir).await else {
                        continue;
                    };

                    for sub_entry in sub_entries {
                        if self.ws.fs_ref().is_symlink(&sub_entry).await {
                            continue;
                        }
                        if self.ws.fs_ref().is_dir(&sub_entry).await {
                            continue;
                        }
                        let Some(fname) = sub_entry.file_name().and_then(|n| n.to_str()) else {
                            continue;
                        };
                        if fname.starts_with('.') || is_temp_file(fname) {
                            continue;
                        }
                        if !sub_entry
                            .extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                        {
                            continue;
                        }

                        // Only sub-indexes are considered here (files with a
                        // `contents` property). Non-index files belong to
                        // their own parent index, not this grandparent.
                        let Ok(sub_index) = self.ws.parse_index(&sub_entry).await else {
                            continue;
                        };
                        if !sub_index.frontmatter.is_index() {
                            continue;
                        }
                        // Skip attachment notes - they use `attachments`, not
                        // `contents`/`part_of`.
                        if sub_index.frontmatter.attachment.is_some() {
                            continue;
                        }

                        let sub_normalized = normalize_path(&sub_entry);
                        if listed_files.contains(&sub_normalized) {
                            continue;
                        }

                        let is_excluded = inherited_exclude_patterns.iter().any(|pattern| {
                            self.path_matches_exclude(pattern, &workspace_root, &sub_entry, fname)
                        });
                        if is_excluded {
                            continue;
                        }

                        result.warnings.push(ValidationWarning::OrphanFile {
                            file: sub_entry,
                            suggested_index: Some(path.clone()),
                        });
                    }
                }
            }
        }

        Ok(result)
    }
}
