//! Git version history CLI command handlers.

use std::path::Path;

use diaryx_git::{
    CommitOptions, CommitResult, HealthTracker, RepoKind, commit_workspace, init_repo, open_repo,
};

use crate::cli::plugin_loader::CliSyncContext;

/// Handle the `diaryx commit` command.
pub fn handle_commit(
    workspace_root: &Path,
    message: Option<String>,
    skip_validation: bool,
) -> bool {
    // Load sync plugin to materialize workspace files
    let ctx = match CliSyncContext::load(workspace_root) {
        Some(ctx) => ctx,
        None => {
            eprintln!("No CRDT database found.");
            eprintln!("Run 'diaryx sync start' first to initialize sync state.");
            return false;
        }
    };

    // Materialize workspace files via sync plugin
    let materialized = match ctx.cmd("MaterializeWorkspace", serde_json::json!({})) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Failed to materialize workspace: {}", e);
            return false;
        }
    };

    let files: Vec<diaryx_git::commit::MaterializedFile> = materialized
        .get("files")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    let path = f.get("path")?.as_str()?.to_string();
                    let content = f.get("content")?.as_str()?.to_string();
                    Some(diaryx_git::commit::MaterializedFile { path, content })
                })
                .collect()
        })
        .unwrap_or_default();

    if files.is_empty() {
        eprintln!("No files to commit");
        return false;
    }

    // Open or init git repo
    let repo = match open_repo(workspace_root) {
        Ok(r) => r,
        Err(_) => {
            println!("Initializing git repository...");
            match init_repo(workspace_root, RepoKind::Standard) {
                Ok(r) => {
                    println!("  Created git repository at {}", workspace_root.display());
                    r
                }
                Err(e) => {
                    eprintln!("Failed to initialize git repository: {}", e);
                    return false;
                }
            }
        }
    };

    let options = CommitOptions {
        message,
        skip_validation,
        ..CommitOptions::default()
    };

    let mut tracker = HealthTracker::new();
    match commit_workspace(&files, &repo, &options, &mut tracker) {
        Ok(result) => {
            print_commit_result(&result);
            true
        }
        Err(e) => {
            eprintln!("Commit failed: {}", e);
            false
        }
    }
}

/// Handle the `diaryx log` command.
pub fn handle_log(workspace_root: &Path, count: usize) -> bool {
    let repo = match open_repo(workspace_root) {
        Ok(r) => r,
        Err(_) => {
            eprintln!("No git repository found. Run 'diaryx commit' first.");
            return false;
        }
    };

    let head = match repo.head() {
        Ok(h) => h,
        Err(_) => {
            println!("No commits yet.");
            return true;
        }
    };

    let mut revwalk = match repo.revwalk() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to walk commits: {}", e);
            return false;
        }
    };

    if let Err(e) = revwalk.push(head.target().unwrap()) {
        eprintln!("Failed to start revwalk: {}", e);
        return false;
    }

    let mut shown = 0;
    for oid in revwalk {
        if shown >= count {
            break;
        }

        let oid = match oid {
            Ok(o) => o,
            Err(e) => {
                eprintln!("Error reading commit: {}", e);
                continue;
            }
        };

        let commit = match repo.find_commit(oid) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error loading commit {}: {}", oid, e);
                continue;
            }
        };

        let short_id = &oid.to_string()[..8];
        let message = commit.message().unwrap_or("(no message)");
        let time = commit.time();
        let dt = chrono::DateTime::from_timestamp(time.seconds(), 0)
            .unwrap_or_default()
            .with_timezone(&chrono::Local);

        println!(
            "{} {} {}",
            short_id,
            dt.format("%Y-%m-%d %H:%M"),
            message.lines().next().unwrap_or("")
        );
        shown += 1;
    }

    if shown == 0 {
        println!("No commits found.");
    }

    true
}

fn print_commit_result(result: &CommitResult) {
    let short_id = &result.commit_id.to_string()[..8];
    println!("Committed {} files [{}]", result.file_count, short_id);
}
