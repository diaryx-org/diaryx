//! Entry operations module.
//!
//! This module provides functionality for working with individual entries:
//! - Frontmatter manipulation (get, set, remove properties)
//! - Content operations (get, set, append, prepend)
//! - Attachment management
//!
//! # Migration Note
//!
//! `DiaryxAppSync` is deprecated. Use [`DiaryxApp`] with `AsyncFileSystem`
//! for all new code. The sync implementation will be removed in a future version.
//!
//! ```ignore
//! use diaryx_core::entry::DiaryxApp;
//! use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
//! use futures_lite::future::block_on;
//!
//! // Use async-first API even in sync contexts
//! let fs = SyncToAsyncFs::new(RealFileSystem);
//! let app = DiaryxApp::new(fs);
//!
//! block_on(app.get_content("note.md"))?;
//! ```

mod helpers;

// Re-export helper functions
pub use helpers::{
    apply_filename_style, extract_first_line_h1, has_non_portable_chars, prettify_filename,
    sanitize_filename, slugify, slugify_title, sync_h1_in_body,
};

use crate::date;
use crate::error::{DiaryxError, Result};
use crate::fs::{AsyncFileSystem, FileSystem};
use crate::link_parser;
use crate::yaml_value::YamlValue;
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

/// Compute a dedup key for an attachment reference.
///
/// Diaryx's current attachment model stores `attachments:` entries as
/// markdown links pointing at *attachment note* files — e.g. a reference
/// `path/to/file.pdf.md` wraps the binary at `path/to/file.pdf`. Historical
/// (Obsidian-style) workspaces and some migration paths stored entries
/// that pointed directly at the binary instead.
///
/// These two forms refer to the same logical attachment, so `add_attachment`
/// must treat them as equal when deduping. We normalise by stripping a
/// single trailing `.md` from the canonical path, collapsing note-form and
/// binary-form references to the same key.
fn attachment_dedup_key(raw_link: &str, from_path: &Path) -> String {
    let parsed = link_parser::parse_link(raw_link);
    let canonical = link_parser::to_canonical(&parsed, from_path);
    canonical
        .strip_suffix(".md")
        .map(String::from)
        .unwrap_or(canonical)
}

/// Async-first Diaryx entry operations.
///
/// This is the main entry API going forward.
pub struct DiaryxApp<FS: AsyncFileSystem> {
    fs: FS,
}

/// Legacy synchronous Diaryx entry operations.
///
/// This preserves the prior `FileSystem`-based implementation during the async refactor.
/// Prefer [`DiaryxApp`].
pub struct DiaryxAppSync<FS: FileSystem> {
    fs: FS,
}

impl<FS: AsyncFileSystem> DiaryxApp<FS> {
    /// DiaryxApp constructor
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    /// Access the underlying filesystem.
    pub fn fs(&self) -> &FS {
        &self.fs
    }

    /// Create a new entry.
    pub async fn create_entry(&self, path: &str) -> Result<()> {
        let content = format!("---\ntitle: {}\n---\n\n# {}\n\n", path, path);
        self.fs
            .create_new(std::path::Path::new(path), &content)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: PathBuf::from(path),
                source: e,
            })?;
        Ok(())
    }

    /// Parses a markdown file and extracts frontmatter and body.
    /// Returns an error if no frontmatter is found.
    async fn parse_file(&self, path: &str) -> Result<(IndexMap<String, YamlValue>, String)> {
        let path_buf = PathBuf::from(path);
        let content = self
            .fs
            .read_to_string(std::path::Path::new(path))
            .await
            .map_err(|e| DiaryxError::FileRead {
                path: path_buf.clone(),
                source: e,
            })?;

        // Check if content starts with frontmatter delimiter
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return Err(DiaryxError::NoFrontmatter(path_buf));
        }

        // Find the closing delimiter
        let rest = &content[4..]; // Skip first "---\n"
        let end_idx = rest
            .find("\n---\n")
            .or_else(|| rest.find("\n---\r\n"))
            .ok_or_else(|| DiaryxError::NoFrontmatter(path_buf.clone()))?;

        let frontmatter_str = &rest[..end_idx];
        let body = &rest[end_idx + 5..]; // Skip "\n---\n"

        // Parse YAML frontmatter into IndexMap to preserve order
        let frontmatter: IndexMap<String, YamlValue> = serde_yaml::from_str(frontmatter_str)?;

        Ok((frontmatter, body.to_string()))
    }

    /// Parses a markdown file, creating empty frontmatter if none exists.
    /// Creates the file if it doesn't exist.
    /// Use this for operations that should create frontmatter when missing (like set).
    async fn parse_file_or_create_frontmatter(
        &self,
        path: &str,
    ) -> Result<(IndexMap<String, YamlValue>, String)> {
        let path_buf = PathBuf::from(path);

        // Try to read the file, if it doesn't exist, return empty frontmatter and body
        let content = match self.fs.read_to_string(std::path::Path::new(path)).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File doesn't exist - return empty frontmatter and body
                // The file will be created when reconstruct_file is called
                return Ok((IndexMap::new(), String::new()));
            }
            Err(e) => {
                return Err(DiaryxError::FileRead {
                    path: path_buf,
                    source: e,
                });
            }
        };

        // Check if content starts with frontmatter delimiter
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            // No frontmatter - return empty frontmatter and entire content as body
            return Ok((IndexMap::new(), content));
        }

        // Find the closing delimiter
        let rest = &content[4..]; // Skip first "---\n"
        let end_idx = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"));

        match end_idx {
            Some(idx) => {
                let frontmatter_str = &rest[..idx];
                let body = &rest[idx + 5..]; // Skip "\n---\n"

                // Parse YAML frontmatter into IndexMap to preserve order
                let frontmatter: IndexMap<String, YamlValue> =
                    serde_yaml::from_str(frontmatter_str)?;

                Ok((frontmatter, body.to_string()))
            }
            None => {
                // Malformed frontmatter (no closing delimiter) - treat as no frontmatter
                Ok((IndexMap::new(), content))
            }
        }
    }

    /// Reconstructs a markdown file with updated frontmatter.
    async fn reconstruct_file(
        &self,
        path: &str,
        frontmatter: &IndexMap<String, YamlValue>,
        body: &str,
    ) -> Result<()> {
        let yaml_str = serde_yaml::to_string(frontmatter)?;
        let content = format!("---\n{}---\n{}", yaml_str, body);
        self.fs
            .write_file(std::path::Path::new(path), &content)
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: PathBuf::from(path),
                source: e,
            })?;
        Ok(())
    }

    // ==================== Frontmatter Methods ====================

    /// Adds or updates a frontmatter property.
    /// Creates frontmatter if none exists.
    pub async fn set_frontmatter_property(
        &self,
        path: &str,
        key: &str,
        value: YamlValue,
    ) -> Result<()> {
        let (mut frontmatter, body) = self.parse_file_or_create_frontmatter(path).await?;
        frontmatter.insert(key.to_string(), value);
        self.reconstruct_file(path, &frontmatter, &body).await
    }

    /// Removes a frontmatter property.
    /// Does nothing if no frontmatter exists or key is not found.
    pub async fn remove_frontmatter_property(&self, path: &str, key: &str) -> Result<()> {
        match self.parse_file(path).await {
            Ok((mut frontmatter, body)) => {
                frontmatter.shift_remove(key);
                self.reconstruct_file(path, &frontmatter, &body).await
            }
            Err(DiaryxError::NoFrontmatter(_)) => Ok(()), // No frontmatter, nothing to remove
            Err(e) => Err(e),
        }
    }

    /// Renames a frontmatter property key.
    /// Returns Ok(true) if the key was found and renamed, Ok(false) if key was not found or no frontmatter.
    pub async fn rename_frontmatter_property(
        &self,
        path: &str,
        old_key: &str,
        new_key: &str,
    ) -> Result<bool> {
        let (frontmatter, body) = match self.parse_file(path).await {
            Ok(result) => result,
            Err(DiaryxError::NoFrontmatter(_)) => return Ok(false), // No frontmatter, key not found
            Err(e) => return Err(e),
        };

        if !frontmatter.contains_key(old_key) {
            return Ok(false);
        }

        // Rebuild the map, replacing old_key with new_key at the same position
        let mut result: IndexMap<String, YamlValue> = IndexMap::new();
        for (k, v) in frontmatter {
            if k == old_key {
                result.insert(new_key.to_string(), v);
            } else {
                result.insert(k, v);
            }
        }

        self.reconstruct_file(path, &result, &body).await?;
        Ok(true)
    }

    /// Gets a frontmatter property value.
    /// Returns Ok(None) if no frontmatter exists or key is not found.
    pub async fn get_frontmatter_property(
        &self,
        path: &str,
        key: &str,
    ) -> Result<Option<YamlValue>> {
        match self.parse_file(path).await {
            Ok((frontmatter, _)) => Ok(frontmatter.get(key).cloned()),
            Err(DiaryxError::NoFrontmatter(_)) => Ok(None), // No frontmatter, key not found
            Err(e) => Err(e),
        }
    }

    /// Gets all frontmatter properties.
    /// Returns empty map if no frontmatter exists.
    pub async fn get_all_frontmatter(&self, path: &str) -> Result<IndexMap<String, YamlValue>> {
        match self.parse_file(path).await {
            Ok((frontmatter, _)) => Ok(frontmatter),
            Err(DiaryxError::NoFrontmatter(_)) => Ok(IndexMap::new()), // No frontmatter, return empty
            Err(e) => Err(e),
        }
    }

    // ==================== Content Methods ====================

    /// Get the content (body) of a file, excluding frontmatter.
    pub async fn get_content(&self, path: &str) -> Result<String> {
        let (_, body) = self.parse_file_or_create_frontmatter(path).await?;
        Ok(body)
    }

    /// Set the content (body) of a file, preserving frontmatter.
    /// Creates frontmatter if none exists.
    pub async fn set_content(&self, path: &str, content: &str) -> Result<()> {
        let (frontmatter, _) = self.parse_file_or_create_frontmatter(path).await?;
        self.reconstruct_file(path, &frontmatter, content).await
    }

    /// Clear the content (body) of a file, preserving frontmatter.
    pub async fn clear_content(&self, path: &str) -> Result<()> {
        self.set_content(path, "").await
    }

    /// Update the 'updated' frontmatter property with the current timestamp (RFC 3339 format).
    /// Creates frontmatter if none exists.
    pub async fn touch_updated(&self, path: &str) -> Result<()> {
        let timestamp = date::current_local_timestamp_rfc3339();
        self.set_frontmatter_property(path, "updated", YamlValue::String(timestamp))
            .await
    }

    /// Save content and optionally update the 'updated' timestamp.
    /// When `auto_update_timestamp` is true (the default), this combines set_content and touch_updated.
    pub async fn save_content(&self, path: &str, content: &str) -> Result<()> {
        self.save_content_with_options(path, content, true).await
    }

    /// Save content with explicit control over timestamp updating.
    pub async fn save_content_with_options(
        &self,
        path: &str,
        content: &str,
        auto_update_timestamp: bool,
    ) -> Result<()> {
        self.set_content(path, content).await?;
        if auto_update_timestamp {
            self.touch_updated(path).await?;
        }
        Ok(())
    }

    /// Append content to the end of a file's body.
    pub async fn append_content(&self, path: &str, content: &str) -> Result<()> {
        let (frontmatter, body) = self.parse_file_or_create_frontmatter(path).await?;
        let new_body = if body.is_empty() {
            content.to_string()
        } else if body.ends_with('\n') {
            format!("{}{}", body, content)
        } else {
            format!("{}\n{}", body, content)
        };
        self.reconstruct_file(path, &frontmatter, &new_body).await
    }

    /// Prepend content to the beginning of a file's body.
    pub async fn prepend_content(&self, path: &str, content: &str) -> Result<()> {
        let (frontmatter, body) = self.parse_file_or_create_frontmatter(path).await?;
        let new_body = if body.is_empty() {
            content.to_string()
        } else if content.ends_with('\n') {
            format!("{}{}", content, body)
        } else {
            format!("{}\n{}", content, body)
        };
        self.reconstruct_file(path, &frontmatter, &new_body).await
    }

    // ==================== Attachment Methods ====================

    /// Add an attachment path to the entry's attachments list.
    /// Creates the attachments property if it doesn't exist.
    pub async fn add_attachment(&self, path: &str, attachment_path: &str) -> Result<()> {
        let (mut frontmatter, body) = self.parse_file_or_create_frontmatter(path).await?;
        let from_path = Path::new(path);
        let target_key = attachment_dedup_key(attachment_path, from_path);

        let attachments = frontmatter
            .entry("attachments".to_string())
            .or_insert(YamlValue::Sequence(vec![]));

        if let YamlValue::Sequence(list) = attachments {
            let exists = list.iter().any(|item| {
                if let YamlValue::String(existing) = item {
                    attachment_dedup_key(existing, from_path) == target_key
                } else {
                    false
                }
            });

            if !exists {
                list.push(YamlValue::String(attachment_path.to_string()));
            }
        }

        self.reconstruct_file(path, &frontmatter, &body).await
    }

    /// Remove an attachment path from the entry's attachments list.
    /// Does nothing if the attachment isn't found.
    pub async fn remove_attachment(&self, path: &str, attachment_path: &str) -> Result<()> {
        let (mut frontmatter, body) = match self.parse_file(path).await {
            Ok(result) => result,
            Err(DiaryxError::NoFrontmatter(_)) => return Ok(()),
            Err(e) => return Err(e),
        };
        let from_path = Path::new(path);
        let target_key = attachment_dedup_key(attachment_path, from_path);

        if let Some(YamlValue::Sequence(list)) = frontmatter.get_mut("attachments") {
            list.retain(|item| {
                if let YamlValue::String(s) = item {
                    attachment_dedup_key(s, from_path) != target_key
                } else {
                    true
                }
            });

            // Remove empty attachments array
            if list.is_empty() {
                frontmatter.shift_remove("attachments");
            }
        }

        self.reconstruct_file(path, &frontmatter, &body).await
    }

    /// Get the list of attachments directly declared in this entry.
    pub async fn get_attachments(&self, path: &str) -> Result<Vec<String>> {
        let (frontmatter, _) = match self.parse_file(path).await {
            Ok(result) => result,
            Err(DiaryxError::NoFrontmatter(_)) => return Ok(vec![]),
            Err(e) => return Err(e),
        };

        match frontmatter.get("attachments") {
            Some(YamlValue::Sequence(list)) => Ok(list
                .iter()
                .filter_map(|v| {
                    if let YamlValue::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .collect()),
            _ => Ok(vec![]),
        }
    }
}

impl<FS: FileSystem> DiaryxAppSync<FS> {
    /// DiaryxAppSync constructor
    pub fn new(fs: FS) -> Self {
        Self { fs }
    }

    /// Access the underlying filesystem.
    pub fn fs(&self) -> &FS {
        &self.fs
    }

    /// Create a new entry
    pub fn create_entry(&self, path: &str) -> Result<()> {
        let content = format!("---\ntitle: {}\n---\n\n# {}\n\n", path, path);
        self.fs.create_new(std::path::Path::new(path), &content)?;
        Ok(())
    }

    /// Parses a markdown file and extracts frontmatter and body
    /// Returns an error if no frontmatter is found
    fn parse_file(&self, path: &str) -> Result<(IndexMap<String, YamlValue>, String)> {
        let path_buf = PathBuf::from(path);
        let content = self
            .fs
            .read_to_string(std::path::Path::new(path))
            .map_err(|e| DiaryxError::FileRead {
                path: path_buf.clone(),
                source: e,
            })?;

        // Check if content starts with frontmatter delimiter
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return Err(DiaryxError::NoFrontmatter(path_buf));
        }

        // Find the closing delimiter
        let rest = &content[4..]; // Skip first "---\n"
        let end_idx = rest
            .find("\n---\n")
            .or_else(|| rest.find("\n---\r\n"))
            .ok_or_else(|| DiaryxError::NoFrontmatter(path_buf.clone()))?;

        let frontmatter_str = &rest[..end_idx];
        let body = &rest[end_idx + 5..]; // Skip "\n---\n"

        // Parse YAML frontmatter into IndexMap to preserve order
        let frontmatter: IndexMap<String, YamlValue> = serde_yaml::from_str(frontmatter_str)?;

        Ok((frontmatter, body.to_string()))
    }

    /// Parses a markdown file, creating empty frontmatter if none exists.
    /// Creates the file if it doesn't exist.
    /// Use this for operations that should create frontmatter when missing (like set).
    fn parse_file_or_create_frontmatter(
        &self,
        path: &str,
    ) -> Result<(IndexMap<String, YamlValue>, String)> {
        let path_buf = PathBuf::from(path);

        // Try to read the file, if it doesn't exist, return empty frontmatter and body
        let content = match self.fs.read_to_string(std::path::Path::new(path)) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File doesn't exist - return empty frontmatter and body
                // The file will be created when reconstruct_file is called
                return Ok((IndexMap::new(), String::new()));
            }
            Err(e) => {
                return Err(DiaryxError::FileRead {
                    path: path_buf,
                    source: e,
                });
            }
        };

        // Check if content starts with frontmatter delimiter
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            // No frontmatter - return empty frontmatter and entire content as body
            return Ok((IndexMap::new(), content));
        }

        // Find the closing delimiter
        let rest = &content[4..]; // Skip first "---\n"
        let end_idx = rest.find("\n---\n").or_else(|| rest.find("\n---\r\n"));

        match end_idx {
            Some(idx) => {
                let frontmatter_str = &rest[..idx];
                let body = &rest[idx + 5..]; // Skip "\n---\n"

                // Parse YAML frontmatter into IndexMap to preserve order
                let frontmatter: IndexMap<String, YamlValue> =
                    serde_yaml::from_str(frontmatter_str)?;

                Ok((frontmatter, body.to_string()))
            }
            None => {
                // Malformed frontmatter (no closing delimiter) - treat as no frontmatter
                Ok((IndexMap::new(), content))
            }
        }
    }

    /// Reconstructs a markdown file with updated frontmatter.
    fn reconstruct_file(
        &self,
        path: &str,
        frontmatter: &IndexMap<String, YamlValue>,
        body: &str,
    ) -> Result<()> {
        let yaml_str = serde_yaml::to_string(frontmatter)?;
        let content = format!("---\n{}---\n{}", yaml_str, body);
        self.fs
            .write_file(std::path::Path::new(path), &content)
            .map_err(|e| DiaryxError::FileWrite {
                path: PathBuf::from(path),
                source: e,
            })?;
        Ok(())
    }

    /// Adds or updates a frontmatter property.
    /// Creates frontmatter if none exists.
    pub fn set_frontmatter_property(&self, path: &str, key: &str, value: YamlValue) -> Result<()> {
        let (mut frontmatter, body) = self.parse_file_or_create_frontmatter(path)?;
        frontmatter.insert(key.to_string(), value);
        self.reconstruct_file(path, &frontmatter, &body)
    }

    /// Removes a frontmatter property.
    /// Does nothing if no frontmatter exists or key is not found.
    pub fn remove_frontmatter_property(&self, path: &str, key: &str) -> Result<()> {
        match self.parse_file(path) {
            Ok((mut frontmatter, body)) => {
                frontmatter.shift_remove(key);
                self.reconstruct_file(path, &frontmatter, &body)
            }
            Err(DiaryxError::NoFrontmatter(_)) => Ok(()), // No frontmatter, nothing to remove
            Err(e) => Err(e),
        }
    }

    /// Renames a frontmatter property key.
    /// Returns Ok(true) if the key was found and renamed, Ok(false) if key was not found or no frontmatter.
    pub fn rename_frontmatter_property(
        &self,
        path: &str,
        old_key: &str,
        new_key: &str,
    ) -> Result<bool> {
        let (frontmatter, body) = match self.parse_file(path) {
            Ok(result) => result,
            Err(DiaryxError::NoFrontmatter(_)) => return Ok(false), // No frontmatter, key not found
            Err(e) => return Err(e),
        };

        if !frontmatter.contains_key(old_key) {
            return Ok(false);
        }

        // Rebuild the map, replacing old_key with new_key at the same position
        let mut result: IndexMap<String, YamlValue> = IndexMap::new();
        for (k, v) in frontmatter {
            if k == old_key {
                result.insert(new_key.to_string(), v);
            } else {
                result.insert(k, v);
            }
        }

        self.reconstruct_file(path, &result, &body)?;
        Ok(true)
    }

    /// Get body content (excluding frontmatter). If no frontmatter exists, returns entire file.
    pub fn get_content(&self, path: &str) -> Result<String> {
        match self.parse_file_or_create_frontmatter(path) {
            Ok((_frontmatter, body)) => Ok(body),
            Err(e) => Err(e),
        }
    }

    /// Set body content, preserving (or creating) frontmatter.
    pub fn set_content(&self, path: &str, content: &str) -> Result<()> {
        let (frontmatter, _old_body) = self.parse_file_or_create_frontmatter(path)?;
        self.reconstruct_file(path, &frontmatter, content)
    }

    /// Clear file body content.
    pub fn clear_content(&self, path: &str) -> Result<()> {
        self.set_content(path, "")
    }

    /// Append content to end of body.
    pub fn append_content(&self, path: &str, content: &str) -> Result<()> {
        let (frontmatter, mut body) = self.parse_file_or_create_frontmatter(path)?;

        if body.is_empty() {
            body = content.to_string();
        } else if body.ends_with('\n') {
            body.push_str(content);
        } else {
            body.push('\n');
            body.push_str(content);
        }

        self.reconstruct_file(path, &frontmatter, &body)
    }

    /// Prepend content to start of body.
    pub fn prepend_content(&self, path: &str, content: &str) -> Result<()> {
        let (frontmatter, body) = self.parse_file_or_create_frontmatter(path)?;
        let new_body = if body.is_empty() {
            content.to_string()
        } else if content.ends_with('\n') {
            format!("{}{}", content, body)
        } else {
            format!("{}\n{}", content, body)
        };
        self.reconstruct_file(path, &frontmatter, &new_body)
    }

    /// Gets a frontmatter property value.
    /// Returns Ok(None) if no frontmatter exists or key is not found.
    pub fn get_frontmatter_property(&self, path: &str, key: &str) -> Result<Option<YamlValue>> {
        match self.parse_file(path) {
            Ok((frontmatter, _)) => Ok(frontmatter.get(key).cloned()),
            Err(DiaryxError::NoFrontmatter(_)) => Ok(None), // No frontmatter, key not found
            Err(e) => Err(e),
        }
    }

    /// Gets all frontmatter properties.
    /// Returns empty map if no frontmatter exists.
    pub fn get_all_frontmatter(&self, path: &str) -> Result<IndexMap<String, YamlValue>> {
        match self.parse_file(path) {
            Ok((frontmatter, _)) => Ok(frontmatter),
            Err(DiaryxError::NoFrontmatter(_)) => Ok(IndexMap::new()), // No frontmatter, return empty
            Err(e) => Err(e),
        }
    }

    // ==================== Attachment Methods ====================

    /// Add an attachment path to the entry's attachments list.
    /// Creates the attachments property if it doesn't exist.
    pub fn add_attachment(&self, path: &str, attachment_path: &str) -> Result<()> {
        let (mut frontmatter, body) = self.parse_file_or_create_frontmatter(path)?;
        let from_path = Path::new(path);
        let target_key = attachment_dedup_key(attachment_path, from_path);

        let attachments = frontmatter
            .entry("attachments".to_string())
            .or_insert(YamlValue::Sequence(vec![]));

        if let YamlValue::Sequence(list) = attachments {
            let exists = list.iter().any(|item| {
                if let YamlValue::String(existing) = item {
                    attachment_dedup_key(existing, from_path) == target_key
                } else {
                    false
                }
            });

            if !exists {
                list.push(YamlValue::String(attachment_path.to_string()));
            }
        }

        self.reconstruct_file(path, &frontmatter, &body)
    }

    /// Remove an attachment path from the entry's attachments list.
    /// Does nothing if the attachment isn't found.
    pub fn remove_attachment(&self, path: &str, attachment_path: &str) -> Result<()> {
        let (mut frontmatter, body) = match self.parse_file(path) {
            Ok(result) => result,
            Err(DiaryxError::NoFrontmatter(_)) => return Ok(()),
            Err(e) => return Err(e),
        };
        let from_path = Path::new(path);
        let target_key = attachment_dedup_key(attachment_path, from_path);

        if let Some(YamlValue::Sequence(list)) = frontmatter.get_mut("attachments") {
            list.retain(|item| {
                if let YamlValue::String(s) = item {
                    attachment_dedup_key(s, from_path) != target_key
                } else {
                    true
                }
            });

            // Remove empty attachments array
            if list.is_empty() {
                frontmatter.shift_remove("attachments");
            }
        }

        self.reconstruct_file(path, &frontmatter, &body)
    }

    /// Get the list of attachments directly declared in this entry.
    pub fn get_attachments(&self, path: &str) -> Result<Vec<String>> {
        let (frontmatter, _) = match self.parse_file(path) {
            Ok(result) => result,
            Err(DiaryxError::NoFrontmatter(_)) => return Ok(vec![]),
            Err(e) => return Err(e),
        };

        match frontmatter.get("attachments") {
            Some(YamlValue::Sequence(list)) => Ok(list
                .iter()
                .filter_map(|v| {
                    if let YamlValue::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .collect()),
            _ => Ok(vec![]),
        }
    }

    /// Resolve an attachment by traversing up the index hierarchy via part_of.
    /// Returns the absolute path to the attachment if found, or None.
    pub fn resolve_attachment(
        &self,
        entry_path: &str,
        attachment_name: &str,
    ) -> Result<Option<PathBuf>> {
        use crate::workspace::IndexFrontmatter;

        let entry_path = Path::new(entry_path);
        let entry_dir = entry_path.parent().unwrap_or(Path::new("."));

        // Parse the file's frontmatter inline (to avoid async Workspace dependency)
        let content = match self.fs.read_to_string(entry_path) {
            Ok(c) => c,
            Err(_) => return Ok(None),
        };

        // Parse frontmatter
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            return Ok(None);
        }

        let rest = &content[4..];
        let end_idx = match rest.find("\n---\n").or_else(|| rest.find("\n---\r\n")) {
            Some(idx) => idx,
            None => return Ok(None),
        };

        let frontmatter_str = &rest[..end_idx];
        let frontmatter: IndexFrontmatter = match serde_yaml::from_str(frontmatter_str) {
            Ok(fm) => fm,
            Err(_) => return Ok(None),
        };

        // First, check attachments directly on this entry
        for att_path in frontmatter.attachments_list() {
            // Parse through link_parser to handle markdown links and different path formats
            let parsed = link_parser::parse_link(att_path);
            let resolved = entry_dir.join(&parsed.path);
            if resolved.file_name().map(|n| n.to_string_lossy()) == Some(attachment_name.into())
                && self.fs.exists(&resolved)
            {
                return Ok(Some(resolved));
            }
            // Also check if the path itself matches
            if parsed.path == attachment_name
                || parsed.path.ends_with(&format!("/{}", attachment_name))
            {
                let resolved = entry_dir.join(&parsed.path);
                if self.fs.exists(&resolved) {
                    return Ok(Some(resolved));
                }
            }
        }

        // Traverse up via part_of
        if let Some(ref parent_rel) = frontmatter.part_of {
            let parsed_parent = link_parser::parse_link(parent_rel);
            let canonical_parent = link_parser::to_canonical(&parsed_parent, entry_path);
            let parent_path = PathBuf::from(canonical_parent);
            if self.fs.exists(&parent_path) {
                return self.resolve_attachment(&parent_path.to_string_lossy(), attachment_name);
            }
        }

        Ok(None)
    }

    // ==================== Frontmatter Sorting ====================

    /// Sort frontmatter keys according to a pattern.
    /// Pattern is comma-separated keys, with "*" meaning "rest alphabetically".
    /// Example: "title,description,*" puts title first, description second, rest alphabetically
    /// Does nothing if no frontmatter exists (won't add empty frontmatter).
    pub fn sort_frontmatter(&self, path: &str, pattern: Option<&str>) -> Result<()> {
        let (frontmatter, body) = match self.parse_file(path) {
            Ok(result) => result,
            Err(DiaryxError::NoFrontmatter(_)) => return Ok(()), // No frontmatter, nothing to sort
            Err(e) => return Err(e),
        };

        let sorted = match pattern {
            Some(p) => self.sort_by_pattern(frontmatter, p),
            None => self.sort_alphabetically(frontmatter),
        };

        self.reconstruct_file(path, &sorted, &body)
    }

    fn sort_alphabetically(
        &self,
        frontmatter: IndexMap<String, YamlValue>,
    ) -> IndexMap<String, YamlValue> {
        let mut pairs: Vec<_> = frontmatter.into_iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        pairs.into_iter().collect()
    }

    fn sort_by_pattern(
        &self,
        frontmatter: IndexMap<String, YamlValue>,
        pattern: &str,
    ) -> IndexMap<String, YamlValue> {
        let priority_keys: Vec<&str> = pattern.split(',').map(|s| s.trim()).collect();

        let mut result = IndexMap::new();
        let mut remaining: IndexMap<String, YamlValue> = frontmatter;

        for key in &priority_keys {
            if *key == "*" {
                // Insert remaining keys alphabetically
                let mut rest: Vec<_> = remaining.drain(..).collect();
                rest.sort_by(|a, b| a.0.cmp(&b.0));
                for (k, v) in rest {
                    result.insert(k, v);
                }
                break;
            } else if let Some(value) = remaining.shift_remove(*key) {
                result.insert(key.to_string(), value);
            }
        }

        // If no "*" was in pattern, append any remaining keys alphabetically.
        if !remaining.is_empty() {
            let mut rest: Vec<_> = remaining.drain(..).collect();
            rest.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, v) in rest {
                result.insert(k, v);
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::SyncToAsyncFs;
    use crate::test_utils::MockFileSystem;

    #[test]
    fn test_get_content() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\nHello, world!");
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs));

        let content = crate::fs::block_on_test(app.get_content("test.md")).unwrap();
        assert_eq!(content, "Hello, world!");
    }

    #[test]
    fn test_get_content_empty_body() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\n");
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs));

        let content = crate::fs::block_on_test(app.get_content("test.md")).unwrap();
        assert_eq!(content, "");
    }

    #[test]
    fn test_get_content_no_frontmatter() {
        let fs = MockFileSystem::new().with_file("test.md", "Just plain content");
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs));

        let content = crate::fs::block_on_test(app.get_content("test.md")).unwrap();
        assert_eq!(content, "Just plain content");
    }

    #[test]
    fn test_set_content() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\nOld content");
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs.clone()));

        crate::fs::block_on_test(app.set_content("test.md", "New content")).unwrap();

        let result = fs.get_content("test.md").unwrap();
        assert!(result.contains("title: Test"));
        assert!(result.contains("New content"));
        assert!(!result.contains("Old content"));
    }

    #[test]
    fn test_set_content_preserves_frontmatter() {
        let fs = MockFileSystem::new().with_file(
            "test.md",
            "---\ntitle: My Title\ndescription: A description\n---\nOld",
        );
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs.clone()));

        crate::fs::block_on_test(app.set_content("test.md", "New body")).unwrap();

        let result = fs.get_content("test.md").unwrap();
        assert!(result.contains("title: My Title"));
        assert!(result.contains("description: A description"));
        assert!(result.contains("New body"));
    }

    #[test]
    fn test_clear_content() {
        let fs =
            MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\nSome content here");
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs.clone()));

        crate::fs::block_on_test(app.clear_content("test.md")).unwrap();

        let result = fs.get_content("test.md").unwrap();
        assert!(result.contains("title: Test"));
        // Content should be empty after the frontmatter closing
        assert!(result.ends_with("---\n"));
    }

    #[test]
    fn test_append_content() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\nFirst line");
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs.clone()));

        crate::fs::block_on_test(app.append_content("test.md", "Second line")).unwrap();

        let result = fs.get_content("test.md").unwrap();
        assert!(result.contains("First line"));
        assert!(result.contains("Second line"));
        // Second line should come after first
        let first_pos = result.find("First line").unwrap();
        let second_pos = result.find("Second line").unwrap();
        assert!(second_pos > first_pos);
    }

    #[test]
    fn test_append_content_adds_newline() {
        let fs = MockFileSystem::new()
            .with_file("test.md", "---\ntitle: Test\n---\nNo trailing newline");
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs.clone()));

        crate::fs::block_on_test(app.append_content("test.md", "Appended")).unwrap();

        let content = crate::fs::block_on_test(app.get_content("test.md")).unwrap();
        assert!(content.contains("No trailing newline\nAppended"));
    }

    #[test]
    fn test_append_content_to_empty_body() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\n");
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs.clone()));

        crate::fs::block_on_test(app.append_content("test.md", "New content")).unwrap();

        let content = crate::fs::block_on_test(app.get_content("test.md")).unwrap();
        assert_eq!(content, "New content");
    }

    #[test]
    fn test_prepend_content() {
        let fs =
            MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\nExisting content");
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs.clone()));

        crate::fs::block_on_test(app.prepend_content("test.md", "# Header")).unwrap();

        let result = fs.get_content("test.md").unwrap();
        assert!(result.contains("# Header"));
        assert!(result.contains("Existing content"));
        // Header should come before existing content
        let header_pos = result.find("# Header").unwrap();
        let existing_pos = result.find("Existing content").unwrap();
        assert!(header_pos < existing_pos);
    }

    #[test]
    fn test_prepend_content_adds_newline() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\nExisting");
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs.clone()));

        crate::fs::block_on_test(app.prepend_content("test.md", "Prepended")).unwrap();

        let content = crate::fs::block_on_test(app.get_content("test.md")).unwrap();
        assert!(content.contains("Prepended\nExisting"));
    }

    #[test]
    fn test_prepend_content_to_empty_body() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\n");
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs.clone()));

        crate::fs::block_on_test(app.prepend_content("test.md", "New content")).unwrap();

        let content = crate::fs::block_on_test(app.get_content("test.md")).unwrap();
        assert_eq!(content, "New content");
    }

    #[test]
    fn test_content_operations_on_nonexistent_file() {
        let fs = MockFileSystem::new();
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs.clone()));

        // set_content should create the file
        crate::fs::block_on_test(app.set_content("new.md", "Content")).unwrap();

        let result = fs.get_content("new.md").unwrap();
        assert!(result.contains("Content"));
    }

    #[test]
    fn test_multiple_content_operations() {
        let fs = MockFileSystem::new().with_file("test.md", "---\ntitle: Test\n---\n");
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs.clone()));

        crate::fs::block_on_test(app.append_content("test.md", "Line 1")).unwrap();
        crate::fs::block_on_test(app.append_content("test.md", "Line 2")).unwrap();
        crate::fs::block_on_test(app.prepend_content("test.md", "# Title")).unwrap();

        let content = crate::fs::block_on_test(app.get_content("test.md")).unwrap();
        assert!(content.starts_with("# Title"));
        assert!(content.contains("Line 1"));
        assert!(content.contains("Line 2"));
    }

    #[test]
    fn test_add_attachment_dedups_note_and_binary_forms() {
        // Seed an index that already references the attachment in note form
        // (markdown link to the `.md` wrapper note). A subsequent call with
        // the bare binary path must be recognised as a duplicate and not
        // appended.
        let existing = "---\n\
             title: Index\n\
             attachments:\n\
             - '[photo.jpg](</folder/photo.jpg.md>)'\n\
             ---\n\
             body\n";
        let fs = MockFileSystem::new().with_file("folder/index.md", existing);
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs.clone()));

        crate::fs::block_on_test(app.add_attachment("folder/index.md", "photo.jpg")).unwrap();

        let result = fs.get_content("folder/index.md").unwrap();
        // Should still have exactly one attachment entry.
        assert_eq!(
            result.matches("photo.jpg").count(),
            2, // once in the link title, once in the link target
            "add_attachment duplicated a note-form reference when given the binary form. Got: {result}"
        );
    }

    #[test]
    fn test_add_attachment_dedups_repeated_binary_form() {
        let fs = MockFileSystem::new().with_file("folder/index.md", "---\ntitle: Index\n---\n");
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs.clone()));

        crate::fs::block_on_test(app.add_attachment("folder/index.md", "photo.jpg")).unwrap();
        crate::fs::block_on_test(app.add_attachment("folder/index.md", "photo.jpg")).unwrap();

        let result = fs.get_content("folder/index.md").unwrap();
        assert_eq!(
            result.matches("photo.jpg").count(),
            1,
            "add_attachment appended the same binary path twice. Got: {result}"
        );
    }

    #[test]
    fn test_remove_attachment_matches_either_form() {
        let existing = "---\n\
             title: Index\n\
             attachments:\n\
             - '[photo.jpg](</folder/photo.jpg.md>)'\n\
             ---\n";
        let fs = MockFileSystem::new().with_file("folder/index.md", existing);
        let app = DiaryxApp::new(SyncToAsyncFs::new(fs.clone()));

        // Remove via the binary path — should strip the note-form entry.
        crate::fs::block_on_test(app.remove_attachment("folder/index.md", "photo.jpg")).unwrap();

        let result = fs.get_content("folder/index.md").unwrap();
        assert!(
            !result.contains("photo.jpg"),
            "remove_attachment didn't strip the note-form entry when given binary path. Got: {result}"
        );
    }
}
