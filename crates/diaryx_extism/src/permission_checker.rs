//! Permission checker implementations for Extism host function gating.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use diaryx_core::fs::SyncToAsyncFs;
use diaryx_core::path_utils::{normalize_sync_path, strip_workspace_root_prefix};
use diaryx_core::plugin::permissions::{
    PermissionCheck, PermissionType, PluginConfig, check_permission,
};
use diaryx_core::workspace::Workspace;
use diaryx_native::RealFileSystem;

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

/// Loads plugin permissions from the workspace config `plugins` field on each
/// check (resolved through the linked settings file, with root-index fallback).
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

        // `plugins` is a workspace config field, so it lives in the linked
        // settings file once a workspace has been migrated (falling back to the
        // root index for un-migrated workspaces). Resolve it through the same
        // config layer the rest of the app uses rather than reading the root
        // index frontmatter directly — otherwise permission checks would read a
        // stale/empty `plugins` and deny everything post-migration.
        let fs = SyncToAsyncFs::new(RealFileSystem);
        let workspace = Workspace::new(fs);
        let config = futures_lite::future::block_on(workspace.get_workspace_config(root_path))
            .map_err(|e| format!("Failed to read workspace config: {e}"))?;

        let plugins_value = match config.plugins {
            Some(v) => v,
            None => return Ok(HashMap::new()),
        };

        serde_json::from_value::<HashMap<String, PluginConfig>>(serde_json::Value::from(
            plugins_value,
        ))
        .map_err(|e| format!("Invalid workspace plugins config: {e}"))
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

    fn storage_quota_bytes(&self, plugin_id: &str) -> Option<u64> {
        // Re-read frontmatter on each call for consistency with
        // check_permission. If the frontmatter is unreadable or the plugin
        // has no quota_bytes configured, return None — host falls back to
        // its default quota.
        self.load_plugins_config()
            .ok()?
            .get(plugin_id)?
            .permissions
            .plugin_storage
            .as_ref()?
            .quota_bytes
    }
}

fn normalize_workspace_file_target(root_index_path: Option<&Path>, target: &str) -> String {
    let normalized_target = target.replace('\\', "/");
    let fallback = normalize_sync_path(&normalized_target);

    let Some(root_index_path) = root_index_path else {
        return fallback;
    };

    let workspace_root = root_index_path.parent().unwrap_or(root_index_path);
    if let Some(relative) = strip_workspace_root_prefix(target, workspace_root) {
        return normalize_sync_path(&relative);
    }

    fallback
}

fn find_root_index_path(workspace_root: &Path) -> Option<PathBuf> {
    if workspace_root
        .extension()
        .is_some_and(|extension| extension == "md")
    {
        return Some(workspace_root.to_path_buf());
    }

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
            quota_bytes: None,
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
    fn normalize_workspace_file_target_strips_corrupted_absolute_workspace_prefix() {
        let root_index = Path::new("/Users/test/journal/README.md");

        let daily = normalize_workspace_file_target(
            Some(root_index),
            "Users/test/journal/Daily/daily_index.md",
        );
        let root =
            normalize_workspace_file_target(Some(root_index), "Users/test/journal/README.md");

        assert_eq!(daily, "Daily/daily_index.md");
        assert_eq!(root, "README.md");
    }

    #[test]
    fn normalize_workspace_file_target_handles_custom_root_index_names() {
        let root_index = Path::new("/Users/test/journal/Adam's Archive.md");

        let root = normalize_workspace_file_target(
            Some(root_index),
            "/Users/test/journal/Adam's Archive.md",
        );

        assert_eq!(root, "Adam's Archive.md");
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
                config: None,
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
