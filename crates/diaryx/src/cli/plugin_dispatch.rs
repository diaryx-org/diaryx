//! Generic plugin command dispatcher.
//!
//! Routes CLI commands declared by plugin manifests to either:
//! - A native handler function (for commands needing native-side resources)
//! - The plugin's WASM `handle_command` export (for pure WASM commands)

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clap::ArgMatches;
use diaryx_core::config::Config;
use diaryx_core::plugin::{CliArgType, CliCommand, PluginManifest};

/// A native handler function signature.
///
/// Receives the parsed clap matches and an optional resolved workspace root.
/// Returns `true` on success, `false` on failure.
type NativeHandlerFn = fn(&ArgMatches, Option<&Path>);

/// Registry of native handler functions keyed by handler ID.
pub struct NativeHandlerRegistry {
    handlers: HashMap<&'static str, NativeHandlerFn>,
}

impl NativeHandlerRegistry {
    /// Create the registry with all registered native handlers.
    pub fn new() -> Self {
        let mut handlers: HashMap<&'static str, NativeHandlerFn> = HashMap::new();

        // Sync handlers
        handlers.insert("sync_login", native_sync_login);
        handlers.insert("sync_verify", native_sync_verify);
        handlers.insert("sync_logout", native_sync_logout);
        handlers.insert("sync_status", native_sync_status);
        handlers.insert("sync_start", native_sync_start);
        handlers.insert("sync_push", native_sync_push);
        handlers.insert("sync_pull", native_sync_pull);
        handlers.insert("sync_config", native_sync_config);

        // Publish handlers
        handlers.insert("publish", native_publish);
        handlers.insert("preview", native_preview);

        Self { handlers }
    }

    /// Look up a native handler by ID.
    pub fn get(&self, id: &str) -> Option<&NativeHandlerFn> {
        self.handlers.get(id)
    }
}

/// Dispatch a plugin command that was matched by clap.
///
/// Walks the plugin manifests to find which plugin owns the matched command name,
/// then dispatches to either a native handler or WASM.
pub fn dispatch_plugin_command(
    name: &str,
    sub_matches: &ArgMatches,
    plugin_manifests: &[(String, PluginManifest)],
) -> bool {
    let registry = NativeHandlerRegistry::new();

    // Find the plugin and CLI command declaration for this command name.
    for (_plugin_id, manifest) in plugin_manifests {
        for cli_cmd in &manifest.cli {
            if cli_cmd.name == name || cli_cmd.aliases.contains(&name.to_string()) {
                return dispatch_single_command(cli_cmd, sub_matches, &registry, _plugin_id);
            }
        }
    }

    eprintln!("Unknown command: {}", name);
    false
}

/// Dispatch a single CLI command (may recurse for subcommands).
fn dispatch_single_command(
    cli_cmd: &CliCommand,
    matches: &ArgMatches,
    registry: &NativeHandlerRegistry,
    plugin_id: &str,
) -> bool {
    // Check for subcommands first
    if !cli_cmd.subcommands.is_empty() {
        if let Some((sub_name, sub_matches)) = matches.subcommand() {
            for sub_cmd in &cli_cmd.subcommands {
                if sub_cmd.name == sub_name || sub_cmd.aliases.contains(&sub_name.to_string()) {
                    return dispatch_single_command(sub_cmd, sub_matches, registry, plugin_id);
                }
            }
            eprintln!("Unknown subcommand: {} {}", cli_cmd.name, sub_name);
            return false;
        }
        // No subcommand provided but subcommands exist — fall through to
        // see if the parent command itself has a handler
    }

    // Resolve workspace root if needed
    let workspace_root = if cli_cmd.requires_workspace {
        Some(resolve_workspace_root())
    } else {
        None
    };

    // Check for native handler
    if let Some(handler_id) = &cli_cmd.native_handler {
        if let Some(handler) = registry.get(handler_id) {
            handler(matches, workspace_root.as_deref());
            return true;
        }
        eprintln!(
            "Native handler '{}' not found for command '{}'",
            handler_id, cli_cmd.name
        );
        return false;
    }

    // Fall back to WASM dispatch
    dispatch_wasm_command(cli_cmd, matches, plugin_id, workspace_root.as_deref())
}

/// Dispatch a command to the plugin's WASM handle_command export.
fn dispatch_wasm_command(
    cli_cmd: &CliCommand,
    matches: &ArgMatches,
    plugin_id: &str,
    workspace_root: Option<&Path>,
) -> bool {
    let command_name = cli_cmd
        .command_name
        .clone()
        .unwrap_or_else(|| to_pascal_case(&cli_cmd.name));

    let params = matches_to_json(matches, cli_cmd);

    // We need a workspace root for the host context, even if the command
    // doesn't strictly need one. Use cwd as fallback.
    let ws_root = workspace_root
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let ctx = match super::plugin_loader::CliPublishContext::load(&ws_root) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("Failed to load plugin '{}': {}", plugin_id, e);
            return false;
        }
    };

    match ctx.cmd(&command_name, params) {
        Ok(data) => {
            // Print result if it contains useful data
            if !data.is_null() {
                if let Ok(pretty) = serde_json::to_string_pretty(&data) {
                    println!("{}", pretty);
                }
            }
            true
        }
        Err(e) => {
            eprintln!("Command failed: {}", e);
            false
        }
    }
}

/// Convert clap `ArgMatches` to a JSON object based on CLI command args spec.
fn matches_to_json(matches: &ArgMatches, cli_cmd: &CliCommand) -> serde_json::Value {
    let mut map = serde_json::Map::new();

    for arg in &cli_cmd.args {
        let id = arg.long.as_deref().unwrap_or(&arg.name);

        if arg.is_flag {
            let val = matches.get_flag(id);
            map.insert(arg.name.clone(), serde_json::Value::Bool(val));
        } else {
            match &arg.value_type {
                CliArgType::Integer => {
                    if let Some(val) = matches.get_one::<String>(id) {
                        if let Ok(n) = val.parse::<i64>() {
                            map.insert(arg.name.clone(), serde_json::Value::Number(n.into()));
                        }
                    }
                }
                CliArgType::Float => {
                    if let Some(val) = matches.get_one::<String>(id) {
                        if let Ok(n) = val.parse::<f64>() {
                            if let Some(num) = serde_json::Number::from_f64(n) {
                                map.insert(arg.name.clone(), serde_json::Value::Number(num));
                            }
                        }
                    }
                }
                CliArgType::Boolean => {
                    if let Some(val) = matches.get_one::<String>(id) {
                        let b = val == "true" || val == "1";
                        map.insert(arg.name.clone(), serde_json::Value::Bool(b));
                    }
                }
                _ => {
                    // String and Path
                    if let Some(val) = matches.get_one::<String>(id) {
                        map.insert(arg.name.clone(), serde_json::Value::String(val.clone()));
                    }
                }
            }
        }
    }

    serde_json::Value::Object(map)
}

/// Convert a kebab-case or snake_case name to PascalCase.
fn to_pascal_case(s: &str) -> String {
    s.split(|c: char| c == '-' || c == '_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Resolve workspace root from config or current directory.
fn resolve_workspace_root() -> PathBuf {
    Config::load()
        .ok()
        .map(|c| c.default_workspace)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

// ============================================================================
// Native handler implementations
// ============================================================================

fn load_config_and_workspace(
    _matches: &ArgMatches,
    workspace_root: Option<&Path>,
) -> (Config, PathBuf) {
    let config = Config::load().unwrap_or_default();
    let ws_root = workspace_root
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| config.default_workspace.clone());
    (config, ws_root)
}

fn native_sync_login(matches: &ArgMatches, _workspace_root: Option<&Path>) {
    let config = Config::load().unwrap_or_default();
    let email = matches
        .get_one::<String>("email")
        .cloned()
        .unwrap_or_default();
    let server = matches.get_one::<String>("server").map(|s| s.as_str());
    super::sync::auth::handle_login(&config, &email, server);
}

fn native_sync_verify(matches: &ArgMatches, _workspace_root: Option<&Path>) {
    let config = Config::load().unwrap_or_default();
    let token = matches
        .get_one::<String>("token")
        .cloned()
        .unwrap_or_default();
    let device_name = matches.get_one::<String>("device-name").map(|s| s.as_str());
    super::sync::auth::handle_verify(&config, &token, device_name);
}

fn native_sync_logout(_matches: &ArgMatches, _workspace_root: Option<&Path>) {
    let config = Config::load().unwrap_or_default();
    super::sync::auth::handle_logout(&config);
}

fn native_sync_status(_matches: &ArgMatches, workspace_root: Option<&Path>) {
    let (config, ws_root) = load_config_and_workspace(_matches, workspace_root);
    super::sync::status::handle_status(&config, &ws_root);
}

fn native_sync_start(matches: &ArgMatches, workspace_root: Option<&Path>) {
    let (config, ws_root) = load_config_and_workspace(matches, workspace_root);
    let background = matches.get_flag("background");
    if background {
        eprintln!("Background mode is not yet implemented.");
        eprintln!("Running in foreground mode instead.");
    }
    super::sync::client::handle_start(&config, &ws_root);
}

fn native_sync_push(_matches: &ArgMatches, workspace_root: Option<&Path>) {
    let (config, ws_root) = load_config_and_workspace(_matches, workspace_root);
    super::sync::client::handle_push(&config, &ws_root);
}

fn native_sync_pull(_matches: &ArgMatches, workspace_root: Option<&Path>) {
    let (config, ws_root) = load_config_and_workspace(_matches, workspace_root);
    super::sync::client::handle_pull(&config, &ws_root);
}

fn native_sync_config(matches: &ArgMatches, _ws: Option<&Path>) {
    let config = Config::load().unwrap_or_default();
    let server = matches.get_one::<String>("server").cloned();
    let workspace_id = matches.get_one::<String>("workspace-id").cloned();
    let show = matches.get_flag("show");
    super::sync::status::handle_config(&config, server, workspace_id, show);
}

fn native_publish(matches: &ArgMatches, _workspace_root: Option<&Path>) {
    let destination: PathBuf = matches
        .get_one::<String>("destination")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("publish"));
    let audience = matches.get_one::<String>("audience").cloned();
    let format = matches
        .get_one::<String>("format")
        .map(|s| s.as_str())
        .unwrap_or("html");
    let single_file = matches.get_flag("single-file");
    let title = matches.get_one::<String>("title").cloned();
    let force = matches.get_flag("force");
    let no_copy_attachments = matches.get_flag("no-copy-attachments");
    let dry_run = matches.get_flag("dry-run");

    super::publish::handle_publish(
        destination,
        None, // workspace_override handled by resolve
        audience,
        format,
        single_file,
        title,
        force,
        no_copy_attachments,
        dry_run,
    );
}

fn native_preview(matches: &ArgMatches, _workspace_root: Option<&Path>) {
    let port: u16 = matches
        .get_one::<String>("port")
        .and_then(|s| s.parse().ok())
        .unwrap_or(3456);
    let no_open = matches.get_flag("no-open");
    let audience = matches.get_one::<String>("audience").cloned();
    let title = matches.get_one::<String>("title").cloned();

    super::preview::handle_preview(None, port, no_open, audience, title);
}

// ============================================================================
// clap command builder
// ============================================================================

/// Leak a `String` to get a `&'static str`.
///
/// Used for clap's API which requires `'static` lifetimes. This is fine because
/// the CLI is short-lived and these are small strings read once from manifests.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// Build a clap `Command` from a plugin's `CliCommand` declaration.
pub fn build_plugin_command(cli_cmd: &CliCommand, plugin_id: &str) -> clap::Command {
    let mut cmd = clap::Command::new(leak_str(&cli_cmd.name)).about(leak_str(&cli_cmd.about));

    if let Some(ref long_about) = cli_cmd.long_about {
        cmd = cmd.long_about(leak_str(long_about));
    }

    for alias in &cli_cmd.aliases {
        cmd = cmd.alias(leak_str(alias));
    }

    for arg_spec in &cli_cmd.args {
        cmd = cmd.arg(build_plugin_arg(arg_spec));
    }

    for sub_cmd in &cli_cmd.subcommands {
        cmd = cmd.subcommand(build_plugin_command(sub_cmd, plugin_id));
    }

    cmd = cmd.after_help(leak_str(&format!("Provided by plugin: {}", plugin_id)));

    cmd
}

/// Build a clap `Arg` from a plugin's `CliArg` declaration.
fn build_plugin_arg(arg_spec: &diaryx_core::plugin::CliArg) -> clap::Arg {
    let id = leak_str(arg_spec.long.as_deref().unwrap_or(&arg_spec.name));
    let mut arg = clap::Arg::new(id).help(leak_str(&arg_spec.help));

    if let Some(short) = arg_spec.short {
        arg = arg.short(short);
    }

    if let Some(ref long) = arg_spec.long {
        arg = arg.long(leak_str(long));
    }

    if arg_spec.is_flag {
        arg = arg.action(clap::ArgAction::SetTrue);
    } else {
        arg = arg.action(clap::ArgAction::Set);

        if arg_spec.required {
            arg = arg.required(true);
        }
    }

    if let Some(ref default) = arg_spec.default_value {
        arg = arg.default_value(leak_str(default));
    }

    arg
}
