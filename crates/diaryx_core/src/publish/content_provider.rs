//! Content provider abstraction for publishing.
//!
//! The [`ContentProvider`] trait decouples the publishing pipeline from its
//! content source. Implementations can read from the local filesystem, from
//! CRDT state, or from any other storage backend.
//!
//! Two planned implementations:
//! - `FilesystemContentProvider` (in `diaryx_publish`) — reads local files
//! - `CrdtContentProvider` (in `diaryx_sync`, feature "server") — reads CRDT state

use std::collections::HashMap;

use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;

/// A single file materialized for publishing.
#[derive(Debug, Clone)]
pub struct MaterializedFile {
    /// Workspace-relative path (e.g., `"notes/hello.md"`).
    pub path: String,
    /// Full file content (frontmatter + body).
    pub content: String,
    /// Parsed frontmatter as JSON key-value pairs.
    pub frontmatter: IndexMap<String, JsonValue>,
}

/// Abstraction over the source of content for publishing.
///
/// This trait breaks the dependency between publishing and sync — a publisher
/// can work with any content source that implements this trait.
#[async_trait]
pub trait ContentProvider: Send + Sync {
    /// Materialize all publishable files from the workspace.
    async fn materialize_files(&self, workspace_id: &str) -> Result<Vec<MaterializedFile>, String>;

    /// Get a mapping of attachment references to their storage locations.
    ///
    /// Returns `(storage_key, mime_type)` keyed by the workspace-relative
    /// attachment path.
    async fn get_attachment_map(
        &self,
        workspace_id: &str,
    ) -> Result<HashMap<String, (String, String)>, String>;
}
