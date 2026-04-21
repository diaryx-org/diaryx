//! CLI handler for publish command

use std::path::{Path, PathBuf};

use diaryx_core::fs::SyncToAsyncFs;
use diaryx_core::workspace::Workspace;
use diaryx_native::{NativeConfigExt, RealFileSystem};

use crate::cli::plugin_loader::CliPublishContext;

/// Valid publish output formats.
const VALID_PUBLISH_FORMATS: &[&str] = &["html", "docx", "epub", "pdf", "latex", "odt", "rst"];

/// Formats that require the publish plugin's converter.
fn requires_converter(format: &str) -> bool {
    matches!(format, "docx" | "epub" | "pdf" | "latex" | "odt" | "rst")
}

/// Get the file extension for a given format.
fn format_extension(format: &str) -> &str {
    match format {
        "html" => "html",
        "docx" => "docx",
        "epub" => "epub",
        "pdf" => "pdf",
        "latex" => "tex",
        "odt" => "odt",
        "rst" => "rst",
        _ => format,
    }
}

/// Helper to run async operations in sync context
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    futures_lite::future::block_on(f)
}

/// Handle the publish command
pub fn handle_publish(
    destination: PathBuf,
    workspace_override: Option<PathBuf>,
    audience: Option<String>,
    format: &str,
    single_file: bool,
    title: Option<String>,
    force: bool,
    no_copy_attachments: bool,
    dry_run: bool,
) {
    // Validate format (publish doesn't support "markdown" — it always starts from HTML)
    if !VALID_PUBLISH_FORMATS.contains(&format) {
        eprintln!(
            "✗ Unsupported publish format: '{}'. Supported: {}",
            format,
            VALID_PUBLISH_FORMATS.join(", ")
        );
        return;
    }

    // Resolve workspace root
    let workspace_root = match resolve_workspace_for_publish(workspace_override) {
        Ok(root) => root,
        Err(e) => {
            eprintln!("✗ {}", e);
            return;
        }
    };

    // Check destination
    if destination.exists() && !force {
        if single_file {
            eprintln!(
                "✗ Destination file '{}' already exists (use --force to overwrite)",
                destination.display()
            );
        } else {
            eprintln!(
                "✗ Destination directory '{}' already exists (use --force to overwrite)",
                destination.display()
            );
        }
        return;
    }

    // Show plan
    println!("Publish Plan");
    println!("============");
    println!("Source: {}", workspace_root.display());
    println!("Destination: {}", destination.display());
    if let Some(ref aud) = audience {
        println!("Audience: {}", aud);
    }
    if format != "html" {
        println!("Format: {}", format);
    }
    println!(
        "Output mode: {}",
        if single_file {
            "single file"
        } else {
            "multiple files"
        }
    );
    println!();

    if dry_run {
        println!("(dry run - no changes made)");
        return;
    }

    // Execute publish via plugin
    let ctx = match CliPublishContext::load(&workspace_root) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("✗ Failed to load publish plugin: {}", e);
            return;
        }
    };

    match ctx.cmd(
        "PublishWorkspace",
        serde_json::json!({
            "workspace_root": workspace_root.to_string_lossy(),
            "destination": destination.to_string_lossy(),
            "single_file": single_file,
            "title": title,
            "audience": audience,
            "force": force,
            "copy_attachments": !no_copy_attachments,
        }),
    ) {
        Ok(result) => {
            let files_processed = result
                .get("files_processed")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let attachments_copied = result
                .get("attachments_copied")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            if files_processed == 0 {
                println!("⚠ No files to publish");
                if audience.is_some() {
                    println!("  (no files match the specified audience)");
                }
                return;
            }

            println!(
                "✓ Published {} page{} to {}",
                files_processed,
                if files_processed == 1 { "" } else { "s" },
                destination.display()
            );
            if attachments_copied > 0 {
                println!(
                    "  Copied {} attachment{}",
                    attachments_copied,
                    if attachments_copied == 1 { "" } else { "s" }
                );
            }

            // Post-process via publish plugin if a non-HTML format was requested
            if requires_converter(format) {
                convert_published_files(&ctx, &destination, format, single_file);
            } else if single_file {
                println!("  Open {} in a browser to view", destination.display());
            } else {
                let index_path = destination.join("index.html");
                println!("  Open {} in a browser to view", index_path.display());
            }
        }
        Err(e) => {
            eprintln!("✗ Publish failed: {}", e);
        }
    }
}

/// Convert published .html files to the target format via the publish plugin.
fn convert_published_files(
    ctx: &CliPublishContext,
    destination: &Path,
    format: &str,
    single_file: bool,
) {
    println!("Converting to {}...", format);
    let ext = format_extension(format);
    let mut converted = 0;
    let mut failed = 0;

    let html_files = if single_file {
        vec![destination.to_path_buf()]
    } else {
        walkdir_html(destination)
    };

    for html_path in &html_files {
        let content = match std::fs::read_to_string(html_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("  ✗ Failed to read {}: {}", html_path.display(), e);
                failed += 1;
                continue;
            }
        };

        match ctx.cmd(
            "ConvertFormat",
            serde_json::json!({
                "content": content,
                "from": "html",
                "to": format,
            }),
        ) {
            Ok(result) => {
                let out_path = html_path.with_extension(ext);
                if let Some(binary_b64) = result.get("binary").and_then(|v| v.as_str()) {
                    use base64::Engine;
                    match base64::engine::general_purpose::STANDARD.decode(binary_b64) {
                        Ok(bytes) => {
                            if let Err(e) = std::fs::write(&out_path, &bytes) {
                                eprintln!("  ✗ Failed to write {}: {}", out_path.display(), e);
                                failed += 1;
                                continue;
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "  ✗ Failed to decode binary output for {}: {}",
                                html_path.display(),
                                e
                            );
                            failed += 1;
                            continue;
                        }
                    }
                } else if let Some(text) = result.get("content").and_then(|v| v.as_str()) {
                    if let Err(e) = std::fs::write(&out_path, text) {
                        eprintln!("  ✗ Failed to write {}: {}", out_path.display(), e);
                        failed += 1;
                        continue;
                    }
                } else {
                    eprintln!("  ✗ Plugin returned no content for {}", html_path.display());
                    failed += 1;
                    continue;
                }

                let _ = std::fs::remove_file(html_path);
                converted += 1;
            }
            Err(e) => {
                eprintln!("  ✗ Failed to convert {}: {}", html_path.display(), e);
                failed += 1;
            }
        }
    }

    if failed == 0 {
        println!("✓ Converted {} files to {}", converted, format);
    } else {
        eprintln!("⚠ Converted {} files, {} failed", converted, failed);
    }
}

/// Collect all `.html` files under a directory recursively.
fn walkdir_html(dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    fn visit(dir: &Path, results: &mut Vec<PathBuf>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, results);
            } else if path.extension().map_or(false, |ext| ext == "html") {
                results.push(path);
            }
        }
    }
    visit(dir, &mut results);
    results
}

/// Resolve the workspace root for publishing
pub(crate) fn resolve_workspace_for_publish(
    workspace_override: Option<PathBuf>,
) -> Result<PathBuf, String> {
    let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));

    // If workspace is explicitly provided, use it
    if let Some(workspace_path) = workspace_override {
        if workspace_path.is_file() {
            return Ok(workspace_path);
        }
        // If it's a directory, find the root index in it
        if let Ok(Some(root)) = block_on(ws.find_root_index_in_dir(&workspace_path)) {
            return Ok(root);
        }
        return Err(format!(
            "No workspace found at '{}'",
            workspace_path.display()
        ));
    }

    // Try current directory first
    let current_dir =
        std::env::current_dir().map_err(|e| format!("Cannot get current directory: {}", e))?;

    if let Ok(Some(root)) = block_on(ws.detect_workspace(&current_dir)) {
        return Ok(root);
    }

    // Fall back to config default
    let config =
        diaryx_core::config::Config::load().map_err(|e| format!("Failed to load config: {}", e))?;

    if let Ok(Some(root)) = block_on(ws.find_root_index_in_dir(&config.default_workspace)) {
        return Ok(root);
    }

    Err("No workspace found. Run 'diaryx init' first or specify --workspace".to_string())
}
