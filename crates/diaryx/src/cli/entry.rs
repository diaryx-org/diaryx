//! Entry command handlers (today, yesterday, open, create, config)

use std::path::Path;

use diaryx_core::crdt::{BinaryRef, FileMetadata};
use diaryx_core::date::parse_date;
use diaryx_core::frontmatter;

use crate::cli::CliDiaryxAppSync;
use crate::cli::sync::CrdtContext;
use crate::cli::util::{
    canonicalize_frontmatter_reference, detect_workspace_link_format, load_config,
    parse_link_format, resolve_paths,
};
use crate::editor::launch_editor;

/// Sync file changes to the local CRDT after editing.
///
/// This updates the workspace CRDT with any changes made to the file.
/// Only syncs if the CRDT database already exists (user has used sync before).
///
/// Returns true if changes were synced, false otherwise.
fn sync_to_crdt(workspace_root: &Path, file_path: &Path, original_content: &str) -> bool {
    // Only sync if CRDT database already exists (user has used sync before)
    let ctx = match CrdtContext::load(workspace_root) {
        Some(ctx) => ctx,
        None => return false, // No CRDT initialized, skip silently
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
    let workspace_link_format_hint = detect_workspace_link_format(workspace_root);
    let file_link_format_hint = fm
        .get("link_format")
        .and_then(|v| v.as_str())
        .and_then(parse_link_format)
        .or(workspace_link_format_hint);
    let file_path_for_links = Path::new(&rel_path);

    // Extract filename from path
    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    // Build FileMetadata from frontmatter
    let metadata = FileMetadata {
        filename,
        title: fm.get("title").and_then(|v| v.as_str()).map(String::from),
        part_of: fm.get("part_of").and_then(|v| v.as_str()).map(|raw| {
            canonicalize_frontmatter_reference(raw, file_path_for_links, file_link_format_hint)
        }),
        contents: fm.get("contents").and_then(|v| {
            v.as_sequence().map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str())
                    .map(|raw| {
                        canonicalize_frontmatter_reference(
                            raw,
                            file_path_for_links,
                            file_link_format_hint,
                        )
                    })
                    .collect()
            })
        }),
        attachments: fm
            .get("attachments")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str())
                    .map(|raw| BinaryRef {
                        path: canonicalize_frontmatter_reference(
                            raw,
                            file_path_for_links,
                            file_link_format_hint,
                        ),
                        source: "local".to_string(),
                        hash: String::new(),
                        mime_type: String::new(),
                        size: 0,
                        uploaded_at: None,
                        deleted: false,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        deleted: false,
        audience: fm.get("audience").and_then(|v| {
            v.as_sequence().map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
        }),
        description: fm
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from),
        extra: std::collections::HashMap::new(),
        modified_at: chrono::Utc::now().timestamp_millis(),
    };

    // Update workspace CRDT (metadata)
    if let Err(e) = ctx.workspace_crdt.set_file(&rel_path, metadata) {
        eprintln!("Warning: Could not update CRDT metadata: {}", e);
    }

    // Update body document
    let body_doc = ctx.body_manager.get_or_create(&rel_path);
    if let Err(e) = body_doc.set_body(&body) {
        eprintln!("Warning: Could not update CRDT body: {}", e);
    }

    true
}

/// Handle the 'today' command
/// Returns true on success, false on error
pub fn handle_today(app: &CliDiaryxAppSync, template: Option<String>) -> bool {
    let config = match load_config() {
        Some(c) => c,
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
        Some(c) => c,
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
/// Supports:
/// - Date strings: "today", "yesterday", "last friday", "2024-01-15"
/// - Fuzzy file matching: "README" -> README.md, "dia" -> diary.md
/// - Exact paths: "./notes/todo.md"
/// - Globs open multiple files: "*.md"
/// - Directories open all workspace files: "."
/// Returns true on success, false on error
pub fn handle_open(app: &CliDiaryxAppSync, path_or_date: &str) -> bool {
    let config = match load_config() {
        Some(c) => c,
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
/// Supports fuzzy path resolution for the parent directory
/// Returns true on success, false on error
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
