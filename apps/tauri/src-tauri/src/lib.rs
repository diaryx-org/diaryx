//! # Diaryx Tauri Library
//!
//! This is the library file for the Tauri backend.
//!

/// Where all the Tauri `invoke` functions are defined.
mod commands;

use commands::{AppState, GuestModeState};
#[cfg(feature = "extism-plugins")]
use commands::{PluginAdapters, RuntimeContextState};

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
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    log::info!("Starting Diaryx application...");

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        // Native iOS keyboard toolbar for TipTap editor (no-op on desktop)
        .plugin(tauri_plugin_editor_toolbar::init());

    // Apple IAP plugin — only included with `--features iap` (for App Store builds)
    #[cfg(feature = "iap")]
    {
        builder = builder.plugin(tauri_plugin_iap::init());
    }

    // Core state
    builder = builder
        .manage(AppState::new())
        .manage(GuestModeState::new());

    // Extism plugin states — only available with extism-plugins feature
    #[cfg(feature = "extism-plugins")]
    {
        builder = builder
            .manage(RuntimeContextState::new())
            .manage(PluginAdapters::new());
    }

    builder
        .setup(|_app| {
            #[cfg(target_os = "ios")]
            setup_ios_edge_to_edge(_app);
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
            commands::pick_workspace_folder,
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
            commands::install_user_plugin,
            commands::uninstall_user_plugin,
            // Extism Plugin Render (IPC for Tauri/iOS when browser Extism unavailable)
            commands::call_plugin_render,
            // OAuth Webview (native popup for OAuth sign-in)
            commands::oauth_webview,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
