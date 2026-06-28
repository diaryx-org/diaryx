//! Metadata-to-frontmatter conversion and file writing utilities.
//!
//! This module provides functions to convert `FileMetadata` into YAML
//! frontmatter format and write files with proper structure.
//!
//! # Link Formats
//!
//! When writing `part_of`, `contents`, and `attachments` to frontmatter, this module creates
//! portable markdown links in the format: `[Title](/workspace/path.md)`
//!
//! These links are:
//! - **Clickable** in editors like Obsidian
//! - **Unambiguous** with workspace-root paths
//! - **Self-documenting** with human-readable titles
//!
//! File writes use a temp + backup strategy to reduce the risk of partial
//! frontmatter updates if the process is interrupted mid-write.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::date;
use crate::error::{DiaryxError, Result};
use crate::frontmatter;
use crate::fs::AsyncFileSystem;
use crate::link_parser;
use crate::yaml;

/// Metadata structure for file frontmatter.
/// This mirrors `FileMetadata` but with simpler types for serialization.
///
/// When serialized to YAML, `part_of`, `contents`, and `attachments` are formatted as markdown links
/// with workspace-root paths: `[Title](/path/to/file.md)`
#[derive(Debug, Clone, Default)]
pub struct FrontmatterMetadata {
    /// Display title from frontmatter
    pub title: Option<String>,
    /// Canonical self-link for this file.
    pub link: Option<String>,
    /// Explicit outbound links declared by this file.
    pub links: Option<Vec<String>>,
    /// Explicit backlinks declared by other files.
    pub link_of: Option<Vec<String>>,
    /// Markdown link to parent index file (e.g., `[Parent](/folder/index.md)`)
    pub part_of: Option<String>,
    /// Markdown links to child files (e.g., `[Child](/folder/child.md)`)
    pub contents: Option<Vec<String>>,
    /// Binary attachment paths
    pub attachments: Option<Vec<String>>,
    /// Visibility/access control tags
    pub audience: Option<Vec<String>>,
    /// File description
    pub description: Option<String>,
    /// Last modification timestamp (milliseconds since Unix epoch)
    pub updated: Option<i64>,
    /// Additional frontmatter properties (excluding internal keys like _body)
    pub extra: HashMap<String, yaml::Value>,
}

impl FrontmatterMetadata {
    /// Parse from a JSON value (typically from `FileMetadata`).
    ///
    /// Note: This basic version doesn't convert paths. For writing files to disk
    /// with proper markdown links, use `from_json_with_markdown_links`.
    pub fn from_json(value: &yaml::Value) -> Self {
        Self::from_json_with_file_path(value, None)
    }

    /// Parse from a JSON value with the canonical file path.
    ///
    /// This method formats `part_of`, `contents`, and `attachments` as markdown links with
    /// workspace-root paths: `[Title](/path/to/file.md)`
    ///
    /// # Arguments
    /// * `value` - The JSON value containing FileMetadata
    /// * `canonical_file_path` - The canonical path of the file being written (e.g., "folder/index.md")
    pub fn from_json_with_file_path(
        value: &yaml::Value,
        _canonical_file_path: Option<&str>,
    ) -> Self {
        // Use the markdown links version with a path_to_title fallback for titles
        Self::from_json_with_markdown_links(value, link_parser::path_to_title)
    }

    /// Parse from a JSON value, formatting links with titles from a resolver function.
    ///
    /// This method formats `part_of`, `contents`, and `attachments` as markdown links with
    /// workspace-root paths: `[Title](/path/to/file.md)`
    ///
    /// # Arguments
    /// * `value` - The JSON value containing FileMetadata
    /// * `title_resolver` - A function that returns a display title for a canonical path
    ///
    /// # Example
    /// ```ignore
    /// let metadata = FrontmatterMetadata::from_json_with_markdown_links(
    ///     &json_value,
    ///     |path| path_to_title(path),
    /// );
    /// ```
    pub fn from_json_with_markdown_links<F>(value: &yaml::Value, title_resolver: F) -> Self
    where
        F: Fn(&str) -> String,
    {
        let obj = value.as_mapping();

        let title = obj
            .and_then(|o| o.get("title"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let link = obj
            .and_then(|o| o.get("link"))
            .and_then(|v| v.as_str())
            .map(|raw_value| {
                let parsed = link_parser::parse_link(raw_value);
                let canonical_path = &parsed.path;
                let link_title = parsed
                    .title
                    .clone()
                    .unwrap_or_else(|| title_resolver(canonical_path));
                link_parser::format_link(canonical_path, &link_title)
            });

        let links = obj
            .and_then(|o| o.get("links"))
            .and_then(|v| v.as_sequence())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|raw_value| {
                        let parsed = link_parser::parse_link(raw_value);
                        let canonical_path = &parsed.path;
                        let link_title = parsed
                            .title
                            .clone()
                            .unwrap_or_else(|| title_resolver(canonical_path));
                        link_parser::format_link(canonical_path, &link_title)
                    })
                    .collect()
            });

        let link_of = obj
            .and_then(|o| o.get("link_of"))
            .and_then(|v| v.as_sequence())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|raw_value| {
                        let parsed = link_parser::parse_link(raw_value);
                        let canonical_path = &parsed.path;
                        let link_title = parsed
                            .title
                            .clone()
                            .unwrap_or_else(|| title_resolver(canonical_path));
                        link_parser::format_link(canonical_path, &link_title)
                    })
                    .collect()
            });

        let part_of = obj
            .and_then(|o| o.get("part_of"))
            .and_then(|v| v.as_str())
            .map(|raw_value| {
                // Parse the incoming value - handles both plain paths and markdown links
                // This prevents double-wrapping if the value is already formatted
                let parsed = link_parser::parse_link(raw_value);
                let canonical_path = &parsed.path;
                // Use existing title from markdown link if available, otherwise resolve
                let link_title = parsed
                    .title
                    .clone()
                    .unwrap_or_else(|| title_resolver(canonical_path));
                link_parser::format_link(canonical_path, &link_title)
            });

        let contents = obj
            .and_then(|o| o.get("contents"))
            .and_then(|v| v.as_sequence())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|raw_value| {
                        // Parse the incoming value - handles both plain paths and markdown links
                        // This prevents double-wrapping if the value is already formatted
                        let parsed = link_parser::parse_link(raw_value);
                        let canonical_path = &parsed.path;
                        // Use existing title from markdown link if available, otherwise resolve
                        let link_title = parsed
                            .title
                            .clone()
                            .unwrap_or_else(|| title_resolver(canonical_path));
                        link_parser::format_link(canonical_path, &link_title)
                    })
                    .collect()
            });

        let attachments = obj
            .and_then(|o| o.get("attachments"))
            .and_then(|v| v.as_sequence())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        // Handle both string and object (BinaryRef) formats
                        if let Some(s) = v.as_str() {
                            Some(s.to_string())
                        } else if let Some(obj) = v.as_mapping() {
                            obj.get("path").and_then(|p| p.as_str()).map(String::from)
                        } else {
                            None
                        }
                    })
                    .map(|raw_value| {
                        // Parse incoming value to avoid double-wrapping existing markdown links.
                        let parsed = link_parser::parse_link(&raw_value);
                        let canonical_path = &parsed.path;
                        let link_title = parsed.title.clone().unwrap_or_else(|| {
                            std::path::Path::new(canonical_path)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(canonical_path)
                                .to_string()
                        });
                        link_parser::format_link(canonical_path, &link_title)
                    })
                    .collect()
            });

        let audience = obj
            .and_then(|o| o.get("audience"))
            .and_then(|v| v.as_sequence())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

        let description = obj
            .and_then(|o| o.get("description"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let updated = obj
            .and_then(|o| o.get("modified_at"))
            .and_then(|v| v.as_i64());

        // Extract extra properties, excluding internal keys
        let mut extra = HashMap::new();
        if let Some(extra_obj) = obj
            .and_then(|o| o.get("extra"))
            .and_then(|v| v.as_mapping())
        {
            for (key, value) in extra_obj {
                // Skip internal keys (starting with _)
                if !key.starts_with('_') {
                    extra.insert(key.clone(), value.clone());
                }
            }
        }

        Self {
            title,
            link,
            links,
            link_of,
            part_of,
            contents,
            attachments,
            audience,
            description,
            updated,
            extra,
        }
    }

    /// Convert to YAML frontmatter string.
    ///
    /// Builds an ordered [`yaml::Mapping`] in the canonical frontmatter field
    /// order and serializes it through `fig` (via [`yaml::serialize_mapping`]),
    /// the same backend the comment-preserving editor uses for in-place property
    /// edits — so fresh whole-file writes and incremental edits share one YAML
    /// style and one escaping implementation. The trailing newline fig appends is
    /// trimmed so the caller's `---\n{yaml}\n---` framing stays tight.
    pub fn to_yaml(&self) -> String {
        let mut map = yaml::Mapping::new();

        if let Some(title) = &self.title {
            map.insert("title".to_string(), yaml::Value::String(title.clone()));
        }

        if let Some(link) = &self.link {
            map.insert("link".to_string(), yaml::Value::String(link.clone()));
        }

        if let Some(links) = &self.links
            && !links.is_empty()
        {
            map.insert("links".to_string(), str_seq(links));
        }

        if let Some(link_of) = &self.link_of
            && !link_of.is_empty()
        {
            map.insert("link_of".to_string(), str_seq(link_of));
        }

        if let Some(part_of) = &self.part_of {
            map.insert("part_of".to_string(), yaml::Value::String(part_of.clone()));
        }

        if let Some(contents) = &self.contents {
            // An explicit empty sequence is preserved (serializes to `[]`) so an
            // index file keeps its identity even with no children.
            map.insert("contents".to_string(), str_seq(contents));
        }

        if let Some(audience) = &self.audience
            && !audience.is_empty()
        {
            map.insert("audience".to_string(), str_seq(audience));
        }

        if let Some(description) = &self.description {
            map.insert(
                "description".to_string(),
                yaml::Value::String(description.clone()),
            );
        }

        if let Some(attachments) = &self.attachments
            && !attachments.is_empty()
        {
            map.insert("attachments".to_string(), str_seq(attachments));
        }

        // Write updated timestamp as a local RFC3339 string, falling back to the
        // raw millis if the timestamp can't be formatted.
        if let Some(updated) = self.updated {
            let value = match date::timestamp_millis_to_local_rfc3339(updated) {
                Some(formatted) => yaml::Value::String(formatted),
                None => yaml::Value::Int(updated),
            };
            map.insert("updated".to_string(), value);
        }

        // Add extra properties
        for (key, value) in &self.extra {
            map.insert(key.clone(), value.clone());
        }

        yaml::serialize_mapping(&map)
            .unwrap_or_default()
            .trim_end()
            .to_string()
    }
}

/// Build a YAML sequence value from a slice of strings.
fn str_seq(items: &[String]) -> yaml::Value {
    yaml::Value::Sequence(items.iter().cloned().map(yaml::Value::String).collect())
}

/// Write a file with metadata as YAML frontmatter and body content.
///
/// Note: This function doesn't convert canonical paths to relative paths.
/// For proper path conversion, use `write_file_with_metadata_and_canonical_path`.
pub async fn write_file_with_metadata<FS: AsyncFileSystem>(
    fs: &FS,
    path: &Path,
    metadata: &yaml::Value,
    body: &str,
) -> Result<()> {
    write_file_with_metadata_and_canonical_path(fs, path, metadata, body, None).await
}

/// Write a file with metadata as YAML frontmatter and body content.
///
/// When `canonical_path` is provided, `contents` and `part_of` paths in the metadata
/// are converted from canonical (workspace-relative) paths to file-relative paths.
///
/// # Arguments
/// * `fs` - The filesystem to write to
/// * `path` - The storage path to write the file to
/// * `metadata` - The JSON metadata (typically from `FileMetadata`)
/// * `body` - The body content of the file
/// * `canonical_path` - The canonical path of the file (e.g., "folder/index.md") for path conversion
pub async fn write_file_with_metadata_and_canonical_path<FS: AsyncFileSystem>(
    fs: &FS,
    path: &Path,
    metadata: &yaml::Value,
    body: &str,
    canonical_path: Option<&str>,
) -> Result<()> {
    let fm = FrontmatterMetadata::from_json_with_file_path(metadata, canonical_path);
    let yaml = fm.to_yaml();

    // Combine frontmatter and body
    let content = if yaml.is_empty() {
        body.to_string()
    } else {
        format!("---\n{}\n---\n{}", yaml, body)
    };

    // Ensure parent directory exists
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs.create_dir_all(parent).await?;
    }

    // Recover from any previous interrupted write
    recover_backup_if_needed(fs, path).await?;

    let temp_path = temp_path_for(path);
    let backup_path = backup_path_for(path);

    let safe_write_result: Result<()> = async {
        if fs.try_exists(&temp_path).await.unwrap_or(false) {
            let _ = fs.remove_file(&temp_path).await;
        }

        fs.write(&temp_path, content.as_bytes())
            .await
            .map_err(|e| DiaryxError::FileWrite {
                path: temp_path.clone(),
                source: e,
            })?;

        // Move existing file out of the way (backup) before swapping in the new content.
        if fs.try_exists(path).await.unwrap_or(false)
            && let Err(e) = fs.rename(path, &backup_path).await
        {
            if is_not_found_io_error(&e) {
                // OPFS/FSA can race between exists() and move(). If the source
                // disappeared, continue with temp->path swap.
                log::warn!(
                    "metadata_writer: backup move skipped for '{}': {}",
                    path.display(),
                    e
                );
            } else {
                return Err(DiaryxError::FileWrite {
                    path: backup_path.clone(),
                    source: e,
                });
            }
        }

        if let Err(e) = fs.rename(&temp_path, path).await {
            // Attempt to restore the backup if swap failed.
            if fs.try_exists(&backup_path).await.unwrap_or(false) {
                let _ = fs.rename(&backup_path, path).await;
            }
            return Err(DiaryxError::FileWrite {
                path: path.to_path_buf(),
                source: e,
            });
        }

        if fs.try_exists(&backup_path).await.unwrap_or(false) {
            let _ = fs.remove_file(&backup_path).await;
        }

        Ok(())
    }
    .await;

    match safe_write_result {
        Ok(()) => Ok(()),
        Err(e) if should_fallback_to_direct_write(&e) => {
            log::warn!(
                "metadata_writer: safe-write failed for '{}', falling back to direct overwrite: {}",
                path.display(),
                e
            );

            // Best-effort cleanup before fallback.
            if fs.try_exists(&temp_path).await.unwrap_or(false) {
                let _ = fs.remove_file(&temp_path).await;
            }

            fs.write(path, content.as_bytes())
                .await
                .map_err(|source| DiaryxError::FileWrite {
                    path: path.to_path_buf(),
                    source,
                })?;

            // If fallback succeeded, stale backup should not remain.
            if fs.try_exists(&backup_path).await.unwrap_or(false) {
                let _ = fs.remove_file(&backup_path).await;
            }

            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Update a file's frontmatter metadata, preserving or replacing the body.
///
/// If `new_body` is `Some`, it replaces the existing body.
/// If `new_body` is `None`, the existing body is preserved.
pub async fn update_file_metadata<FS: AsyncFileSystem>(
    fs: &FS,
    path: &Path,
    metadata: &yaml::Value,
    new_body: Option<&str>,
) -> Result<()> {
    // Determine the body content
    let body = if let Some(b) = new_body {
        b.to_string()
    } else {
        // Read existing body from file
        let content = fs
            .read_to_string(path)
            .await
            .map_err(|e| DiaryxError::FileRead {
                path: path.to_path_buf(),
                source: e,
            })?;

        let parsed = frontmatter::parse_or_empty(&content)?;
        parsed.body
    };

    // Preserve existing frontmatter fields when the incoming metadata omits them.
    let mut merged_metadata = metadata.clone();
    if let Some(obj) = merged_metadata.as_mapping_mut()
        && let Ok(existing_content) = fs.read_to_string(path).await
        && let Ok(parsed) = frontmatter::parse_or_empty(&existing_content)
    {
        let fm = &parsed.frontmatter;

        let missing_contents = obj.get("contents").map(|v| v.is_null()).unwrap_or(true);
        if missing_contents && let Some(seq) = fm.get("contents").and_then(|v| v.as_sequence()) {
            let preserved: Vec<yaml::Value> = seq
                .iter()
                .filter_map(|v| v.as_str().map(|s| yaml::Value::String(s.to_string())))
                .collect();
            obj.insert("contents".to_string(), yaml::Value::Sequence(preserved));
        }

        let missing_part_of = obj.get("part_of").map(|v| v.is_null()).unwrap_or(true);
        if missing_part_of && let Some(parent) = fm.get("part_of").and_then(|v| v.as_str()) {
            obj.insert(
                "part_of".to_string(),
                yaml::Value::String(parent.to_string()),
            );
        }
    }

    write_file_with_metadata(fs, path, &merged_metadata, &body).await
}

fn temp_path_for(path: &Path) -> PathBuf {
    match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => path.with_file_name(format!("{}.tmp", name)),
        None => path.with_extension("tmp"),
    }
}

fn backup_path_for(path: &Path) -> PathBuf {
    match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => path.with_file_name(format!("{}.bak", name)),
        None => path.with_extension("bak"),
    }
}

fn should_fallback_to_direct_write(err: &DiaryxError) -> bool {
    if let DiaryxError::FileWrite { source, .. } = err {
        if is_not_found_io_error(source) {
            return true;
        }
        if source.kind() == ErrorKind::AlreadyExists {
            return true;
        }
    }

    let msg = err.to_string();
    msg.contains("NoModificationAllowedError")
        || msg.contains("InvalidStateError")
        || msg.contains("NotAllowedError")
        || msg.contains("NotFoundError")
        || msg.contains("A requested file or directory could not be found")
        || msg.contains("The object can not be found here")
        || msg.contains("already exists")
}

fn is_not_found_io_error(err: &std::io::Error) -> bool {
    err.kind() == ErrorKind::NotFound
        || err.to_string().contains("NotFoundError")
        || err
            .to_string()
            .contains("A requested file or directory could not be found")
        || err.to_string().contains("The object can not be found here")
}

async fn recover_backup_if_needed<FS: AsyncFileSystem>(fs: &FS, path: &Path) -> Result<()> {
    let backup_path = backup_path_for(path);
    if fs.try_exists(&backup_path).await.unwrap_or(false) {
        if !fs.try_exists(path).await.unwrap_or(false) {
            fs.rename(&backup_path, path)
                .await
                .map_err(|e| DiaryxError::FileWrite {
                    path: path.to_path_buf(),
                    source: e,
                })?;
        } else {
            let _ = fs.remove_file(&backup_path).await;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFileSystem, SyncToAsyncFs, block_on_test};
    use std::io;
    use std::sync::Mutex;

    struct NotFoundOnBackupMoveFs {
        inner: InMemoryFileSystem,
        always_fail_backup_move: bool,
        fail_backup_move_once: Mutex<bool>,
    }

    impl NotFoundOnBackupMoveFs {
        fn fail_once() -> Self {
            Self {
                inner: InMemoryFileSystem::new(),
                always_fail_backup_move: false,
                fail_backup_move_once: Mutex::new(true),
            }
        }

        fn fail_always() -> Self {
            Self {
                inner: InMemoryFileSystem::new(),
                always_fail_backup_move: true,
                fail_backup_move_once: Mutex::new(false),
            }
        }

        fn should_fail_backup_move(&self, to: &Path) -> bool {
            if !to.to_string_lossy().ends_with(".bak") {
                return false;
            }
            if self.always_fail_backup_move {
                return true;
            }
            let mut once = self.fail_backup_move_once.lock().unwrap();
            if *once {
                *once = false;
                return true;
            }
            false
        }
    }

    impl FileSystem for NotFoundOnBackupMoveFs {
        fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
            self.inner.read(path)
        }

        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            self.inner.read_to_string(path)
        }

        fn read_dir(&self, path: &Path) -> io::Result<Vec<crate::fs::DirEntry>> {
            self.inner.read_dir(path)
        }

        fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
            self.inner.write(path, contents)
        }

        fn create_new(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
            self.inner.create_new(path, contents)
        }

        fn create_dir(&self, path: &Path) -> io::Result<()> {
            self.inner.create_dir(path)
        }

        fn create_dir_all(&self, path: &Path) -> io::Result<()> {
            self.inner.create_dir_all(path)
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            self.inner.remove_file(path)
        }

        fn remove_dir(&self, path: &Path) -> io::Result<()> {
            self.inner.remove_dir(path)
        }

        fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
            self.inner.remove_dir_all(path)
        }

        fn metadata(&self, path: &Path) -> io::Result<crate::fs::Metadata> {
            self.inner.metadata(path)
        }

        fn symlink_metadata(&self, path: &Path) -> io::Result<crate::fs::Metadata> {
            self.inner.symlink_metadata(path)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            if self.should_fail_backup_move(to) {
                return Err(io::Error::new(
                    ErrorKind::NotFound,
                    "NotFoundError: A requested file or directory could not be found",
                ));
            }
            self.inner.rename(from, to)
        }
    }

    #[test]
    fn test_frontmatter_metadata_from_json() {
        let json = serde_json::json!({
            "title": "Test Title",
            "part_of": "workspace/parent.md",
            "contents": ["child1.md", "child2.md"],
            "description": "A test file",
            "extra": {
                "custom_key": "custom_value",
                "_body": "should be excluded"
            }
        })
        .into();

        let fm = FrontmatterMetadata::from_json(&json);
        assert_eq!(fm.title, Some("Test Title".to_string()));
        // Now formatted as markdown link with workspace-root path
        assert_eq!(
            fm.part_of,
            Some("[Parent](/workspace/parent.md)".to_string())
        );
        assert_eq!(
            fm.contents,
            Some(vec![
                "[Child1](/child1.md)".to_string(),
                "[Child2](/child2.md)".to_string()
            ])
        );
        assert_eq!(fm.description, Some("A test file".to_string()));
        assert!(fm.extra.contains_key("custom_key"));
        assert!(!fm.extra.contains_key("_body")); // Internal key excluded
    }

    #[test]
    fn test_frontmatter_metadata_to_yaml() {
        let fm = FrontmatterMetadata {
            title: Some("Test Title".to_string()),
            link: None,
            links: None,
            link_of: None,
            part_of: Some("[Parent Index](/folder/parent.md)".to_string()),
            contents: Some(vec!["[Child](/folder/child.md)".to_string()]),
            audience: None,
            description: Some("A description".to_string()),
            attachments: None,
            updated: None,
            extra: HashMap::new(),
        };

        let yaml = fm.to_yaml();
        assert!(yaml.contains("title: Test Title"));
        // Markdown links are quoted in YAML because they contain special
        // characters; fig emits single-quoted scalars and column-0 block items.
        assert!(yaml.contains("part_of: '[Parent Index](/folder/parent.md)'"));
        assert!(yaml.contains("contents:"));
        assert!(yaml.contains("- '[Child](/folder/child.md)'"));
        assert!(yaml.contains("description: A description"));
    }

    #[test]
    fn test_frontmatter_metadata_to_yaml_uses_local_offset_for_updated() {
        let fm = FrontmatterMetadata {
            title: Some("Test Title".to_string()),
            link: None,
            links: None,
            link_of: None,
            part_of: None,
            contents: None,
            audience: None,
            description: None,
            attachments: None,
            updated: Some(1_700_000_000_000),
            extra: HashMap::new(),
        };

        let yaml = fm.to_yaml();
        let updated_line = yaml
            .lines()
            .find(|line| line.starts_with("updated: "))
            .unwrap();
        let value = updated_line
            .strip_prefix("updated: ")
            .unwrap()
            .trim_matches('"');
        let parsed = chrono::DateTime::parse_from_rfc3339(value).unwrap();
        assert_eq!(parsed.timestamp_millis(), 1_700_000_000_000);
        assert!(!value.ends_with('Z'));
    }

    #[test]
    fn test_empty_contents_written_as_empty_array() {
        // Empty contents (Some([])) should be written as "contents: []"
        // to preserve index file identity
        let fm = FrontmatterMetadata {
            title: Some("Root Index".to_string()),
            link: None,
            links: None,
            link_of: None,
            part_of: None,
            contents: Some(vec![]), // Empty but explicitly set
            audience: None,
            description: None,
            attachments: None,
            updated: None,
            extra: HashMap::new(),
        };

        let yaml = fm.to_yaml();
        assert!(
            yaml.contains("contents: []"),
            "Empty contents should be written as 'contents: []', got: {}",
            yaml
        );
    }

    #[test]
    fn test_none_contents_not_written() {
        // None contents should NOT be written at all
        let fm = FrontmatterMetadata {
            title: Some("Regular File".to_string()),
            link: None,
            links: None,
            link_of: None,
            part_of: Some("parent.md".to_string()),
            contents: None, // Not an index file
            audience: None,
            description: None,
            attachments: None,
            updated: None,
            extra: HashMap::new(),
        };

        let yaml = fm.to_yaml();
        assert!(
            !yaml.contains("contents"),
            "None contents should not be written, got: {}",
            yaml
        );
    }

    #[test]
    fn test_from_json_with_markdown_links_formats_part_of() {
        let json = serde_json::json!({
            "title": "Child File",
            "part_of": "folder/parent.md",
        })
        .into();

        let fm = FrontmatterMetadata::from_json_with_file_path(&json, Some("folder/child.md"));
        // Now formatted as markdown link with workspace-root path
        assert_eq!(fm.part_of, Some("[Parent](/folder/parent.md)".to_string()));
    }

    #[test]
    fn test_from_json_with_markdown_links_formats_contents() {
        let json = serde_json::json!({
            "title": "Parent Index",
            "contents": ["folder/child1.md", "folder/sub/child2.md"],
        })
        .into();

        let fm = FrontmatterMetadata::from_json_with_file_path(&json, Some("folder/index.md"));
        // Contents formatted as markdown links with workspace-root paths
        assert_eq!(
            fm.contents,
            Some(vec![
                "[Child1](/folder/child1.md)".to_string(),
                "[Child2](/folder/sub/child2.md)".to_string()
            ])
        );
    }

    #[test]
    fn test_from_json_with_markdown_links_formats_attachments() {
        let json = serde_json::json!({
            "title": "Entry",
            "attachments": ["folder/_attachments/image.png", "folder/_attachments/report.pdf"],
        })
        .into();

        let fm = FrontmatterMetadata::from_json_with_file_path(&json, Some("folder/entry.md"));
        assert_eq!(
            fm.attachments,
            Some(vec![
                "[image.png](/folder/_attachments/image.png)".to_string(),
                "[report.pdf](/folder/_attachments/report.pdf)".to_string(),
            ])
        );
    }

    #[test]
    fn test_from_json_with_markdown_links_formats_binary_ref_attachments() {
        let json = serde_json::json!({
            "title": "Entry",
            "attachments": [
                { "path": "folder/_attachments/photo.jpg", "hash": "abc" }
            ],
        })
        .into();

        let fm = FrontmatterMetadata::from_json_with_file_path(&json, Some("folder/entry.md"));
        assert_eq!(
            fm.attachments,
            Some(vec![
                "[photo.jpg](/folder/_attachments/photo.jpg)".to_string()
            ])
        );
    }

    #[test]
    fn test_from_json_with_custom_title_resolver() {
        let json = serde_json::json!({
            "title": "Index",
            "part_of": "root/parent.md",
            "contents": ["root/child.md"],
        })
        .into();

        // Use a custom title resolver that returns a fixed title
        let fm = FrontmatterMetadata::from_json_with_markdown_links(&json, |path| {
            if path == "root/parent.md" {
                "My Custom Parent Title".to_string()
            } else if path == "root/child.md" {
                "My Custom Child Title".to_string()
            } else {
                link_parser::path_to_title(path)
            }
        });

        assert_eq!(
            fm.part_of,
            Some("[My Custom Parent Title](/root/parent.md)".to_string())
        );
        assert_eq!(
            fm.contents,
            Some(vec!["[My Custom Child Title](/root/child.md)".to_string()])
        );
    }

    #[test]
    fn test_roundtrip_markdown_link_to_yaml() {
        let fm = FrontmatterMetadata {
            title: Some("Test Entry".to_string()),
            link: None,
            links: None,
            link_of: None,
            part_of: Some("[Parent Index](/Folder/index.md)".to_string()),
            contents: Some(vec!["[Child Entry](/Folder/child.md)".to_string()]),
            audience: None,
            description: None,
            attachments: None,
            updated: None,
            extra: HashMap::new(),
        };

        let yaml = fm.to_yaml();
        // Genuine round-trip: the serialized YAML must reparse to the same
        // scalar values. This proves fig quotes the special-character links
        // correctly regardless of the exact quote style it chooses.
        let map = crate::yaml::parse_mapping(&yaml).expect("reparse to_yaml output");
        assert_eq!(
            map.get("part_of").and_then(|v| v.as_str()),
            Some("[Parent Index](/Folder/index.md)")
        );
        let contents = map
            .get("contents")
            .and_then(|v| v.as_sequence())
            .expect("contents sequence");
        assert_eq!(
            contents.first().and_then(|v| v.as_str()),
            Some("[Child Entry](/Folder/child.md)")
        );
    }

    #[test]
    fn test_from_json_with_already_formatted_markdown_links() {
        // Regression test: already-formatted markdown links should not be double-wrapped
        let json = serde_json::json!({
            "title": "Child File",
            // part_of is ALREADY a markdown link (from sync or corrupted data)
            "part_of": "[Parent Index](/folder/parent.md)",
            "contents": [
                "[Child A](/folder/child_a.md)",
                "[Child B](/folder/child_b.md)",
            ],
        })
        .into();

        let fm = FrontmatterMetadata::from_json_with_file_path(&json, Some("folder/child.md"));

        // Should preserve the link format, NOT double-wrap
        assert_eq!(
            fm.part_of,
            Some("[Parent Index](/folder/parent.md)".to_string())
        );
        assert_eq!(
            fm.contents,
            Some(vec![
                "[Child A](/folder/child_a.md)".to_string(),
                "[Child B](/folder/child_b.md)".to_string(),
            ])
        );
    }

    #[test]
    fn test_from_json_handles_mixed_formats() {
        // Test handling mixed formats in contents (some plain, some markdown)
        let json = serde_json::json!({
            "title": "Mixed Index",
            "contents": [
                "folder/plain_path.md",
                "[Already Formatted](/folder/formatted.md)",
            ],
        })
        .into();

        let fm = FrontmatterMetadata::from_json_with_file_path(&json, Some("folder/index.md"));

        assert_eq!(
            fm.contents,
            Some(vec![
                "[Plain Path](/folder/plain_path.md)".to_string(),
                "[Already Formatted](/folder/formatted.md)".to_string(),
            ])
        );
    }

    #[test]
    fn test_write_file_with_metadata_tolerates_not_found_during_backup_move_once() {
        let fs = SyncToAsyncFs::new(NotFoundOnBackupMoveFs::fail_once());
        let path = Path::new("README.md");

        block_on_test(fs.write(path, "original".as_bytes())).unwrap();

        let metadata = serde_json::json!({ "title": "My Journal" }).into();
        block_on_test(write_file_with_metadata(
            &fs,
            path,
            &metadata,
            "# first edit",
        ))
        .unwrap();

        let updated = block_on_test(fs.read_to_string(path)).unwrap();
        assert!(updated.contains("# first edit"));
    }

    #[test]
    fn test_write_file_with_metadata_tolerates_not_found_during_backup_move_every_time() {
        let fs = SyncToAsyncFs::new(NotFoundOnBackupMoveFs::fail_always());
        let path = Path::new("README.md");

        block_on_test(fs.write(path, "original".as_bytes())).unwrap();
        let metadata = serde_json::json!({ "title": "My Journal" }).into();

        block_on_test(write_file_with_metadata(&fs, path, &metadata, "# edit one")).unwrap();
        block_on_test(write_file_with_metadata(&fs, path, &metadata, "# edit two")).unwrap();

        let updated = block_on_test(fs.read_to_string(path)).unwrap();
        assert!(updated.contains("# edit two"));
    }
}
