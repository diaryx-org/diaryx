//! Delete command handler — multi-entry deletion with tree-aware ordering

use std::collections::HashSet;
use std::io::{self, Write};
use std::path::PathBuf;

use diaryx_core::config::Config;
use diaryx_core::workspace::{Workspace, prepare_delete_plan, selection_includes_descendants};

use crate::cli::util::{load_config, resolve_paths};
use crate::cli::{CliDiaryxAppSync, CliWorkspace, block_on};

/// Handle the 'delete' command.
/// Returns true on success, false on error.
pub fn handle_delete(
    ws: &CliWorkspace,
    app: &CliDiaryxAppSync,
    paths: Vec<String>,
    yes: bool,
    dry_run: bool,
) -> bool {
    let config = match load_config() {
        Some(c) => c,
        None => return false,
    };

    if paths.is_empty() {
        eprintln!("✗ No paths specified");
        return false;
    }

    // Resolve all path arguments
    let mut resolved: Vec<PathBuf> = Vec::new();
    for p in &paths {
        let matched = resolve_paths(p, &config, app);
        if matched.is_empty() {
            eprintln!("✗ No files matched: {}", p);
            return false;
        }
        resolved.extend(matched);
    }

    if resolved.is_empty() {
        eprintln!("✗ No files matched");
        return false;
    }

    // Detect workspace root from the first resolved path
    let workspace_root_index = match block_on(ws.detect_workspace(&resolved[0])) {
        Ok(Some(idx)) => idx,
        Ok(None) => {
            // Fallback: use config default workspace
            if let Ok(Some(root)) = block_on(ws.find_root_index_in_dir(&config.default_workspace)) {
                root
            } else {
                eprintln!(
                    "✗ Could not detect workspace for: {}",
                    resolved[0].display()
                );
                return false;
            }
        }
        Err(e) => {
            eprintln!("✗ Error detecting workspace: {}", e);
            return false;
        }
    };

    let workspace_root = workspace_root_index
        .parent()
        .unwrap_or(&workspace_root_index)
        .to_path_buf();

    // Build a rooted workspace for correct link resolution
    let link_format = Config::load().map(|c| c.link_format).unwrap_or_default();
    let rws = Workspace::with_link_format(ws.fs_ref().clone(), workspace_root.clone(), link_format);

    // Build the workspace tree
    let mut visited = HashSet::new();
    let tree = match block_on(rws.build_tree_with_depth(&workspace_root_index, None, &mut visited))
    {
        Ok(t) => t,
        Err(e) => {
            eprintln!("✗ Error building workspace tree: {}", e);
            return false;
        }
    };

    // Compute the deletion plan using the tree-selection functions
    let plan = prepare_delete_plan(&tree, &resolved);
    if plan.is_empty() {
        println!("Nothing to delete.");
        return true;
    }

    let has_descendants = selection_includes_descendants(&tree, &resolved);

    // Display what will be deleted
    println!("Will delete {} entries:", plan.len());
    for path in &plan {
        // Display relative to workspace root when possible
        let display = path.strip_prefix(&workspace_root).unwrap_or(path).display();
        println!("  {}", display);
    }

    if has_descendants {
        println!();
        println!("This will also remove descendant entries.");
    }

    // Dry-run: stop here
    if dry_run {
        println!();
        println!("(dry run — no changes made)");
        return true;
    }

    // Confirm unless --yes
    if !yes {
        println!();
        print!("Continue? [y/N] ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            eprintln!("✗ Failed to read input");
            return false;
        }

        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            println!("Cancelled.");
            return true;
        }
    }

    // Execute the deletion plan
    let mut deleted = 0usize;
    let mut had_error = false;
    for path in &plan {
        match block_on(rws.delete_entry(path)) {
            Ok(()) => {
                deleted += 1;
            }
            Err(e) => {
                eprintln!("✗ Error deleting {}: {}", path.display(), e);
                had_error = true;
            }
        }
    }

    if deleted > 0 {
        println!("✓ Deleted {} entries", deleted);
    }

    !had_error
}
