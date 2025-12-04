//! Shared utilities for CLI commands

use diaryx_core::config::Config;
use diaryx_core::entry::DiaryxApp;
use diaryx_core::fs::RealFileSystem;
use diaryx_core::workspace::Workspace;
use glob::glob;
use serde_yaml::Value;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Result of a confirmation prompt
pub enum ConfirmResult {
    Yes,
    No,
    All,
    Quit,
}

/// Prompt user for confirmation
pub fn prompt_confirm(message: &str) -> ConfirmResult {
    print!("{} [y/n/a/q] ", message);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return ConfirmResult::Quit;
    }

    match input.trim().to_lowercase().as_str() {
        "y" | "yes" => ConfirmResult::Yes,
        "n" | "no" => ConfirmResult::No,
        "a" | "all" => ConfirmResult::All,
        "q" | "quit" => ConfirmResult::Quit,
        _ => ConfirmResult::No,
    }
}

/// Check if a path pattern contains glob characters
pub fn is_glob_pattern(path: &str) -> bool {
    path.contains('*') || path.contains('?') || path.contains('[')
}

/// Resolve a path pattern to a list of files
/// Returns either a single resolved path (for dates/literals) or multiple paths (for globs/workspace)
///
/// Special handling:
/// - `.` resolves to all files in the current workspace (traversing from local index)
/// - Glob patterns (`*.md`, `**/*.md`) match files by pattern
/// - Date strings (via chrono-english) resolve to dated entry paths
/// - Literal paths are returned as-is
pub fn resolve_paths(path: &str, config: &Config, app: &DiaryxApp<RealFileSystem>) -> Vec<PathBuf> {
    // Handle directories as workspace-aware path resolution
    let path_buf = Path::new(path);
    if path_buf.is_dir() {
        return resolve_workspace_files_in_dir(path_buf);
    }

    // Check if it's a glob pattern
    if is_glob_pattern(path) {
        match glob(path) {
            Ok(paths) => {
                let mut result: Vec<PathBuf> = paths
                    .filter_map(|p| p.ok())
                    .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
                    .collect();
                result.sort();
                result
            }
            Err(e) => {
                eprintln!("✗ Invalid glob pattern: {}", e);
                vec![]
            }
        }
    } else {
        // Try to resolve as date or literal path first
        let resolved = app.resolve_path(path, config);

        // If the resolved path exists, use it
        if resolved.exists() {
            return vec![resolved];
        }

        // If path doesn't exist and doesn't look like a date, try fuzzy matching
        // (date resolution would have returned a path in base_dir)
        if !resolved.starts_with(&config.base_dir) || path.contains('/') || path.contains('\\') {
            // This was likely meant as a literal path that doesn't exist
            // Try fuzzy matching in current directory
            if let Some(matches) = fuzzy_match_files(path) {
                if !matches.is_empty() {
                    return matches;
                }
            }
        }

        // Fall back to the resolved path (may not exist, but that's the user's intent)
        vec![resolved]
    }
}

/// Resolve a directory to all files in its workspace
/// Finds the local index in the directory and traverses its contents
fn resolve_workspace_files_in_dir(dir: &Path) -> Vec<PathBuf> {
    let fs = RealFileSystem;
    let workspace = Workspace::new(fs);

    // Canonicalize the directory path
    let dir = match dir.canonicalize() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("✗ Could not resolve directory '{}': {}", dir.display(), e);
            return vec![];
        }
    };

    // Find a local index in the directory
    match workspace.find_any_index_in_dir(&dir) {
        Ok(Some(index_path)) => {
            // Collect all files from the index
            match workspace.collect_workspace_files(&index_path) {
                Ok(files) => files,
                Err(e) => {
                    eprintln!("✗ Error traversing workspace: {}", e);
                    vec![]
                }
            }
        }
        Ok(None) => {
            // No index found, fall back to all .md files in the directory
            let glob_pattern = format!("{}/*.md", dir.display());
            match glob(&glob_pattern) {
                Ok(paths) => {
                    let mut result: Vec<PathBuf> = paths.filter_map(|p| p.ok()).collect();
                    result.sort();
                    if result.is_empty() {
                        eprintln!(
                            "⚠ No index file found in '{}' and no .md files present",
                            dir.display()
                        );
                    }
                    result
                }
                Err(e) => {
                    eprintln!("✗ Error listing files: {}", e);
                    vec![]
                }
            }
        }
        Err(e) => {
            eprintln!("✗ Error searching for index: {}", e);
            vec![]
        }
    }
}

/// Fuzzy match a string against .md files in the current directory
/// Returns files where the filename (without extension) contains the query (case-insensitive)
/// or where the query is a prefix of the filename
fn fuzzy_match_files(query: &str) -> Option<Vec<PathBuf>> {
    let current_dir = std::env::current_dir().ok()?;
    let query_lower = query.to_lowercase();

    let mut matches: Vec<(PathBuf, usize)> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&current_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Only consider .md files
            if path.extension().is_none_or(|ext| ext != "md") {
                continue;
            }

            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let stem_lower = stem.to_lowercase();

                // Score the match:
                // - Exact match (without extension): highest priority (score 0)
                // - Prefix match: high priority (score 1)
                // - Contains match: lower priority (score 2)
                let score = if stem_lower == query_lower {
                    Some(0)
                } else if stem_lower.starts_with(&query_lower) {
                    Some(1)
                } else if stem_lower.contains(&query_lower) {
                    Some(2)
                } else {
                    None
                };

                if let Some(s) = score {
                    matches.push((path, s));
                }
            }
        }
    }

    if matches.is_empty() {
        return None;
    }

    // Sort by score (best first), then by path name
    matches.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    // Return all matches with the best score
    let best_score = matches[0].1;
    let best_matches: Vec<PathBuf> = matches
        .into_iter()
        .filter(|(_, score)| *score == best_score)
        .map(|(path, _)| path)
        .collect();

    Some(best_matches)
}

/// Load config or print error message
pub fn load_config() -> Option<Config> {
    match Config::load() {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("✗ Error loading config: {}", e);
            eprintln!("  Run 'diaryx init' first");
            None
        }
    }
}

/// Format a YAML value for display
pub fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Sequence(items) => {
            let items_str: Vec<String> = items.iter().map(format_value).collect();
            format!("[{}]", items_str.join(", "))
        }
        _ => serde_yaml::to_string(value)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}
