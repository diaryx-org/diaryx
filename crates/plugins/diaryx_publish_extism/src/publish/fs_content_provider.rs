//! Filesystem-backed content provider for publishing.
//!
//! Reads workspace files directly from the local filesystem,
//! enabling publishing without any sync/CRDT dependency.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::content_provider::{ContentProvider, MaterializedFile};
use async_trait::async_trait;
use diaryx_core::fs::AsyncFileSystem;
use diaryx_core::workspace::Workspace;

/// Content provider that reads files from a local filesystem.
///
/// This enables publishing from local workspace directories without
/// requiring CRDT sync or any server connection.
pub struct FilesystemContentProvider<FS: AsyncFileSystem> {
    fs: FS,
    workspace_root: PathBuf,
}

impl<FS: AsyncFileSystem + Clone> FilesystemContentProvider<FS> {
    /// Create a new filesystem content provider.
    ///
    /// `workspace_root` is the path to the workspace root index file
    /// (e.g., `path/to/workspace/README.md`).
    pub fn new(fs: FS, workspace_root: PathBuf) -> Self {
        Self { fs, workspace_root }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl<FS: AsyncFileSystem + Clone> ContentProvider for FilesystemContentProvider<FS> {
    async fn materialize_files(
        &self,
        _workspace_id: &str,
    ) -> Result<Vec<MaterializedFile>, String> {
        let workspace = Workspace::new(self.fs.clone());
        let workspace_dir = self.workspace_root.parent().unwrap_or(Path::new("."));

        let tree = workspace
            .build_tree(&self.workspace_root)
            .await
            .map_err(|e| format!("failed to build workspace tree: {}", e))?;

        let mut files = Vec::new();
        collect_tree_files(&self.fs, workspace_dir, &tree, &mut files).await;
        Ok(files)
    }

    async fn get_attachment_map(
        &self,
        _workspace_id: &str,
    ) -> Result<HashMap<String, (String, String)>, String> {
        // Local filesystem provider doesn't have a content-addressed attachment map.
        // Attachments are referenced by relative path and resolved at publish time.
        Ok(HashMap::new())
    }
}

/// Recursively collect files from a workspace tree node.
async fn collect_tree_files<FS: AsyncFileSystem>(
    fs: &FS,
    workspace_dir: &Path,
    node: &diaryx_core::workspace::TreeNode,
    files: &mut Vec<MaterializedFile>,
) {
    let full_path = workspace_dir.join(&node.path);
    let path_str = node.path.to_string_lossy().replace('\\', "/");

    if let Ok(content) = fs.read_to_string(&full_path).await {
        let frontmatter = match diaryx_core::frontmatter::parse_or_empty(&content) {
            Ok(parsed) => parsed
                .frontmatter
                .into_iter()
                .map(|(k, v)| (k, serde_json::Value::from(v)))
                .collect(),
            Err(_) => indexmap::IndexMap::new(),
        };

        files.push(MaterializedFile {
            path: path_str,
            content,
            frontmatter,
        });
    }

    for child in &node.children {
        Box::pin(collect_tree_files(fs, workspace_dir, child, files)).await;
    }
}
