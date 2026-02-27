//! Filesystem-backed content provider for publishing.
//!
//! Reads workspace files directly from the local filesystem,
//! enabling publishing without any sync/CRDT dependency.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use diaryx_core::fs::AsyncFileSystem;
use diaryx_core::publish::content_provider::{ContentProvider, MaterializedFile};
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

#[async_trait]
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
                .map(|(k, v)| (k, yaml_to_json(v)))
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

/// Convert serde_yaml::Value to serde_json::Value.
fn yaml_to_json(yaml: serde_yaml::Value) -> serde_json::Value {
    match yaml {
        serde_yaml::Value::Null => serde_json::Value::Null,
        serde_yaml::Value::Bool(b) => serde_json::Value::Bool(b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                serde_json::Value::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::json!(f)
            } else {
                serde_json::Value::Null
            }
        }
        serde_yaml::Value::String(s) => serde_json::Value::String(s),
        serde_yaml::Value::Sequence(arr) => {
            serde_json::Value::Array(arr.into_iter().map(yaml_to_json).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .into_iter()
                .filter_map(|(k, v)| k.as_str().map(|s| (s.to_string(), yaml_to_json(v))))
                .collect();
            serde_json::Value::Object(obj)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_to_json(tagged.value),
    }
}
