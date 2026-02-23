use tauri::{plugin::PluginApi, AppHandle, Runtime};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_editor_toolbar);

pub fn init<R: Runtime>(
    _app: &AppHandle<R>,
    api: PluginApi<R, ()>,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "ios")]
    api.register_ios_plugin(init_plugin_editor_toolbar)?;

    #[cfg(not(target_os = "ios"))]
    let _ = api;

    Ok(())
}
