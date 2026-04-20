use crate::util::{require_env, run_checked, workspace_root};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "Usage: cargo xtask tauri <subcommand> [args...]

Subcommands:
  macos [args...]         Run `cargo tauri <args>` from apps/tauri with the
                          macOS/iOS clang toolchain pinned. Defaults to `dev`
                          when no args are given.
                          Flags:
                            --dev-ipc   Inject `-- --features dev-ipc`
  ios [args...]           Clean stale swift-rs build artifacts, then run
                          `cargo tauri ios <args> -- --features apple` from
                          apps/tauri. Defaults to `dev` when no args are given.
  render-updater-config   Render apps/tauri/src-tauri/tauri.updater.conf.json
                          from the TAURI_UPDATER_PUBLIC_KEY env var.
";

pub fn run(args: &[String]) -> Result<(), String> {
    let Some((sub, rest)) = args.split_first() else {
        println!("{USAGE}");
        return Ok(());
    };
    if sub == "-h" || sub == "--help" || sub == "help" {
        println!("{USAGE}");
        return Ok(());
    }
    match sub.as_str() {
        "macos" => run_macos(rest),
        "ios" => run_ios(rest),
        "render-updater-config" => render_updater_config(rest),
        other => Err(format!("unknown tauri subcommand: {other}\n\n{USAGE}")),
    }
}

fn run_macos(args: &[String]) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("tauri macos requires macOS".into());
    }
    let (mut tauri_args, dev_ipc) = extract_dev_ipc_flag(args);
    if tauri_args.is_empty() {
        tauri_args.push("dev".into());
    }
    if dev_ipc {
        tauri_args = inject_features_flag(tauri_args, "dev-ipc");
    }

    let tauri_dir = workspace_root().join("apps/tauri");
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&tauri_dir).arg("tauri").args(&tauri_args);
    run_checked(&mut cmd, "cargo tauri")
}

fn run_ios(args: &[String]) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("tauri ios requires macOS".into());
    }
    clean_swift_rs_cache()?;

    let mut tauri_args: Vec<String> = args.iter().cloned().collect();
    if tauri_args.is_empty() {
        tauri_args.push("dev".into());
    }
    let tauri_args = inject_features_flag(tauri_args, "apple");

    let tauri_dir = workspace_root().join("apps/tauri");
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&tauri_dir)
        .args(["tauri", "ios"])
        .args(&tauri_args);
    run_checked(&mut cmd, "cargo tauri ios")
}

fn extract_dev_ipc_flag(args: &[String]) -> (Vec<String>, bool) {
    let mut dev_ipc = false;
    let rest: Vec<String> = args
        .iter()
        .filter(|a| {
            if a.as_str() == "--dev-ipc" {
                dev_ipc = true;
                false
            } else {
                true
            }
        })
        .cloned()
        .collect();
    (rest, dev_ipc)
}

fn inject_features_flag(mut args: Vec<String>, feature: &str) -> Vec<String> {
    if !args.iter().any(|a| a == "--") {
        args.push("--".into());
    }
    args.push("--features".into());
    args.push(feature.into());
    args
}

fn clean_swift_rs_cache() -> Result<(), String> {
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    if !target_dir.is_dir() {
        return Ok(());
    }

    let mut to_remove: Vec<PathBuf> = Vec::new();
    collect_swift_rs_build_dirs(&target_dir, &mut to_remove)
        .map_err(|e| format!("walk {}: {e}", target_dir.display()))?;
    to_remove.sort();
    to_remove.dedup();

    for build_dir in &to_remove {
        fs::remove_dir_all(build_dir)
            .map_err(|e| format!("rm -rf {}: {e}", build_dir.display()))?;
    }
    if !to_remove.is_empty() {
        println!(
            "==> Cleaned {} stale swift-rs build dir(s)",
            to_remove.len()
        );
    }
    Ok(())
}

// swift-rs writes absolute paths into Swift module cache artifacts. If the repo
// moves, stale artifacts can fail iOS builds with module cache path mismatches.
// Find every `.../build/<hash>/out/swift-rs` directory and queue its grand-
// parent `build/<hash>` for removal so Cargo re-runs the owning build script.
fn collect_swift_rs_build_dirs(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in rd {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if entry.file_name() == "swift-rs" {
            if let Some(build_dir) = swift_rs_build_dir(&path) {
                out.push(build_dir);
                continue;
            }
        }
        collect_swift_rs_build_dirs(&path, out)?;
    }
    Ok(())
}

fn swift_rs_build_dir(swift_rs_dir: &Path) -> Option<PathBuf> {
    let out_dir = swift_rs_dir.parent()?;
    if out_dir.file_name()? != "out" {
        return None;
    }
    let hash_dir = out_dir.parent()?;
    let build_dir = hash_dir.parent()?;
    if build_dir.file_name()? != "build" {
        return None;
    }
    Some(hash_dir.to_path_buf())
}

fn render_updater_config(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "Usage: cargo xtask tauri render-updater-config\n\n\
             Renders apps/tauri/src-tauri/tauri.updater.conf.json from the\n\
             TAURI_UPDATER_PUBLIC_KEY env var (a minisign public key).\n"
        );
        return Ok(());
    }
    if !args.is_empty() {
        return Err("render-updater-config takes no arguments".into());
    }
    let pubkey = require_env("TAURI_UPDATER_PUBLIC_KEY")?;
    let escaped = pubkey.replace('\\', "\\\\").replace('"', "\\\"");
    let content = format!(
        "{{\n  \"bundle\": {{\n    \"createUpdaterArtifacts\": true\n  }},\n  \"plugins\": {{\n    \"updater\": {{\n      \"pubkey\": \"{escaped}\",\n      \"endpoints\": [\n        \"https://github.com/diaryx-org/diaryx/releases/latest/download/latest.json\"\n      ],\n      \"windows\": {{\n        \"installMode\": \"passive\"\n      }}\n    }}\n  }}\n}}\n"
    );
    let out = workspace_root().join("apps/tauri/src-tauri/tauri.updater.conf.json");
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    fs::write(&out, &content).map_err(|e| format!("write {}: {e}", out.display()))?;
    println!("Wrote updater config to {}", out.display());
    Ok(())
}
