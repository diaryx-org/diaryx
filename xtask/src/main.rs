mod build_plugin;
mod build_wasm;
mod publish_ios;
mod publish_macos;
mod release_plugin;
mod sync_bindings;
mod sync_marketplace;
mod sync_versions;
mod tauri;
mod update_agents_index;
mod util;

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some((sub, rest)) = args.split_first() else {
        print_help();
        return ExitCode::FAILURE;
    };

    let result = match sub.as_str() {
        "build-wasm" => build_wasm::run(rest),
        "build-plugin" => build_plugin::run(rest),
        "publish-ios" => publish_ios::run(rest),
        "publish-macos" => publish_macos::run(rest),
        "release-plugin" => release_plugin::run(rest),
        "sync-bindings" => sync_bindings::run(rest),
        "sync-marketplace" => sync_marketplace::run(rest),
        "sync-versions" => sync_versions::run(rest),
        "tauri" => tauri::run(rest),
        "update-agents-index" => update_agents_index::run(rest),
        "help" | "-h" | "--help" => {
            print_help();
            return ExitCode::SUCCESS;
        }
        other => {
            eprintln!("unknown xtask subcommand: {other}\n");
            print_help();
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
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
}
