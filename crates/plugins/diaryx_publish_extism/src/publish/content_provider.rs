//! Content provider abstraction for publishing.
//!
//! The [`ContentProvider`] trait decouples the publishing pipeline from its
//! content source. Implementations can read from the local filesystem, from
//! CRDT state, or from any other storage backend.
//!
//! Two implementations:
//! - `FilesystemContentProvider` (in this module's sibling `fs_content_provider`) — reads local files
//! - `CrdtContentProvider` — reads CRDT state (server-side; lives with the server adapter that owns the CRDT storage)

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
///
/// On native targets, implementors must be `Send + Sync`. On WASM, these
/// bounds are relaxed since everything is single-threaded.
#[cfg(not(target_arch = "wasm32"))]
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

/// WASM variant without `Send + Sync` bounds (single-threaded environment).
#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
pub trait ContentProvider {
    /// Materialize all publishable files from the workspace.
    async fn materialize_files(&self, workspace_id: &str) -> Result<Vec<MaterializedFile>, String>;

    /// Get a mapping of attachment references to their storage locations.
    async fn get_attachment_map(
        &self,
        workspace_id: &str,
    ) -> Result<HashMap<String, (String, String)>, String>;
}
