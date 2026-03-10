//! Entry command handlers (open, create)

use std::path::Path;

use crate::cli::CliDiaryxAppSync;
use crate::cli::util::{apply_workspace_config, load_config, resolve_paths};
use crate::editor::launch_editor;

/// Handle the 'open' command
pub fn handle_open(app: &CliDiaryxAppSync, path_or_date: &str) -> bool {
    let config = match load_config() {
        Some(c) => apply_workspace_config(c),
        None => return false,
    };

    // Use shared path resolution (handles directories, globs, fuzzy matching, dates)
    let paths = resolve_paths(path_or_date, &config, app);

    if paths.is_empty() {
        eprintln!("✗ No files matched: {}", path_or_date);
        return false;
    }

    let mut had_error = false;
    if paths.len() == 1 && !paths[0].exists() {
        eprintln!("✗ File not found: {}", paths[0].display());
        return false;
    }

    // Open all resolved files
    for path in &paths {
        if !path.exists() {
            eprintln!("✗ File not found: {}", path.display());
            had_error = true;
            continue;
        }

        if paths.len() > 1 {
            println!("Opening: {}", path.display());
        }

        if let Err(e) = launch_editor(path, &config) {
            eprintln!("✗ Error launching editor for {}: {}", path.display(), e);
            had_error = true;
        }
    }

    !had_error
}

/// Handle the 'create' command
pub fn handle_create(
    app: &CliDiaryxAppSync,
    path: &str,
    template: Option<String>,
    title: Option<String>,
) -> bool {
    let config = match load_config() {
        Some(c) => c,
        None => return false,
    };

    let path_buf = Path::new(path);

    // Create parent directories if they don't exist
    if let Some(parent) = path_buf.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!("✗ Error creating directories: {}", e);
        return false;
    }

    // Use template-based creation
    let workspace_dir = Some(config.default_workspace.as_path());
    match app.create_entry_from_template(
        path_buf,
        template.as_deref(),
        title.as_deref(),
        workspace_dir,
    ) {
        Ok(_) => {
            println!("✓ Created entry: {}", path);
            true
        }
        Err(e) => {
            eprintln!("✗ Error creating entry: {}", e);
            false
        }
    }
}
