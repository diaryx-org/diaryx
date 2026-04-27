mod build_plugin;
mod build_wasm;
mod check;
mod clean;
mod install_hooks;
mod pre_commit;
mod publish_ios;
mod publish_macos;
mod release_plugin;
mod sync_bindings;
mod sync_marketplace;
mod sync_versions;
mod tauri;
mod update_agents_index;
mod util;
mod web;

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let stray_target = detect_stray_target_xtask();
    if let Some(path) = &stray_target {
        eprintln!(
            "warning: xtask built into a stray target dir at\n  {}\n\
             The canonical location is\n  {}\n\
             (Cargo's `xtask` alias uses `--target-dir target/xtask`, resolved relative to CWD,\n\
             so running xtask from a subdirectory creates these strays.)\n\
             Running the task now, then removing the stray tree on exit.\n",
            path.display(),
            util::workspace_root().join("target/xtask").display()
        );
    }

    let args: Vec<String> = env::args().skip(1).collect();
    let Some((sub, rest)) = args.split_first() else {
        print_help();
        cleanup_stray(stray_target.as_deref());
        return ExitCode::FAILURE;
    };

    let result = match sub.as_str() {
        "build-wasm" => build_wasm::run(rest),
        "build-plugin" => build_plugin::run(rest),
        "check" => check::run(rest),
        "clean" => clean::run(rest),
        "install-hooks" => install_hooks::run(rest),
        "pre-commit" => pre_commit::run(rest),
        "publish-ios" => publish_ios::run(rest),
        "publish-macos" => publish_macos::run(rest),
        "release-plugin" => release_plugin::run(rest),
        "sync-bindings" => sync_bindings::run(rest),
        "sync-marketplace" => sync_marketplace::run(rest),
        "sync-versions" => sync_versions::run(rest),
        "tauri" => tauri::run(rest),
        "update-agents-index" => update_agents_index::run(rest),
        "web" => web::run(rest),
        "help" | "-h" | "--help" => {
            print_help();
            cleanup_stray(stray_target.as_deref());
            return ExitCode::SUCCESS;
        }
        other => {
            eprintln!("unknown xtask subcommand: {other}\n");
            print_help();
            cleanup_stray(stray_target.as_deref());
            return ExitCode::FAILURE;
        }
    };

    let exit_code = match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    };
    cleanup_stray(stray_target.as_deref());
    exit_code
}

/// If the running binary lives under a `target/xtask/` that isn't the one at
/// the workspace root, return that stray `target/xtask/` path. Otherwise None.
fn detect_stray_target_xtask() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?.canonicalize().ok()?;
    let mut cursor = exe.parent()?;
    let stray = loop {
        let name = cursor.file_name()?.to_str()?;
        let parent = cursor.parent()?;
        let parent_name = parent.file_name().and_then(|n| n.to_str());
        if name == "xtask" && parent_name == Some("target") {
            break cursor.to_path_buf();
        }
        cursor = parent;
    };
    let canonical = util::workspace_root().join("target/xtask");
    let canonical_norm = canonical.canonicalize().unwrap_or(canonical);
    if stray == canonical_norm {
        None
    } else {
        Some(stray)
    }
}

/// Remove a stray `target/xtask/` tree. Unix unlinks on open-file descriptors
/// don't kill the running process, so this is safe even though the binary
/// lives inside the tree being removed.
fn cleanup_stray(path: Option<&std::path::Path>) {
    let Some(path) = path else { return };
    eprintln!("==> Removing stray target dir: {}", path.display());
    if let Err(e) = fs::remove_dir_all(path) {
        eprintln!("warning: failed to remove {}: {e}", path.display());
        return;
    }
    // If the parent `target/` dir is now empty, rmdir it too. Ignore errors —
    // non-empty parents (or missing ones) are fine.
    if let Some(parent) = path.parent() {
        let _ = fs::remove_dir(parent);
    }
}

fn print_help() {
    eprintln!("Usage: cargo xtask <task> [args...]\n");
    eprintln!("Tasks:");
    eprintln!(
        "  build-wasm [--panic-hook]    Build crates/diaryx_wasm for apps/web (wasm-pack + wasm-opt)"
    );
    eprintln!(
        "                               --panic-hook/--debug: enable console_error_panic_hook"
    );
    eprintln!(
        "  build-plugin <name>          Build a plugin WASM (pass --release for size-optimized output)"
    );
    eprintln!(
        "  check [--fix]                Run cargo fmt + clippy (rust lane) concurrently with"
    );
    eprintln!(
        "                               svelte-check (web lane). --fix applies clippy/fmt autofixes."
    );
    eprintln!(
        "  clean [--dry-run]            Run `cargo clean` and remove stray nested target/ dirs"
    );
    eprintln!("                               in apps/web and crates/diaryx_wasm");
    eprintln!(
        "  install-hooks [--force]      Install .git/hooks/pre-commit that calls `cargo xtask pre-commit`"
    );
    eprintln!(
        "  pre-commit [--all]           Run the project pre-commit checks (whitespace/EOF fixers,"
    );
    eprintln!("                               check-yaml/json, cargo fmt+clippy, svelte-check,");
    eprintln!(
        "                               sync-versions, update-agents-index). Invoked from the git hook."
    );
    eprintln!(
        "  publish-ios                  Build the iOS App Store export and upload via altool (macOS only)"
    );
    eprintln!(
        "  publish-macos <build>        Build, sign, package, and upload the macOS App Store .pkg (macOS only)"
    );
    eprintln!("  release-plugin <name> [--upload]");
    eprintln!(
        "                               Build a release WASM + dist/ artifact; with --upload, cuts a GitHub Release"
    );
    eprintln!("                               and opens a plugin-registry PR");
    eprintln!(
        "  sync-bindings                Sync ts-rs bindings into apps/web/src/lib/backend/generated/"
    );
    eprintln!(
        "  sync-marketplace             Fetch marketplace registries from the production CDN"
    );
    eprintln!(
        "  sync-versions                Propagate README.md version → Cargo.toml / tauri.conf.json / package.json / flake.nix"
    );
    eprintln!("  tauri <subcommand>           Run Diaryx Tauri builds. Subcommands: macos, ios,");
    eprintln!(
        "                               render-updater-config. See `cargo xtask tauri --help`."
    );
    eprintln!("  update-agents-index          Refresh the workspace tree in AGENTS.md");
    eprintln!(
        "  web [script] [args...]       Run `bun run <script>` in apps/web (defaults to `dev`)"
    );
}
