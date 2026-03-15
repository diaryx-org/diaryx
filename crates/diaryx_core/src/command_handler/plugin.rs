//! Plugin operation command handlers.

use crate::command::Response;
use crate::diaryx::Diaryx;
use crate::error::{DiaryxError, Result};
use crate::fs::AsyncFileSystem;

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
}
