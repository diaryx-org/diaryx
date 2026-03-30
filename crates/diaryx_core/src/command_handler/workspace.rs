//! Workspace operation command handlers.

use std::path::Path;

use crate::command::Response;
use crate::diaryx::Diaryx;
use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    pub(crate) async fn cmd_find_root_index(&self, directory: String) -> Result<Response> {
        let ws = self.workspace().inner();
        match ws.find_root_index_in_dir(Path::new(&directory)).await? {
            Some(path) => Ok(Response::String(path.to_string_lossy().to_string())),
            None => Err(DiaryxError::WorkspaceNotFound(std::path::PathBuf::from(
                &directory,
            ))),
        }
    }

    pub(crate) async fn cmd_get_available_audiences(&self, path: String) -> Result<Response> {
        let resolved = self.resolve_fs_path(&path);
        let ws = self.workspace().inner();
        let link_format = Some(self.link_format());
        let mut audiences = std::collections::HashSet::new();
        let mut visited = std::collections::HashSet::new();
        let workspace_root = resolved.parent().unwrap_or(Path::new(".")).to_path_buf();

        Self::collect_audiences_recursive(
            &ws,
            &resolved,
            &mut audiences,
            &mut visited,
            &workspace_root,
            link_format,
        )
        .await;

        let mut result: Vec<String> = audiences.into_iter().collect();
        result.sort();
        Ok(Response::Strings(result))
    }

    pub(crate) async fn cmd_get_effective_audience(&self, path: String) -> Result<Response> {
        use crate::command::EffectiveAudienceResult;
        use std::collections::HashSet;

        let ws = self.workspace().inner();
        let mut current_path = self.resolve_fs_path(&path);

        let workspace_root = self.workspace_root().unwrap_or_else(|| {
            current_path
                .parent()
                .and_then(|p| p.parent())
                .unwrap_or(Path::new("."))
                .to_path_buf()
        });

        let ws_config = ws.get_workspace_config(&current_path).await.ok();
        let link_format = ws_config.as_ref().map(|c| c.link_format);
        let default_audience = ws_config.as_ref().and_then(|c| c.default_audience.clone());

        // Parse the entry's frontmatter
        let index = ws.parse_index_with_hint(&current_path, link_format).await?;

        // If entry has explicit audience, return it directly
        if let Some(ref audience_tags) = index.frontmatter.audience {
            let can_inherit = index.frontmatter.part_of.is_some();
            return Ok(Response::EffectiveAudience(EffectiveAudienceResult {
                tags: audience_tags.clone(),
                inherited: false,
                source_title: None,
                can_inherit,
                default_audience_applied: false,
            }));
        }

        // No explicit audience — check if entry has a parent
        let part_of: String = match &index.frontmatter.part_of {
            Some(po) => po.clone(),
            None => {
                // Root entry with no audience — apply default_audience if set
                if let Some(ref da) = default_audience {
                    return Ok(Response::EffectiveAudience(EffectiveAudienceResult {
                        tags: vec![da.clone()],
                        inherited: false,
                        source_title: None,
                        can_inherit: false,
                        default_audience_applied: true,
                    }));
                }
                // No default_audience = private
                return Ok(Response::EffectiveAudience(EffectiveAudienceResult {
                    tags: vec![],
                    inherited: false,
                    source_title: None,
                    can_inherit: false,
                    default_audience_applied: false,
                }));
            }
        };

        // Walk up the part_of chain
        let mut visited = HashSet::new();
        visited.insert(current_path.to_string_lossy().to_string());

        let parent_path = index.resolve_path(&part_of);
        current_path = if parent_path.is_absolute() {
            parent_path
        } else {
            workspace_root.join(&parent_path)
        };

        const MAX_DEPTH: usize = 100;
        for _ in 0..MAX_DEPTH {
            let path_str = current_path.to_string_lossy().to_string();
            if visited.contains(&path_str) {
                break;
            }
            visited.insert(path_str);

            if let Ok(ancestor) = ws.parse_index_with_hint(&current_path, link_format).await {
                if let Some(ref ancestor_audience) = ancestor.frontmatter.audience
                    && !ancestor_audience.is_empty()
                {
                    return Ok(Response::EffectiveAudience(EffectiveAudienceResult {
                        tags: ancestor_audience.clone(),
                        inherited: true,
                        source_title: ancestor.frontmatter.title.clone(),
                        can_inherit: true,
                        default_audience_applied: false,
                    }));
                }

                // Move to next ancestor
                if let Some(ref next_part_of) = ancestor.frontmatter.part_of {
                    let next_path = ancestor.resolve_path(next_part_of);
                    current_path = if next_path.is_absolute() {
                        next_path
                    } else {
                        workspace_root.join(&next_path)
                    };
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Exhausted chain with no audience found — apply default_audience if set
        if let Some(ref da) = default_audience {
            Ok(Response::EffectiveAudience(EffectiveAudienceResult {
                tags: vec![da.clone()],
                inherited: false,
                source_title: None,
                can_inherit: true,
                default_audience_applied: true,
            }))
        } else {
            // No default_audience = private
            Ok(Response::EffectiveAudience(EffectiveAudienceResult {
                tags: vec![],
                inherited: false,
                source_title: None,
                can_inherit: true,
                default_audience_applied: false,
            }))
        }
    }

    pub(crate) async fn cmd_get_workspace_tree(
        &self,
        path: Option<String>,
        depth: Option<u32>,
        audience: Option<String>,
    ) -> Result<Response> {
        let root_path = path.unwrap_or_else(|| "workspace/index.md".to_string());
        let resolved_root_path = self.resolve_fs_path(&root_path);
        log::info!(
            "[CommandHandler] GetWorkspaceTree called: path={}, resolved_path={}, depth={:?}, audience={:?}",
            root_path,
            resolved_root_path.display(),
            depth,
            audience
        );
        let tree = self
            .workspace()
            .inner()
            .build_tree_with_depth(
                &resolved_root_path,
                depth.map(|d| d as usize),
                &mut std::collections::HashSet::new(),
            )
            .await?;

        // If an audience filter is specified, prune nodes not visible to that audience
        let tree = if let Some(ref audience) = audience {
            self.filter_tree_by_audience(tree, audience).await
        } else {
            tree
        };

        log::info!(
            "[CommandHandler] GetWorkspaceTree result: name={}, children_count={}",
            tree.name,
            tree.children.len()
        );
        Ok(Response::Tree(tree))
    }

    pub(crate) async fn cmd_get_workspace_file_set(&self, path: String) -> Result<Response> {
        let resolved_root_path = self.resolve_fs_path(&path);
        let files = self
            .workspace()
            .inner()
            .collect_workspace_file_set(&resolved_root_path)
            .await?;
        Ok(Response::Strings(files))
    }

    pub(crate) async fn cmd_get_filesystem_tree(
        &self,
        path: Option<String>,
        show_hidden: bool,
        depth: Option<u32>,
    ) -> Result<Response> {
        let root_path = path.unwrap_or_else(|| "workspace".to_string());
        let tree = self
            .workspace()
            .inner()
            .build_filesystem_tree_with_depth(
                Path::new(&root_path),
                show_hidden,
                depth.map(|d| d as usize),
            )
            .await?;
        Ok(Response::Tree(tree))
    }

    pub(crate) async fn cmd_create_workspace(
        &self,
        path: Option<String>,
        name: Option<String>,
    ) -> Result<Response> {
        let ws_path = path.unwrap_or_else(|| "workspace".to_string());
        let ws_name = name.as_deref();
        let ws = self.workspace().inner();
        let readme_path = ws
            .init_workspace(Path::new(&ws_path), ws_name, None)
            .await?;
        Ok(Response::String(readme_path.to_string_lossy().to_string()))
    }

    pub(crate) async fn cmd_prepare_multi_delete(
        &self,
        paths: Vec<String>,
        tree_path: Option<String>,
    ) -> Result<Response> {
        let root_path = tree_path.unwrap_or_else(|| "workspace/index.md".to_string());
        let resolved_root_path = self.resolve_fs_path(&root_path);
        let tree = self
            .workspace()
            .inner()
            .build_tree_with_depth(
                &resolved_root_path,
                None,
                &mut std::collections::HashSet::new(),
            )
            .await?;
        let path_bufs: Vec<std::path::PathBuf> =
            paths.iter().map(std::path::PathBuf::from).collect();
        let plan = crate::workspace::prepare_delete_plan(&tree, &path_bufs);
        Ok(Response::Strings(
            plan.into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
        ))
    }

    pub(crate) async fn cmd_check_delete_includes_descendants(
        &self,
        paths: Vec<String>,
        tree_path: Option<String>,
    ) -> Result<Response> {
        let root_path = tree_path.unwrap_or_else(|| "workspace/index.md".to_string());
        let resolved_root_path = self.resolve_fs_path(&root_path);
        let tree = self
            .workspace()
            .inner()
            .build_tree_with_depth(
                &resolved_root_path,
                None,
                &mut std::collections::HashSet::new(),
            )
            .await?;
        let path_bufs: Vec<std::path::PathBuf> =
            paths.iter().map(std::path::PathBuf::from).collect();
        let result = crate::workspace::selection_includes_descendants(&tree, &path_bufs);
        Ok(Response::Bool(result))
    }

    pub(crate) async fn cmd_get_available_parent_indexes(
        &self,
        file_path: String,
        workspace_root: String,
    ) -> Result<Response> {
        // Find all index files between the file and the workspace root
        let ws = self.workspace().inner();
        let resolved_file_path = self.resolve_fs_path(&file_path);
        let resolved_workspace_root = self.resolve_fs_path(&workspace_root);
        let file = resolved_file_path.as_path();
        let root_index = resolved_workspace_root.as_path();
        let root_dir = root_index.parent().unwrap_or(root_index);

        let mut parents = Vec::new();

        // Start from the file's directory and walk up to the workspace root
        let file_dir = file.parent().unwrap_or(Path::new("."));
        let mut current = file_dir.to_path_buf();

        loop {
            // Look for index files in this directory
            if let Ok(files) = ws.fs_ref().list_files(&current).await {
                for file_path in files {
                    // Check if it's a markdown file
                    if file_path.extension().is_some_and(|ext| ext == "md")
                        && !ws.fs_ref().is_dir(&file_path).await
                    {
                        // Try to parse and check if it has contents (is an index)
                        if let Ok(index) = ws.parse_index(&file_path).await
                            && index.frontmatter.is_index()
                        {
                            parents.push(file_path.to_string_lossy().to_string());
                        }
                    }
                }
            }

            // Stop if we've reached or passed the workspace root
            if current == root_dir || !current.starts_with(root_dir) {
                break;
            }

            // Go up one level
            match current.parent() {
                Some(parent) if parent != current => {
                    current = parent.to_path_buf();
                }
                _ => break,
            }
        }

        // Always include the workspace root if not already present
        let root_str = root_index.to_string_lossy().to_string();
        if !parents.contains(&root_str) && ws.fs_ref().exists(root_index).await {
            parents.push(root_str);
        }

        // Sort for consistent ordering
        parents.sort();
        Ok(Response::Strings(parents))
    }

    pub(crate) async fn cmd_search_workspace(
        &self,
        pattern: String,
        options: crate::command::SearchOptions,
    ) -> Result<Response> {
        use crate::search::SearchQuery;

        let query = if options.search_frontmatter {
            if let Some(prop) = options.property {
                SearchQuery::property(&pattern, prop)
            } else {
                SearchQuery::frontmatter(&pattern)
            }
        } else {
            SearchQuery::content(&pattern)
        }
        .case_sensitive(options.case_sensitive);

        let workspace_path = options
            .workspace_path
            .unwrap_or_else(|| "workspace/index.md".to_string());
        let resolved_workspace_path = self.resolve_fs_path(&workspace_path);
        let results = self
            .search()
            .search_workspace(&resolved_workspace_path, &query)
            .await?;
        Ok(Response::SearchResults(results))
    }
}
