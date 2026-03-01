//! Entry command handlers (today, yesterday, open, create, config)

use std::path::Path;

use diaryx_core::date::parse_date;
use diaryx_core::frontmatter;

use crate::cli::CliDiaryxAppSync;
use crate::cli::plugin_loader::CliSyncContext;
use crate::cli::util::{apply_workspace_config, load_config, resolve_paths};
use crate::editor::launch_editor;

/// Sync file changes to the local CRDT after editing.
///
/// This updates the workspace CRDT with any changes made to the file.
/// Only syncs if the sync plugin WASM is available and CRDT database exists.
///
/// Returns true if changes were synced, false otherwise.
fn sync_to_crdt(workspace_root: &Path, file_path: &Path, original_content: &str) -> bool {
    // Only sync if CRDT database already exists (user has used sync before)
    let ctx = match CliSyncContext::load(workspace_root) {
        Some(ctx) => ctx,
        None => return false, // No CRDT initialized or plugin not available, skip silently
    };

    // Read current content
    let current_content = match std::fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(_) => return false,
    };

    if current_content == original_content {
        return false; // No changes
    }

    // Get relative path for CRDT key (always use forward slashes for consistency)
    let rel_path = match file_path.strip_prefix(workspace_root) {
        Ok(p) => p
            .iter()
            .map(|c| c.to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => return false,
    };

    // Parse frontmatter and body
    let (fm, body) = match frontmatter::parse_or_empty(&current_content) {
        Ok(parsed) => (parsed.frontmatter, parsed.body),
        Err(_) => return false,
    };

    // Extract filename from path
    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    // Build metadata as JSON for the plugin command
    let metadata = serde_json::json!({
        "filename": filename,
        "title": fm.get("title").and_then(|v| v.as_str()),
        "part_of": fm.get("part_of").and_then(|v| v.as_str()),
        "contents": fm.get("contents").and_then(|v| {
            v.as_sequence().map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<&str>>()
            })
        }),
        "attachments": fm.get("attachments").and_then(|v| {
            v.as_sequence().map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str())
                    .map(|raw| serde_json::json!({
                        "path": raw,
                        "source": "local",
                    }))
                    .collect::<Vec<_>>()
            })
        }).unwrap_or_default(),
        "audience": fm.get("audience").and_then(|v| {
            v.as_sequence().map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<&str>>()
            })
        }),
        "description": fm.get("description").and_then(|v| v.as_str()),
        "deleted": false,
        "modified_at": chrono::Utc::now().timestamp_millis(),
    });

    // Update workspace CRDT (metadata) via plugin
    if let Err(e) = ctx.cmd(
        "SetCrdtFile",
        serde_json::json!({
            "path": rel_path,
            "metadata": metadata,
        }),
    ) {
        eprintln!("Warning: Could not update CRDT metadata: {}", e);
    }

    // Update body document via plugin
    if let Err(e) = ctx.cmd(
        "SetBodyContent",
        serde_json::json!({
            "doc_name": rel_path,
            "content": body,
        }),
    ) {
        eprintln!("Warning: Could not update CRDT body: {}", e);
    }

    true
}

/// Handle the 'today' command
/// Returns true on success, false on error
pub fn handle_today(app: &CliDiaryxAppSync, template: Option<String>) -> bool {
    let config = match load_config() {
        Some(c) => apply_workspace_config(c),
        None => return false,
    };

    match parse_date("today") {
        Ok(date) => {
            // Validate hierarchy and duplicate detection
            if let Ok(warnings) = app.validate_daily_hierarchy(&date, &config) {
                for warning in warnings {
                    eprintln!("! Warning: {}", warning);
                }
            }

            match app.ensure_dated_entry_with_template(&date, &config, template.as_deref()) {
                Ok(path) => {
                    println!("Opening: {}", path.display());

                    // Read content before opening editor
                    let original_content = std::fs::read_to_string(&path).unwrap_or_default();

                    if let Err(e) = launch_editor(&path, &config) {
                        eprintln!("✗ Error launching editor: {}", e);
                        return false;
                    }

                    // Sync changes to CRDT after editor closes
                    sync_to_crdt(&config.default_workspace, &path, &original_content);

                    true
                }
                Err(e) => {
                    eprintln!("✗ Error creating entry: {}", e);
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("✗ Error parsing date: {}", e);
            false
        }
    }
}

/// Handle the 'yesterday' command
/// Returns true on success, false on error
pub fn handle_yesterday(app: &CliDiaryxAppSync, template: Option<String>) -> bool {
    let config = match load_config() {
        Some(c) => apply_workspace_config(c),
        None => return false,
    };

    match parse_date("yesterday") {
        Ok(date) => {
            // Validate hierarchy and duplicate detection
            if let Ok(warnings) = app.validate_daily_hierarchy(&date, &config) {
                for warning in warnings {
                    eprintln!("! Warning: {}", warning);
                }
            }

            match app.ensure_dated_entry_with_template(&date, &config, template.as_deref()) {
                Ok(path) => {
                    println!("Opening: {}", path.display());

                    // Read content before opening editor
                    let original_content = std::fs::read_to_string(&path).unwrap_or_default();

                    if let Err(e) = launch_editor(&path, &config) {
                        eprintln!("✗ Error launching editor: {}", e);
                        return false;
                    }

                    // Sync changes to CRDT after editor closes
                    sync_to_crdt(&config.default_workspace, &path, &original_content);

                    true
                }
                Err(e) => {
                    eprintln!("✗ Error creating entry: {}", e);
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("✗ Error parsing date: {}", e);
            false
        }
    }
}

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
    let workspace_root = &config.default_workspace;

    // For single files that don't exist, check if this was meant as a date
    if paths.len() == 1 && !paths[0].exists() {
        // Try to parse as a date and create the entry
        if let Ok(date) = parse_date(path_or_date) {
            match app.ensure_dated_entry(&date, &config) {
                Ok(path) => {
                    println!("Opening: {}", path.display());

                    // Read content before opening editor
                    let original_content = std::fs::read_to_string(&path).unwrap_or_default();

                    if let Err(e) = launch_editor(&path, &config) {
                        eprintln!("✗ Error launching editor: {}", e);
                        return false;
                    }

                    // Sync changes to CRDT after editor closes
                    sync_to_crdt(workspace_root, &path, &original_content);

                    return true;
                }
                Err(e) => {
                    eprintln!("✗ Error creating entry: {}", e);
                    return false;
                }
            }
        }
        // Not a date and file doesn't exist
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

        // Read content before opening editor
        let original_content = std::fs::read_to_string(path).unwrap_or_default();

        if let Err(e) = launch_editor(path, &config) {
            eprintln!("✗ Error launching editor for {}: {}", path.display(), e);
            had_error = true;
        } else {
            // Sync changes to CRDT after editor closes (only if editor succeeded)
            sync_to_crdt(workspace_root, path, &original_content);
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
