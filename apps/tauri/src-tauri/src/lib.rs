//! # Diaryx Tauri Library
//!
//! This is the library file for the Tauri backend.
//!

#[cfg(all(feature = "apple", feature = "desktop-updater"))]
compile_error!(
    "The `apple` and `desktop-updater` features are mutually exclusive. \
Use `apple` for App Store builds and `desktop-updater` for direct desktop distribution."
);

/// Where all the Tauri `invoke` functions are defined.
mod auth_client;
mod auth_commands;
mod commands;
mod credentials;
#[cfg(debug_assertions)]
mod dev_ipc;
mod logging;
#[cfg(target_os = "macos")]
mod macos_security_scoped;

use auth_commands::AuthServiceState;
use commands::{AppState, GuestModeState};
#[cfg(feature = "extism-plugins")]
use commands::{PluginAdapters, RuntimeContextState};
use tauri::Manager;

/// Configure the iOS WKWebView to render edge-to-edge, extending content into
/// safe areas. Without this, the webview stops at the bottom safe area boundary,
/// leaving a visible gap above the home indicator.
///
/// Sets `scrollView.contentInsetAdjustmentBehavior = .never` via ObjC runtime,
/// which makes `viewport-fit=cover` and `env(safe-area-inset-*)` work correctly.
#[cfg(target_os = "ios")]
fn setup_ios_edge_to_edge(app: &tauri::App) {
    use tauri::Manager;

    unsafe extern "C" {
        fn sel_registerName(name: *const std::ffi::c_char) -> *const std::ffi::c_void;
        fn objc_msgSend(
            obj: *const std::ffi::c_void,
            sel: *const std::ffi::c_void,
            ...
        ) -> *const std::ffi::c_void;
    }

    if let Some(webview) = app.get_webview_window("main") {
        let _ = webview.with_webview(move |wv| unsafe {
            let wkwebview = wv.inner() as *const std::ffi::c_void;

            // wkwebview.scrollView
            let scroll_view_sel =
                sel_registerName(b"scrollView\0".as_ptr() as *const std::ffi::c_char);
            let scroll_view = objc_msgSend(wkwebview, scroll_view_sel);

            // scrollView.contentInsetAdjustmentBehavior = .never (rawValue 2)
            let set_behavior_sel = sel_registerName(
                b"setContentInsetAdjustmentBehavior:\0".as_ptr() as *const std::ffi::c_char,
            );
            objc_msgSend(scroll_view, set_behavior_sel, 2isize);
        });
    }
}

/// Run function used by Tauri clients. Builds Tauri plugins and invokable commands.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        // Native iOS keyboard toolbar for TipTap editor (no-op on desktop)
        .plugin(tauri_plugin_editor_toolbar::init());

    // Tauri updater plugin — only for direct desktop distribution builds.
    #[cfg(all(
        feature = "desktop-updater",
        not(any(target_os = "android", target_os = "ios"))
    ))]
    {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    // Apple IAP plugin — only included with `--features iap` (for App Store builds)
    #[cfg(feature = "iap")]
    {
        builder = builder.plugin(tauri_plugin_iap::init());
    }

    // Apple iCloud Drive plugin — only included with `--features icloud` (for iOS builds)
    #[cfg(feature = "icloud")]
    {
        builder = builder.plugin(tauri_plugin_icloud::init());
    }

    // Core state
    builder = builder
        .manage(AppState::new())
        .manage(GuestModeState::new())
        .manage(AuthServiceState::new());

    // Extism plugin states — only available with extism-plugins feature
    #[cfg(feature = "extism-plugins")]
    {
        builder = builder
            .manage(RuntimeContextState::new())
            .manage(PluginAdapters::new());
    }

    builder
        .setup(|app| {
            if let Ok(data_dir) = app.path().app_data_dir() {
                // Credential store uses app data dir on Android for file-based fallback
                app.manage(credentials::CredentialStoreDir(data_dir.clone()));

                let (_, log_file) = crate::logging::log_paths(&data_dir);
                if let Err(err) = crate::logging::init(&log_file) {
                    eprintln!("[Diaryx] Failed to initialize file-backed logging: {err}");
                } else {
                    log::info!(
                        "Starting Diaryx application (log file: {})",
                        log_file.display()
                    );
                }
            } else {
                eprintln!("[Diaryx] Failed to resolve app data directory for logging");
            }

            #[cfg(target_os = "ios")]
            setup_ios_edge_to_edge(app);

            #[cfg(debug_assertions)]
            if let Some(guard) = crate::dev_ipc::start(app.handle()) {
                app.manage(guard);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // ============================================================
            // UNIFIED COMMAND API - All operations go through execute()
            // ============================================================
            commands::execute,
            // ============================================================
            // PLATFORM-SPECIFIC COMMANDS
            // These cannot be moved to execute() as they require platform
            // features (file dialogs, app paths, etc.)
            // ============================================================

            // App initialization (iOS-compatible)
            commands::initialize_app,
            commands::get_app_paths,
            commands::read_log_file,
            commands::pick_workspace_folder,
            commands::authorize_workspace_path,
            commands::reveal_in_file_manager,
            commands::read_binary_file,
            commands::write_binary_file,
            commands::check_for_app_update,
            commands::install_app_update,
            // Export
            commands::export_to_zip,
            commands::export_to_format,
            // Import (file picker dialogs)
            commands::import_from_zip,
            commands::pick_and_import_zip,
            commands::import_from_zip_data,
            // Chunked Import (for large files)
            commands::start_import_upload,
            commands::append_import_chunk,
            commands::finish_import_upload,
            // Guest Mode (for share sessions)
            commands::start_guest_mode,
            commands::end_guest_mode,
            commands::is_guest_mode,
            commands::set_runtime_context,
            // Workspace Reinitialization
            commands::reinitialize_workspace,
            // HTTP Proxy (iOS CORS bypass)
            commands::proxy_fetch,
            // Extism User Plugin Management
            commands::inspect_user_plugin,
            commands::install_user_plugin,
            commands::uninstall_user_plugin,
            commands::execute_plugin_command_with_files,
            commands::get_plugin_component_html,
            // Extism Plugin Render (IPC for Tauri/iOS when browser Extism unavailable)
            commands::call_plugin_render,
            // OAuth Webview (native popup for OAuth sign-in)
            commands::oauth_webview,
            // iCloud Drive workspace storage
            commands::set_icloud_enabled,
            commands::get_icloud_workspace_info,
            commands::list_icloud_workspaces,
            commands::link_icloud_workspace,
            commands::restore_icloud_workspace,
            // Secure credential storage (OS keychain / encrypted file on Android)
            credentials::store_credential,
            credentials::get_credential,
            credentials::remove_credential,
            // Auth service (keyring-backed diaryx_core::auth::AuthService)
            auth_commands::auth_server_url,
            auth_commands::auth_set_server_url,
            auth_commands::auth_is_authenticated,
            auth_commands::auth_get_metadata,
            auth_commands::auth_request_magic_link,
            auth_commands::auth_verify_magic_link,
            auth_commands::auth_verify_code,
            auth_commands::auth_get_me,
            auth_commands::auth_refresh_token,
            auth_commands::auth_logout,
            auth_commands::auth_get_devices,
            auth_commands::auth_rename_device,
            auth_commands::auth_delete_device,
            auth_commands::auth_delete_account,
            auth_commands::auth_create_workspace,
            auth_commands::auth_rename_workspace,
            auth_commands::auth_delete_workspace,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
