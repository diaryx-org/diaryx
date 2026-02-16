#![warn(missing_docs)]

//! UniFFI-friendly API for Apple clients.
//!
//! This crate provides a small, stable surface area that can be bound to Swift
//! while `apps/apple` is incrementally migrated to `diaryx_core`.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use diaryx_core::frontmatter;
use diaryx_core::fs::{AsyncFileSystem, RealFileSystem, SyncToAsyncFs};
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

/// Entry payload returned by `get_entry`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct EntryData {
    /// Stable ID for the entry (currently the workspace-relative path).
    pub id: String,
    /// Workspace-relative path to the markdown file.
    pub path: String,
    /// Raw markdown content (including frontmatter).
    pub markdown: String,
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

        Ok(EntryData {
            id: id.clone(),
            path: id,
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

uniffi::setup_scaffolding!();
