#![warn(missing_docs)]

//! UniFFI-friendly API for Apple clients.
//!
//! This crate provides a small, stable surface area that can be bound to Swift
//! while `apps/apple` is incrementally migrated to `diaryx_core`.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use diaryx_core::frontmatter;
use diaryx_core::fs::{AsyncFileSystem, RealFileSystem, SyncToAsyncFs};
use indexmap::IndexMap;
use serde_yaml::Value;
use thiserror::Error;

/// Entry metadata returned by `list_entries`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct EntrySummary {
    /// Stable ID for the entry (currently the workspace-relative path).
    pub id: String,
    /// Workspace-relative path to the markdown file.
    pub path: String,
    /// Parsed `title` frontmatter field when present.
    pub title: Option<String>,
}

/// A single frontmatter key-value pair, flattened for UniFFI.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MetadataField {
    /// The frontmatter key (e.g. "title", "tags").
    pub key: String,
    /// Stringified scalar value. Empty when the value is an array.
    pub value: String,
    /// Array items when the value is a YAML sequence. Empty for scalars.
    pub values: Vec<String>,
}

/// Entry payload returned by `get_entry`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct EntryData {
    /// Stable ID for the entry (currently the workspace-relative path).
    pub id: String,
    /// Workspace-relative path to the markdown file.
    pub path: String,
    /// Raw markdown content (including frontmatter). Kept for backward compat.
    pub markdown: String,
    /// Body content without frontmatter (what the editor should display).
    pub body: String,
    /// Parsed frontmatter fields.
    pub metadata: Vec<MetadataField>,
}

/// Errors surfaced through UniFFI.
#[derive(Debug, Error, uniffi::Error)]
pub enum DiaryxAppleError {
    /// The provided workspace path does not exist.
    #[error("Workspace does not exist: {0}")]
    WorkspaceNotFound(String),
    /// The provided workspace path is not a directory.
    #[error("Workspace path is not a directory: {0}")]
    WorkspaceNotDirectory(String),
    /// Invalid entry path (must be workspace-relative without `..` traversal).
    #[error("Invalid entry path: {0}")]
    InvalidEntryPath(String),
    /// Underlying IO or core-layer failure.
    #[error("{0}")]
    Core(String),
}

/// Convert a YAML `Value` to a display string.
fn yaml_value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Sequence(_) | Value::Mapping(_) | Value::Tagged(_) => serde_yaml::to_string(value)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

/// Convert parsed frontmatter into a Vec of `MetadataField`.
fn frontmatter_to_metadata(fm: &IndexMap<String, Value>) -> Vec<MetadataField> {
    fm.iter()
        .map(|(key, value)| match value {
            Value::Sequence(seq) => MetadataField {
                key: key.clone(),
                value: String::new(),
                values: seq.iter().map(yaml_value_to_string).collect(),
            },
            _ => MetadataField {
                key: key.clone(),
                value: yaml_value_to_string(value),
                values: Vec::new(),
            },
        })
        .collect()
}

/// Workspace handle for Apple clients.
#[derive(uniffi::Object)]
pub struct DiaryxAppleWorkspace {
    workspace_root: PathBuf,
    fs: SyncToAsyncFs<RealFileSystem>,
}

#[uniffi::export]
impl DiaryxAppleWorkspace {
    /// Open an existing workspace directory.
    #[uniffi::constructor]
    pub fn new(workspace_path: String) -> Result<Arc<Self>, DiaryxAppleError> {
        let root = PathBuf::from(&workspace_path);
        if !root.exists() {
            return Err(DiaryxAppleError::WorkspaceNotFound(workspace_path));
        }
        if !root.is_dir() {
            return Err(DiaryxAppleError::WorkspaceNotDirectory(workspace_path));
        }

        Ok(Arc::new(Self {
            workspace_root: root,
            fs: SyncToAsyncFs::new(RealFileSystem),
        }))
    }

    /// Get the absolute workspace root path.
    pub fn workspace_root(&self) -> String {
        self.workspace_root.to_string_lossy().into_owned()
    }

    /// List all markdown entries in the workspace recursively.
    pub fn list_entries(&self) -> Result<Vec<EntrySummary>, DiaryxAppleError> {
        let mut files =
            futures_lite::future::block_on(self.fs.list_md_files_recursive(&self.workspace_root))
                .map_err(|e| DiaryxAppleError::Core(format!("Failed to list entries: {e}")))?;

        files.sort();

        let mut entries = Vec::with_capacity(files.len());
        for full_path in files {
            let relative = self.to_relative(&full_path)?;
            let markdown = futures_lite::future::block_on(self.fs.read_to_string(&full_path))
                .map_err(|e| {
                    DiaryxAppleError::Core(format!(
                        "Failed to read entry '{}': {e}",
                        full_path.display()
                    ))
                })?;

            let title = frontmatter::parse(&markdown).ok().and_then(|parsed| {
                frontmatter::get_string(&parsed.frontmatter, "title").map(str::to_owned)
            });

            entries.push(EntrySummary {
                id: relative.clone(),
                path: relative,
                title,
            });
        }

        Ok(entries)
    }

    /// Get a single entry by workspace-relative path.
    pub fn get_entry(&self, id: String) -> Result<EntryData, DiaryxAppleError> {
        let rel = Self::validated_relative_path(&id)?;
        let full = self.workspace_root.join(&rel);
        let markdown =
            futures_lite::future::block_on(self.fs.read_to_string(&full)).map_err(|e| {
                DiaryxAppleError::Core(format!("Failed to read entry '{}': {e}", full.display()))
            })?;

        let parsed = frontmatter::parse_or_empty(&markdown)
            .map_err(|e| DiaryxAppleError::Core(format!("Failed to parse frontmatter: {e}")))?;

        Ok(EntryData {
            id: id.clone(),
            path: id,
            body: parsed.body.clone(),
            metadata: frontmatter_to_metadata(&parsed.frontmatter),
            markdown,
        })
    }

    /// Save raw markdown for a single entry by workspace-relative path.
    pub fn save_entry(&self, id: String, markdown: String) -> Result<(), DiaryxAppleError> {
        let rel = Self::validated_relative_path(&id)?;
        let full = self.workspace_root.join(&rel);
        futures_lite::future::block_on(self.fs.write_file(&full, &markdown)).map_err(|e| {
            DiaryxAppleError::Core(format!("Failed to save entry '{}': {e}", full.display()))
        })
    }

    /// Save only the body content for an entry, preserving existing frontmatter.
    ///
    /// Reads the current file, parses its frontmatter, then writes back the
    /// original frontmatter combined with the new body.
    pub fn save_entry_body(&self, id: String, body: String) -> Result<(), DiaryxAppleError> {
        let rel = Self::validated_relative_path(&id)?;
        let full = self.workspace_root.join(&rel);

        let existing =
            futures_lite::future::block_on(self.fs.read_to_string(&full)).map_err(|e| {
                DiaryxAppleError::Core(format!("Failed to read entry '{}': {e}", full.display()))
            })?;

        let parsed = frontmatter::parse_or_empty(&existing)
            .map_err(|e| DiaryxAppleError::Core(format!("Failed to parse frontmatter: {e}")))?;

        let content = if parsed.frontmatter.is_empty() {
            body
        } else {
            frontmatter::serialize(&parsed.frontmatter, &body).map_err(|e| {
                DiaryxAppleError::Core(format!("Failed to serialize frontmatter: {e}"))
            })?
        };

        futures_lite::future::block_on(self.fs.write_file(&full, &content)).map_err(|e| {
            DiaryxAppleError::Core(format!("Failed to save entry '{}': {e}", full.display()))
        })
    }
}

/// Open an existing workspace directory.
#[uniffi::export]
pub fn open_workspace(
    workspace_path: String,
) -> Result<Arc<DiaryxAppleWorkspace>, DiaryxAppleError> {
    DiaryxAppleWorkspace::new(workspace_path)
}

impl DiaryxAppleWorkspace {
    fn validated_relative_path(path: &str) -> Result<PathBuf, DiaryxAppleError> {
        if path.trim().is_empty() {
            return Err(DiaryxAppleError::InvalidEntryPath(
                "Path cannot be empty".to_string(),
            ));
        }

        let p = Path::new(path);
        if p.is_absolute() {
            return Err(DiaryxAppleError::InvalidEntryPath(format!(
                "Path must be workspace-relative: {path}"
            )));
        }

        if p.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(DiaryxAppleError::InvalidEntryPath(format!(
                "Path cannot contain '..' or absolute-prefix components: {path}"
            )));
        }

        Ok(p.to_path_buf())
    }

    fn to_relative(&self, full_path: &Path) -> Result<String, DiaryxAppleError> {
        full_path
            .strip_prefix(&self.workspace_root)
            .map_err(|_| {
                DiaryxAppleError::Core(format!(
                    "File '{}' is outside workspace root '{}'.",
                    full_path.display(),
                    self.workspace_root.display()
                ))
            })
            .map(|p| p.to_string_lossy().replace('\\', "/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yaml_value_to_string_scalar() {
        assert_eq!(
            yaml_value_to_string(&Value::String("hello".into())),
            "hello"
        );
        assert_eq!(yaml_value_to_string(&Value::Bool(true)), "true");
        assert_eq!(
            yaml_value_to_string(&Value::Number(serde_yaml::Number::from(42))),
            "42"
        );
    }

    #[test]
    fn test_yaml_value_to_string_null() {
        assert_eq!(yaml_value_to_string(&Value::Null), "");
    }

    #[test]
    fn test_frontmatter_to_metadata_scalars() {
        let mut fm = IndexMap::new();
        fm.insert("title".to_string(), Value::String("My Title".into()));
        fm.insert("draft".to_string(), Value::Bool(false));

        let fields = frontmatter_to_metadata(&fm);
        assert_eq!(fields.len(), 2);

        assert_eq!(fields[0].key, "title");
        assert_eq!(fields[0].value, "My Title");
        assert!(fields[0].values.is_empty());

        assert_eq!(fields[1].key, "draft");
        assert_eq!(fields[1].value, "false");
        assert!(fields[1].values.is_empty());
    }

    #[test]
    fn test_frontmatter_to_metadata_array() {
        let mut fm = IndexMap::new();
        fm.insert(
            "tags".to_string(),
            Value::Sequence(vec![
                Value::String("rust".into()),
                Value::String("swift".into()),
            ]),
        );

        let fields = frontmatter_to_metadata(&fm);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].key, "tags");
        assert!(fields[0].value.is_empty());
        assert_eq!(fields[0].values, vec!["rust", "swift"]);
    }

    #[test]
    fn test_frontmatter_to_metadata_null_value() {
        let mut fm = IndexMap::new();
        fm.insert("empty".to_string(), Value::Null);

        let fields = frontmatter_to_metadata(&fm);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].key, "empty");
        assert!(fields[0].value.is_empty());
        assert!(fields[0].values.is_empty());
    }

    #[test]
    fn test_frontmatter_to_metadata_empty() {
        let fm = IndexMap::new();
        let fields = frontmatter_to_metadata(&fm);
        assert!(fields.is_empty());
    }
}

uniffi::setup_scaffolding!();
