//! Native publish command — drives `diaryx_core::publish::PublishService`
//! directly over the keyring auth client's `NamespaceProvider`, with no Extism
//! plugin. The server renders the HTML (ARK Layer 3); the client just collects
//! sources and uploads them.

use diaryx_core::fs::SyncToAsyncFs;
use diaryx_core::publish::PublishService;
use diaryx_core::workspace::Workspace;
use diaryx_native::fs::RealFileSystem;
use tauri::{AppHandle, Manager, State};

use crate::auth_commands::{AuthServiceState, ensure_service};
use crate::commands::AppState;

/// Publish the active workspace to its namespace: collect the file-declared
/// audiences' markdown sources, diff against the server, upload the changes, and
/// trigger the server-side render. Returns a JSON receipt.
#[tauri::command]
pub async fn publish_to_namespace(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    namespace_id: String,
    base_url: Option<String>,
) -> Result<serde_json::Value, String> {
    let service = ensure_service(&state, &app)
        .await
        .map_err(|e| format!("{e:?}"))?;

    let workspace_dir = {
        let app_state = app.state::<AppState>();
        let guard = app_state
            .workspace_path
            .lock()
            .map_err(|_| "workspace lock poisoned".to_string())?;
        guard
            .clone()
            .ok_or_else(|| "No active workspace".to_string())?
    };

    let fs = SyncToAsyncFs::new(RealFileSystem);
    let root_index = Workspace::new(fs.clone())
        .find_root_index_in_dir(&workspace_dir)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Could not find the workspace root index".to_string())?;

    let (plan, outcome) = PublishService::new(service.client())
        .publish_workspace(fs, &root_index, &namespace_id, base_url.as_deref())
        .await?;

    Ok(serde_json::json!({
        "uploaded": outcome.uploaded,
        "skipped_unchanged": plan.totals.unchanged,
        "deleted": outcome.deleted,
        "bytes_uploaded": outcome.bytes_uploaded,
        "audiences_deleted": outcome.audiences_deleted,
        "built": outcome.built,
        "summary": serde_json::Value::from(plan.to_summary_json()),
    }))
}

/// Preview a publish: collect the file-declared audiences' sources and diff them
/// against the server WITHOUT uploading, deleting, or rendering. Returns the
/// plan summary JSON (same shape `PublishPlan::to_summary_json` produces).
#[tauri::command]
pub async fn preview_to_namespace(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    namespace_id: String,
) -> Result<serde_json::Value, String> {
    let service = ensure_service(&state, &app)
        .await
        .map_err(|e| format!("{e:?}"))?;

    let workspace_dir = {
        let app_state = app.state::<AppState>();
        let guard = app_state
            .workspace_path
            .lock()
            .map_err(|_| "workspace lock poisoned".to_string())?;
        guard
            .clone()
            .ok_or_else(|| "No active workspace".to_string())?
    };

    let fs = SyncToAsyncFs::new(RealFileSystem);
    let root_index = Workspace::new(fs.clone())
        .find_root_index_in_dir(&workspace_dir)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Could not find the workspace root index".to_string())?;

    let plan = PublishService::new(service.client())
        .plan_workspace(fs, &root_index, &namespace_id)
        .await?;

    Ok(serde_json::Value::from(plan.to_summary_json()))
}
