//! CLI handler for export command

use std::path::PathBuf;

use diaryx_core::export::{ExportOptions, ExportPlan, Exporter};
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_core::workspace::Workspace;
use std::path::Path;

#[cfg(feature = "plugins")]
use crate::cli::plugin_loader::CliPublishContext;

/// Supported export formats.
const SUPPORTED_FORMATS: &[&str] = &[
    "markdown", "html", "docx", "epub", "pdf", "latex", "odt", "rst",
];

/// Formats that require the publish plugin's converter.
fn requires_converter(format: &str) -> bool {
    matches!(
        format,
        "docx" | "epub" | "pdf" | "latex" | "odt" | "rst" | "html"
    )
}

/// Get the file extension for a given format.
#[cfg(feature = "plugins")]
fn format_extension(format: &str) -> &str {
    match format {
        "markdown" => "md",
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

/// Handle the export command
#[allow(clippy::too_many_arguments)]
pub fn handle_export(
    workspace_root: PathBuf,
    audience: &str,
    destination: &Path,
    format: &str,
    force: bool,
    keep_audience: bool,
    verbose: bool,
    dry_run: bool,
) {
    // Validate format
    if !SUPPORTED_FORMATS.contains(&format) {
        eprintln!(
            "✗ Unsupported format: '{}'. Supported: {}",
            format,
            SUPPORTED_FORMATS.join(", ")
        );
        return;
    }

    let fs = SyncToAsyncFs::new(RealFileSystem);
    let exporter = Exporter::new(fs.clone());

    // Read workspace config for default_audience
    let ws = Workspace::new(fs);
    let default_audience = block_on(ws.get_workspace_config(&workspace_root))
        .ok()
        .and_then(|c| c.default_audience);

    // Plan the export
    let plan = match block_on(exporter.plan_export(
        &workspace_root,
        audience,
        destination,
        default_audience.as_deref(),
    )) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("✗ Failed to plan export: {}", e);
            return;
        }
    };

    // Show plan summary
    println!("Export Plan");
    println!("===========");
    println!("Audience: {}", plan.audience);
    if format != "markdown" {
        println!("Format: {}", format);
    }
    println!("Source: {}", plan.source_root.display());
    println!("Destination: {}", destination.display());
    println!();

    if plan.included.is_empty() {
        println!("⚠ No files to export for audience '{}'", audience);
        println!();
        if !plan.excluded.is_empty() {
            println!("All {} files were excluded:", plan.excluded.len());
            for excluded in &plan.excluded {
                println!("  - {} ({})", excluded.path.display(), excluded.reason);
            }
        }
        return;
    }

    println!(
        "Files to export: {} | Files excluded: {}",
        plan.included.len(),
        plan.excluded.len()
    );
    println!();

    if verbose {
        print_verbose_plan(&plan);
    }

    if dry_run {
        println!("(dry run - no changes made)");
        return;
    }

    // Execute the export (writes markdown files to destination)
    let options = ExportOptions {
        force,
        keep_audience,
    };

    match block_on(exporter.execute_export(&plan, &options)) {
        Ok(stats) => {
            println!("✓ {}", stats);
        }
        Err(e) => {
            eprintln!("✗ Export failed: {}", e);
            if !force && destination.exists() {
                eprintln!("  (use --force to overwrite existing destination)");
            }
            return;
        }
    }

    // Post-process via publish plugin if a non-markdown format was requested
    if requires_converter(format) {
        #[cfg(feature = "plugins")]
        {
            convert_exported_files(&workspace_root, destination, format, verbose);
        }
        #[cfg(not(feature = "plugins"))]
        {
            eprintln!(
                "✗ Format '{}' requires the plugins feature. Rebuild with --features plugins",
                format
            );
            return;
        }
    }

    println!("  Exported to: {}", destination.display());
}

/// Convert exported .md files to the target format via the publish plugin.
#[cfg(feature = "plugins")]
fn convert_exported_files(workspace_root: &Path, destination: &Path, format: &str, verbose: bool) {
    let ctx = match CliPublishContext::load(workspace_root) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("✗ Failed to load publish plugin: {}", e);
            return;
        }
    };

    println!("Converting to {}...", format);
    let ext = format_extension(format);
    let mut converted = 0;
    let mut failed = 0;

    for md_path in walkdir(destination) {
        let content = match std::fs::read_to_string(&md_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("  ✗ Failed to read {}: {}", md_path.display(), e);
                failed += 1;
                continue;
            }
        };

        match ctx.cmd(
            "ConvertFormat",
            serde_json::json!({
                "content": content,
                "from": "markdown",
                "to": format,
            }),
        ) {
            Ok(result) => {
                let out_path = md_path.with_extension(ext);
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
                                md_path.display(),
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
                    eprintln!("  ✗ Plugin returned no content for {}", md_path.display());
                    failed += 1;
                    continue;
                }

                let _ = std::fs::remove_file(&md_path);
                if verbose {
                    println!("  Converted: {}", md_path.with_extension(ext).display());
                }
                converted += 1;
            }
            Err(e) => {
                eprintln!("  ✗ Failed to convert {}: {}", md_path.display(), e);
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

/// Print detailed information about the export plan
fn print_verbose_plan(plan: &ExportPlan) {
    println!("Included files:");
    for file in &plan.included {
        println!("  ✓ {}", file.relative_path.display());
        if !file.filtered_contents.is_empty() {
            println!(
                "    (contents filtered: {})",
                file.filtered_contents.join(", ")
            );
        }
    }
    println!();

    if !plan.excluded.is_empty() {
        println!("Excluded files:");
        for excluded in &plan.excluded {
            println!("  ✗ {} - {}", excluded.path.display(), excluded.reason);
        }
        println!();
    }
}

/// Collect all `.md` files under a directory recursively.
#[cfg(feature = "plugins")]
fn walkdir(dir: &Path) -> Vec<PathBuf> {
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
            } else if path.extension().is_some_and(|ext| ext == "md") {
                results.push(path);
            }
        }
    }
    visit(dir, &mut results);
    results
}

/// Resolve the workspace root for export
pub fn resolve_workspace_for_export(
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
