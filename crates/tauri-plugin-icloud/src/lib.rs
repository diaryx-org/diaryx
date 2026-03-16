use serde::{Deserialize, Serialize};
#[cfg(mobile)]
use tauri::Manager;
use tauri::{
    plugin::{Builder, TauriPlugin},
    AppHandle, Runtime,
};

#[cfg(mobile)]
pub mod mobile;

// Container identifier used across both iOS Swift plugin and macOS filesystem paths.
const CONTAINER_ID: &str = "iCloud.org.diaryx.app";

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

// ============================================================================
// Public cross-platform API
// ============================================================================
//
// These functions are meant to be called from the Tauri app's commands.rs.
// On iOS they delegate to the Swift plugin via managed state; on macOS they
// use direct filesystem operations (iCloud Drive is a regular directory on
// macOS); on other platforms they return a not-available error.

/// Check whether iCloud Drive is available on this device.
pub async fn check_available<R: Runtime>(
    _app: &AppHandle<R>,
) -> Result<ICloudAvailability, String> {
    #[cfg(mobile)]
    {
        _app.state::<mobile::ICloud<R>>()
            .check_icloud_available()
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(all(not(mobile), target_os = "macos"))]
    {
        // On macOS, iCloud Drive manifests as ~/Library/Mobile Documents/.
        // If this directory exists the user has iCloud Drive enabled.
        let available = dirs::home_dir()
            .map(|h| h.join("Library/Mobile Documents").is_dir())
            .unwrap_or(false);
        Ok(ICloudAvailability {
            is_available: available,
        })
    }
    #[cfg(not(any(mobile, target_os = "macos")))]
    {
        Ok(ICloudAvailability {
            is_available: false,
        })
    }
}

/// Resolve the iCloud container and its Documents subdirectory.
pub async fn get_container_url<R: Runtime>(
    _app: &AppHandle<R>,
) -> Result<ICloudContainerInfo, String> {
    #[cfg(mobile)]
    {
        _app.state::<mobile::ICloud<R>>()
            .get_icloud_container_url()
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(all(not(mobile), target_os = "macos"))]
    {
        // macOS iCloud container lives at:
        //   ~/Library/Mobile Documents/iCloud~<bundle-id-with-tildes>/
        // For "iCloud.org.diaryx.app" the folder name is "iCloud~org~diaryx~app".
        let folder_name = CONTAINER_ID
            .strip_prefix("iCloud.")
            .unwrap_or(CONTAINER_ID)
            .replace('.', "~");
        let folder_name = format!("iCloud~{folder_name}");

        let container = dirs::home_dir()
            .ok_or_else(|| "Cannot determine home directory".to_string())?
            .join("Library/Mobile Documents")
            .join(&folder_name);
        let documents = container.join("Documents");

        std::fs::create_dir_all(&documents)
            .map_err(|e| format!("Failed to create iCloud container: {e}"))?;

        Ok(ICloudContainerInfo {
            container_url: container.to_string_lossy().into_owned(),
            documents_url: documents.to_string_lossy().into_owned(),
        })
    }
    #[cfg(not(any(mobile, target_os = "macos")))]
    {
        Err("iCloud is not supported on this platform".into())
    }
}

/// Ask the system to download a file that may still be an iCloud placeholder.
///
/// On macOS, accessing the file triggers automatic download so this is a no-op.
pub async fn do_trigger_download<R: Runtime>(
    _app: &AppHandle<R>,
    path: String,
) -> Result<(), String> {
    #[cfg(mobile)]
    {
        _app.state::<mobile::ICloud<R>>()
            .trigger_download(path)
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(all(not(mobile), target_os = "macos"))]
    {
        // macOS downloads iCloud files transparently on access. No-op.
        let _ = path;
        Ok(())
    }
    #[cfg(not(any(mobile, target_os = "macos")))]
    {
        let _ = path;
        Err("iCloud is not supported on this platform".into())
    }
}

/// Start monitoring iCloud sync status (emits Tauri events).
///
/// On macOS this is currently a no-op (macOS handles sync transparently).
pub async fn do_start_monitoring<R: Runtime>(_app: &AppHandle<R>) -> Result<(), String> {
    #[cfg(mobile)]
    {
        _app.state::<mobile::ICloud<R>>()
            .start_status_monitoring()
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(mobile))]
    {
        // macOS handles iCloud sync transparently — no monitoring needed.
        Ok(())
    }
}

/// Stop monitoring iCloud sync status.
pub async fn do_stop_monitoring<R: Runtime>(_app: &AppHandle<R>) -> Result<(), String> {
    #[cfg(mobile)]
    {
        _app.state::<mobile::ICloud<R>>()
            .stop_status_monitoring()
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(mobile))]
    {
        Ok(())
    }
}

/// Migrate workspace files into the iCloud container.
///
/// On iOS this uses `FileManager.setUbiquitous` for proper upload tracking.
/// On macOS copying into the container directory triggers automatic upload.
pub async fn do_migrate_to_icloud<R: Runtime>(
    _app: &AppHandle<R>,
    source_path: String,
    dest_path: String,
) -> Result<MigrationResult, String> {
    #[cfg(mobile)]
    {
        _app.state::<mobile::ICloud<R>>()
            .migrate_to_icloud(source_path, dest_path)
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(all(not(mobile), target_os = "macos"))]
    {
        copy_directory_recursive(&source_path, &dest_path)
    }
    #[cfg(not(any(mobile, target_os = "macos")))]
    {
        let _ = (source_path, dest_path);
        Err("iCloud is not supported on this platform".into())
    }
}

/// Migrate workspace files out of the iCloud container to local storage.
pub async fn do_migrate_from_icloud<R: Runtime>(
    _app: &AppHandle<R>,
    source_path: String,
    dest_path: String,
) -> Result<MigrationResult, String> {
    #[cfg(mobile)]
    {
        _app.state::<mobile::ICloud<R>>()
            .migrate_from_icloud(source_path, dest_path)
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(all(not(mobile), target_os = "macos"))]
    {
        copy_directory_recursive(&source_path, &dest_path)
    }
    #[cfg(not(any(mobile, target_os = "macos")))]
    {
        let _ = (source_path, dest_path);
        Err("iCloud is not supported on this platform".into())
    }
}

/// Recursive directory copy used on macOS where iCloud Drive sync is automatic.
#[cfg(all(not(mobile), target_os = "macos"))]
fn copy_directory_recursive(source: &str, dest: &str) -> Result<MigrationResult, String> {
    use std::path::Path;

    let source = Path::new(source);
    let dest = Path::new(dest);
    let mut count: usize = 0;

    std::fs::create_dir_all(dest).map_err(|e| format!("Failed to create destination: {e}"))?;

    let walker = walkdir(source).map_err(|e| format!("Failed to enumerate source: {e}"))?;
    for entry in walker {
        let (entry_path, is_dir) = entry.map_err(|e| format!("Walk error: {e}"))?;
        let relative = entry_path
            .strip_prefix(source)
            .map_err(|e| format!("Strip prefix error: {e}"))?;
        let dest_path = dest.join(relative);

        if is_dir {
            std::fs::create_dir_all(&dest_path)
                .map_err(|e| format!("Failed to create dir: {e}"))?;
        } else {
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent dir: {e}"))?;
            }
            if dest_path.exists() {
                std::fs::remove_file(&dest_path)
                    .map_err(|e| format!("Failed to remove existing: {e}"))?;
            }
            std::fs::copy(&entry_path, &dest_path)
                .map_err(|e| format!("Failed to copy file: {e}"))?;
            count += 1;
        }
    }

    Ok(MigrationResult {
        files_migrated: count,
    })
}

/// Simple directory walker that yields (path, is_dir) tuples.
#[cfg(all(not(mobile), target_os = "macos"))]
fn walkdir(
    root: &std::path::Path,
) -> Result<Vec<Result<(std::path::PathBuf, bool), String>>, std::io::Error> {
    let mut results = Vec::new();
    walkdir_inner(root, &mut results)?;
    Ok(results)
}

#[cfg(all(not(mobile), target_os = "macos"))]
fn walkdir_inner(
    dir: &std::path::Path,
    results: &mut Vec<Result<(std::path::PathBuf, bool), String>>,
) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            results.push(Ok((path.clone(), true)));
            walkdir_inner(&path, results)?;
        } else {
            results.push(Ok((path, false)));
        }
    }
    Ok(())
}

// ============================================================================
// Tauri command handlers (called from the frontend via invoke)
// ============================================================================

#[tauri::command]
async fn check_icloud_available<R: Runtime>(
    app: AppHandle<R>,
) -> Result<ICloudAvailability, String> {
    check_available(&app).await
}

#[tauri::command]
async fn get_icloud_container_url<R: Runtime>(
    app: AppHandle<R>,
) -> Result<ICloudContainerInfo, String> {
    get_container_url(&app).await
}

#[tauri::command]
async fn trigger_download<R: Runtime>(app: AppHandle<R>, path: String) -> Result<(), String> {
    do_trigger_download(&app, path).await
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
        // macOS handles sync transparently — always report up-to-date.
        Ok(ICloudSyncStatus {
            total_items: 0,
            uploading: 0,
            downloading: 0,
            up_to_date: true,
        })
    }
}

#[tauri::command]
async fn start_status_monitoring<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    do_start_monitoring(&app).await
}

#[tauri::command]
async fn stop_status_monitoring<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    do_stop_monitoring(&app).await
}

#[tauri::command]
async fn migrate_to_icloud<R: Runtime>(
    app: AppHandle<R>,
    source_path: String,
    dest_path: String,
) -> Result<MigrationResult, String> {
    do_migrate_to_icloud(&app, source_path, dest_path).await
}

#[tauri::command]
async fn migrate_from_icloud<R: Runtime>(
    app: AppHandle<R>,
    source_path: String,
    dest_path: String,
) -> Result<MigrationResult, String> {
    do_migrate_from_icloud(&app, source_path, dest_path).await
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
