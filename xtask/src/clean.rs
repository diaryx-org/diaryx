use crate::util::{run_checked, workspace_root};
use std::fs;
use std::path::Path;
use std::process::Command;

const USAGE: &str = "Usage: cargo xtask clean [--dry-run]

Removes Cargo build artifacts across the workspace:
  - Runs `cargo clean` at the workspace root (wipes the main `target/`,
    including any rust-analyzer check dir inside it — re-run a cargo
    command to repopulate it).
  - Removes stray nested target dirs left behind when `cargo xtask ...`
    is invoked from a subdirectory: apps/web/target, crates/diaryx_wasm/target.

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
    let nested = [
        root.join("apps/web/target"),
        root.join("crates/diaryx_wasm/target"),
    ];

    if dry_run {
        println!("[dry-run] would run: cargo clean (cwd: {})", root.display());
        for path in &nested {
            if path.is_dir() {
                println!("[dry-run] would remove: {}", path.display());
            }
        }
        return Ok(());
    }

    println!("==> cargo clean (workspace root)");
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&root).arg("clean");
    run_checked(&mut cmd, "cargo clean")?;

    for path in &nested {
        remove_if_exists(path)?;
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
