//! Entry command handlers (open, create)

use std::path::Path;

use crate::cli::CliDiaryxAppSync;
use crate::cli::util::{apply_workspace_config, load_config, resolve_paths};
use crate::editor::launch_editor;

#[cfg(feature = "plugins")]
use super::plugin_loader::CliPluginContext;

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
    _app: &CliDiaryxAppSync,
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

    // Derive title from filename if not provided
    let title = title.unwrap_or_else(|| {
        path_buf
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string()
    });

    let filename = path_buf
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled");

    // Try to render template via the templating plugin
    let content = 'tmpl: {
        #[cfg(feature = "plugins")]
        {
            let workspace_root = &config.default_workspace;
            if let Ok(ctx) = CliPluginContext::load(workspace_root, "diaryx.templating") {
                let params = serde_json::json!({
                    "template": template.as_deref().unwrap_or("note"),
                    "title": title,
                    "filename": filename,
                });
                if let Ok(result) = ctx.cmd("RenderCreationTemplate", params) {
                    if let Some(content) = result.as_str() {
                        break 'tmpl content.to_string();
                    }
                }
            }
        }
        // Fallback
        format!("---\ntitle: {}\n---\n\n# {}\n\n", title, title)
    };

    match std::fs::write(path_buf, &content) {
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
