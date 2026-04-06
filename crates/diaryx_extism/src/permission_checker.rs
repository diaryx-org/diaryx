//! Permission checker implementations for Extism host function gating.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use diaryx_core::frontmatter;
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_core::plugin::permissions::{
    PermissionCheck, PermissionType, PluginConfig, check_permission,
};
use diaryx_core::workspace::Workspace;

use crate::host_fns::PermissionChecker;

/// A permissive checker that allows every permission request.
///
/// Use when the plugin is already trusted but no workspace frontmatter is
/// available to read restrictions from (e.g. during workspace download
/// before the root index exists on disk).
pub struct AllowAllPermissionChecker;

impl PermissionChecker for AllowAllPermissionChecker {
    fn check_permission(
        &self,
        _plugin_id: &str,
        _permission_type: PermissionType,
        _target: &str,
    ) -> Result<(), String> {
        Ok(())
    }
}

/// A strict checker that denies every permission request.
pub struct DenyAllPermissionChecker;

impl PermissionChecker for DenyAllPermissionChecker {
    fn check_permission(
        &self,
        plugin_id: &str,
        permission_type: PermissionType,
        target: &str,
    ) -> Result<(), String> {
        Err(format!(
            "Permission denied: no permissions configured for plugin '{}' ({} on '{}')",
            plugin_id,
            permission_type.key(),
            target
        ))
    }
}

/// Loads plugin permissions from root frontmatter `plugins` on each check.
pub struct FrontmatterPermissionChecker {
    root_index_path: Option<PathBuf>,
}

impl FrontmatterPermissionChecker {
    /// Build a checker from a workspace directory path.
    pub fn from_workspace_root(workspace_root: Option<PathBuf>) -> Self {
        let root_index_path = workspace_root.as_deref().and_then(find_root_index_path);
        Self { root_index_path }
    }

    fn load_plugins_config(&self) -> Result<HashMap<String, PluginConfig>, String> {
        let root_path = self.root_index_path.as_ref().ok_or_else(|| {
            "Workspace root index not available for permission checks".to_string()
        })?;

        let content = std::fs::read_to_string(root_path)
            .map_err(|e| format!("Failed to read root index '{}': {e}", root_path.display()))?;
        let parsed = frontmatter::parse_or_empty(&content)
            .map_err(|e| format!("Failed to parse root frontmatter: {e}"))?;

        let plugins_value = match parsed.frontmatter.get("plugins") {
            Some(v) => v.clone(),
            None => return Ok(HashMap::new()),
        };

        serde_json::from_value::<HashMap<String, PluginConfig>>(serde_json::Value::from(
            plugins_value,
        ))
        .map_err(|e| format!("Invalid root frontmatter plugins config: {e}"))
    }

    fn normalize_target(&self, permission_type: PermissionType, target: &str) -> String {
        match permission_type {
            PermissionType::ReadFiles
            | PermissionType::EditFiles
            | PermissionType::CreateFiles
            | PermissionType::DeleteFiles
            | PermissionType::MoveFiles => {
                normalize_workspace_file_target(self.root_index_path.as_deref(), target)
            }
            PermissionType::HttpRequests
            | PermissionType::ExecuteCommands
            | PermissionType::PluginStorage => target.to_string(),
        }
    }
}

impl PermissionChecker for FrontmatterPermissionChecker {
    fn check_permission(
        &self,
        plugin_id: &str,
        permission_type: PermissionType,
        target: &str,
    ) -> Result<(), String> {
        let plugins_config = self.load_plugins_config()?;
        let normalized_target = self.normalize_target(permission_type, target);
        match check_permission(
            &plugins_config,
            plugin_id,
            permission_type,
            &normalized_target,
        ) {
            PermissionCheck::Allowed => Ok(()),
            PermissionCheck::Denied => Err(format!(
                "Permission denied for plugin '{}': {} on '{}'",
                plugin_id,
                permission_type.key(),
                normalized_target
            )),
            PermissionCheck::NotConfigured => Err(format!(
                "Permission not configured for plugin '{}': {} on '{}'. \
                 Add plugins.{}.permissions.{} in root frontmatter.",
                plugin_id,
                permission_type.key(),
                normalized_target,
                plugin_id,
                permission_type.key(),
            )),
        }
    }
}

fn normalize_workspace_file_target(root_index_path: Option<&Path>, target: &str) -> String {
    let normalized_target = target.replace('\\', "/");
    let fallback = normalized_target
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string();

    let Some(root_index_path) = root_index_path else {
        return fallback;
    };

    let workspace_root = root_index_path.parent().unwrap_or(root_index_path);
    let target_path = Path::new(target);
    if target_path.is_absolute()
        && let Ok(stripped) = target_path.strip_prefix(workspace_root)
    {
        return stripped
            .to_string_lossy()
            .replace('\\', "/")
            .trim_start_matches("./")
            .trim_start_matches('/')
            .to_string();
    }

    // Fallback to normalized string matching for cross-platform absolute paths.
    let normalized_root = workspace_root
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    if !normalized_root.is_empty() {
        let prefix = format!("{normalized_root}/");
        if normalized_target.starts_with(&prefix) {
            return normalized_target[prefix.len()..]
                .trim_start_matches("./")
                .trim_start_matches('/')
                .to_string();
        }
    }

    fallback
}

fn find_root_index_path(workspace_root: &Path) -> Option<PathBuf> {
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let workspace = Workspace::new(fs);
    futures_lite::future::block_on(workspace.find_root_index_in_dir(workspace_root))
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use diaryx_core::plugin::permissions::{PermissionRule, PluginPermissions};

    fn rule(include: &[&str]) -> PermissionRule {
        PermissionRule {
            include: include.iter().map(|value| (*value).to_string()).collect(),
            exclude: Vec::new(),
        }
    }

    #[test]
    fn normalize_workspace_file_target_strips_absolute_workspace_prefix() {
        let root_index = Path::new("/Users/test/journal/README.md");

        let daily = normalize_workspace_file_target(
            Some(root_index),
            "/Users/test/journal/Daily/daily_index.md",
        );
        let root =
            normalize_workspace_file_target(Some(root_index), "/Users/test/journal/README.md");

        assert_eq!(daily, "Daily/daily_index.md");
        assert_eq!(root, "README.md");
    }

    #[test]
    fn normalized_targets_match_workspace_relative_frontmatter_rules() {
        let mut plugins = HashMap::new();
        plugins.insert(
            "diaryx.daily".to_string(),
            PluginConfig {
                download: None,
                permissions: PluginPermissions {
                    read_files: Some(rule(&["Daily", "README.md"])),
                    ..PluginPermissions::default()
                },
            },
        );

        let root_index = Path::new("/Users/test/journal/README.md");
        let daily_target = normalize_workspace_file_target(
            Some(root_index),
            "/Users/test/journal/Daily/daily_index.md",
        );
        let root_target =
            normalize_workspace_file_target(Some(root_index), "/Users/test/journal/README.md");

        assert_eq!(
            check_permission(
                &plugins,
                "diaryx.daily",
                PermissionType::ReadFiles,
                &daily_target,
            ),
            PermissionCheck::Allowed
        );
        assert_eq!(
            check_permission(
                &plugins,
                "diaryx.daily",
                PermissionType::ReadFiles,
                &root_target,
            ),
            PermissionCheck::Allowed
        );
    }
}
