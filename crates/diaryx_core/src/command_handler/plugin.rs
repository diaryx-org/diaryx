//! Plugin operation command handlers.

use crate::command::Response;
use crate::diaryx::Diaryx;
use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;
use crate::yaml;

impl<FS: AsyncFileSystem + Clone> Diaryx<FS> {
    pub(crate) async fn cmd_plugin_command(
        &self,
        plugin: String,
        command: String,
        params: serde_json::Value,
    ) -> Result<Response> {
        let result = self
            .plugin_registry()
            .handle_plugin_command(&plugin, &command, params)
            .await;
        match result {
            Some(Ok(value)) => Ok(Response::PluginResult(value)),
            Some(Err(e)) => Err(DiaryxError::Plugin(e.to_string())),
            None => Err(DiaryxError::Plugin(format!(
                "No plugin '{plugin}' handles command '{command}'"
            ))),
        }
    }

    pub(crate) fn cmd_get_plugin_manifests(&self) -> Result<Response> {
        let manifests = self.plugin_registry().get_all_manifests();
        Ok(Response::PluginManifests(manifests))
    }

    pub(crate) async fn cmd_get_plugin_config(&self, plugin: String) -> Result<Response> {
        let Some(wp) = self.find_workspace_plugin(&plugin) else {
            return Err(DiaryxError::Plugin(format!("Plugin '{plugin}' not found")));
        };

        // The workspace settings file (`plugins.<id>.config` in Config.md) is
        // the source of truth for declarative config. Fall back to the
        // plugin's in-memory config when the workspace has none stored yet
        // (or when no workspace is open, e.g. the settings dialog at startup).
        if let Some(root_index) = self.current_root_index().await
            && let Some(config) = self
                .workspace()
                .inner()
                .get_workspace_plugin_config(&root_index, &plugin)
                .await?
        {
            return Ok(Response::PluginResult(config));
        }

        let config = wp.get_config().await.unwrap_or(serde_json::Value::Null);
        Ok(Response::PluginResult(config))
    }

    pub(crate) async fn cmd_set_plugin_config(
        &self,
        plugin: String,
        config: serde_json::Value,
    ) -> Result<Response> {
        let Some(wp) = self.find_workspace_plugin(&plugin) else {
            return Err(DiaryxError::Plugin(format!("Plugin '{plugin}' not found")));
        };

        // Persist declarative config to Config.md (the source of truth). Plugin
        // *state/blobs* stay in `host::storage`; only user-editable settings
        // live here, so they are human-editable, git-diffable, and synced.
        let root_index = self.current_root_index().await;
        if let Some(ref root_index) = root_index {
            self.workspace()
                .inner()
                .set_workspace_plugin_config(root_index, &plugin, config.clone())
                .await?;
        }

        // Notify the guest (updates in-memory config + pushes to the guest's
        // `set_config` export). Persistence is the host's job now — the guest
        // no longer writes declarative config to `host::storage`.
        let reconcile = wp
            .set_config(config)
            .await
            .map_err(|e| DiaryxError::Plugin(e.to_string()))?;

        // Tier 1: the guest may hand back a normalized/updated config to
        // persist (host-chosen location: this plugin's own `config` subkey).
        if let Some(ref root_index) = root_index
            && let Some(updated) = reconcile.config
        {
            self.workspace()
                .inner()
                .set_workspace_plugin_config(root_index, &plugin, updated)
                .await?;
        }
        // TODO(reconcile surfacing): `reconcile.permission_request` and
        // `reconcile.migrations` are surfaced for user approval in the frontend
        // phase; not yet applied here.
        Ok(Response::Ok)
    }

    /// Find a registered workspace plugin by id, cloning the `Arc` so the
    /// registry borrow isn't held across later workspace operations.
    fn find_workspace_plugin(
        &self,
        plugin: &str,
    ) -> Option<std::sync::Arc<dyn crate::plugin::WorkspacePlugin>> {
        self.plugin_registry()
            .workspace_plugins()
            .iter()
            .find(|wp| wp.id().0 == plugin)
            .cloned()
    }

    /// Resolve the current workspace's root index path, if a workspace is open
    /// and its root index can be located. Used to read/write plugin config in
    /// the linked settings file without threading the path through commands.
    async fn current_root_index(&self) -> Option<std::path::PathBuf> {
        let root = self.workspace_root()?;
        self.workspace()
            .inner()
            .find_root_index_in_dir(&root)
            .await
            .ok()
            .flatten()
    }

    pub(crate) async fn cmd_remove_workspace_plugin_data(
        &self,
        root_index_path: String,
        plugin: String,
    ) -> Result<Response> {
        // `plugins` and `disabled_plugins` are workspace config fields, so they
        // live in the linked settings file once a workspace has been migrated.
        // Resolve where they actually live — the linked file if the root index
        // points to one, else the root index itself for un-migrated workspaces —
        // and edit that file.
        let (_, config_path) = self
            .workspace()
            .inner()
            .resolve_config_source(std::path::Path::new(&root_index_path))
            .await?;
        let target = config_path
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| root_index_path.clone());

        let frontmatter = self.entry().get_frontmatter(&target).await?;

        if let Some(plugins_value) = frontmatter.get("plugins").cloned()
            && let Some(next_plugins) = remove_plugin_from_mapping(plugins_value, &plugin)
        {
            if next_plugins.is_empty() {
                self.entry()
                    .remove_frontmatter_property(&target, "plugins")
                    .await?;
            } else {
                self.entry()
                    .set_frontmatter_property(
                        &target,
                        "plugins",
                        yaml::Value::Mapping(next_plugins),
                    )
                    .await?;
            }
        }

        if let Some(disabled_value) = frontmatter.get("disabled_plugins").cloned()
            && let Some(next_disabled) = remove_plugin_from_disabled_list(disabled_value, &plugin)
        {
            if next_disabled.is_empty() {
                self.entry()
                    .remove_frontmatter_property(&target, "disabled_plugins")
                    .await?;
            } else {
                self.entry()
                    .set_frontmatter_property(
                        &target,
                        "disabled_plugins",
                        yaml::Value::Sequence(next_disabled),
                    )
                    .await?;
            }
        }

        Ok(Response::Ok)
    }
}

fn remove_plugin_from_mapping(value: yaml::Value, plugin: &str) -> Option<yaml::Mapping> {
    let mut mapping = match value {
        yaml::Value::Mapping(mapping) => mapping,
        _ => return None,
    };
    let removed = mapping.shift_remove(plugin).is_some();
    if removed { Some(mapping) } else { None }
}

fn remove_plugin_from_disabled_list(value: yaml::Value, plugin: &str) -> Option<Vec<yaml::Value>> {
    let items = match value {
        yaml::Value::Sequence(items) => items,
        _ => return None,
    };
    let mut removed = false;
    let filtered = items
        .into_iter()
        .filter(|item| {
            let should_keep = !matches!(item, yaml::Value::String(id) if id == plugin);
            if !should_keep {
                removed = true;
            }
            should_keep
        })
        .collect::<Vec<_>>();
    if removed { Some(filtered) } else { None }
}

#[cfg(test)]
mod tests {
    use super::{remove_plugin_from_disabled_list, remove_plugin_from_mapping};
    use crate::yaml;

    #[test]
    fn remove_plugin_from_mapping_drops_target_entry_only() {
        let mut mapping = yaml::Mapping::new();
        mapping.insert(
            "diaryx.sync".to_string(),
            yaml::Value::String("sync".into()),
        );
        mapping.insert(
            "diaryx.daily".to_string(),
            yaml::Value::String("daily".into()),
        );

        let result = remove_plugin_from_mapping(yaml::Value::Mapping(mapping), "diaryx.sync")
            .expect("removed");

        assert!(!result.contains_key("diaryx.sync"));
        assert!(result.contains_key("diaryx.daily"));
    }

    #[test]
    fn remove_plugin_from_disabled_list_filters_target() {
        let result = remove_plugin_from_disabled_list(
            yaml::Value::Sequence(vec![
                yaml::Value::String("diaryx.sync".into()),
                yaml::Value::String("diaryx.daily".into()),
            ]),
            "diaryx.sync",
        )
        .expect("removed");

        assert_eq!(result, vec![yaml::Value::String("diaryx.daily".into())]);
    }
}
