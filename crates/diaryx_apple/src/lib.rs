#![warn(missing_docs)]

//! UniFFI-friendly API for Apple clients.
//!
//! This crate provides a small, stable surface area that can be bound to Swift
//! while `apps/apple` is incrementally migrated to `diaryx_core`.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use diaryx_core::frontmatter;
use diaryx_core::fs::{AsyncFileSystem, RealFileSystem, SyncToAsyncFs};
use diaryx_core::workspace::{TreeNode, Workspace};
use indexmap::IndexMap;
use serde_yaml::Value;
use thiserror::Error;

type Ws = Workspace<SyncToAsyncFs<RealFileSystem>>;

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

/// A node in the workspace file tree, flattened for UniFFI.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TreeNodeData {
    /// Display name (title from frontmatter, or filename).
    pub name: String,
    /// Optional description from frontmatter.
    pub description: Option<String>,
    /// Workspace-relative path to this node's file or directory.
    pub path: String,
    /// Whether this node is a directory/folder (has children).
    pub is_folder: bool,
    /// Child nodes.
    pub children: Vec<TreeNodeData>,
}

/// Result of creating a child entry.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CreateChildResultData {
    /// Workspace-relative path to the newly created child.
    pub child_path: String,
    /// Workspace-relative path to the parent (may differ from input if converted).
    pub parent_path: String,
    /// Whether the parent was converted from a leaf to an index.
    pub parent_converted: bool,
    /// Original parent path before conversion (only set when `parent_converted` is true).
    pub original_parent_path: Option<String>,
}

/// UniFFI-friendly representation of a frontmatter value.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum FrontmatterValue {
    /// A text string value.
    Text {
        /// The string content.
        value: String,
    },
    /// A boolean value.
    Bool {
        /// The boolean content.
        value: bool,
    },
    /// A list of strings.
    StringArray {
        /// The string items.
        values: Vec<String>,
    },
}

impl FrontmatterValue {
    fn to_yaml(&self) -> Value {
        match self {
            FrontmatterValue::Text { value } => Value::String(value.clone()),
            FrontmatterValue::Bool { value } => Value::Bool(*value),
            FrontmatterValue::StringArray { values } => {
                Value::Sequence(values.iter().map(|v| Value::String(v.clone())).collect())
            }
        }
    }
}

/// Convert a core `TreeNode` to a UniFFI-friendly `TreeNodeData`.
fn tree_node_to_data(node: &TreeNode, workspace_root: &Path) -> TreeNodeData {
    let path = node
        .path
        .strip_prefix(workspace_root)
        .unwrap_or(&node.path)
        .to_string_lossy()
        .replace('\\', "/");

    TreeNodeData {
        name: node.name.clone(),
        description: node.description.clone(),
        path,
        is_folder: !node.children.is_empty(),
        children: node
            .children
            .iter()
            .map(|c| tree_node_to_data(c, workspace_root))
            .collect(),
    }
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
            body: parsed
                .body
                .strip_prefix('\n')
                .unwrap_or(&parsed.body)
                .to_string(),
            metadata: frontmatter_to_metadata(&parsed.frontmatter),
            markdown,
        })
    }

    /// Save raw markdown for a single entry by workspace-relative path.
    pub fn save_entry(&self, id: String, markdown: String) -> Result<(), DiaryxAppleError> {
        let rel = Self::validated_relative_path(&id)?;
        let full = self.workspace_root.join(&rel);
        std::fs::write(&full, markdown).map_err(|e| {
            DiaryxAppleError::Core(format!("Failed to save entry '{}': {e}", full.display()))
        })
    }

    /// Create a new markdown entry at the given workspace-relative path.
    ///
    /// Parent directories are created automatically. Returns an error if the
    /// file already exists.
    pub fn create_entry(&self, path: String, markdown: String) -> Result<(), DiaryxAppleError> {
        let rel = Self::validated_relative_path(&path)?;
        let full = self.workspace_root.join(&rel);

        if full.exists() {
            return Err(DiaryxAppleError::Core(format!(
                "Entry already exists: {path}"
            )));
        }

        // Create parent directories if needed
        if let Some(parent) = full.parent() {
            if !parent.exists() {
                futures_lite::future::block_on(self.fs.create_dir_all(parent)).map_err(|e| {
                    DiaryxAppleError::Core(format!(
                        "Failed to create directories for '{}': {e}",
                        parent.display()
                    ))
                })?;
            }
        }

        futures_lite::future::block_on(self.fs.write_file(&full, &markdown)).map_err(|e| {
            DiaryxAppleError::Core(format!("Failed to create entry '{}': {e}", full.display()))
        })
    }

    /// Create a subfolder inside the workspace.
    pub fn create_folder(&self, path: String) -> Result<(), DiaryxAppleError> {
        let rel = Self::validated_relative_path(&path)?;
        let full = self.workspace_root.join(&rel);

        futures_lite::future::block_on(self.fs.create_dir_all(&full)).map_err(|e| {
            DiaryxAppleError::Core(format!("Failed to create folder '{}': {e}", full.display()))
        })
    }

    /// Build a workspace tree following `contents`/`part_of` hierarchy.
    ///
    /// Looks for a root index file (has `contents` but no `part_of`) in the
    /// workspace root directory. If found, builds the tree by following
    /// frontmatter `contents` references — the same logic used by the web
    /// app's left sidebar. Falls back to a filesystem directory tree when
    /// no root index exists.
    pub fn build_file_tree(&self) -> Result<TreeNodeData, DiaryxAppleError> {
        let ws = self.make_workspace()?;

        // Try to find a root index and build a contents-based tree
        let root_index =
            futures_lite::future::block_on(ws.find_root_index_in_dir(&self.workspace_root))
                .map_err(|e| {
                    DiaryxAppleError::Core(format!("Failed to scan for root index: {e}"))
                })?;

        let tree = if let Some(root_path) = root_index {
            futures_lite::future::block_on(ws.build_tree(&root_path)).map_err(|e| {
                DiaryxAppleError::Core(format!("Failed to build workspace tree: {e}"))
            })?
        } else {
            // No root index — fall back to filesystem tree
            futures_lite::future::block_on(ws.build_filesystem_tree(&self.workspace_root, false))
                .map_err(|e| DiaryxAppleError::Core(format!("Failed to build file tree: {e}")))?
        };

        Ok(tree_node_to_data(&tree, &self.workspace_root))
    }

    /// Create a child entry under `parent_path`.
    ///
    /// If the parent is a leaf file it is automatically converted to an index
    /// (the `parent_converted` flag will be set in the result). Returns the
    /// workspace-relative paths of both the new child and the (possibly moved)
    /// parent.
    pub fn create_child_entry(
        &self,
        parent_path: String,
        title: Option<String>,
    ) -> Result<CreateChildResultData, DiaryxAppleError> {
        let rel = Self::validated_relative_path(&parent_path)?;
        let abs = self.workspace_root.join(&rel);
        let ws = self.make_workspace()?;

        let result = futures_lite::future::block_on(
            ws.create_child_entry_with_result(&abs, title.as_deref()),
        )
        .map_err(|e| DiaryxAppleError::Core(format!("Failed to create child entry: {e}")))?;

        Ok(CreateChildResultData {
            child_path: self.core_path_to_relative(&result.child_path)?,
            parent_path: self.core_path_to_relative(&result.parent_path)?,
            parent_converted: result.parent_converted,
            original_parent_path: result
                .original_parent_path
                .as_deref()
                .map(|p| self.core_path_to_relative(p))
                .transpose()?,
        })
    }

    /// Move an entry from one location to another.
    pub fn move_entry(&self, from_path: String, to_path: String) -> Result<(), DiaryxAppleError> {
        let from_rel = Self::validated_relative_path(&from_path)?;
        let to_rel = Self::validated_relative_path(&to_path)?;
        let abs_from = self.workspace_root.join(&from_rel);
        let abs_to = self.workspace_root.join(&to_rel);
        let ws = self.make_workspace()?;

        futures_lite::future::block_on(ws.move_entry(&abs_from, &abs_to))
            .map_err(|e| DiaryxAppleError::Core(format!("Failed to move entry: {e}")))
    }

    /// Reparent an entry under a new parent, updating frontmatter links.
    ///
    /// Returns the new workspace-relative path of the moved entry.
    pub fn attach_and_move_entry_to_parent(
        &self,
        entry_path: String,
        parent_path: String,
    ) -> Result<String, DiaryxAppleError> {
        let entry_rel = Self::validated_relative_path(&entry_path)?;
        let parent_rel = Self::validated_relative_path(&parent_path)?;
        let abs_entry = self.workspace_root.join(&entry_rel);
        let abs_parent = self.workspace_root.join(&parent_rel);
        let ws = self.make_workspace()?;

        let new_path = futures_lite::future::block_on(
            ws.attach_and_move_entry_to_parent(&abs_entry, &abs_parent),
        )
        .map_err(|e| DiaryxAppleError::Core(format!("Failed to reparent entry: {e}")))?;
        self.to_relative(&new_path)
    }

    /// Convert a leaf entry to an index (directory with index file).
    ///
    /// Returns the new workspace-relative path (e.g. `foo.md` → `foo/foo.md`).
    pub fn convert_to_index(&self, path: String) -> Result<String, DiaryxAppleError> {
        let rel = Self::validated_relative_path(&path)?;
        let abs = self.workspace_root.join(&rel);
        let ws = self.make_workspace()?;

        let new_path = futures_lite::future::block_on(ws.convert_to_index(&abs))
            .map_err(|e| DiaryxAppleError::Core(format!("Failed to convert to index: {e}")))?;
        self.to_relative(&new_path)
    }

    /// Convert an index entry back to a leaf file.
    ///
    /// Returns the new workspace-relative path (e.g. `foo/foo.md` → `foo.md`).
    pub fn convert_to_leaf(&self, path: String) -> Result<String, DiaryxAppleError> {
        let rel = Self::validated_relative_path(&path)?;
        let abs = self.workspace_root.join(&rel);
        let ws = self.make_workspace()?;

        let new_path = futures_lite::future::block_on(ws.convert_to_leaf(&abs))
            .map_err(|e| DiaryxAppleError::Core(format!("Failed to convert to leaf: {e}")))?;
        self.to_relative(&new_path)
    }

    /// Set a frontmatter property on an entry.
    pub fn set_frontmatter_property(
        &self,
        path: String,
        key: String,
        value: FrontmatterValue,
    ) -> Result<(), DiaryxAppleError> {
        let rel = Self::validated_relative_path(&path)?;
        let abs = self.workspace_root.join(&rel);
        let ws = self.make_workspace()?;

        futures_lite::future::block_on(ws.set_frontmatter_property(&abs, &key, value.to_yaml()))
            .map_err(|e| DiaryxAppleError::Core(format!("Failed to set frontmatter property: {e}")))
    }

    /// Remove a frontmatter property from an entry.
    pub fn remove_frontmatter_property(
        &self,
        path: String,
        key: String,
    ) -> Result<(), DiaryxAppleError> {
        let rel = Self::validated_relative_path(&path)?;
        let abs = self.workspace_root.join(&rel);

        let content =
            futures_lite::future::block_on(self.fs.read_to_string(&abs)).map_err(|e| {
                DiaryxAppleError::Core(format!("Failed to read entry '{}': {e}", abs.display()))
            })?;

        let mut parsed = frontmatter::parse_or_empty(&content)
            .map_err(|e| DiaryxAppleError::Core(format!("Failed to parse frontmatter: {e}")))?;

        frontmatter::remove_property(&mut parsed.frontmatter, &key);

        let new_content = if parsed.frontmatter.is_empty() {
            parsed.body.clone()
        } else {
            frontmatter::serialize(&parsed.frontmatter, &parsed.body).map_err(|e| {
                DiaryxAppleError::Core(format!("Failed to serialize frontmatter: {e}"))
            })?
        };

        std::fs::write(&abs, new_content).map_err(|e| {
            DiaryxAppleError::Core(format!("Failed to write entry '{}': {e}", abs.display()))
        })
    }

    /// Rename an entry file.
    ///
    /// Returns the new workspace-relative path.
    pub fn rename_entry(
        &self,
        path: String,
        new_filename: String,
    ) -> Result<String, DiaryxAppleError> {
        let rel = Self::validated_relative_path(&path)?;
        let abs = self.workspace_root.join(&rel);
        let ws = self.make_workspace()?;

        let new_path = futures_lite::future::block_on(ws.rename_entry(&abs, &new_filename))
            .map_err(|e| DiaryxAppleError::Core(format!("Failed to rename entry: {e}")))?;
        self.to_relative(&new_path)
    }

    /// Delete an entry from the workspace.
    pub fn delete_entry(&self, path: String) -> Result<(), DiaryxAppleError> {
        let rel = Self::validated_relative_path(&path)?;
        let abs = self.workspace_root.join(&rel);
        let ws = self.make_workspace()?;

        futures_lite::future::block_on(ws.delete_entry(&abs))
            .map_err(|e| DiaryxAppleError::Core(format!("Failed to delete entry: {e}")))
    }

    /// Save only the body content for an entry, preserving existing frontmatter.
    ///
    /// Reads the current file and uses `replace_body()` to swap in the new body
    /// while keeping the raw frontmatter block byte-for-byte (no YAML
    /// re-serialization).
    pub fn save_entry_body(&self, id: String, body: String) -> Result<(), DiaryxAppleError> {
        let rel = Self::validated_relative_path(&id)?;
        let full = self.workspace_root.join(&rel);

        let existing =
            futures_lite::future::block_on(self.fs.read_to_string(&full)).map_err(|e| {
                DiaryxAppleError::Core(format!("Failed to read entry '{}': {e}", full.display()))
            })?;

        let content = frontmatter::replace_body(&existing, &body);

        std::fs::write(&full, content).map_err(|e| {
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

/// Create a new workspace directory and return a handle to it.
///
/// Creates all parent directories as needed. Returns an error if the path
/// already exists and is not a directory.
#[uniffi::export]
pub fn create_workspace(
    workspace_path: String,
) -> Result<Arc<DiaryxAppleWorkspace>, DiaryxAppleError> {
    let root = PathBuf::from(&workspace_path);

    if root.exists() {
        if !root.is_dir() {
            return Err(DiaryxAppleError::WorkspaceNotDirectory(workspace_path));
        }
        // Already exists as a directory — just open it
        return DiaryxAppleWorkspace::new(workspace_path);
    }

    std::fs::create_dir_all(&root).map_err(|e| {
        DiaryxAppleError::Core(format!(
            "Failed to create workspace '{}': {e}",
            root.display()
        ))
    })?;

    DiaryxAppleWorkspace::new(workspace_path)
}

impl DiaryxAppleWorkspace {
    /// Build a configured `Workspace` with link_format awareness.
    fn make_workspace(&self) -> Result<Ws, DiaryxAppleError> {
        let ws = Workspace::new(self.fs.clone());
        let root_index =
            futures_lite::future::block_on(ws.find_root_index_in_dir(&self.workspace_root))
                .ok()
                .flatten();

        if let Some(root_path) = root_index {
            if let Ok(config) = futures_lite::future::block_on(ws.get_workspace_config(&root_path))
            {
                return Ok(Workspace::with_link_format(
                    self.fs.clone(),
                    self.workspace_root.clone(),
                    config.link_format,
                ));
            }
        }
        Ok(ws)
    }

    /// Convert a path string from core (which may be absolute) to workspace-relative.
    fn core_path_to_relative(&self, path_str: &str) -> Result<String, DiaryxAppleError> {
        let p = Path::new(path_str);
        if p.is_absolute() {
            self.to_relative(p)
        } else {
            Ok(path_str.replace('\\', "/"))
        }
    }

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

    // ---- Integration tests: full save round-trip ----

    /// Helper: create a workspace with a single file, return (workspace, relative_path).
    fn setup_workspace(filename: &str, content: &str) -> (Arc<DiaryxAppleWorkspace>, String) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(filename);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file_path, content).unwrap();

        let ws = DiaryxAppleWorkspace::new(dir.path().to_string_lossy().into_owned()).unwrap();
        // Leak the TempDir so it isn't deleted while the workspace is in use
        std::mem::forget(dir);
        (ws, filename.to_string())
    }

    #[test]
    fn test_save_body_roundtrip_no_frontmatter() {
        let (ws, path) = setup_workspace("note.md", "Original body");

        // Read: body should be the full content (no frontmatter)
        let entry = ws.get_entry(path.clone()).unwrap();
        assert_eq!(entry.body, "Original body");
        assert!(entry.metadata.is_empty());

        // Save new body
        ws.save_entry_body(path.clone(), "Updated body".to_string())
            .unwrap();

        // Read back: body should reflect the save
        let entry2 = ws.get_entry(path).unwrap();
        assert_eq!(entry2.body, "Updated body");
        assert!(entry2.metadata.is_empty());
    }

    #[test]
    fn test_save_body_preserves_frontmatter() {
        let content = "---\ntitle: My Note\ntags:\n  - rust\n  - swift\n---\nOriginal body";
        let (ws, path) = setup_workspace("note.md", content);

        // Read: frontmatter should be parsed
        let entry = ws.get_entry(path.clone()).unwrap();
        assert_eq!(entry.body, "Original body");
        assert!(
            entry
                .metadata
                .iter()
                .any(|m| m.key == "title" && m.value == "My Note")
        );
        assert!(
            entry
                .metadata
                .iter()
                .any(|m| m.key == "tags" && m.values == vec!["rust", "swift"])
        );

        // Save new body
        ws.save_entry_body(path.clone(), "New body content".to_string())
            .unwrap();

        // Read back: body should be updated, frontmatter preserved
        let entry2 = ws.get_entry(path).unwrap();
        assert_eq!(entry2.body, "New body content");
        assert!(
            entry2
                .metadata
                .iter()
                .any(|m| m.key == "title" && m.value == "My Note")
        );
        assert!(
            entry2
                .metadata
                .iter()
                .any(|m| m.key == "tags" && m.values == vec!["rust", "swift"])
        );
    }

    #[test]
    fn test_save_body_multiple_edits() {
        let content = "---\ntitle: Journal\n---\nFirst version";
        let (ws, path) = setup_workspace("journal.md", content);

        // Simulate multiple edits (like a user typing then saving repeatedly)
        ws.save_entry_body(path.clone(), "Second version".to_string())
            .unwrap();
        ws.save_entry_body(path.clone(), "Third version".to_string())
            .unwrap();
        ws.save_entry_body(path.clone(), "Final version".to_string())
            .unwrap();

        let entry = ws.get_entry(path).unwrap();
        assert_eq!(entry.body, "Final version");
        assert!(
            entry
                .metadata
                .iter()
                .any(|m| m.key == "title" && m.value == "Journal")
        );
    }

    #[test]
    fn test_create_then_save_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let ws = DiaryxAppleWorkspace::new(dir.path().to_string_lossy().into_owned()).unwrap();
        std::mem::forget(dir);

        // Create a new entry
        ws.create_entry(
            "2026/02/16.md".to_string(),
            "---\ntitle: Today\n---\n".to_string(),
        )
        .unwrap();

        // Read it back
        let entry = ws.get_entry("2026/02/16.md".to_string()).unwrap();
        assert_eq!(entry.body, "");
        assert!(
            entry
                .metadata
                .iter()
                .any(|m| m.key == "title" && m.value == "Today")
        );

        // Save body content
        ws.save_entry_body(
            "2026/02/16.md".to_string(),
            "Had a great day writing Rust.".to_string(),
        )
        .unwrap();

        // Read back: body updated, frontmatter preserved
        let entry2 = ws.get_entry("2026/02/16.md".to_string()).unwrap();
        assert_eq!(entry2.body, "Had a great day writing Rust.");
        assert!(
            entry2
                .metadata
                .iter()
                .any(|m| m.key == "title" && m.value == "Today")
        );
    }

    #[test]
    fn test_save_body_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();
        let content = "---\ntitle: Disk Test\n---\nBefore save";
        std::fs::write(dir_path.join("test.md"), content).unwrap();

        let ws = DiaryxAppleWorkspace::new(dir_path.to_string_lossy().into_owned()).unwrap();

        ws.save_entry_body("test.md".to_string(), "After save".to_string())
            .unwrap();

        // Verify the actual file on disk (bypass the API)
        let on_disk = std::fs::read_to_string(dir_path.join("test.md")).unwrap();
        assert!(
            on_disk.contains("title: Disk Test"),
            "Frontmatter should be preserved on disk: {on_disk}"
        );
        assert!(
            on_disk.contains("After save"),
            "New body should be on disk: {on_disk}"
        );
        assert!(
            !on_disk.contains("Before save"),
            "Old body should be gone from disk: {on_disk}"
        );
    }

    #[test]
    fn test_switch_files_saves_correct_content() {
        // Simulates the file-switch flow: edit file A, then load file B.
        // Verifies that saving A's content doesn't corrupt B.
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();
        std::fs::write(dir_path.join("a.md"), "---\ntitle: A\n---\nBody A").unwrap();
        std::fs::write(dir_path.join("b.md"), "---\ntitle: B\n---\nBody B").unwrap();

        let ws = DiaryxAppleWorkspace::new(dir_path.to_string_lossy().into_owned()).unwrap();

        // Step 1: Read file A
        let a1 = ws.get_entry("a.md".to_string()).unwrap();
        assert_eq!(a1.body, "Body A");

        // Step 2: User edits A's body (simulated)
        let edited_a = "Edited Body A";

        // Step 3: Before switching to B, save A with the CORRECT path and content
        ws.save_entry_body("a.md".to_string(), edited_a.to_string())
            .unwrap();

        // Step 4: Load file B
        let b = ws.get_entry("b.md".to_string()).unwrap();
        assert_eq!(b.body, "Body B"); // B should be untouched

        // Step 5: Switch back to A — should see our edit
        let a2 = ws.get_entry("a.md".to_string()).unwrap();
        assert_eq!(a2.body, "Edited Body A");

        // Verify B is still intact on disk
        let b_disk = std::fs::read_to_string(dir_path.join("b.md")).unwrap();
        assert!(b_disk.contains("Body B"), "B should be untouched: {b_disk}");
    }

    #[test]
    fn test_save_body_preserves_yaml_formatting_exactly() {
        // Original file with specific YAML formatting (quoted strings, indented arrays)
        let original =
            "---\ntitle: \"Quoted Title\"\ntags:\n  - rust\n  - swift\n---\n\nOriginal body";
        let (ws, path) = setup_workspace("formatted.md", original);

        ws.save_entry_body(path.clone(), "New body".to_string())
            .unwrap();

        // Read raw file from disk — frontmatter must be preserved byte-for-byte
        let on_disk =
            std::fs::read_to_string(PathBuf::from(ws.workspace_root()).join(&path)).unwrap();

        assert!(
            on_disk.starts_with("---\ntitle: \"Quoted Title\"\ntags:\n  - rust\n  - swift\n---\n"),
            "YAML formatting should be preserved exactly: {on_disk}"
        );
        assert!(
            on_disk.ends_with("\nNew body"),
            "Body should be updated: {on_disk}"
        );
    }

    // ---- Hierarchy manipulation tests ----

    /// Helper: create a workspace with a root index that has `contents` linking to a child.
    fn setup_hierarchy_workspace() -> (Arc<DiaryxAppleWorkspace>, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        // Root index file with contents referencing a child
        std::fs::write(
            dir_path.join("root.md"),
            "---\ntitle: Root\ncontents:\n  - '[Child](/child.md)'\n---\nRoot body\n",
        )
        .unwrap();

        // Child leaf file with part_of back to root
        std::fs::write(
            dir_path.join("child.md"),
            "---\ntitle: Child\npart_of: '[Root](/root.md)'\n---\nChild body\n",
        )
        .unwrap();

        let ws = DiaryxAppleWorkspace::new(dir_path.to_string_lossy().into_owned()).unwrap();
        std::mem::forget(dir);
        (ws, dir_path)
    }

    #[test]
    fn test_create_child_under_index() {
        let (ws, dir_path) = setup_hierarchy_workspace();

        let result = ws
            .create_child_entry("root.md".to_string(), Some("New Child".to_string()))
            .unwrap();

        assert!(!result.parent_converted, "Root is already an index");
        assert!(!result.child_path.is_empty());

        // Child file should exist on disk
        let child_abs = dir_path.join(&result.child_path);
        assert!(
            child_abs.exists(),
            "Child file should exist: {}",
            result.child_path
        );

        // Child should have title frontmatter (may be YAML-quoted)
        let content = std::fs::read_to_string(&child_abs).unwrap();
        assert!(
            content.contains("New Child"),
            "Child should have title: {content}"
        );
    }

    #[test]
    fn test_create_child_converts_leaf() {
        let (ws, dir_path) = setup_hierarchy_workspace();

        // Create a child under the leaf "child.md" — should convert it to an index
        let result = ws
            .create_child_entry("child.md".to_string(), Some("Grandchild".to_string()))
            .unwrap();

        assert!(
            result.parent_converted,
            "Leaf parent should be converted to index"
        );
        assert!(result.original_parent_path.is_some());

        // The parent path should now be inside a directory
        let parent_abs = dir_path.join(&result.parent_path);
        assert!(
            parent_abs.exists(),
            "Converted parent should exist: {}",
            result.parent_path
        );

        // Grandchild should exist
        let child_abs = dir_path.join(&result.child_path);
        assert!(
            child_abs.exists(),
            "Grandchild should exist: {}",
            result.child_path
        );
    }

    #[test]
    fn test_delete_entry() {
        let (ws, dir_path) = setup_hierarchy_workspace();

        // Delete the child leaf
        ws.delete_entry("child.md".to_string()).unwrap();
        assert!(
            !dir_path.join("child.md").exists(),
            "child.md should be deleted"
        );

        // Verify parent's contents no longer references the deleted child
        let root_content = std::fs::read_to_string(dir_path.join("root.md")).unwrap();
        assert!(
            !root_content.contains("child"),
            "child should be removed from root's contents: {root_content}"
        );
    }

    #[test]
    fn test_delete_index_cleans_parent_contents() {
        let (ws, dir_path) = setup_hierarchy_workspace();

        // Convert child.md to index → child/child.md
        let index_path = ws.convert_to_index("child.md".to_string()).unwrap();
        assert!(dir_path.join(&index_path).exists());

        // Root should reference the new index path
        let root_before = std::fs::read_to_string(dir_path.join("root.md")).unwrap();
        assert!(
            root_before.contains("child/child.md"),
            "Root should reference index: {root_before}"
        );

        // Delete the index (it has empty contents)
        ws.delete_entry(index_path.clone()).unwrap();
        assert!(!dir_path.join(&index_path).exists());

        // Root should no longer reference it
        let root_after = std::fs::read_to_string(dir_path.join("root.md")).unwrap();
        assert!(
            !root_after.contains("child"),
            "Deleted index should be removed from root's contents: {root_after}"
        );
    }

    #[test]
    fn test_rename_entry() {
        let (ws, dir_path) = setup_hierarchy_workspace();

        let new_path = ws
            .rename_entry("child.md".to_string(), "renamed.md".to_string())
            .unwrap();

        assert!(
            !dir_path.join("child.md").exists(),
            "Old file should be gone"
        );
        assert!(
            dir_path.join(&new_path).exists(),
            "Renamed file should exist at: {new_path}"
        );
    }

    #[test]
    fn test_convert_to_index_and_back() {
        let (ws, dir_path) = setup_hierarchy_workspace();

        // Convert child.md to index
        let index_path = ws.convert_to_index("child.md".to_string()).unwrap();
        assert!(
            dir_path.join(&index_path).exists(),
            "Index file should exist at: {index_path}"
        );
        assert!(
            !dir_path.join("child.md").exists(),
            "Original leaf should be gone"
        );

        // Convert back to leaf
        let leaf_path = ws.convert_to_leaf(index_path.clone()).unwrap();
        assert!(
            dir_path.join(&leaf_path).exists(),
            "Leaf file should exist at: {leaf_path}"
        );
        assert!(
            !dir_path.join(&index_path).exists(),
            "Index file should be gone"
        );
    }

    #[test]
    fn test_move_entry() {
        let (ws, dir_path) = setup_hierarchy_workspace();

        // Create a subdirectory and move child into it
        std::fs::create_dir_all(dir_path.join("subdir")).unwrap();
        ws.move_entry("child.md".to_string(), "subdir/child.md".to_string())
            .unwrap();

        assert!(
            !dir_path.join("child.md").exists(),
            "Original should be gone"
        );
        assert!(
            dir_path.join("subdir/child.md").exists(),
            "Moved file should exist"
        );
    }

    #[test]
    fn test_set_frontmatter_property() {
        let (ws, _dir_path) = setup_hierarchy_workspace();

        ws.set_frontmatter_property(
            "child.md".to_string(),
            "draft".to_string(),
            FrontmatterValue::Bool { value: true },
        )
        .unwrap();

        let entry = ws.get_entry("child.md".to_string()).unwrap();
        assert!(
            entry
                .metadata
                .iter()
                .any(|m| m.key == "draft" && m.value == "true"),
            "draft property should be set"
        );
    }

    #[test]
    fn test_remove_frontmatter_property() {
        let (ws, _dir_path) = setup_hierarchy_workspace();

        // "title" exists in child.md — remove it
        ws.remove_frontmatter_property("child.md".to_string(), "title".to_string())
            .unwrap();

        let entry = ws.get_entry("child.md".to_string()).unwrap();
        assert!(
            !entry.metadata.iter().any(|m| m.key == "title"),
            "title property should be removed"
        );
    }

    #[test]
    fn test_attach_and_move_to_parent() {
        let (ws, dir_path) = setup_hierarchy_workspace();

        // Create a second index to reparent under
        std::fs::create_dir_all(dir_path.join("other")).unwrap();
        std::fs::write(
            dir_path.join("other/other.md"),
            "---\ntitle: Other\npart_of: '[Root](/root.md)'\n---\nOther body\n",
        )
        .unwrap();

        // Also add it to root's contents so it's a valid index
        let root_content = std::fs::read_to_string(dir_path.join("root.md")).unwrap();
        let updated = root_content.replace(
            "  - '[Child](/child.md)'",
            "  - '[Child](/child.md)'\n  - '[Other](/other/other.md)'",
        );
        std::fs::write(dir_path.join("root.md"), updated).unwrap();

        let new_path = ws
            .attach_and_move_entry_to_parent("child.md".to_string(), "other/other.md".to_string())
            .unwrap();

        assert!(
            !dir_path.join("child.md").exists(),
            "Original should be gone"
        );
        assert!(
            dir_path.join(&new_path).exists(),
            "Reparented file should exist at: {new_path}"
        );
    }

    #[test]
    fn test_drop_file_onto_leaf_no_duplicate_contents() {
        // Simulates dragging new-entry-1.md onto new-entry.md (a leaf).
        // The leaf is auto-converted to an index, and the dragged file
        // should appear exactly once in contents with a formatted link.
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        // Root index
        std::fs::write(
            dir_path.join("root.md"),
            "---\ntitle: Root\ncontents:\n  - '[Entry A](/entry-a.md)'\n  - '[Entry B](/entry-b.md)'\n---\n",
        )
        .unwrap();

        // Two leaf files
        std::fs::write(
            dir_path.join("entry-a.md"),
            "---\ntitle: Entry A\npart_of: '[Root](/root.md)'\n---\nBody A\n",
        )
        .unwrap();
        std::fs::write(
            dir_path.join("entry-b.md"),
            "---\ntitle: Entry B\npart_of: '[Root](/root.md)'\n---\nBody B\n",
        )
        .unwrap();

        let ws = DiaryxAppleWorkspace::new(dir_path.to_string_lossy().into_owned()).unwrap();
        std::mem::forget(dir);

        // Drop entry-b onto entry-a (leaf → auto-converts to index)
        let new_path = ws
            .attach_and_move_entry_to_parent("entry-b.md".to_string(), "entry-a.md".to_string())
            .unwrap();

        // entry-b should now be inside entry-a's directory
        assert!(
            dir_path.join(&new_path).exists(),
            "Moved file should exist at: {new_path}"
        );
        assert!(
            !dir_path.join("entry-b.md").exists(),
            "Original entry-b.md should be gone"
        );

        // entry-a should be converted to index at entry-a/entry-a.md
        let index_path = dir_path.join("entry-a/entry-a.md");
        assert!(index_path.exists(), "entry-a should be converted to index");

        // Read the index contents — entry-b should appear exactly ONCE
        let index_content = std::fs::read_to_string(&index_path).unwrap();
        let count = index_content.matches("entry-b").count();
        assert_eq!(
            count, 1,
            "entry-b should appear exactly once in contents, found {count} times:\n{index_content}"
        );

        // The entry should be a formatted markdown link, not a plain filename
        assert!(
            index_content.contains("[Entry B]"),
            "Contents entry should be a formatted link:\n{index_content}"
        );

        // Check entry-b's part_of is a formatted link, not a plain filename
        let entry_b_content = std::fs::read_to_string(dir_path.join(&new_path)).unwrap();
        assert!(
            entry_b_content.contains("[Entry A]"),
            "part_of should be a formatted link, not plain filename:\n{entry_b_content}"
        );
    }
}

uniffi::setup_scaffolding!();
