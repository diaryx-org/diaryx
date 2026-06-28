//! Workspace tree building, filesystem traversal, and tree formatting.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::fs::AsyncFileSystem;
use crate::link_parser::LinkFormat;
use crate::utils::is_workspace_skip_dir;
use crate::yaml;

use super::*;

impl<FS: AsyncFileSystem> Workspace<FS> {
    /// Build a tree structure from the workspace hierarchy
    pub async fn build_tree(&self, root_path: &Path) -> Result<TreeNode> {
        self.build_tree_with_depth(root_path, None, &mut HashSet::new())
            .await
    }

    /// Build a tree structure with depth limit and cycle detection
    /// `max_depth` of None means unlimited, Some(0) means just the root node
    pub async fn build_tree_with_depth(
        &self,
        root_path: &Path,
        max_depth: Option<usize>,
        visited: &mut HashSet<PathBuf>,
    ) -> Result<TreeNode> {
        // Get link format from workspace config for proper path resolution
        let link_format = self
            .get_workspace_config(root_path)
            .await
            .map(|c| c.link_format)
            .ok();

        // Get the actual workspace root directory.
        // IMPORTANT: We must use self.root_path (the configured workspace root) rather than
        // deriving from root_path. When loading children for nested paths like
        // ./new-entry/new-entry-1/new-entry-1.md, workspace-root paths in contents
        // (like /new-entry/new-entry-1/new-entry.md) need to be resolved relative to
        // the ACTUAL workspace root, not the parent of the current file.
        let workspace_root = self
            .root_path
            .clone()
            .unwrap_or_else(|| root_path.parent().unwrap_or(Path::new(".")).to_path_buf());

        self.build_tree_with_depth_and_format(
            root_path,
            max_depth,
            visited,
            link_format,
            &workspace_root,
        )
        .await
    }

    /// Build a tree structure with depth limit, cycle detection, and explicit link format.
    ///
    /// This is the internal implementation that handles tree building.
    /// Use `build_tree_with_depth` for public API which auto-detects the link format.
    pub(crate) async fn build_tree_with_depth_and_format(
        &self,
        root_path: &Path,
        max_depth: Option<usize>,
        visited: &mut HashSet<PathBuf>,
        link_format: Option<LinkFormat>,
        workspace_root: &Path,
    ) -> Result<TreeNode> {
        let index = self.parse_index_with_hint(root_path, link_format).await?;

        // Canonicalize path for cycle detection
        let canonical = root_path
            .canonicalize()
            .unwrap_or_else(|_| root_path.to_path_buf());

        // Check for cycles
        if visited.contains(&canonical) {
            return Ok(TreeNode {
                name: format!(
                    "{} (cycle)",
                    root_path.file_name().unwrap_or_default().to_string_lossy()
                ),
                description: None,
                path: root_path.to_path_buf(),
                is_index: false,
                children: Vec::new(),
                properties: std::collections::HashMap::new(),
                audience: Vec::new(),
            });
        }
        visited.insert(canonical);

        let name = index
            .frontmatter
            .display_name()
            .map(String::from)
            .unwrap_or_else(|| {
                // Fall back to filename without extension
                root_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(String::from)
                    .unwrap_or_else(|| root_path.display().to_string())
            });

        let mut children = Vec::new();
        let contents = index.frontmatter.contents_list();
        let child_count = contents.len();
        let mut seen_child_paths = HashSet::new();

        log::debug!(
            "[build_tree] Processing {}: contents={:?}, workspace_root={:?}",
            root_path.display(),
            contents,
            workspace_root
        );

        // Check if we've hit depth limit
        let at_depth_limit = max_depth.map(|d| d == 0).unwrap_or(false);

        if at_depth_limit && child_count > 0 {
            // Show truncation indicator
            children.push(TreeNode {
                name: format!("... ({} more)", child_count),
                description: None,
                path: root_path.to_path_buf(),
                is_index: false,
                children: Vec::new(),
                properties: std::collections::HashMap::new(),
                audience: Vec::new(),
            });
        } else {
            let next_depth = max_depth.map(|d| d.saturating_sub(1));

            for child_path_str in contents {
                let child_path = index.resolve_path(child_path_str);

                // Make path absolute if needed by joining with workspace root
                let absolute_child_path = if child_path.is_absolute() {
                    child_path.clone()
                } else {
                    workspace_root.join(&child_path)
                };

                let exists = self
                    .fs
                    .try_exists(&absolute_child_path)
                    .await
                    .unwrap_or(false);
                log::debug!(
                    "[build_tree] Child '{}' resolved to {:?}, exists={}",
                    child_path_str,
                    absolute_child_path,
                    exists
                );

                // Guard against duplicate entries in `contents` that resolve to
                // the same path. Duplicates can crash keyed UI rendering and do
                // not add useful information to the tree.
                if !seen_child_paths.insert(absolute_child_path.clone()) {
                    log::debug!(
                        "[build_tree] Skipping duplicate child path: {:?}",
                        absolute_child_path
                    );
                    continue;
                }

                // Only include if the file exists
                if exists {
                    match Box::pin(self.build_tree_with_depth_and_format(
                        &absolute_child_path,
                        next_depth,
                        visited,
                        link_format,
                        workspace_root,
                    ))
                    .await
                    {
                        Ok(child_node) => children.push(child_node),
                        Err(_) => {
                            // If we can't parse a child, include it as a leaf with error indication
                            children.push(TreeNode {
                                name: format!("{} (error)", child_path_str),
                                description: None,
                                path: absolute_child_path,
                                is_index: false,
                                children: Vec::new(),
                                properties: std::collections::HashMap::new(),
                                audience: Vec::new(),
                            });
                        }
                    }
                }
                // Ignore non-existent paths (as per spec: "ignore by default")
            }
        }

        let is_index = index.frontmatter.is_index();
        let audience = index.frontmatter.audience.clone().unwrap_or_default();
        Ok(TreeNode {
            name,
            description: index.frontmatter.description,
            path: root_path.to_path_buf(),
            is_index,
            children,
            properties: std::collections::HashMap::new(),
            audience,
        })
    }

    /// Build a tree structure from the actual filesystem (for "Show All Files" mode)
    /// Unlike build_tree, this scans directories for actual files rather than following contents references
    pub async fn build_filesystem_tree(
        &self,
        root_dir: &Path,
        show_hidden: bool,
    ) -> Result<TreeNode> {
        self.build_filesystem_tree_with_depth(root_dir, show_hidden, None)
            .await
    }

    /// Build a filesystem tree with optional depth limiting for lazy loading.
    /// Reads `exclude` patterns from the root directory's index file and skips
    /// matching entries during traversal.
    pub async fn build_filesystem_tree_with_depth(
        &self,
        root_dir: &Path,
        show_hidden: bool,
        max_depth: Option<usize>,
    ) -> Result<TreeNode> {
        let mut parse_cache: HashMap<PathBuf, Option<IndexFile>> = HashMap::new();
        let exclude_patterns = self
            .exclude_patterns_for_dir_cached(root_dir, root_dir, &mut parse_cache)
            .await;
        let mut trace = FilesystemTreeTrace::default();
        let tree = self
            .build_filesystem_tree_recursive(
                root_dir,
                root_dir,
                show_hidden,
                max_depth,
                &exclude_patterns,
                &mut trace,
                &mut parse_cache,
            )
            .await?;

        log::info!(
            "[Workspace] Filesystem tree explored {} directories, pruned {} excluded dirs and {} built-in skip dirs",
            trace.explored_dirs.len(),
            trace.pruned_excluded_dirs.len(),
            trace.pruned_skip_dirs.len(),
        );
        log::debug!(
            "[Workspace] Filesystem tree explored directories: {:?}",
            trace.explored_dirs
        );
        log::debug!(
            "[Workspace] Filesystem tree pruned excluded directories: {:?}",
            trace.pruned_excluded_dirs
        );
        log::debug!(
            "[Workspace] Filesystem tree pruned built-in skip directories: {:?}",
            trace.pruned_skip_dirs
        );

        Ok(tree)
    }

    /// Walk the workspace under `root_dir` collecting every entry's frontmatter
    /// `id` (its ARK file blade) into a set, for local mint rejection-checking.
    ///
    /// Reads each markdown file's frontmatter, so it is O(files in workspace).
    /// That is acceptable at current (alpha) workspace sizes; a cached
    /// per-session blade set is the known performance follow-up. Hidden files,
    /// temp files, and built-in skip directories are pruned to match the rest
    /// of the workspace traversal.
    pub async fn collect_file_blades(&self, root_dir: &Path) -> HashSet<String> {
        let mut blades = HashSet::new();
        let mut dirs = vec![root_dir.to_path_buf()];
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = self.fs.read_dir(&dir).await.map(|entries| {
                entries
                    .into_iter()
                    .map(|e| e.path().to_path_buf())
                    .collect::<Vec<_>>()
            }) else {
                continue;
            };
            for entry in entries {
                let file_name = entry
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if file_name.starts_with('.') || crate::fs::is_temp_file(&file_name) {
                    continue;
                }
                let is_dir = self
                    .fs
                    .metadata(&entry)
                    .await
                    .map(|m| m.is_dir())
                    .unwrap_or(false);
                if is_dir {
                    if !is_workspace_skip_dir(&entry) {
                        dirs.push(entry);
                    }
                } else if entry.extension().and_then(|e| e.to_str()) == Some("md")
                    && let Ok(content) = self.fs.read_to_string(&entry).await
                    && let Ok(parsed) = crate::frontmatter::parse_or_empty(&content)
                    && let Some(id) = crate::frontmatter::get_string(&parsed.frontmatter, "id")
                {
                    blades.insert(id.to_string());
                }
            }
        }
        blades
    }

    #[allow(clippy::too_many_arguments)]
    async fn build_filesystem_tree_recursive(
        &self,
        dir: &Path,
        root_dir: &Path,
        show_hidden: bool,
        max_depth: Option<usize>,
        exclude_patterns: &[String],
        trace: &mut FilesystemTreeTrace,
        parse_cache: &mut HashMap<PathBuf, Option<IndexFile>>,
    ) -> Result<TreeNode> {
        trace.record_explored(dir);
        // Get directory name for display
        let dir_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| dir.to_string_lossy().to_string());

        // Try to find an index file in this directory to get title/description
        let (name, description, index_path) =
            if let Ok(Some(index)) = self.find_any_index_in_dir_cached(dir, parse_cache).await {
                if let Ok(parsed) = self.parse_index_cached(&index, parse_cache).await {
                    let title = parsed.frontmatter.title.unwrap_or_else(|| dir_name.clone());
                    (title, parsed.frontmatter.description, Some(index))
                } else {
                    (dir_name.clone(), None, Some(index))
                }
            } else {
                (dir_name.clone(), None, None)
            };

        // The path to use - if there's an index, use it; otherwise use the directory
        let node_path = index_path.unwrap_or_else(|| dir.to_path_buf());

        // Check if we've hit depth limit
        let at_depth_limit = max_depth.map(|d| d == 0).unwrap_or(false);

        // List all entries in this directory
        let mut children = Vec::new();
        if let Ok(entries) = self.fs.read_dir(dir).await.map(|entries| {
            entries
                .into_iter()
                .map(|e| e.path().to_path_buf())
                .collect::<Vec<_>>()
        }) {
            let mut entries: Vec<_> = entries.into_iter().collect();
            entries.sort(); // Sort alphabetically

            let mut filtered_entries = Vec::new();
            for entry in entries {
                let file_name = entry
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let hidden = !show_hidden && file_name.starts_with('.');
                let temp = crate::fs::is_temp_file(&file_name);
                let excluded = exclude_patterns.iter().any(|pattern| {
                    self.path_matches_exclude(pattern, root_dir, &entry, &file_name)
                });

                if hidden || temp || excluded {
                    if excluded
                        && self
                            .fs
                            .metadata(&entry)
                            .await
                            .map(|m| m.is_dir())
                            .unwrap_or(false)
                    {
                        trace.record_pruned_excluded(&entry);
                    }
                    continue;
                }

                if self
                    .fs
                    .metadata(&entry)
                    .await
                    .map(|m| m.is_dir())
                    .unwrap_or(false)
                    && is_workspace_skip_dir(&entry)
                {
                    trace.record_pruned_skip(&entry);
                    continue;
                }

                filtered_entries.push(entry);
            }

            // If at depth limit, show truncation indicator
            if at_depth_limit && !filtered_entries.is_empty() {
                children.push(TreeNode {
                    name: format!("... ({} more)", filtered_entries.len()),
                    description: None,
                    path: node_path.clone(),
                    is_index: false,
                    children: Vec::new(),
                    properties: std::collections::HashMap::new(),
                    audience: Vec::new(),
                });
            } else {
                let next_depth = max_depth.map(|d| d.saturating_sub(1));

                for entry in filtered_entries {
                    let file_name = entry
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();

                    if self
                        .fs
                        .metadata(&entry)
                        .await
                        .map(|m| m.is_dir())
                        .unwrap_or(false)
                    {
                        let child_exclude_patterns = self
                            .exclude_patterns_for_dir_cached(&entry, root_dir, parse_cache)
                            .await;
                        // Recurse into subdirectory with decremented depth
                        if let Ok(child_tree) = Box::pin(self.build_filesystem_tree_recursive(
                            &entry,
                            root_dir,
                            show_hidden,
                            next_depth,
                            &child_exclude_patterns,
                            trace,
                            parse_cache,
                        ))
                        .await
                        {
                            children.push(child_tree);
                        }
                    } else {
                        // For markdown files, parse once and use for both
                        // index detection and title extraction.
                        if entry.extension().is_some_and(|e| e == "md") {
                            if let Ok(parsed) = self.parse_index_cached(&entry, parse_cache).await {
                                // Skip index files (already represented by parent dir)
                                if parsed.frontmatter.is_index() {
                                    continue;
                                }
                                children.push(TreeNode {
                                    name: parsed.frontmatter.title.unwrap_or(file_name.clone()),
                                    description: parsed.frontmatter.description,
                                    audience: parsed
                                        .frontmatter
                                        .audience
                                        .clone()
                                        .unwrap_or_default(),
                                    path: entry,
                                    is_index: false,
                                    children: Vec::new(),
                                    properties: std::collections::HashMap::new(),
                                });
                            } else {
                                // Non-parseable .md file — show as leaf
                                children.push(TreeNode {
                                    name: file_name.clone(),
                                    description: None,
                                    path: entry,
                                    is_index: false,
                                    children: Vec::new(),
                                    properties: std::collections::HashMap::new(),
                                    audience: Vec::new(),
                                });
                            }
                        } else {
                            // Non-markdown file
                            children.push(TreeNode {
                                name: file_name.clone(),
                                description: None,
                                path: entry,
                                is_index: false,
                                children: Vec::new(),
                                properties: std::collections::HashMap::new(),
                                audience: Vec::new(),
                            });
                        }
                    }
                }
            }
        }

        Ok(TreeNode {
            name,
            description,
            path: node_path,
            is_index: true,
            children,
            properties: std::collections::HashMap::new(),
            audience: Vec::new(),
        })
    }

    /// Format tree for display (like the `tree` command)
    pub fn format_tree(&self, node: &TreeNode, prefix: &str) -> String {
        let mut result = String::new();

        // Add the current node (root has no connector)
        result.push_str(prefix);
        result.push_str(&node.name);

        // Add description if present
        if let Some(ref desc) = node.description {
            result.push_str(" - ");
            result.push_str(desc);
        }

        // Add properties if present (for root node)
        if !node.properties.is_empty() {
            let props: Vec<String> = node
                .properties
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            result.push_str(" [");
            result.push_str(&props.join(", "));
            result.push(']');
        }

        result.push('\n');

        // Add children
        let child_count = node.children.len();
        for (i, child) in node.children.iter().enumerate() {
            let is_last_child = i == child_count - 1;
            let connector = if is_last_child {
                "└── "
            } else {
                "├── "
            };
            let child_prefix = if is_last_child { "    " } else { "│   " };

            result.push_str(prefix);
            result.push_str(connector);
            result.push_str(&format_tree_node(
                child,
                &format!("{}{}", prefix, child_prefix),
            ));
        }

        result
    }

    /// Format tree for display with custom delimiter-separated properties.
    ///
    /// Properties are output as values only (not key=value), in the order specified
    /// by the `properties` list, separated by `delimiter`.
    pub fn format_tree_with_delimiter(
        &self,
        node: &TreeNode,
        prefix: &str,
        properties: &[String],
        delimiter: &str,
    ) -> String {
        let mut result = String::new();

        // Add the current node (root has no connector)
        result.push_str(prefix);

        // Collect property values in order specified
        let prop_values: Vec<&str> = properties
            .iter()
            .filter_map(|key| node.properties.get(key).map(|v| v.as_str()))
            .collect();

        // Join with delimiter
        result.push_str(&prop_values.join(delimiter));
        result.push('\n');

        // Add children
        let child_count = node.children.len();
        for (i, child) in node.children.iter().enumerate() {
            let is_last_child = i == child_count - 1;
            let connector = if is_last_child {
                "└── "
            } else {
                "├── "
            };
            let child_prefix = if is_last_child { "    " } else { "│   " };

            result.push_str(prefix);
            result.push_str(connector);
            result.push_str(&self.format_tree_node_with_delimiter(
                child,
                &format!("{}{}", prefix, child_prefix),
                properties,
                delimiter,
            ));
        }

        result
    }

    /// Format a tree node with custom delimiter-separated properties (recursive helper).
    pub(crate) fn format_tree_node_with_delimiter(
        &self,
        node: &TreeNode,
        prefix: &str,
        properties: &[String],
        delimiter: &str,
    ) -> String {
        let mut result = String::new();

        // Collect property values in order specified
        let prop_values: Vec<&str> = properties
            .iter()
            .filter_map(|key| node.properties.get(key).map(|v| v.as_str()))
            .collect();

        // Join with delimiter
        result.push_str(&prop_values.join(delimiter));
        result.push('\n');

        // Add children
        let child_count = node.children.len();
        for (i, child) in node.children.iter().enumerate() {
            let is_last_child = i == child_count - 1;
            let connector = if is_last_child {
                "└── "
            } else {
                "├── "
            };
            let child_prefix = if is_last_child { "    " } else { "│   " };

            result.push_str(prefix);
            result.push_str(connector);
            result.push_str(&self.format_tree_node_with_delimiter(
                child,
                &format!("{}{}", prefix, child_prefix),
                properties,
                delimiter,
            ));
        }

        result
    }

    /// Get workspace info as formatted string
    pub async fn workspace_info(&self, root_path: &Path) -> Result<String> {
        self.workspace_info_with_depth(root_path, None).await
    }

    /// Get workspace info as formatted string with depth limit
    /// `max_depth` of None means unlimited
    pub async fn workspace_info_with_depth(
        &self,
        root_path: &Path,
        max_depth: Option<usize>,
    ) -> Result<String> {
        let mut visited = HashSet::new();
        let tree = self
            .build_tree_with_depth(root_path, max_depth, &mut visited)
            .await?;
        Ok(self.format_tree(&tree, "").trim_end().to_string())
    }

    /// Get workspace info as formatted string with depth limit and property extraction.
    ///
    /// `max_depth` of None means unlimited.
    /// `properties` is a list of frontmatter property names to extract and display.
    /// `delimiter` is the separator between property values (e.g., " - ").
    ///
    /// Special virtual properties:
    /// - `filename`: The actual file name (e.g., `README.md`)
    /// - `path`: Workspace-relative file path
    pub async fn workspace_info_with_properties(
        &self,
        root_path: &Path,
        max_depth: Option<usize>,
        properties: &[String],
        delimiter: &str,
    ) -> Result<String> {
        let mut visited = HashSet::new();
        let mut tree = self
            .build_tree_with_depth(root_path, max_depth, &mut visited)
            .await?;

        // Determine workspace root (parent directory of root index file)
        let workspace_root = root_path.parent().unwrap_or(Path::new("."));

        // Extract properties from frontmatter for each node
        self.populate_tree_properties(&mut tree, properties, workspace_root)
            .await;

        Ok(self
            .format_tree_with_delimiter(&tree, "", properties, delimiter)
            .trim_end()
            .to_string())
    }

    /// Recursively populate properties for a tree node and its children
    pub(crate) async fn populate_tree_properties(
        &self,
        node: &mut TreeNode,
        properties: &[String],
        workspace_root: &Path,
    ) {
        // Extract properties for this node
        node.properties = self
            .extract_properties(&node.path, properties, workspace_root)
            .await;

        // Recursively process children
        for child in &mut node.children {
            Box::pin(self.populate_tree_properties(child, properties, workspace_root)).await;
        }
    }

    /// Extract specified properties from a file's frontmatter.
    ///
    /// Handles virtual properties:
    /// - `filename`: The actual file name
    /// - `path`: Workspace-relative file path
    pub(crate) async fn extract_properties(
        &self,
        path: &Path,
        property_names: &[String],
        workspace_root: &Path,
    ) -> std::collections::HashMap<String, String> {
        let mut result = std::collections::HashMap::new();

        for name in property_names {
            let value = match name.as_str() {
                // Virtual properties
                "filename" => path.file_name().and_then(|n| n.to_str()).map(String::from),
                "path" => {
                    // Get workspace-relative path
                    let rel_path = path
                        .strip_prefix(workspace_root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    Some(rel_path)
                }

                // Frontmatter properties
                _ => match self.get_frontmatter_property(path, name).await {
                    Ok(Some(yaml::Value::String(s))) => Some(s),
                    Ok(Some(yaml::Value::Int(n))) => Some(n.to_string()),
                    // Format floats via ryu to avoid pulling `core::num::flt2dec`
                    // (~15 KB of WASM) for what is only a display-time path.
                    Ok(Some(yaml::Value::Float(f))) => {
                        let mut buf = ryu::Buffer::new();
                        Some(buf.format(f).to_string())
                    }
                    Ok(Some(yaml::Value::Bool(b))) => Some(b.to_string()),
                    Ok(Some(yaml::Value::Sequence(seq))) => {
                        // Join sequence values with ", "
                        let strings: Vec<String> = seq
                            .iter()
                            .filter_map(|v| match v {
                                yaml::Value::String(s) => Some(s.clone()),
                                yaml::Value::Int(n) => Some(n.to_string()),
                                yaml::Value::Float(f) => {
                                    let mut buf = ryu::Buffer::new();
                                    Some(buf.format(*f).to_string())
                                }
                                yaml::Value::Bool(b) => Some(b.to_string()),
                                _ => None,
                            })
                            .collect();
                        if strings.is_empty() {
                            None
                        } else {
                            Some(strings.join(", "))
                        }
                    }
                    _ => None,
                },
            };

            if let Some(v) = value {
                result.insert(name.clone(), v);
            }
        }

        result
    }
}
