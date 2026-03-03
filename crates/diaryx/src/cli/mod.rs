#![doc = include_str!("./README.md")]

/// Clap argument definitions
mod args;

/// Attachment management
mod attachment;

/// Config command handlers
mod config;

/// Body content manipulation
mod content;

/// `open` and `create` commands
mod entry;

/// `diaryx_core` export with audience filtering
mod export;

/// Import from external formats (email, etc.)
mod import;

/// normalize command changes filenames to slug
mod normalize;

/// Frontmatter property manipulation
mod property;

/// Preview workspace as local HTTP server
mod preview;

/// `diaryx_core` publish
mod publish;

/// Search command handler
mod search;

/// diaryx sort command (sorting frontmatter properties)
mod sort;

/// Sync commands for remote synchronization
mod sync;

/// Navigate workspace hierarchy with TUI
mod nav;

/// Web-based editing via local sync server
#[cfg(feature = "edit")]
mod edit;

/// Template management
mod template;

/// Plugin storage for Extism plugins
mod plugin_storage;

/// Plugin loading and context (Extism integration)
mod plugin_loader;

/// Plugin management (install, remove, list, search, update)
mod plugin_manager;

/// Generic plugin command dispatcher (native handlers + WASM)
mod plugin_dispatch;

/// Shared CLI utilities
mod util;

/// `diaryx_core` workspace index management
mod workspace;

use clap::{CommandFactory, FromArgMatches};
use std::path::PathBuf;

use diaryx_core::config::Config;
use diaryx_core::entry::{DiaryxApp, DiaryxAppSync};
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_core::workspace::Workspace;

/// Type alias for the async filesystem used throughout the CLI.
/// Wraps RealFileSystem with SyncToAsyncFs for use with async-first core APIs.
pub type AsyncFs = SyncToAsyncFs<RealFileSystem>;

/// Type alias for DiaryxApp with the CLI's async filesystem.
/// Used for async operations (frontmatter, content, attachments).
#[allow(dead_code)]
pub type CliDiaryxApp = DiaryxApp<AsyncFs>;

/// Type alias for the sync DiaryxApp.
/// Used for operations that haven't been migrated to async yet.
pub type CliDiaryxAppSync = DiaryxAppSync<RealFileSystem>;

/// Type alias for Workspace with the CLI's async filesystem.
pub type CliWorkspace = Workspace<AsyncFs>;

/// Helper to run async operations in sync context
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    futures_lite::future::block_on(f)
}

pub use args::Cli;
use args::Commands;

/// Main entry point for the CLI.
///
/// Uses a two-phase parse:
/// 1. Discover installed plugin manifests (fast JSON reads, no WASM)
/// 2. Build augmented clap command with dynamic plugin commands
/// 3. Parse args — if a core command, dispatch normally; if a plugin command,
///    route through the generic plugin dispatcher
pub fn run_cli() {
    // Phase 1: Discover installed plugin manifests
    let plugin_manifests = plugin_loader::discover_plugin_manifests();

    // Phase 2: Build augmented clap command
    let mut app = Cli::command();
    for (plugin_id, manifest) in &plugin_manifests {
        for cli_cmd in &manifest.cli {
            app = app.subcommand(plugin_dispatch::build_plugin_command(cli_cmd, plugin_id));
        }
    }

    let matches = app.get_matches();

    // Phase 3: Try core command first
    if let Ok(cli) = Cli::from_arg_matches(&matches) {
        let success = dispatch_core_command(cli);
        if !success {
            std::process::exit(1);
        }
        return;
    }

    // Phase 4: Must be a plugin command
    if let Some((name, sub_matches)) = matches.subcommand() {
        let success =
            plugin_dispatch::dispatch_plugin_command(name, sub_matches, &plugin_manifests);
        if !success {
            std::process::exit(1);
        }
    }
}

/// Dispatch a core (statically-defined) CLI command.
fn dispatch_core_command(cli: Cli) -> bool {
    // Setup dependencies
    let async_fs = SyncToAsyncFs::new(RealFileSystem);
    let _app = DiaryxApp::new(async_fs.clone());
    let app_sync = DiaryxAppSync::new(RealFileSystem);
    let ws = Workspace::new(async_fs);

    match cli.command {
        Commands::Init {
            default_workspace,
            title,
            description,
        } => handle_init(default_workspace, title, description, &ws),

        Commands::Open { path } => entry::handle_open(&app_sync, &path),

        Commands::Config { command } => config::handle_config_command(command, cli.workspace, &ws),

        Commands::Create {
            path,
            template,
            title,
        } => entry::handle_create(&app_sync, &path, template, title),

        Commands::Property { operation } => property::handle_property_command(&app_sync, operation),

        Commands::Template { command } => template::handle_template_command(command, &app_sync),

        Commands::Sort {
            path,
            pattern,
            abc: _,
            default,
            index,
            yes,
            dry_run,
        } => sort::handle_sort_command(&app_sync, path, pattern, default, index, yes, dry_run),

        Commands::Workspace { command } => {
            workspace::handle_workspace_command(command, cli.workspace, &ws, &app_sync)
        }

        Commands::NormalizeFilename {
            path,
            title,
            yes,
            dry_run,
        } => {
            normalize::handle_normalize_filename(&app_sync, &ws, &path, title, yes, dry_run);
            true
        }

        Commands::Export {
            audience,
            destination,
            format,
            force,
            keep_audience,
            verbose,
            dry_run,
        } => {
            let workspace_root = match export::resolve_workspace_for_export(cli.workspace) {
                Ok(root) => root,
                Err(e) => {
                    eprintln!("✗ {}", e);
                    std::process::exit(1);
                }
            };
            export::handle_export(
                workspace_root,
                &audience,
                &destination,
                &format,
                force,
                keep_audience,
                verbose,
                dry_run,
            );
            true
        }

        Commands::Uninstall { yes } => handle_uninstall(yes),

        Commands::Content { operation } => {
            content::handle_content_command(&app_sync, operation);
            true
        }

        Commands::Search {
            pattern,
            frontmatter,
            property,
            case_sensitive,
            limit,
            context,
            count,
        } => {
            search::handle_search(
                pattern,
                cli.workspace,
                frontmatter,
                property,
                case_sensitive,
                limit,
                context,
                count,
            );
            true
        }

        Commands::Attachment { command } => {
            let current_dir = std::env::current_dir().unwrap_or_default();
            attachment::handle_attachment_command(command, &ws, &app_sync, &current_dir);
            true
        }

        Commands::Import { command } => {
            import::handle_import_command(command, cli.workspace);
            true
        }

        Commands::Plugin { command } => {
            plugin_manager::handle_plugin_command(command);
            true
        }

        Commands::Nav { path, depth } => {
            let current_dir = std::env::current_dir().unwrap_or_default();
            let config = Config::load().ok();
            nav::handle_nav(cli.workspace, &ws, &config, &current_dir, path, depth)
        }

        #[cfg(feature = "edit")]
        Commands::Edit { url, port } => {
            // Edit defaults to the current directory (not the configured default workspace)
            // since it's meant for editing local files in a web editor.
            let workspace_root = cli
                .workspace
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(edit::handle_edit(&workspace_root, url, port))
        }
    }
}

/// Resolve the workspace root directory from CLI arg or config.
///
/// Accepts a filesystem path **or** a registered workspace name. If the value
/// exists on disk it is used directly; otherwise we try to match it against the
/// workspace registry by name.
fn resolve_workspace_root(workspace_arg: Option<PathBuf>) -> PathBuf {
    if let Some(ws) = workspace_arg {
        // If the path exists on disk, use it directly
        if ws.exists() {
            return ws;
        }
        // Try matching as a registered workspace name
        if let Some(name) = ws.to_str() {
            if let Ok(cfg) = Config::load() {
                let reg = cfg.workspace_registry();
                if let Some(entry) = reg.find_by_name(name) {
                    if let Some(ref path) = entry.path {
                        return path.clone();
                    }
                }
            }
        }
        // Fall through to literal path (backward compat)
        return ws;
    }
    Config::load()
        .ok()
        .map(|c| c.default_workspace)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Handle the uninstall command
/// Returns true on success, false on error
fn handle_uninstall(yes: bool) -> bool {
    use std::io::{self, Write};

    // Determine the binary location
    let binary_path = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("✗ Could not determine binary location: {}", e);
            return false;
        }
    };

    println!("Uninstall diaryx");
    println!("================");
    println!();
    println!("This will remove the diaryx binary at:");
    println!("  {}", binary_path.display());
    println!();
    println!("Note: Your config, workspace, and entries will NOT be removed.");
    println!();

    // Confirm unless -y flag is provided
    if !yes {
        print!("Are you sure you want to uninstall? [y/N] ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            eprintln!("✗ Failed to read input");
            return false;
        }

        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            println!("Uninstall cancelled.");
            return true; // User cancelled, not an error
        }
    }

    // Remove the binary
    match std::fs::remove_file(&binary_path) {
        Ok(()) => {
            println!();
            println!("✓ Diaryx has been uninstalled.");
            println!();
            println!("To reinstall, run:");
            println!(
                "  curl -fsSL https://raw.githubusercontent.com/diaryx-org/diaryx-core/refs/heads/master/install.sh | bash"
            );
            true
        }
        Err(e) => {
            eprintln!("✗ Failed to remove binary: {}", e);
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                eprintln!();
                eprintln!("Try running with elevated permissions:");
                eprintln!("  sudo {} uninstall -y", binary_path.display());
            }
            false
        }
    }
}

/// Handle the init command
/// Returns true on success, false on error
fn handle_init(
    default_workspace: Option<PathBuf>,
    title: Option<String>,
    description: Option<String>,
    ws: &Workspace<SyncToAsyncFs<RealFileSystem>>,
) -> bool {
    let dir = default_workspace.unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("diaryx")
    });

    // Initialize config
    match Config::init_with_options(dir.clone()) {
        Ok(_) => {
            println!("✓ Initialized diaryx configuration");
            println!("  Default workspace: {}", dir.display());
            if let Some(config_path) = Config::config_path() {
                println!("  Config file: {}", config_path.display());
            }
        }
        Err(e) => {
            eprintln!("✗ Error initializing config: {}", e);
            return false;
        }
    }

    // Initialize workspace (create README.md)
    match block_on(ws.init_workspace(&dir, title.as_deref(), description.as_deref())) {
        Ok(readme_path) => {
            println!("✓ Initialized workspace");
            println!("  Index file: {}", readme_path.display());
        }
        Err(e) => {
            // Don't fail if workspace already exists
            if !matches!(
                e,
                diaryx_core::error::DiaryxError::WorkspaceAlreadyExists(_)
            ) {
                eprintln!("✗ Error initializing workspace: {}", e);
                return false;
            } else {
                println!("  Workspace already initialized");
            }
        }
    }

    // Auto-register the workspace
    let canonical = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
    if let Ok(mut cfg) = Config::load() {
        let mut reg = cfg.workspace_registry();
        if reg.find_by_path(&canonical).is_none() {
            let display_name = title.unwrap_or_else(|| {
                canonical
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "workspace".into())
            });
            let entry = reg.register(display_name, Some(canonical));
            let id = entry.id.clone();
            reg.set_default(&id);
            cfg.apply_registry(&reg);
            let _ = cfg.save();
        }
    }

    true
}
