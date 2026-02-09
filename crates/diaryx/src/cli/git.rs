//! Git version history CLI command handlers.

use std::path::Path;
use std::sync::Arc;

use diaryx_core::crdt::git::{
    CommitOptions, CommitResult, RepoKind, commit_workspace, init_repo, open_repo,
};
use diaryx_core::crdt::self_healing::HealthTracker;
use diaryx_core::crdt::{BodyDocManager, CrdtStorage, SqliteStorage, WorkspaceCrdt};

/// Handle the `diaryx commit` command.
pub fn handle_commit(
    workspace_root: &Path,
    message: Option<String>,
    skip_validation: bool,
) -> bool {
    // Open or initialize storage
    let db_path = workspace_root.join(".diaryx").join("crdt.db");
    if !db_path.exists() {
        eprintln!("No CRDT database found at {}", db_path.display());
        eprintln!("Run 'diaryx sync start' first to initialize sync state.");
        return false;
    }

    let storage = match SqliteStorage::open(&db_path) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("Failed to open CRDT storage: {}", e);
            return false;
        }
    };

    // Load workspace CRDT - try to find the workspace ID
    let workspace_id = find_workspace_id(&storage);
    let workspace_id = match workspace_id {
        Some(id) => id,
        None => {
            eprintln!("No workspace found in CRDT storage.");
            return false;
        }
    };

    let workspace_doc_name = format!("workspace:{}", workspace_id);
    let workspace = match WorkspaceCrdt::load_with_name(storage.clone(), workspace_doc_name) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Failed to load workspace CRDT: {}", e);
            return false;
        }
    };
    let body_docs = BodyDocManager::new(storage.clone());

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
    match commit_workspace(
        &(storage as Arc<dyn CrdtStorage>),
        &workspace,
        &body_docs,
        &repo,
        &workspace_id,
        &options,
        &mut tracker,
    ) {
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
    if result.compacted {
        println!("  CRDT updates compacted");
    }
}

/// Find the workspace ID by scanning storage for workspace docs.
fn find_workspace_id(storage: &Arc<SqliteStorage>) -> Option<String> {
    let docs = storage.list_docs().ok()?;
    for doc in docs {
        if let Some(id) = doc.strip_prefix("workspace:") {
            return Some(id.to_string());
        }
    }
    None
}
