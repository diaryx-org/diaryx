use serde::Deserialize;
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::{ICloudAvailability, ICloudContainerInfo, ICloudSyncStatus, MigrationResult};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_icloud);

pub struct ICloud<R: Runtime>(PluginHandle<R>);

/// Wrapper for Swift responses that wrap values in a `value` key,
/// since `invoke.resolve()` requires a dictionary.
#[derive(Deserialize)]
struct ValueResponse<T> {
    value: T,
}

pub fn init<R: Runtime>(
    _app: &AppHandle<R>,
    api: PluginApi<R, ()>,
) -> Result<ICloud<R>, Box<dyn std::error::Error>> {
    #[cfg(target_os = "ios")]
    let handle = api.register_ios_plugin(init_plugin_icloud)?;

    #[cfg(target_os = "android")]
    let handle = api.register_android_plugin("org.diaryx.icloud", "ICloudPlugin")?;

    Ok(ICloud(handle))
}

impl<R: Runtime> ICloud<R> {
    pub async fn check_icloud_available(
        &self,
    ) -> Result<ICloudAvailability, Box<dyn std::error::Error>> {
        let result: ICloudAvailability = self
            .0
            .run_mobile_plugin_async("checkIcloudAvailable", serde_json::json!({}))
            .await?;
        Ok(result)
    }

    pub async fn get_icloud_container_url(
        &self,
    ) -> Result<ICloudContainerInfo, Box<dyn std::error::Error>> {
        let result: ICloudContainerInfo = self
            .0
            .run_mobile_plugin_async("getIcloudContainerUrl", serde_json::json!({}))
            .await?;
        Ok(result)
    }

    pub async fn trigger_download(&self, path: String) -> Result<(), Box<dyn std::error::Error>> {
        let _: serde_json::Value = self
            .0
            .run_mobile_plugin_async("triggerDownload", serde_json::json!({ "path": path }))
            .await?;
        Ok(())
    }

    pub async fn get_sync_status(&self) -> Result<ICloudSyncStatus, Box<dyn std::error::Error>> {
        let result: ICloudSyncStatus = self
            .0
            .run_mobile_plugin_async("getSyncStatus", serde_json::json!({}))
            .await?;
        Ok(result)
    }

    pub async fn start_status_monitoring(&self) -> Result<(), Box<dyn std::error::Error>> {
        let _: serde_json::Value = self
            .0
            .run_mobile_plugin_async("startStatusMonitoring", serde_json::json!({}))
            .await?;
        Ok(())
    }

    pub async fn stop_status_monitoring(&self) -> Result<(), Box<dyn std::error::Error>> {
        let _: serde_json::Value = self
            .0
            .run_mobile_plugin_async("stopStatusMonitoring", serde_json::json!({}))
            .await?;
        Ok(())
    }

    pub async fn migrate_to_icloud(
        &self,
        source_path: String,
        dest_path: String,
    ) -> Result<MigrationResult, Box<dyn std::error::Error>> {
        let result: MigrationResult = self
            .0
            .run_mobile_plugin_async(
                "migrateToIcloud",
                serde_json::json!({
                    "sourcePath": source_path,
                    "destPath": dest_path,
                }),
            )
            .await?;
        Ok(result)
    }

    pub async fn migrate_from_icloud(
        &self,
        source_path: String,
        dest_path: String,
    ) -> Result<MigrationResult, Box<dyn std::error::Error>> {
        let result: MigrationResult = self
            .0
            .run_mobile_plugin_async(
                "migrateFromIcloud",
                serde_json::json!({
                    "sourcePath": source_path,
                    "destPath": dest_path,
                }),
            )
            .await?;
        Ok(result)
    }
}
