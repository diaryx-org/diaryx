//! Plugin operation command handlers.

use crate::command::Response;
use crate::diaryx::Diaryx;
use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;
use serde_yaml::{Mapping, Value};

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
        for wp in self.plugin_registry().workspace_plugins() {
            if wp.id().0 == plugin {
                let config = wp.get_config().await;
                return Ok(Response::PluginResult(
                    config.unwrap_or(serde_json::Value::Null),
                ));
            }
        }
        Err(DiaryxError::Plugin(format!("Plugin '{plugin}' not found")))
    }

    pub(crate) async fn cmd_set_plugin_config(
        &self,
        plugin: String,
        config: serde_json::Value,
    ) -> Result<Response> {
        for wp in self.plugin_registry().workspace_plugins() {
            if wp.id().0 == plugin {
                wp.set_config(config)
                    .await
                    .map_err(|e| DiaryxError::Plugin(e.to_string()))?;
                return Ok(Response::Ok);
            }
        }
        Err(DiaryxError::Plugin(format!("Plugin '{plugin}' not found")))
    }

    pub(crate) async fn cmd_remove_workspace_plugin_data(
        &self,
        root_index_path: String,
        plugin: String,
    ) -> Result<Response> {
        let canonical_path = self.get_canonical_path(&root_index_path);
        let frontmatter = self.entry().get_frontmatter(&root_index_path).await?;

        if let Some(plugins_value) = frontmatter.get("plugins").cloned()
            && let Some(next_plugins) = remove_plugin_from_mapping(plugins_value, &plugin)
        {
            if next_plugins.is_empty() {
                self.entry()
                    .remove_frontmatter_property(&root_index_path, "plugins")
                    .await?;
            } else {
                self.entry()
                    .set_frontmatter_property(
                        &root_index_path,
                        "plugins",
                        Value::Mapping(next_plugins),
                    )
                    .await?;
            }
        }

        if let Some(disabled_value) = frontmatter.get("disabled_plugins").cloned()
            && let Some(next_disabled) = remove_plugin_from_disabled_list(disabled_value, &plugin)
        {
            if next_disabled.is_empty() {
                self.entry()
                    .remove_frontmatter_property(&root_index_path, "disabled_plugins")
                    .await?;
            } else {
                self.entry()
                    .set_frontmatter_property(
                        &root_index_path,
                        "disabled_plugins",
                        Value::Sequence(next_disabled),
                    )
                    .await?;
            }
        }

        self.plugin_registry()
            .track_file_for_sync(&canonical_path)
            .await;
        self.emit_workspace_sync().await;
        Ok(Response::Ok)
    }
}

fn remove_plugin_from_mapping(value: Value, plugin: &str) -> Option<Mapping> {
    let mut mapping = match value {
        Value::Mapping(mapping) => mapping,
        _ => return None,
    };
    let removed = mapping.remove(Value::String(plugin.to_string())).is_some();
    if removed { Some(mapping) } else { None }
}

fn remove_plugin_from_disabled_list(value: Value, plugin: &str) -> Option<Vec<Value>> {
    let items = match value {
        Value::Sequence(items) => items,
        _ => return None,
    };
    let mut removed = false;
    let filtered = items
        .into_iter()
        .filter(|item| {
            let should_keep = !matches!(item, Value::String(id) if id == plugin);
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
    use serde_yaml::{Mapping, Value};

    #[test]
    fn remove_plugin_from_mapping_drops_target_entry_only() {
        let mut mapping = Mapping::new();
        mapping.insert(
            Value::String("diaryx.sync".into()),
            Value::String("sync".into()),
        );
        mapping.insert(
            Value::String("diaryx.daily".into()),
            Value::String("daily".into()),
        );

        let result =
            remove_plugin_from_mapping(Value::Mapping(mapping), "diaryx.sync").expect("removed");

        assert!(!result.contains_key(&Value::String("diaryx.sync".into())));
        assert!(result.contains_key(&Value::String("diaryx.daily".into())));
    }

    #[test]
    fn remove_plugin_from_disabled_list_filters_target() {
        let result = remove_plugin_from_disabled_list(
            Value::Sequence(vec![
                Value::String("diaryx.sync".into()),
                Value::String("diaryx.daily".into()),
            ]),
            "diaryx.sync",
        )
        .expect("removed");

        assert_eq!(result, vec![Value::String("diaryx.daily".into())]);
    }
}
