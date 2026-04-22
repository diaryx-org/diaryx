use crate::util::{run_checked, workspace_root};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "Usage: cargo xtask clean [--dry-run]

Removes Cargo build artifacts across the workspace:
  - Runs `cargo clean` at the workspace root (wipes the main `target/`,
    including any rust-analyzer check dir inside it — re-run a cargo
    command to repopulate it).
  - Walks the workspace to find every stray `target/xtask/` left behind
    when `cargo xtask ...` is invoked from a subdirectory, and removes
    each. The walk prunes `.git/`, `node_modules/`, `.*` dot-dirs, and
    does not descend into any `target/` (so it's fast even with a full
    node_modules/).

Does not touch node_modules, dist/, generated WASM bundles, or anything
outside the repo (so a separate rust-analyzer CARGO_TARGET_DIR set via
your shell/editor is preserved).

Flags:
  --dry-run    Print what would be removed without removing it.
";

pub fn run(args: &[String]) -> Result<(), String> {
    let mut dry_run = false;
    for arg in args {
        match arg.as_str() {
            "--dry-run" | "-n" => dry_run = true,
            "-h" | "--help" | "help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown flag for clean: {other}\n\n{USAGE}")),
        }
    }

    let root = workspace_root();
    let canonical_xtask = root.join("target/xtask");
    let canonical_norm = canonical_xtask
        .canonicalize()
        .unwrap_or_else(|_| canonical_xtask.clone());

    let mut strays = Vec::new();
    walk(&root, &canonical_norm, &mut strays);
    strays.sort();
    strays.dedup();

    if dry_run {
        println!("[dry-run] would run: cargo clean (cwd: {})", root.display());
        for stray in &strays {
            println!("[dry-run] would remove: {}", stray.display());
        }
        return Ok(());
    }

    println!("==> cargo clean (workspace root)");
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&root).arg("clean");
    run_checked(&mut cmd, "cargo clean")?;

    for stray in &strays {
        remove_if_exists(stray)?;
        if let Some(parent) = stray.parent() {
            let _ = fs::remove_dir(parent);
        }
    }

    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    println!("==> rm -rf {}", path.display());
    fs::remove_dir_all(path).map_err(|e| format!("rm -rf {}: {e}", path.display()))?;
    Ok(())
}

/// Recursively find `target/xtask/` dirs under `dir` that are NOT the canonical
/// workspace `target/xtask/`. Prunes noisy dirs and does not descend into any
/// `target/` (it only inspects whether `target/xtask/` exists as a direct child).
fn walk(dir: &Path, canonical: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let name = match entry.file_name().into_string() {
            Ok(s) => s,
            Err(_) => continue,
        };
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let path = entry.path();
        if name == "target" {
            let xtask_sub = path.join("xtask");
            if xtask_sub.is_dir() {
                let normalized = xtask_sub
                    .canonicalize()
                    .unwrap_or_else(|_| xtask_sub.clone());
                if normalized != *canonical {
                    out.push(xtask_sub);
                }
            }
            continue;
        }
        walk(&path, canonical, out);
    }
}
