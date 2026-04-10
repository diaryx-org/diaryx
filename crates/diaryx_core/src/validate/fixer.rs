//! Auto-fix routines for validation warnings and errors.
//!
//! [`ValidationFixer`] owns all the "apply a fix for this warning" logic —
//! the dispatch hub is [`ValidationFixer::fix_warning`], which exhaustively
//! matches every [`super::types::ValidationWarning`] variant and delegates to
//! a per-variant helper. Callers that want to avoid knowing about variants at
//! all should go through `fix_warning` / `fix_error` / `fix_all`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::fs::AsyncFileSystem;
use crate::link_parser::{self, LinkFormat};
use crate::path_utils::{normalize_sync_path, strip_workspace_root_prefix};
use crate::utils::path::relative_path_from_file_to_target;
use crate::workspace::Workspace;

use super::check::canonicalize_link_value;
use super::types::{ValidationError, ValidationResult, ValidationWarning};

// ============================================================================
// ValidationFixer - Fix validation issues
// ============================================================================

/// Result of attempting to fix a validation issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct FixResult {
    /// Whether the fix was successful.
    pub success: bool,
    /// Description of what was done (or why it failed).
    pub message: String,
}

impl FixResult {
    /// Create a successful fix result.
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
        }
    }

    /// Create a failed fix result.
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
        }
    }
}

/// Fixer for validation issues (async-first).
///
/// This struct provides methods to automatically fix validation errors and warnings.
pub struct ValidationFixer<FS: AsyncFileSystem> {
    fs: FS,
    link_format: LinkFormat,
    root_path: Option<PathBuf>,
}

impl<FS: AsyncFileSystem> ValidationFixer<FS> {
    /// Create a new fixer.
    pub fn new(fs: FS) -> Self {
        Self {
            fs,
            link_format: LinkFormat::default(),
            root_path: None,
        }
    }

    /// Create a new fixer with workspace link format support.
    pub fn with_link_format(fs: FS, root_path: PathBuf, link_format: LinkFormat) -> Self {
        Self {
            fs,
            link_format,
            root_path: Some(root_path),
        }
    }

    // ==================== Internal Frontmatter Helpers ====================

    /// Get a frontmatter property from a file. Returns `None` if the file
    /// doesn't exist, has no frontmatter, or the key is missing.
    async fn get_frontmatter_property(
        &self,
        path: &Path,
        key: &str,
    ) -> Option<crate::yaml_value::YamlValue> {
        let content = self.fs.read_to_string(path).await.ok()?;
        let parsed = crate::frontmatter::parse_or_empty(&content).ok()?;
        crate::frontmatter::get_property(&parsed.frontmatter, key).cloned()
    }

    /// Set a frontmatter property in a file, creating the file (and
    /// frontmatter block) if necessary.
    async fn set_frontmatter_property(
        &self,
        path: &Path,
        key: &str,
        value: crate::yaml_value::YamlValue,
    ) -> Result<()> {
        let (mut frontmatter, body) = match self.fs.read_to_string(path).await {
            Ok(content) => {
                let parsed = crate::frontmatter::parse_or_empty(&content)?;
                (parsed.frontmatter, parsed.body)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                (indexmap::IndexMap::new(), String::new())
            }
            Err(e) => {
                return Err(crate::error::DiaryxError::FileRead {
                    path: path.to_path_buf(),
                    source: e,
                });
            }
        };

        crate::frontmatter::set_property(&mut frontmatter, key, value);
        let new_content = crate::frontmatter::serialize(&frontmatter, &body)?;

        self.fs.write_file(path, &new_content).await.map_err(|e| {
            crate::error::DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            }
        })
    }

    /// Remove a frontmatter property from a file. A missing file or missing
    /// property is treated as a no-op.
    async fn remove_frontmatter_property(&self, path: &Path, key: &str) -> Result<()> {
        let Ok(content) = self.fs.read_to_string(path).await else {
            return Ok(()); // File doesn't exist — nothing to remove.
        };

        let mut parsed = crate::frontmatter::parse_or_empty(&content)?;
        if parsed.frontmatter.is_empty() {
            return Ok(()); // No frontmatter or malformed block.
        }
        crate::frontmatter::remove_property(&mut parsed.frontmatter, key);

        let new_content = crate::frontmatter::serialize(&parsed.frontmatter, &parsed.body)?;
        self.fs.write_file(path, &new_content).await.map_err(|e| {
            crate::error::DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            }
        })
    }

    // ==================== Link Format Helpers ====================

    /// Get the canonical (workspace-relative) path for a filesystem path.
    pub(super) fn get_canonical(&self, path: &Path) -> String {
        let raw = if let Some(ref root) = self.root_path {
            let path_string = path.to_string_lossy();
            strip_workspace_root_prefix(&path_string, root)
                .unwrap_or_else(|| path_string.to_string())
        } else {
            path.to_string_lossy().to_string()
        };
        normalize_sync_path(&raw)
    }

    /// Read title from a file's frontmatter, falling back to filename stem.
    async fn resolve_title(&self, path: &Path) -> String {
        if let Ok(content) = self.fs.read_to_string(path).await
            && let Ok(parsed) = crate::frontmatter::parse_or_empty(&content)
            && let Some(title) = crate::frontmatter::get_string(&parsed.frontmatter, "title")
        {
            return title.to_string();
        }
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string()
    }

    /// Format a link from one file to another using the configured link format.
    /// Falls back to a plain relative path when no root_path is configured.
    async fn format_link(&self, target: &Path, from: &Path) -> String {
        if self.root_path.is_some() {
            let target_canonical = self.get_canonical(target);
            let from_canonical = self.get_canonical(from);
            let title = self.resolve_title(target).await;
            link_parser::format_link_with_format(
                &target_canonical,
                &title,
                self.link_format,
                &from_canonical,
            )
        } else {
            relative_path_from_file_to_target(from, target)
        }
    }

    async fn format_self_link(&self, file: &Path) -> String {
        let canonical = self.get_canonical(file);
        let title = self.resolve_title(file).await;
        link_parser::format_link_with_format(&canonical, &title, self.link_format, &canonical)
    }

    // ==================== Fix Methods ====================

    /// Fix a broken `part_of` reference by removing it.
    pub async fn fix_broken_part_of(&self, file: &Path) -> FixResult {
        match self.remove_frontmatter_property(file, "part_of").await {
            Ok(_) => FixResult::success(format!("Removed broken part_of from {}", file.display())),
            Err(e) => FixResult::failure(format!(
                "Failed to remove part_of from {}: {}",
                file.display(),
                e
            )),
        }
    }

    /// Fix a broken `contents` reference by removing it from the index.
    pub async fn fix_broken_contents_ref(&self, index: &Path, target: &str) -> FixResult {
        match self.get_frontmatter_property(index, "contents").await {
            Some(crate::yaml_value::YamlValue::Sequence(items)) => {
                let filtered: Vec<crate::yaml_value::YamlValue> = items
                    .into_iter()
                    .filter(|item| {
                        if let crate::yaml_value::YamlValue::String(s) = item {
                            s != target
                        } else {
                            true
                        }
                    })
                    .collect();

                match self
                    .set_frontmatter_property(
                        index,
                        "contents",
                        crate::yaml_value::YamlValue::Sequence(filtered),
                    )
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Removed broken contents ref '{}' from {}",
                        target,
                        index.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to update contents in {}: {}",
                        index.display(),
                        e
                    )),
                }
            }
            _ => FixResult::failure(format!("Could not read contents from {}", index.display())),
        }
    }

    /// Fix a broken `attachments` reference by removing it.
    pub async fn fix_broken_attachment(&self, file: &Path, attachment: &str) -> FixResult {
        match self.get_frontmatter_property(file, "attachments").await {
            Some(crate::yaml_value::YamlValue::Sequence(items)) => {
                let filtered: Vec<crate::yaml_value::YamlValue> = items
                    .into_iter()
                    .filter(|item| {
                        if let crate::yaml_value::YamlValue::String(s) = item {
                            s != attachment
                        } else {
                            true
                        }
                    })
                    .collect();

                let result = if filtered.is_empty() {
                    self.remove_frontmatter_property(file, "attachments").await
                } else {
                    self.set_frontmatter_property(
                        file,
                        "attachments",
                        crate::yaml_value::YamlValue::Sequence(filtered),
                    )
                    .await
                };

                match result {
                    Ok(_) => FixResult::success(format!(
                        "Removed broken attachment '{}' from {}",
                        attachment,
                        file.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to update attachments in {}: {}",
                        file.display(),
                        e
                    )),
                }
            }
            _ => FixResult::failure(format!(
                "Could not read attachments from {}",
                file.display()
            )),
        }
    }

    /// Fix a non-portable path by replacing it with the normalized version.
    pub async fn fix_non_portable_path(
        &self,
        file: &Path,
        property: &str,
        old_value: &str,
        new_value: &str,
    ) -> FixResult {
        match property {
            "part_of" => {
                match self
                    .set_frontmatter_property(
                        file,
                        "part_of",
                        crate::yaml_value::YamlValue::String(new_value.to_string()),
                    )
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Normalized {} '{}' -> '{}' in {}",
                        property,
                        old_value,
                        new_value,
                        file.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to update {} in {}: {}",
                        property,
                        file.display(),
                        e
                    )),
                }
            }
            "contents" | "attachments" | "links" | "link_of" => {
                match self.get_frontmatter_property(file, property).await {
                    Some(crate::yaml_value::YamlValue::Sequence(items)) => {
                        let updated: Vec<crate::yaml_value::YamlValue> = items
                            .into_iter()
                            .map(|item| {
                                if let crate::yaml_value::YamlValue::String(ref s) = item
                                    && s == old_value
                                {
                                    return crate::yaml_value::YamlValue::String(
                                        new_value.to_string(),
                                    );
                                }
                                item
                            })
                            .collect();

                        match self
                            .set_frontmatter_property(
                                file,
                                property,
                                crate::yaml_value::YamlValue::Sequence(updated),
                            )
                            .await
                        {
                            Ok(_) => FixResult::success(format!(
                                "Normalized {} '{}' -> '{}' in {}",
                                property,
                                old_value,
                                new_value,
                                file.display()
                            )),
                            Err(e) => FixResult::failure(format!(
                                "Failed to update {} in {}: {}",
                                property,
                                file.display(),
                                e
                            )),
                        }
                    }
                    _ => FixResult::failure(format!(
                        "Could not read {} from {}",
                        property,
                        file.display()
                    )),
                }
            }
            _ => FixResult::failure(format!("Unknown property: {}", property)),
        }
    }

    /// Rename a file with a non-portable filename to a sanitized version.
    pub async fn fix_non_portable_filename(
        &self,
        file: &Path,
        suggested_filename: &str,
    ) -> FixResult {
        let ws = Workspace::new(&self.fs);
        match ws.rename_entry(file, suggested_filename).await {
            Ok(new_path) => FixResult::success(format!(
                "Renamed '{}' -> '{}'",
                file.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                new_path.display()
            )),
            Err(e) => FixResult::failure(format!("Failed to rename {}: {}", file.display(), e)),
        }
    }

    /// Add an unlisted file to an index's contents.
    pub async fn fix_unlisted_file(&self, index: &Path, file: &Path) -> FixResult {
        let formatted = self.format_link(file, index).await;

        match self.get_frontmatter_property(index, "contents").await {
            Some(crate::yaml_value::YamlValue::Sequence(mut items)) => {
                items.push(crate::yaml_value::YamlValue::String(formatted.clone()));
                match self
                    .set_frontmatter_property(
                        index,
                        "contents",
                        crate::yaml_value::YamlValue::Sequence(items),
                    )
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Added '{}' to contents in {}",
                        formatted,
                        index.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to update contents in {}: {}",
                        index.display(),
                        e
                    )),
                }
            }
            None => {
                // No contents yet, create it
                match self
                    .set_frontmatter_property(
                        index,
                        "contents",
                        crate::yaml_value::YamlValue::Sequence(vec![
                            crate::yaml_value::YamlValue::String(formatted.clone()),
                        ]),
                    )
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Added '{}' to new contents in {}",
                        formatted,
                        index.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to create contents in {}: {}",
                        index.display(),
                        e
                    )),
                }
            }
            _ => FixResult::failure(format!("Could not read contents from {}", index.display())),
        }
    }

    /// Wrap an orphan binary file in a markdown attachment note and add that
    /// note to an index's `attachments` list.
    ///
    /// Under the current attachment model, `attachments` must contain markdown
    /// "attachment notes" — a note has an `attachment:` property pointing at
    /// the binary asset. This fix creates such a note next to the binary (if
    /// one doesn't already exist) and links the note — not the binary — into
    /// the index.
    pub async fn fix_orphan_binary_file(&self, index: &Path, file: &Path) -> FixResult {
        // The attachment note lives next to the binary, with `.md` appended so
        // it keeps the extension visible (e.g. `photo.jpg.md`).
        let Some(binary_filename) = file.file_name().and_then(|n| n.to_str()) else {
            return FixResult::failure(format!(
                "Cannot derive filename for binary {}",
                file.display()
            ));
        };
        let Some(parent_dir) = file.parent() else {
            return FixResult::failure(format!(
                "Binary {} has no parent directory",
                file.display()
            ));
        };
        let note_path = parent_dir.join(format!("{binary_filename}.md"));

        // Create the attachment note if it doesn't already exist.
        if !self.fs.exists(&note_path).await {
            let self_link = self.format_self_link(&note_path).await;
            let attachment_link = self.format_attachment_link(file, &note_path).await;
            let content = format!(
                "---\ntitle: {title}\nlink: \"{self_link}\"\nattachment: \"{attachment_link}\"\n---\n",
                title = binary_filename,
            );
            if let Err(e) = self.fs.write_file(&note_path, &content).await {
                return FixResult::failure(format!(
                    "Failed to create attachment note {}: {}",
                    note_path.display(),
                    e
                ));
            }
        }

        // Reference the NOTE (not the binary) from the index's attachments.
        let note_link = self.format_link(&note_path, index).await;

        match self.get_frontmatter_property(index, "attachments").await {
            Some(crate::yaml_value::YamlValue::Sequence(mut items)) => {
                if !items.iter().any(
                    |v| matches!(v, crate::yaml_value::YamlValue::String(s) if s == &note_link),
                ) {
                    items.push(crate::yaml_value::YamlValue::String(note_link.clone()));
                }
                match self
                    .set_frontmatter_property(
                        index,
                        "attachments",
                        crate::yaml_value::YamlValue::Sequence(items),
                    )
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Wrapped '{}' in attachment note and added to {}",
                        file.display(),
                        index.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to update attachments in {}: {}",
                        index.display(),
                        e
                    )),
                }
            }
            None => match self
                .set_frontmatter_property(
                    index,
                    "attachments",
                    crate::yaml_value::YamlValue::Sequence(vec![
                        crate::yaml_value::YamlValue::String(note_link.clone()),
                    ]),
                )
                .await
            {
                Ok(_) => FixResult::success(format!(
                    "Wrapped '{}' in attachment note and added to new attachments in {}",
                    file.display(),
                    index.display()
                )),
                Err(e) => FixResult::failure(format!(
                    "Failed to create attachments in {}: {}",
                    index.display(),
                    e
                )),
            },
            _ => FixResult::failure(format!(
                "Could not read attachments from {}",
                index.display()
            )),
        }
    }

    /// Format a frontmatter `attachment:` link from a note to a binary asset.
    /// Preserves the filename (with extension) as the link title so prettification
    /// doesn't drop extensions like `.png`.
    async fn format_attachment_link(&self, binary: &Path, from_note: &Path) -> String {
        if self.root_path.is_some() {
            let target_canonical = self.get_canonical(binary);
            let from_canonical = self.get_canonical(from_note);
            let title = binary
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&target_canonical)
                .to_string();
            link_parser::format_link_with_format(
                &target_canonical,
                &title,
                self.link_format,
                &from_canonical,
            )
        } else {
            relative_path_from_file_to_target(from_note, binary)
        }
    }

    /// Fix a missing `part_of` by setting it to point to the given index.
    pub async fn fix_missing_part_of(&self, file: &Path, index: &Path) -> FixResult {
        let formatted = self.format_link(index, file).await;

        match self
            .set_frontmatter_property(
                file,
                "part_of",
                crate::yaml_value::YamlValue::String(formatted.clone()),
            )
            .await
        {
            Ok(_) => FixResult::success(format!(
                "Set part_of to '{}' in {}",
                formatted,
                file.display()
            )),
            Err(e) => FixResult::failure(format!(
                "Failed to set part_of in {}: {}",
                file.display(),
                e
            )),
        }
    }

    /// Fix a broken `links` reference by removing it.
    pub async fn fix_broken_link_ref(&self, file: &Path, target: &str) -> FixResult {
        match self.get_frontmatter_property(file, "links").await {
            Some(crate::yaml_value::YamlValue::Sequence(items)) => {
                let filtered: Vec<crate::yaml_value::YamlValue> = items
                    .into_iter()
                    .filter(|item| match item {
                        crate::yaml_value::YamlValue::String(s) => s != target,
                        _ => true,
                    })
                    .collect();

                if filtered.is_empty() {
                    match self.remove_frontmatter_property(file, "links").await {
                        Ok(_) => FixResult::success(format!(
                            "Removed broken link '{}' from {}",
                            target,
                            file.display()
                        )),
                        Err(e) => FixResult::failure(format!(
                            "Failed to update links in {}: {}",
                            file.display(),
                            e
                        )),
                    }
                } else {
                    match self
                        .set_frontmatter_property(
                            file,
                            "links",
                            crate::yaml_value::YamlValue::Sequence(filtered),
                        )
                        .await
                    {
                        Ok(_) => FixResult::success(format!(
                            "Removed broken link '{}' from {}",
                            target,
                            file.display()
                        )),
                        Err(e) => FixResult::failure(format!(
                            "Failed to update links in {}: {}",
                            file.display(),
                            e
                        )),
                    }
                }
            }
            _ => FixResult::failure(format!("Could not read links from {}", file.display())),
        }
    }

    /// Fix an invalid `link` by rewriting it to the canonical self-link.
    pub async fn fix_invalid_self_link(&self, file: &Path) -> FixResult {
        let formatted = self.format_self_link(file).await;

        match self
            .set_frontmatter_property(
                file,
                "link",
                crate::yaml_value::YamlValue::String(formatted.clone()),
            )
            .await
        {
            Ok(_) => {
                FixResult::success(format!("Set link to '{}' in {}", formatted, file.display()))
            }
            Err(e) => {
                FixResult::failure(format!("Failed to set link in {}: {}", file.display(), e))
            }
        }
    }

    /// Fix a missing backlink by appending the suggested source link to `link_of`.
    pub async fn fix_missing_backlink(&self, file: &Path, suggested: &str) -> FixResult {
        match self.get_frontmatter_property(file, "link_of").await {
            Some(crate::yaml_value::YamlValue::Sequence(mut items)) => {
                items.push(crate::yaml_value::YamlValue::String(suggested.to_string()));
                match self
                    .set_frontmatter_property(
                        file,
                        "link_of",
                        crate::yaml_value::YamlValue::Sequence(items),
                    )
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Added backlink '{}' to {}",
                        suggested,
                        file.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to update link_of in {}: {}",
                        file.display(),
                        e
                    )),
                }
            }
            None => match self
                .set_frontmatter_property(
                    file,
                    "link_of",
                    crate::yaml_value::YamlValue::Sequence(vec![
                        crate::yaml_value::YamlValue::String(suggested.to_string()),
                    ]),
                )
                .await
            {
                Ok(_) => FixResult::success(format!(
                    "Added backlink '{}' to {}",
                    suggested,
                    file.display()
                )),
                Err(e) => FixResult::failure(format!(
                    "Failed to create link_of in {}: {}",
                    file.display(),
                    e
                )),
            },
            _ => FixResult::failure(format!("Could not read link_of from {}", file.display())),
        }
    }

    /// Fix a stale backlink by removing it from `link_of`.
    pub async fn fix_stale_backlink(&self, file: &Path, value: &str) -> FixResult {
        match self.get_frontmatter_property(file, "link_of").await {
            Some(crate::yaml_value::YamlValue::Sequence(items)) => {
                let filtered: Vec<crate::yaml_value::YamlValue> = items
                    .into_iter()
                    .filter(|item| match item {
                        crate::yaml_value::YamlValue::String(s) => s != value,
                        _ => true,
                    })
                    .collect();

                if filtered.is_empty() {
                    match self.remove_frontmatter_property(file, "link_of").await {
                        Ok(_) => FixResult::success(format!(
                            "Removed stale backlink '{}' from {}",
                            value,
                            file.display()
                        )),
                        Err(e) => FixResult::failure(format!(
                            "Failed to update link_of in {}: {}",
                            file.display(),
                            e
                        )),
                    }
                } else {
                    match self
                        .set_frontmatter_property(
                            file,
                            "link_of",
                            crate::yaml_value::YamlValue::Sequence(filtered),
                        )
                        .await
                    {
                        Ok(_) => FixResult::success(format!(
                            "Removed stale backlink '{}' from {}",
                            value,
                            file.display()
                        )),
                        Err(e) => FixResult::failure(format!(
                            "Failed to update link_of in {}: {}",
                            file.display(),
                            e
                        )),
                    }
                }
            }
            _ => FixResult::failure(format!("Could not read link_of from {}", file.display())),
        }
    }

    /// Fix a missing attachment backlink by appending the suggested source
    /// link to the attachment note's `attachment_of`.
    pub async fn fix_missing_attachment_backlink(&self, file: &Path, suggested: &str) -> FixResult {
        match self.get_frontmatter_property(file, "attachment_of").await {
            Some(crate::yaml_value::YamlValue::Sequence(mut items)) => {
                items.push(crate::yaml_value::YamlValue::String(suggested.to_string()));
                match self
                    .set_frontmatter_property(
                        file,
                        "attachment_of",
                        crate::yaml_value::YamlValue::Sequence(items),
                    )
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Added attachment backlink '{}' to {}",
                        suggested,
                        file.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to update attachment_of in {}: {}",
                        file.display(),
                        e
                    )),
                }
            }
            None => match self
                .set_frontmatter_property(
                    file,
                    "attachment_of",
                    crate::yaml_value::YamlValue::Sequence(vec![
                        crate::yaml_value::YamlValue::String(suggested.to_string()),
                    ]),
                )
                .await
            {
                Ok(_) => FixResult::success(format!(
                    "Added attachment backlink '{}' to {}",
                    suggested,
                    file.display()
                )),
                Err(e) => FixResult::failure(format!(
                    "Failed to create attachment_of in {}: {}",
                    file.display(),
                    e
                )),
            },
            _ => FixResult::failure(format!(
                "Could not read attachment_of from {}",
                file.display()
            )),
        }
    }

    /// Fix a stale attachment backlink by removing it from `attachment_of`.
    pub async fn fix_stale_attachment_backlink(&self, file: &Path, value: &str) -> FixResult {
        match self.get_frontmatter_property(file, "attachment_of").await {
            Some(crate::yaml_value::YamlValue::Sequence(items)) => {
                let filtered: Vec<crate::yaml_value::YamlValue> = items
                    .into_iter()
                    .filter(|item| match item {
                        crate::yaml_value::YamlValue::String(s) => s != value,
                        _ => true,
                    })
                    .collect();

                if filtered.is_empty() {
                    match self
                        .remove_frontmatter_property(file, "attachment_of")
                        .await
                    {
                        Ok(_) => FixResult::success(format!(
                            "Removed stale attachment backlink '{}' from {}",
                            value,
                            file.display()
                        )),
                        Err(e) => FixResult::failure(format!(
                            "Failed to update attachment_of in {}: {}",
                            file.display(),
                            e
                        )),
                    }
                } else {
                    match self
                        .set_frontmatter_property(
                            file,
                            "attachment_of",
                            crate::yaml_value::YamlValue::Sequence(filtered),
                        )
                        .await
                    {
                        Ok(_) => FixResult::success(format!(
                            "Removed stale attachment backlink '{}' from {}",
                            value,
                            file.display()
                        )),
                        Err(e) => FixResult::failure(format!(
                            "Failed to update attachment_of in {}: {}",
                            file.display(),
                            e
                        )),
                    }
                }
            }
            _ => FixResult::failure(format!(
                "Could not read attachment_of from {}",
                file.display()
            )),
        }
    }

    /// Dedupe a frontmatter list property, preserving the first occurrence of
    /// each canonical value.
    ///
    /// Duplicates are detected by canonical-link equivalence: if two entries
    /// resolve to the same canonical path under the fixer's link format, only
    /// the first is kept. Non-string entries pass through untouched.
    pub async fn fix_duplicate_list_entry(&self, file: &Path, property: &str) -> FixResult {
        let items = match self.get_frontmatter_property(file, property).await {
            Some(crate::yaml_value::YamlValue::Sequence(items)) => items,
            _ => {
                return FixResult::failure(format!(
                    "Could not read {} list from {}",
                    property,
                    file.display()
                ));
            }
        };

        let file_canonical = self.get_canonical(file);
        let link_format = Some(self.link_format);

        let mut seen_canonical: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let original_len = items.len();
        let mut deduped: Vec<crate::yaml_value::YamlValue> = Vec::with_capacity(original_len);

        for item in items {
            match &item {
                crate::yaml_value::YamlValue::String(raw) => {
                    let canonical = canonicalize_link_value(raw, &file_canonical, link_format);
                    if seen_canonical.insert(canonical) {
                        deduped.push(item);
                    }
                }
                _ => deduped.push(item),
            }
        }

        let removed = original_len - deduped.len();
        if removed == 0 {
            return FixResult::success(format!(
                "No duplicates to remove from {} in {}",
                property,
                file.display()
            ));
        }

        match self
            .set_frontmatter_property(
                file,
                property,
                crate::yaml_value::YamlValue::Sequence(deduped),
            )
            .await
        {
            Ok(_) => FixResult::success(format!(
                "Removed {removed} duplicate {property} entr{plural} from {}",
                file.display(),
                plural = if removed == 1 { "y" } else { "ies" },
            )),
            Err(e) => FixResult::failure(format!(
                "Failed to update {} in {}: {}",
                property,
                file.display(),
                e
            )),
        }
    }

    /// Fix a circular reference by removing a contents reference from a file.
    ///
    /// This removes the specified reference from the file's `contents` array,
    /// breaking the cycle.
    pub async fn fix_circular_reference(
        &self,
        file: &Path,
        contents_ref_to_remove: &str,
    ) -> FixResult {
        match self.get_frontmatter_property(file, "contents").await {
            Some(crate::yaml_value::YamlValue::Sequence(items)) => {
                let filtered: Vec<crate::yaml_value::YamlValue> = items
                    .into_iter()
                    .filter(|item| {
                        if let crate::yaml_value::YamlValue::String(s) = item {
                            s != contents_ref_to_remove
                        } else {
                            true
                        }
                    })
                    .collect();

                match self
                    .set_frontmatter_property(
                        file,
                        "contents",
                        crate::yaml_value::YamlValue::Sequence(filtered),
                    )
                    .await
                {
                    Ok(_) => FixResult::success(format!(
                        "Removed circular reference '{}' from {}",
                        contents_ref_to_remove,
                        file.display()
                    )),
                    Err(e) => FixResult::failure(format!(
                        "Failed to remove circular reference from {}: {}",
                        file.display(),
                        e
                    )),
                }
            }
            _ => FixResult::failure(format!("Could not read contents from {}", file.display())),
        }
    }

    /// Fix a validation error.
    pub async fn fix_error(&self, error: &ValidationError) -> FixResult {
        match error {
            ValidationError::BrokenPartOf { file, target: _ } => {
                self.fix_broken_part_of(file).await
            }
            ValidationError::BrokenContentsRef { index, target } => {
                self.fix_broken_contents_ref(index, target).await
            }
            ValidationError::BrokenAttachment { file, attachment } => {
                self.fix_broken_attachment(file, attachment).await
            }
            ValidationError::BrokenLinkRef { file, target } => {
                self.fix_broken_link_ref(file, target).await
            }
        }
    }

    /// Fix a validation warning.
    ///
    /// Returns `None` if the warning type cannot be automatically fixed.
    pub async fn fix_warning(&self, warning: &ValidationWarning) -> Option<FixResult> {
        match warning {
            ValidationWarning::NonPortablePath {
                file,
                property,
                value,
                suggested,
            } => Some(
                self.fix_non_portable_path(file, property, value, suggested)
                    .await,
            ),
            ValidationWarning::OrphanBinaryFile {
                file,
                suggested_index,
            } => {
                if let Some(index) = suggested_index {
                    Some(self.fix_orphan_binary_file(index, file).await)
                } else {
                    None
                }
            }
            ValidationWarning::MissingPartOf {
                file,
                suggested_index,
            } => {
                if let Some(index) = suggested_index {
                    Some(self.fix_missing_part_of(file, index).await)
                } else {
                    None
                }
            }
            ValidationWarning::OrphanFile {
                file,
                suggested_index,
            } => {
                // Fix by adding the file to the nearest parent index's contents
                if let Some(index) = suggested_index {
                    Some(self.fix_unlisted_file(index, file).await)
                } else {
                    None
                }
            }
            ValidationWarning::UnlinkedEntry {
                path,
                is_dir,
                suggested_index,
                index_file,
            } => {
                if let Some(index) = suggested_index {
                    if *is_dir {
                        // For directories, we need to link the index file inside, not the directory itself
                        if let Some(dir_index) = index_file {
                            Some(self.fix_unlisted_file(index, dir_index).await)
                        } else {
                            // Directory has no index file - can't auto-fix
                            None
                        }
                    } else {
                        // For files, add directly to contents
                        Some(self.fix_unlisted_file(index, path).await)
                    }
                } else {
                    None
                }
            }
            ValidationWarning::CircularReference {
                suggested_file,
                suggested_remove_part_of,
                ..
            } => {
                // Can auto-fix if we have a suggestion
                if let (Some(file), Some(contents_ref)) = (suggested_file, suggested_remove_part_of)
                {
                    Some(self.fix_circular_reference(file, contents_ref).await)
                } else {
                    None
                }
            }
            ValidationWarning::NonPortableFilename {
                file,
                suggested_filename,
                ..
            } => Some(
                self.fix_non_portable_filename(file, suggested_filename)
                    .await,
            ),
            ValidationWarning::InvalidSelfLink { file, .. } => {
                Some(self.fix_invalid_self_link(file).await)
            }
            ValidationWarning::MissingBacklink {
                file, suggested, ..
            } => Some(self.fix_missing_backlink(file, suggested).await),
            ValidationWarning::StaleBacklink { file, value } => {
                Some(self.fix_stale_backlink(file, value).await)
            }
            ValidationWarning::MissingAttachmentBacklink {
                file, suggested, ..
            } => Some(self.fix_missing_attachment_backlink(file, suggested).await),
            ValidationWarning::StaleAttachmentBacklink { file, value } => {
                Some(self.fix_stale_attachment_backlink(file, value).await)
            }
            ValidationWarning::DuplicateListEntry { file, property, .. } => {
                Some(self.fix_duplicate_list_entry(file, property).await)
            }
            // These cannot be auto-fixed
            ValidationWarning::MultipleIndexes { .. } => None,
            ValidationWarning::InvalidContentsRef { .. } => None,
            ValidationWarning::InvalidAttachmentRef { .. } => None,
        }
    }

    /// Attempt to fix all errors in a validation result.
    ///
    /// Returns a list of fix results for each error.
    pub async fn fix_all_errors(&self, result: &ValidationResult) -> Vec<FixResult> {
        let mut fixes = Vec::new();
        for error in &result.errors {
            fixes.push(self.fix_error(error).await);
        }
        fixes
    }

    /// Attempt to fix all fixable warnings in a validation result.
    ///
    /// Returns a list of fix results for warnings that could be fixed.
    /// Warnings that cannot be auto-fixed are skipped.
    pub async fn fix_all_warnings(&self, result: &ValidationResult) -> Vec<FixResult> {
        let mut fixes = Vec::new();
        for warning in &result.warnings {
            if let Some(fix) = self.fix_warning(warning).await {
                fixes.push(fix);
            }
        }
        fixes
    }

    /// Attempt to fix all errors and fixable warnings in a validation result.
    ///
    /// Returns a tuple of (error fix results, warning fix results).
    pub async fn fix_all(&self, result: &ValidationResult) -> (Vec<FixResult>, Vec<FixResult>) {
        (
            self.fix_all_errors(result).await,
            self.fix_all_warnings(result).await,
        )
    }
}
