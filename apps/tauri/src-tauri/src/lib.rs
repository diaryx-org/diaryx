// src-tauri/src/lib.rs

// 1. Declare the commands module here so the library owns it
mod commands;

// 2. Create the run function that mobile and desktop will both use
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    log::info!("Starting Diaryx application...");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            // App initialization (iOS-compatible)
            commands::initialize_app,
            commands::get_app_paths,
            commands::create_workspace,
            // Configuration
            commands::get_config,
            commands::save_config,
            // Workspace
            commands::get_workspace_tree,
            // Entries
            commands::get_entry,
            commands::save_entry,
            commands::create_entry,
            commands::delete_entry,
            commands::move_entry,
            commands::attach_entry_to_parent,
            // Search
            commands::search_workspace,
            // Frontmatter
            commands::get_frontmatter,
            commands::set_frontmatter_property,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
