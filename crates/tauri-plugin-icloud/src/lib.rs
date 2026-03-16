use serde::{Deserialize, Serialize};
#[cfg(mobile)]
use tauri::Manager;
use tauri::{
    plugin::{Builder, TauriPlugin},
    AppHandle, Runtime,
};

#[cfg(mobile)]
pub mod mobile;

// --- Models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ICloudAvailability {
    pub is_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ICloudContainerInfo {
    pub container_url: String,
    pub documents_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ICloudSyncStatus {
    pub total_items: usize,
    pub uploading: usize,
    pub downloading: usize,
    pub up_to_date: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationResult {
    pub files_migrated: usize,
}

// --- Commands ---

#[tauri::command]
async fn check_icloud_available<R: Runtime>(
    app: AppHandle<R>,
) -> Result<ICloudAvailability, String> {
    #[cfg(mobile)]
    {
        app.state::<mobile::ICloud<R>>()
            .check_icloud_available()
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(mobile))]
    {
        let _ = app;
        Err("iCloud is only available on iOS".into())
    }
}

#[tauri::command]
async fn get_icloud_container_url<R: Runtime>(
    app: AppHandle<R>,
) -> Result<ICloudContainerInfo, String> {
    #[cfg(mobile)]
    {
        app.state::<mobile::ICloud<R>>()
            .get_icloud_container_url()
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(mobile))]
    {
        let _ = app;
        Err("iCloud is only available on iOS".into())
    }
}

#[tauri::command]
async fn trigger_download<R: Runtime>(app: AppHandle<R>, path: String) -> Result<(), String> {
    #[cfg(mobile)]
    {
        app.state::<mobile::ICloud<R>>()
            .trigger_download(path)
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(mobile))]
    {
        let _ = (app, path);
        Err("iCloud is only available on iOS".into())
    }
}

#[tauri::command]
async fn get_sync_status<R: Runtime>(app: AppHandle<R>) -> Result<ICloudSyncStatus, String> {
    #[cfg(mobile)]
    {
        app.state::<mobile::ICloud<R>>()
            .get_sync_status()
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(mobile))]
    {
        let _ = app;
        Err("iCloud is only available on iOS".into())
    }
}

#[tauri::command]
async fn start_status_monitoring<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    #[cfg(mobile)]
    {
        app.state::<mobile::ICloud<R>>()
            .start_status_monitoring()
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(mobile))]
    {
        let _ = app;
        Err("iCloud is only available on iOS".into())
    }
}

#[tauri::command]
async fn stop_status_monitoring<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    #[cfg(mobile)]
    {
        app.state::<mobile::ICloud<R>>()
            .stop_status_monitoring()
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(mobile))]
    {
        let _ = app;
        Err("iCloud is only available on iOS".into())
    }
}

#[tauri::command]
async fn migrate_to_icloud<R: Runtime>(
    app: AppHandle<R>,
    source_path: String,
    dest_path: String,
) -> Result<MigrationResult, String> {
    #[cfg(mobile)]
    {
        app.state::<mobile::ICloud<R>>()
            .migrate_to_icloud(source_path, dest_path)
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(mobile))]
    {
        let _ = (app, source_path, dest_path);
        Err("iCloud is only available on iOS".into())
    }
}

#[tauri::command]
async fn migrate_from_icloud<R: Runtime>(
    app: AppHandle<R>,
    source_path: String,
    dest_path: String,
) -> Result<MigrationResult, String> {
    #[cfg(mobile)]
    {
        app.state::<mobile::ICloud<R>>()
            .migrate_from_icloud(source_path, dest_path)
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(mobile))]
    {
        let _ = (app, source_path, dest_path);
        Err("iCloud is only available on iOS".into())
    }
}

// --- Plugin init ---

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("icloud")
        .invoke_handler(tauri::generate_handler![
            check_icloud_available,
            get_icloud_container_url,
            trigger_download,
            get_sync_status,
            start_status_monitoring,
            stop_status_monitoring,
            migrate_to_icloud,
            migrate_from_icloud,
        ])
        .setup(|app, api| {
            #[cfg(mobile)]
            {
                let icloud = mobile::init(app, api)?;
                app.manage(icloud);
            }
            #[cfg(not(mobile))]
            {
                let _ = (app, api);
            }
            Ok(())
        })
        .build()
}
