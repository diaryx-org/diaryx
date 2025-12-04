//! Workspace command handlers

use diaryx_core::config::Config;
use diaryx_core::editor::launch_editor;
use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::RealFileSystem;
use diaryx_core::workspace::Workspace;
use serde_yaml::Value;
use std::path::{Path, PathBuf};

use crate::cli::args::WorkspaceCommands;
use crate::cli::util::resolve_paths;

pub fn handle_workspace_command(
    command: WorkspaceCommands,
    workspace_override: Option<PathBuf>,
    ws: &Workspace<RealFileSystem>,
    app: &DiaryxApp<RealFileSystem>,
) {
    let config = Config::load().ok();
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    match command {
        WorkspaceCommands::Info => {
            handle_info(workspace_override, ws, &config, &current_dir);
        }

        WorkspaceCommands::Init {
            dir,
            title,
            description,
        } => {
            handle_init(ws, dir, title, description, &current_dir);
        }

        WorkspaceCommands::Path => {
            handle_path(workspace_override, ws, &config, &current_dir);
        }

        WorkspaceCommands::Add {
            parent_or_child,
            child,
            dry_run,
        } => {
            if let Some(ref cfg) = config {
                let (parent, child) =
                    resolve_parent_child(ws, &current_dir, &parent_or_child, child);
                if let (Some(p), Some(c)) = (parent, child) {
                    handle_add(app, cfg, &p, &c, dry_run);
                }
            } else {
                eprintln!("✗ No config found. Run 'diaryx init' first");
            }
        }

        WorkspaceCommands::Create {
            parent_or_name,
            name,
            title,
            description,
            index,
            edit,
        } => {
            if let Some(ref cfg) = config {
                let (parent, name) = resolve_parent_name(ws, &current_dir, &parent_or_name, name);
                if let (Some(p), Some(n)) = (parent, name) {
                    handle_create(app, cfg, &p, &n, title, description, index, edit);
                }
            } else {
                eprintln!("✗ No config found. Run 'diaryx init' first");
            }
        }

        WorkspaceCommands::Remove {
            parent_or_child,
            child,
            dry_run,
        } => {
            if let Some(ref cfg) = config {
                let (parent, child) =
                    resolve_parent_child(ws, &current_dir, &parent_or_child, child);
                if let (Some(p), Some(c)) = (parent, child) {
                    handle_remove(app, cfg, &p, &c, dry_run);
                }
            } else {
                eprintln!("✗ No config found. Run 'diaryx init' first");
            }
        }

        WorkspaceCommands::Mv {
            source,
            dest,
            dry_run,
        } => {
            if let Some(ref cfg) = config {
                handle_mv(app, cfg, ws, &source, &dest, dry_run);
            } else {
                eprintln!("✗ No config found. Run 'diaryx init' first");
            }
        }

        WorkspaceCommands::Orphans { dir, recursive } => {
            handle_orphans(ws, &current_dir, dir, recursive);
        }
    }
}

/// Handle the 'workspace mv' command
/// Moves/renames a file while updating workspace hierarchy references
fn handle_mv(
    app: &DiaryxApp<RealFileSystem>,
    config: &Config,
    _ws: &Workspace<RealFileSystem>,
    source: &str,
    dest: &str,
    dry_run: bool,
) {
    // Resolve source path (should be a single file)
    let source_paths = resolve_paths(source, config, app);
    if source_paths.is_empty() {
        eprintln!("✗ No files matched source: {}", source);
        return;
    }
    if source_paths.len() > 1 {
        eprintln!("✗ Source must be a single file, but matched multiple:");
        for p in &source_paths {
            eprintln!("  {}", p.display());
        }
        return;
    }
    let source_path = &source_paths[0];

    if !source_path.exists() {
        eprintln!("✗ Source file does not exist: {}", source_path.display());
        return;
    }

    // Determine destination path
    let dest_input = PathBuf::from(dest);
    // Canonicalize the destination directory if it exists, to get clean paths
    let dest_path = if dest_input.is_dir() {
        // If dest is a directory, move file into it with same name
        let canonical_dir = dest_input.canonicalize().unwrap_or(dest_input);
        canonical_dir.join(source_path.file_name().unwrap_or_default())
    } else if !dest.ends_with(".md") {
        // Add .md extension if not present
        let with_ext = PathBuf::from(format!("{}.md", dest));
        // Try to canonicalize the parent directory
        if let Some(parent) = with_ext.parent() {
            if parent.exists() {
                if let Ok(canonical_parent) = parent.canonicalize() {
                    canonical_parent.join(with_ext.file_name().unwrap_or_default())
                } else {
                    with_ext
                }
            } else {
                with_ext
            }
        } else {
            with_ext
        }
    } else {
        // Try to canonicalize the parent directory
        if let Some(parent) = dest_input.parent() {
            if parent.exists() {
                if let Ok(canonical_parent) = parent.canonicalize() {
                    canonical_parent.join(dest_input.file_name().unwrap_or_default())
                } else {
                    dest_input
                }
            } else {
                dest_input
            }
        } else {
            dest_input
        }
    };

    if dest_path.exists() {
        eprintln!("✗ Destination already exists: {}", dest_path.display());
        return;
    }

    // Read source file's frontmatter to find its part_of (parent)
    let source_str = source_path.to_string_lossy();
    let parent_path = match app.get_frontmatter_property(&source_str, "part_of") {
        Ok(Some(serde_yaml::Value::String(part_of))) => {
            // Resolve relative path from source file's directory
            source_path.parent().map(|dir| dir.join(&part_of))
        }
        _ => None,
    };

    // Canonicalize source path for consistent relative path calculations
    let source_canonical = source_path.canonicalize().unwrap_or_else(|_| source_path.clone());

    // Calculate what the old and new relative paths would be (from parent's perspective)
    let (old_relative, new_relative) = if let Some(ref parent) = parent_path {
        // Canonicalize parent path for consistent relative path calculations
        let parent_canonical = parent.canonicalize().unwrap_or_else(|_| parent.clone());
        let parent_dir = parent_canonical.parent().unwrap_or(&parent_canonical);
        
        let old_rel = pathdiff::diff_paths(&source_canonical, parent_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| {
                source_path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
        let new_rel = pathdiff::diff_paths(&dest_path, parent_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| {
                dest_path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
        (old_rel, new_rel)
    } else {
        // No parent, just use filenames
        let old_rel = source_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        let new_rel = dest_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        (old_rel, new_rel)
    };

    if dry_run {
        println!(
            "Would move '{}' to '{}'",
            source_path.display(),
            dest_path.display()
        );
        if let Some(ref parent) = parent_path {
            if parent.exists() {
                println!(
                    "Would update contents in '{}': '{}' -> '{}'",
                    parent.display(),
                    old_relative,
                    new_relative
                );
            }
        }
        // Check for children that reference this file
        if let Ok(Some(serde_yaml::Value::Sequence(contents))) =
            app.get_frontmatter_property(&source_str, "contents")
        {
            if !contents.is_empty() {
                println!("Would update part_of in {} child file(s)", contents.len());
            }
        }
        return;
    }

    // 1. Update parent's contents (if parent exists)
    if let Some(ref parent) = parent_path {
        if parent.exists() {
            let parent_str = parent.to_string_lossy();
            if let Ok(Some(serde_yaml::Value::Sequence(mut items))) =
                app.get_frontmatter_property(&parent_str, "contents")
            {
                // Replace old reference with new reference
                for item in &mut items {
                    if let serde_yaml::Value::String(s) = item {
                        if *s == old_relative {
                            *s = new_relative.clone();
                        }
                    }
                }
                if let Err(e) = app.set_frontmatter_property(
                    &parent_str,
                    "contents",
                    serde_yaml::Value::Sequence(items),
                ) {
                    eprintln!("✗ Error updating parent contents: {}", e);
                    return;
                }
                println!(
                    "✓ Updated contents in '{}': '{}' -> '{}'",
                    parent.display(),
                    old_relative,
                    new_relative
                );
            }
        }
    }

    // 2. Get children before moving (need to update their part_of after move)
    let children: Vec<PathBuf> = match app.get_frontmatter_property(&source_str, "contents") {
        Ok(Some(serde_yaml::Value::Sequence(contents))) => contents
            .iter()
            .filter_map(|v| {
                if let serde_yaml::Value::String(s) = v {
                    source_path.parent().map(|dir| dir.join(s))
                } else {
                    None
                }
            })
            .filter(|p| p.exists())
            .collect(),
        _ => Vec::new(),
    };

    // 3. Move the file
    if let Some(parent_dir) = dest_path.parent() {
        if !parent_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(parent_dir) {
                eprintln!("✗ Error creating directory: {}", e);
                return;
            }
        }
    }

    if let Err(e) = std::fs::rename(source_path, &dest_path) {
        eprintln!("✗ Error moving file: {}", e);
        return;
    }
    println!(
        "✓ Moved '{}' to '{}'",
        source_path.display(),
        dest_path.display()
    );

    // 4. Update part_of in the moved file (if parent is in same directory, path stays same)
    // Only update if the relative path to parent changed
    if parent_path.is_some() {
        let dest_str = dest_path.to_string_lossy();
        if let Some(ref parent) = parent_path {
            let new_part_of = calculate_relative_path(&dest_path, parent);
            if let Err(e) = app.set_frontmatter_property(
                &dest_str,
                "part_of",
                serde_yaml::Value::String(new_part_of),
            ) {
                eprintln!("⚠ Error updating part_of in moved file: {}", e);
            }
        }
    }

    // 5. Update children's part_of to point to new location
    for child in &children {
        let child_str = child.to_string_lossy();
        let new_part_of = calculate_relative_path(child, &dest_path);
        if let Err(e) = app.set_frontmatter_property(
            &child_str,
            "part_of",
            serde_yaml::Value::String(new_part_of),
        ) {
            eprintln!("⚠ Error updating part_of in '{}': {}", child.display(), e);
        } else {
            println!("✓ Updated part_of in '{}'", child.display());
        }
    }
}

/// Handle the 'workspace orphans' command
/// Finds markdown files not connected to the workspace hierarchy
fn handle_orphans(
    ws: &Workspace<RealFileSystem>,
    current_dir: &Path,
    dir: Option<PathBuf>,
    recursive: bool,
) {
    let search_dir = dir.unwrap_or_else(|| current_dir.to_path_buf());

    // Find the local index to get workspace files
    let index_path = match ws.find_any_index_in_dir(&search_dir) {
        Ok(Some(path)) => path,
        Ok(None) => {
            eprintln!("✗ No index file found in '{}'", search_dir.display());
            return;
        }
        Err(e) => {
            eprintln!("✗ Error finding index: {}", e);
            return;
        }
    };

    // Collect all files in the workspace hierarchy
    let workspace_files: std::collections::HashSet<PathBuf> =
        match ws.collect_workspace_files(&index_path) {
            Ok(files) => files
                .into_iter()
                .filter_map(|p| p.canonicalize().ok())
                .collect(),
            Err(e) => {
                eprintln!("✗ Error collecting workspace files: {}", e);
                return;
            }
        };

    // Find all markdown files in the directory
    let all_md_files = if recursive {
        collect_md_files_recursive(&search_dir)
    } else {
        collect_md_files(&search_dir)
    };

    // Find orphans (files not in workspace hierarchy)
    let mut orphans: Vec<PathBuf> = all_md_files
        .into_iter()
        .filter(|p| {
            if let Ok(canonical) = p.canonicalize() {
                !workspace_files.contains(&canonical)
            } else {
                true // Include files we can't canonicalize
            }
        })
        .collect();

    orphans.sort();

    if orphans.is_empty() {
        println!("✓ No orphan files found");
    } else {
        println!("Found {} orphan file(s):", orphans.len());
        for orphan in &orphans {
            // Try to show relative path
            if let Ok(relative) = orphan.strip_prefix(&search_dir) {
                println!("  {}", relative.display());
            } else {
                println!("  {}", orphan.display());
            }
        }
    }
}

/// Collect markdown files in a directory (non-recursive)
fn collect_md_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
                files.push(path);
            }
        }
    }
    files
}

/// Collect markdown files in a directory (recursive)
fn collect_md_files_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_md_files_recursive_helper(dir, &mut files);
    files
}

fn collect_md_files_recursive_helper(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                collect_md_files_recursive_helper(&path, files);
            } else if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
                files.push(path);
            }
        }
    }
}

/// Resolve parent and child arguments, using local index as default parent if only one arg provided
fn resolve_parent_child(
    ws: &Workspace<RealFileSystem>,
    current_dir: &Path,
    parent_or_child: &str,
    child: Option<String>,
) -> (Option<String>, Option<String>) {
    match child {
        // Two arguments provided: parent_or_child is parent, child is child
        Some(c) => (Some(parent_or_child.to_string()), Some(c)),
        // One argument provided: find local index as parent, parent_or_child is child
        None => match ws.find_any_index_in_dir(current_dir) {
            Ok(Some(index_path)) => {
                let parent = index_path.to_string_lossy().to_string();
                (Some(parent), Some(parent_or_child.to_string()))
            }
            Ok(None) => {
                eprintln!("✗ No index file found in current directory");
                eprintln!("  Either specify a parent explicitly or create an index first");
                (None, None)
            }
            Err(e) => {
                eprintln!("✗ Error finding index: {}", e);
                (None, None)
            }
        },
    }
}

/// Resolve parent and name arguments for create, using local index as default parent if only one arg provided
fn resolve_parent_name(
    ws: &Workspace<RealFileSystem>,
    current_dir: &Path,
    parent_or_name: &str,
    name: Option<String>,
) -> (Option<String>, Option<String>) {
    match name {
        // Two arguments provided: parent_or_name is parent, name is name
        Some(n) => (Some(parent_or_name.to_string()), Some(n)),
        // One argument provided: find local index as parent, parent_or_name is name
        None => match ws.find_any_index_in_dir(current_dir) {
            Ok(Some(index_path)) => {
                let parent = index_path.to_string_lossy().to_string();
                (Some(parent), Some(parent_or_name.to_string()))
            }
            Ok(None) => {
                eprintln!("✗ No index file found in current directory");
                eprintln!("  Either specify a parent explicitly or create an index first");
                (None, None)
            }
            Err(e) => {
                eprintln!("✗ Error finding index: {}", e);
                (None, None)
            }
        },
    }
}

/// Handle the 'workspace info' command
fn handle_info(
    workspace_override: Option<PathBuf>,
    ws: &Workspace<RealFileSystem>,
    config: &Option<Config>,
    current_dir: &Path,
) {
    let root_path = if let Some(ref override_path) = workspace_override {
        override_path.clone()
    } else if let Ok(Some(detected)) = ws.detect_workspace(current_dir) {
        detected
    } else if let Some(ref cfg) = config {
        if let Ok(Some(root)) = ws.find_root_index_in_dir(&cfg.base_dir) {
            root
        } else {
            eprintln!("✗ No workspace found");
            eprintln!("  Run 'diaryx init' or 'diaryx workspace init' first");
            return;
        }
    } else {
        eprintln!("✗ No workspace found");
        eprintln!("  Run 'diaryx init' or 'diaryx workspace init' first");
        return;
    };

    match ws.workspace_info(&root_path) {
        Ok(tree_output) => {
            println!("{}", tree_output);
        }
        Err(e) => eprintln!("✗ Error reading workspace: {}", e),
    }
}

/// Handle the 'workspace init' command
fn handle_init(
    ws: &Workspace<RealFileSystem>,
    dir: Option<PathBuf>,
    title: Option<String>,
    description: Option<String>,
    current_dir: &Path,
) {
    let target_dir = dir.unwrap_or_else(|| current_dir.to_path_buf());

    match ws.init_workspace(&target_dir, title.as_deref(), description.as_deref()) {
        Ok(readme_path) => {
            println!("✓ Initialized workspace");
            println!("  Index file: {}", readme_path.display());
        }
        Err(e) => eprintln!("✗ Error initializing workspace: {}", e),
    }
}

/// Handle the 'workspace path' command
fn handle_path(
    workspace_override: Option<PathBuf>,
    ws: &Workspace<RealFileSystem>,
    config: &Option<Config>,
    current_dir: &Path,
) {
    let root_path = if let Some(ref override_path) = workspace_override {
        Some(override_path.clone())
    } else if let Ok(Some(detected)) = ws.detect_workspace(current_dir) {
        Some(detected)
    } else if let Some(ref cfg) = config {
        ws.find_root_index_in_dir(&cfg.base_dir).ok().flatten()
    } else {
        None
    };

    match root_path {
        Some(path) => {
            if let Some(dir) = path.parent() {
                println!("{}", dir.display());
            } else {
                println!("{}", path.display());
            }
        }
        None => {
            eprintln!("✗ No workspace found");
            eprintln!("  Run 'diaryx init' or 'diaryx workspace init' first");
        }
    }
}

/// Handle the 'workspace add' command
/// Adds an existing file as a child of a parent index
fn handle_add(
    app: &DiaryxApp<RealFileSystem>,
    config: &Config,
    parent: &str,
    child: &str,
    dry_run: bool,
) {
    // Resolve parent path (should be a single file)
    let parent_paths = resolve_paths(parent, config, app);
    if parent_paths.is_empty() {
        eprintln!("✗ No files matched parent: {}", parent);
        return;
    }
    if parent_paths.len() > 1 {
        eprintln!("✗ Parent must be a single file, but matched multiple:");
        for p in &parent_paths {
            eprintln!("  {}", p.display());
        }
        return;
    }
    let parent_path = &parent_paths[0];

    // Resolve child path (should be a single file)
    let child_paths = resolve_paths(child, config, app);
    if child_paths.is_empty() {
        eprintln!("✗ No files matched child: {}", child);
        return;
    }
    if child_paths.len() > 1 {
        eprintln!("✗ Child must be a single file, but matched multiple:");
        for p in &child_paths {
            eprintln!("  {}", p.display());
        }
        return;
    }
    let child_path = &child_paths[0];

    // Calculate relative path from parent to child
    let relative_child = calculate_relative_path(parent_path, child_path);
    let relative_parent = calculate_relative_path(child_path, parent_path);

    if dry_run {
        println!(
            "Would add '{}' to contents of '{}'",
            relative_child,
            parent_path.display()
        );
        println!(
            "Would set part_of to '{}' in '{}'",
            relative_parent,
            child_path.display()
        );
        return;
    }

    let parent_str = parent_path.to_string_lossy();
    let child_str = child_path.to_string_lossy();

    // Update parent's contents
    match app.get_frontmatter_property(&parent_str, "contents") {
        Ok(Some(Value::Sequence(mut items))) => {
            // Check if already present
            let child_value = Value::String(relative_child.clone());
            if items.contains(&child_value) {
                println!(
                    "⚠ '{}' is already in contents of '{}'",
                    relative_child,
                    parent_path.display()
                );
            } else {
                items.push(child_value);
                if let Err(e) =
                    app.set_frontmatter_property(&parent_str, "contents", Value::Sequence(items))
                {
                    eprintln!("✗ Error updating parent contents: {}", e);
                    return;
                }
                println!(
                    "✓ Added '{}' to contents of '{}'",
                    relative_child,
                    parent_path.display()
                );
            }
        }
        Ok(Some(_)) => {
            eprintln!("✗ Parent's 'contents' property is not a list");
            return;
        }
        Ok(None) => {
            // Create contents with just this child
            let items = vec![Value::String(relative_child.clone())];
            if let Err(e) =
                app.set_frontmatter_property(&parent_str, "contents", Value::Sequence(items))
            {
                eprintln!("✗ Error creating parent contents: {}", e);
                return;
            }
            println!(
                "✓ Created contents with '{}' in '{}'",
                relative_child,
                parent_path.display()
            );
        }
        Err(e) => {
            eprintln!("✗ Error reading parent: {}", e);
            return;
        }
    }

    // Update child's part_of
    if let Err(e) = app.set_frontmatter_property(
        &child_str,
        "part_of",
        Value::String(relative_parent.clone()),
    ) {
        eprintln!("✗ Error updating child part_of: {}", e);
        return;
    }
    println!(
        "✓ Set part_of to '{}' in '{}'",
        relative_parent,
        child_path.display()
    );
}

/// Handle the 'workspace create' command
/// Creates a new child file under a parent index
#[allow(clippy::too_many_arguments)]
fn handle_create(
    app: &DiaryxApp<RealFileSystem>,
    config: &Config,
    parent: &str,
    name: &str,
    title: Option<String>,
    description: Option<String>,
    is_index: bool,
    edit: bool,
) {
    // Resolve parent path
    let parent_paths = resolve_paths(parent, config, app);
    if parent_paths.is_empty() {
        eprintln!("✗ No files matched parent: {}", parent);
        return;
    }
    if parent_paths.len() > 1 {
        eprintln!("✗ Parent must be a single file, but matched multiple:");
        for p in &parent_paths {
            eprintln!("  {}", p.display());
        }
        return;
    }
    let parent_path = &parent_paths[0];

    // Determine child path (same directory as parent)
    let parent_dir = parent_path.parent().unwrap_or(Path::new("."));
    let child_filename = if name.ends_with(".md") {
        name.to_string()
    } else {
        format!("{}.md", name)
    };
    let child_path = parent_dir.join(&child_filename);

    if child_path.exists() {
        eprintln!("✗ File already exists: {}", child_path.display());
        return;
    }

    // Calculate relative paths
    let relative_child = child_filename.clone();
    let relative_parent = parent_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| parent_path.to_string_lossy().to_string());

    // Build frontmatter
    let display_title = title.unwrap_or_else(|| {
        // Convert filename to title (capitalize, remove extension)
        let stem = name.trim_end_matches(".md");
        stem.chars()
            .enumerate()
            .map(|(i, c)| {
                if i == 0 {
                    c.to_uppercase().next().unwrap_or(c)
                } else {
                    c
                }
            })
            .collect()
    });

    let mut frontmatter = format!("---\ntitle: {}\n", display_title);
    if let Some(ref desc) = description {
        frontmatter.push_str(&format!("description: {}\n", desc));
    }
    frontmatter.push_str(&format!("part_of: {}\n", relative_parent));
    if is_index {
        frontmatter.push_str("contents: []\n");
    }
    frontmatter.push_str("---\n\n");

    // Add body content
    let body = format!("# {}\n\n", display_title);
    let content = format!("{}{}", frontmatter, body);

    // Create the file
    if let Err(e) = std::fs::write(&child_path, &content) {
        eprintln!("✗ Error creating file: {}", e);
        return;
    }
    println!("✓ Created '{}'", child_path.display());

    // Update parent's contents
    let parent_str = parent_path.to_string_lossy();
    match app.get_frontmatter_property(&parent_str, "contents") {
        Ok(Some(Value::Sequence(mut items))) => {
            items.push(Value::String(relative_child.clone()));
            if let Err(e) =
                app.set_frontmatter_property(&parent_str, "contents", Value::Sequence(items))
            {
                eprintln!("✗ Error updating parent contents: {}", e);
                return;
            }
            println!(
                "✓ Added '{}' to contents of '{}'",
                relative_child,
                parent_path.display()
            );
        }
        Ok(Some(_)) => {
            eprintln!("⚠ Parent's 'contents' property is not a list, skipping update");
        }
        Ok(None) => {
            // Create contents with just this child
            let items = vec![Value::String(relative_child.clone())];
            if let Err(e) =
                app.set_frontmatter_property(&parent_str, "contents", Value::Sequence(items))
            {
                eprintln!("✗ Error creating parent contents: {}", e);
                return;
            }
            println!(
                "✓ Created contents with '{}' in '{}'",
                relative_child,
                parent_path.display()
            );
        }
        Err(e) => {
            eprintln!("⚠ Error reading parent (file was still created): {}", e);
        }
    }

    // Open in editor if requested
    if edit {
        if let Ok(cfg) = Config::load() {
            println!("Opening: {}", child_path.display());
            if let Err(e) = launch_editor(&child_path, &cfg) {
                eprintln!("✗ Error launching editor: {}", e);
            }
        }
    }
}

/// Handle the 'workspace remove' command
/// Removes a child from a parent's hierarchy (does not delete the file)
fn handle_remove(
    app: &DiaryxApp<RealFileSystem>,
    config: &Config,
    parent: &str,
    child: &str,
    dry_run: bool,
) {
    // Resolve parent path
    let parent_paths = resolve_paths(parent, config, app);
    if parent_paths.is_empty() {
        eprintln!("✗ No files matched parent: {}", parent);
        return;
    }
    if parent_paths.len() > 1 {
        eprintln!("✗ Parent must be a single file, but matched multiple:");
        for p in &parent_paths {
            eprintln!("  {}", p.display());
        }
        return;
    }
    let parent_path = &parent_paths[0];

    // Resolve child path
    let child_paths = resolve_paths(child, config, app);
    if child_paths.is_empty() {
        eprintln!("✗ No files matched child: {}", child);
        return;
    }
    if child_paths.len() > 1 {
        eprintln!("✗ Child must be a single file, but matched multiple:");
        for p in &child_paths {
            eprintln!("  {}", p.display());
        }
        return;
    }
    let child_path = &child_paths[0];

    // Calculate relative path from parent to child
    let relative_child = calculate_relative_path(parent_path, child_path);

    if dry_run {
        println!(
            "Would remove '{}' from contents of '{}'",
            relative_child,
            parent_path.display()
        );
        println!("Would remove part_of from '{}'", child_path.display());
        return;
    }

    let parent_str = parent_path.to_string_lossy();
    let child_str = child_path.to_string_lossy();

    // Update parent's contents
    match app.get_frontmatter_property(&parent_str, "contents") {
        Ok(Some(Value::Sequence(mut items))) => {
            let child_value = Value::String(relative_child.clone());
            let original_len = items.len();
            items.retain(|item| item != &child_value);

            if items.len() == original_len {
                println!(
                    "⚠ '{}' was not in contents of '{}'",
                    relative_child,
                    parent_path.display()
                );
            } else {
                if let Err(e) =
                    app.set_frontmatter_property(&parent_str, "contents", Value::Sequence(items))
                {
                    eprintln!("✗ Error updating parent contents: {}", e);
                    return;
                }
                println!(
                    "✓ Removed '{}' from contents of '{}'",
                    relative_child,
                    parent_path.display()
                );
            }
        }
        Ok(Some(_)) => {
            eprintln!("✗ Parent's 'contents' property is not a list");
            return;
        }
        Ok(None) => {
            println!(
                "⚠ Parent '{}' has no contents property",
                parent_path.display()
            );
        }
        Err(e) => {
            eprintln!("✗ Error reading parent: {}", e);
            return;
        }
    }

    // Remove child's part_of
    if let Err(e) = app.remove_frontmatter_property(&child_str, "part_of") {
        eprintln!("✗ Error removing child part_of: {}", e);
        return;
    }
    println!("✓ Removed part_of from '{}'", child_path.display());
}

/// Calculate the relative path from one file to another
fn calculate_relative_path(from: &Path, to: &Path) -> String {
    // If they're in the same directory, just use the filename
    if from.parent() == to.parent() {
        return to
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| to.to_string_lossy().to_string());
    }

    // Try to use pathdiff for proper relative path calculation
    if let (Ok(from_canon), Ok(to_canon)) = (from.canonicalize(), to.canonicalize()) {
        if let Some(from_dir) = from_canon.parent() {
            if let Some(rel) = pathdiff::diff_paths(&to_canon, from_dir) {
                return rel.to_string_lossy().to_string();
            }
        }
    }

    // Fallback: just use the filename
    to.file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| to.to_string_lossy().to_string())
}
