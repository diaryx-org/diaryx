//! Core data types used across the Diaryx codebase.
//!
//! These types represent file metadata, binary attachments, and version history
//! structures that are used in the command/response API and across multiple crates.

use std::collections::HashMap;

/// Parses a value that should be a string, but may be an integer/float/bool.
/// Converts non-string scalars to their string representation; null → `None`.
/// Used via `#[fig(deserialize_with = ..)]`.
fn deserialize_string_lenient(value: &fig::Value) -> Result<Option<String>, fig::Error> {
    Ok(match value {
        fig::Value::Null => None,
        fig::Value::Str(s) => Some(s.clone()),
        fig::Value::Int(i) => Some(i.to_string()),
        fig::Value::Uint(u) => Some(u.to_string()),
        fig::Value::Float(f) => Some(f.to_string()),
        fig::Value::Bool(b) => Some(b.to_string()),
        other => {
            return Err(fig::Error::Message(format!(
                "expected string or number, got {other:?}"
            )));
        }
    })
}

/// Lightweight filesystem metadata returned by backend commands and plugin hosts.
#[derive(Debug, Clone, Default, PartialEq, Eq, fig::ToValue, fig::FromValue)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct FileInfo {
    /// Whether the path currently exists.
    #[fig(default)]
    pub exists: bool,

    /// File size in bytes when known.
    #[fig(default)]
    pub size_bytes: Option<u64>,

    /// Modification time in milliseconds since Unix epoch when known.
    #[fig(default)]
    pub modified_at_ms: Option<i64>,
}

/// Metadata for a file in the workspace.
///
/// This represents the synchronized state of a file's frontmatter properties.
///
/// ## Doc-ID Based Architecture
///
/// Files are keyed by stable document IDs (ARK file blades) rather than file
/// paths. This makes renames trivial property updates rather than delete+create
/// operations.
///
/// The actual filesystem path is derived from the `filename` field and the parent chain:
/// - `filename`: The file's name on disk (e.g., "my-note.md")
/// - `part_of`: Document ID of the parent (or None for root files)
#[derive(Debug, Clone, Default, PartialEq, fig::ToValue, fig::FromValue)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct FileMetadata {
    /// Filename on disk (e.g., "my-note.md"). Required for non-deleted files.
    /// For files created before the doc-ID migration, this may be empty and
    /// should be derived from the path key during migration.
    #[fig(default)]
    pub filename: String,

    /// Display title from frontmatter
    #[fig(default, deserialize_with = "deserialize_string_lenient")]
    pub title: Option<String>,

    /// Canonical self-link declared in frontmatter.
    #[fig(default)]
    pub link: Option<String>,

    /// Explicit outbound links declared in frontmatter.
    #[fig(default)]
    pub links: Option<Vec<String>>,

    /// Explicit backlinks declared in frontmatter.
    #[fig(default)]
    pub link_of: Option<Vec<String>>,

    /// Document ID of parent file (e.g., "qx4r9d", an ARK file blade), or None
    /// for root files. Note: For backward compatibility during migration, this
    /// may temporarily contain absolute paths which will be converted to doc_ids.
    #[fig(default)]
    pub part_of: Option<String>,

    /// Document IDs of child files.
    /// Note: For backward compatibility during migration, this may temporarily
    /// contain relative paths which will be converted to doc_ids.
    #[fig(default)]
    pub contents: Option<Vec<String>>,

    /// Binary attachment references
    #[fig(default)]
    pub attachments: Vec<BinaryRef>,

    /// Singular link to the binary asset for attachment-note entries.
    #[fig(default)]
    pub attachment: Option<String>,

    /// Reverse links from entries whose `attachments` reference this note.
    #[fig(default)]
    pub attachment_of: Option<Vec<String>>,

    /// Soft deletion tombstone - if true, file is considered deleted
    #[fig(default)]
    pub deleted: bool,

    /// Visibility/access control tags
    #[fig(default)]
    pub audience: Option<Vec<String>>,

    /// File description from frontmatter
    #[fig(default, deserialize_with = "deserialize_string_lenient")]
    pub description: Option<String>,

    /// Additional frontmatter properties not covered by other fields
    #[fig(default)]
    pub extra: HashMap<String, crate::yaml::Value>,

    /// Unix timestamp of last modification (milliseconds)
    #[fig(default)]
    pub modified_at: i64,
}

impl FileMetadata {
    /// Build FileMetadata from parsed YAML frontmatter.
    ///
    /// Tries a fast JSON round-trip first, then falls back to manual field extraction.
    /// Unknown frontmatter keys are preserved in `extra`.
    pub fn from_frontmatter(fm: &indexmap::IndexMap<String, crate::yaml::Value>) -> Self {
        /// Parse the frontmatter "updated" value into a timestamp (ms).
        fn parse_updated_value(value: &crate::yaml::Value) -> Option<i64> {
            if let Some(num) = value.as_i64() {
                return Some(num);
            }
            if let Some(num) = value.as_f64() {
                return Some(num as i64);
            }
            if let Some(raw) = value.as_str() {
                if let Ok(num) = raw.parse::<i64>() {
                    return Some(num);
                }
                if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(raw) {
                    return Some(parsed.timestamp_millis());
                }
            }
            None
        }

        let known_fields: &[&str] = &[
            "title",
            "link",
            "links",
            "link_of",
            "part_of",
            "contents",
            "audience",
            "description",
            "attachments",
            "attachment",
            "attachment_of",
            "deleted",
            "modified_at",
            "updated",
            "filename",
            "extra",
        ];

        // Fast path: build a fig map from the frontmatter and decode directly
        // (serde-free) for automatic field mapping.
        let __fm_value = fig::Value::Map(
            fm.iter()
                .map(|(k, v)| (fig::Value::Str(k.clone()), fig::ToValue::to_value(v)))
                .collect(),
        );
        if let Ok(mut metadata) = <FileMetadata as fig::FromValue>::from_value(&__fm_value) {
            if let Some(updated) = fm.get("updated").and_then(parse_updated_value) {
                metadata.modified_at = updated;
            }
            if metadata.modified_at == 0 {
                metadata.modified_at = chrono::Utc::now().timestamp_millis();
            }

            // Preserve unknown frontmatter fields in extra
            for (key, value) in fm {
                if !known_fields.contains(&key.as_str()) && !metadata.extra.contains_key(key) {
                    metadata.extra.insert(key.clone(), value.clone());
                }
            }

            return metadata;
        }

        // Fallback: manual extraction of known fields
        let mut metadata = FileMetadata::default();

        if let Some(title) = fm.get("title") {
            metadata.title = title.as_str().map(String::from);
        }
        if let Some(link) = fm.get("link") {
            metadata.link = link.as_str().map(String::from);
        }
        if let Some(links) = fm.get("links")
            && let Some(seq) = links.as_sequence()
        {
            metadata.links = Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            );
        }
        if let Some(link_of) = fm.get("link_of")
            && let Some(seq) = link_of.as_sequence()
        {
            metadata.link_of = Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            );
        }
        if let Some(part_of) = fm.get("part_of") {
            metadata.part_of = part_of.as_str().map(String::from);
        }
        if let Some(contents) = fm.get("contents")
            && let Some(seq) = contents.as_sequence()
        {
            metadata.contents = Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            );
        }
        if let Some(audience) = fm.get("audience")
            && let Some(seq) = audience.as_sequence()
        {
            metadata.audience = Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            );
        }
        if let Some(description) = fm.get("description") {
            metadata.description = description.as_str().map(String::from);
        }
        if let Some(attachments) = fm.get("attachments")
            && let Some(seq) = attachments.as_sequence()
        {
            metadata.attachments = seq
                .iter()
                .filter_map(|value| {
                    use crate::yaml;
                    match value {
                        yaml::Value::String(path) => Some(BinaryRef {
                            path: path.clone(),
                            source: "local".to_string(),
                            hash: String::new(),
                            mime_type: String::new(),
                            size: 0,
                            uploaded_at: None,
                            deleted: false,
                        }),
                        yaml::Value::Mapping(map) => {
                            let path = map.get("path").and_then(|v| v.as_str())?;
                            let source = map
                                .get("source")
                                .and_then(|v| v.as_str())
                                .unwrap_or("local");
                            let hash = map.get("hash").and_then(|v| v.as_str()).unwrap_or("");
                            let mime_type =
                                map.get("mime_type").and_then(|v| v.as_str()).unwrap_or("");
                            let size = map.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                            let uploaded_at = map.get("uploaded_at").and_then(|v| v.as_i64());
                            let deleted = map
                                .get("deleted")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);

                            Some(BinaryRef {
                                path: path.to_string(),
                                source: source.to_string(),
                                hash: hash.to_string(),
                                mime_type: mime_type.to_string(),
                                size,
                                uploaded_at,
                                deleted,
                            })
                        }
                        _ => None,
                    }
                })
                .collect();
        }
        if let Some(attachment) = fm.get("attachment") {
            metadata.attachment = attachment.as_str().map(String::from);
        }
        if let Some(attachment_of) = fm.get("attachment_of")
            && let Some(seq) = attachment_of.as_sequence()
        {
            metadata.attachment_of = Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
            );
        }

        // Store remaining fields in extra
        for (key, value) in fm {
            if !known_fields.contains(&key.as_str()) {
                metadata.extra.insert(key.clone(), value.clone());
            }
        }

        if let Some(updated) = fm.get("updated").and_then(parse_updated_value) {
            metadata.modified_at = updated;
        } else if metadata.modified_at == 0 {
            metadata.modified_at = chrono::Utc::now().timestamp_millis();
        }
        metadata
    }

    /// Create new FileMetadata with the given title
    pub fn new(title: Option<String>) -> Self {
        Self {
            title,
            modified_at: chrono::Utc::now().timestamp_millis(),
            ..Default::default()
        }
    }

    /// Create new FileMetadata with filename and title
    pub fn with_filename(filename: String, title: Option<String>) -> Self {
        Self {
            filename,
            title,
            modified_at: chrono::Utc::now().timestamp_millis(),
            ..Default::default()
        }
    }

    /// Mark this file as deleted (soft delete)
    pub fn mark_deleted(&mut self) {
        self.deleted = true;
        self.modified_at = chrono::Utc::now().timestamp_millis();
    }

    /// Check if this file is an index (has contents)
    pub fn is_index(&self) -> bool {
        self.contents.as_ref().is_some_and(|c| !c.is_empty())
    }

    /// Check if two FileMetadata are semantically equal (ignoring `modified_at`).
    ///
    /// This is used for change detection during sync to avoid false positives
    /// when `modified_at` timestamps differ but content is the same.
    pub fn is_content_equal(&self, other: &Self) -> bool {
        self.filename == other.filename
            && self.title == other.title
            && self.link == other.link
            && self.links == other.links
            && self.link_of == other.link_of
            && self.part_of == other.part_of
            && self.contents == other.contents
            && self.attachments == other.attachments
            && self.attachment == other.attachment
            && self.attachment_of == other.attachment_of
            && self.deleted == other.deleted
            && self.audience == other.audience
            && self.description == other.description
            && self.extra == other.extra
    }

    /// Convert a title to a normalized filename.
    ///
    /// Rules:
    /// - Lowercase
    /// - Replace spaces and underscores with hyphens
    /// - Remove non-alphanumeric characters (except hyphens)
    /// - Collapse multiple hyphens
    /// - Append .md extension
    ///
    /// Example: "My Note Title" → "my-note-title.md"
    pub fn normalize_title_to_filename(title: &str) -> String {
        let normalized: String = title
            .to_lowercase()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c
                } else if c == ' ' || c == '_' {
                    '-'
                } else if c == '-' {
                    c
                } else {
                    // Skip other characters
                    '-'
                }
            })
            .collect();

        // Collapse multiple hyphens and trim leading/trailing hyphens
        let collapsed: String = normalized
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");

        if collapsed.is_empty() {
            "untitled.md".to_string()
        } else {
            format!("{}.md", collapsed)
        }
    }

    /// Check if this metadata uses the legacy path-based format.
    ///
    /// Returns true if part_of contains a path (has '/') rather than an ID blade.
    pub fn is_legacy_format(&self) -> bool {
        self.part_of
            .as_ref()
            .is_some_and(|p| p.contains('/') || p.ends_with(".md"))
    }
}

/// Reference to a binary attachment file.
///
/// Binary files (images, PDFs, etc.) are stored separately from the document
/// content, with only their metadata tracked here.
#[derive(Debug, Clone, PartialEq, fig::ToValue, fig::FromValue)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(export, export_to = "bindings/"))]
pub struct BinaryRef {
    /// Relative path within workspace (e.g., "_attachments/image.png")
    pub path: String,

    /// Source of the binary: "local", "pending", or external URL
    pub source: String,

    /// SHA-256 hash for deduplication and integrity
    pub hash: String,

    /// MIME type (e.g., "image/png")
    pub mime_type: String,

    /// File size in bytes
    pub size: u64,

    /// Unix timestamp when uploaded (milliseconds)
    pub uploaded_at: Option<i64>,

    /// Soft deletion tombstone
    pub deleted: bool,
}

impl BinaryRef {
    /// Create a new local binary reference
    pub fn new_local(path: String, hash: String, mime_type: String, size: u64) -> Self {
        Self {
            path,
            source: "local".to_string(),
            hash,
            mime_type,
            size,
            uploaded_at: Some(chrono::Utc::now().timestamp_millis()),
            deleted: false,
        }
    }

    /// Create a pending binary reference (not yet uploaded)
    pub fn new_pending(path: String, mime_type: String, size: u64) -> Self {
        Self {
            path,
            source: "pending".to_string(),
            hash: String::new(),
            mime_type,
            size,
            uploaded_at: None,
            deleted: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_metadata_default() {
        let meta = FileMetadata::default();
        assert!(meta.title.is_none());
        assert!(meta.filename.is_empty());
        assert!(!meta.deleted);
        assert!(meta.attachments.is_empty());
    }

    #[test]
    fn test_file_metadata_new() {
        let meta = FileMetadata::new(Some("Test".to_string()));
        assert_eq!(meta.title, Some("Test".to_string()));
        assert!(meta.modified_at > 0);
    }

    #[test]
    fn test_file_metadata_with_filename() {
        let meta = FileMetadata::with_filename("test.md".to_string(), Some("Test".to_string()));
        assert_eq!(meta.filename, "test.md");
        assert_eq!(meta.title, Some("Test".to_string()));
        assert!(meta.modified_at > 0);
    }

    #[test]
    fn test_file_metadata_mark_deleted() {
        let mut meta = FileMetadata::default();
        let original_time = meta.modified_at;
        std::thread::sleep(std::time::Duration::from_millis(1));
        meta.mark_deleted();
        assert!(meta.deleted);
        assert!(meta.modified_at > original_time);
    }

    #[test]
    fn test_normalize_title_to_filename() {
        assert_eq!(
            FileMetadata::normalize_title_to_filename("My Note Title"),
            "my-note-title.md"
        );
        assert_eq!(
            FileMetadata::normalize_title_to_filename("Hello World!"),
            "hello-world.md"
        );
        assert_eq!(
            FileMetadata::normalize_title_to_filename("Test_File Name"),
            "test-file-name.md"
        );
        assert_eq!(
            FileMetadata::normalize_title_to_filename("  Multiple   Spaces  "),
            "multiple-spaces.md"
        );
        assert_eq!(FileMetadata::normalize_title_to_filename(""), "untitled.md");
        assert_eq!(
            FileMetadata::normalize_title_to_filename("!!!"),
            "untitled.md"
        );
    }

    #[test]
    fn test_is_legacy_format() {
        let mut meta = FileMetadata::default();
        assert!(!meta.is_legacy_format()); // No part_of

        meta.part_of = Some("qx4r9d".to_string());
        assert!(!meta.is_legacy_format()); // ID blade format

        meta.part_of = Some("workspace/index.md".to_string());
        assert!(meta.is_legacy_format()); // Path format

        meta.part_of = Some("index.md".to_string());
        assert!(meta.is_legacy_format()); // Filename with .md
    }

    #[test]
    fn test_binary_ref_new_local() {
        let binary = BinaryRef::new_local(
            "test.png".to_string(),
            "abc123".to_string(),
            "image/png".to_string(),
            1024,
        );
        assert_eq!(binary.source, "local");
        assert!(binary.uploaded_at.is_some());
    }
}
