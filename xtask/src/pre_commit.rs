use crate::util::{run_checked, workspace_root};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

const USAGE: &str = "Usage: cargo xtask pre-commit [--all]

Runs the project's pre-commit checks. Hooks that modify files
(trailing-whitespace, end-of-file-fixer, cargo fmt, sync-versions,
update-agents-index) apply their changes on disk; any file whose
working-tree contents differ from the index after the run causes the
commit to fail so you can review and re-stage.

Flags:
  --all    Run against the entire working tree (ignore staging filter).
           Useful for ad-hoc runs outside a git commit.
";

pub fn run(args: &[String]) -> Result<(), String> {
    let mut all = false;
    for arg in args {
        match arg.as_str() {
            "--all" => all = true,
            "-h" | "--help" | "help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown flag for pre-commit: {other}\n\n{USAGE}")),
        }
    }

    let root = workspace_root();

    let paths = if all {
        list_tracked_files(&root)?
    } else {
        list_staged_files(&root)?
    };

    if paths.is_empty() {
        println!("pre-commit: no files to check");
        return Ok(());
    }

    let dirty_before = working_tree_dirty(&root)?;

    let mut failures: Vec<String> = Vec::new();

    // --- Built-in hooks ---
    run_trailing_whitespace(&root, &paths, &mut failures);
    run_end_of_file_fixer(&root, &paths, &mut failures);
    run_check_yaml(&root, &paths, &mut failures);
    run_check_json(&root, &paths, &mut failures);

    // --- Local hooks (gated by file filters) ---
    let rels: Vec<&str> = paths.iter().map(|p| p.as_str()).collect();

    if rels.iter().any(|p| *p == "README.md") {
        println!("==> sync-versions");
        if let Err(e) = crate::sync_versions::run(&[]) {
            failures.push(format!("sync-versions: {e}"));
        }
    }

    let has_rust = rels.iter().any(|p| p.ends_with(".rs"));
    if has_rust {
        println!("==> cargo fmt --all");
        let mut cmd = Command::new("cargo");
        cmd.current_dir(&root).args(["fmt", "--all"]);
        if let Err(e) = run_checked(&mut cmd, "cargo fmt --all") {
            failures.push(e);
        }

        println!("==> cargo clippy -p diaryx_core -- -D warnings");
        let mut cmd = Command::new("cargo");
        cmd.current_dir(&root)
            .args(["clippy", "-p", "diaryx_core", "--", "-D", "warnings"]);
        if let Err(e) = run_checked(&mut cmd, "cargo clippy") {
            failures.push(e);
        }
    }

    let has_web_ts_or_svelte = rels
        .iter()
        .any(|p| p.starts_with("apps/web/") && (p.ends_with(".svelte") || p.ends_with(".ts")));
    if has_web_ts_or_svelte {
        println!("==> svelte-check (apps/web)");
        let mut cmd = Command::new("bun");
        cmd.current_dir(root.join("apps/web"))
            .args(["run", "check"]);
        if let Err(e) = run_checked(&mut cmd, "bun run check") {
            failures.push(e);
        }
    }

    let touches_readme_for_index = rels.iter().any(|p| {
        *p == "README.md"
            || (p.ends_with("/README.md") && (p.starts_with("crates/") || p.starts_with("apps/")))
    });
    if touches_readme_for_index {
        println!("==> update-agents-index");
        if let Err(e) = crate::update_agents_index::run(&[]) {
            failures.push(format!("update-agents-index: {e}"));
        }
    }

    // --- Detect hook-caused working-tree modifications ---
    let dirty_after = working_tree_dirty(&root)?;
    let newly_dirty: Vec<&String> = dirty_after.difference(&dirty_before).collect();

    if !newly_dirty.is_empty() || !failures.is_empty() {
        eprintln!();
        if !newly_dirty.is_empty() {
            eprintln!("pre-commit: hooks modified these files — review and re-stage:");
            for p in &newly_dirty {
                eprintln!("  {p}");
            }
        }
        for f in &failures {
            eprintln!("pre-commit: {f}");
        }
        return Err("pre-commit checks failed".to_string());
    }

    println!("pre-commit: all checks passed");
    Ok(())
}

// ===== Staged / tracked file enumeration =====

fn list_staged_files(root: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "diff",
            "--cached",
            "--name-only",
            "--diff-filter=ACMR",
            "-z",
        ])
        .output()
        .map_err(|e| format!("git diff --cached: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git diff --cached failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(split_null(&output.stdout))
}

fn list_tracked_files(root: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "-z"])
        .output()
        .map_err(|e| format!("git ls-files: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(split_null(&output.stdout))
}

fn split_null(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .filter_map(|s| std::str::from_utf8(s).ok().map(str::to_string))
        .collect()
}

fn working_tree_dirty(root: &Path) -> Result<BTreeSet<String>, String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["diff", "--name-only", "-z"])
        .output()
        .map_err(|e| format!("git diff: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(split_null(&output.stdout).into_iter().collect())
}

// ===== Built-in: trailing-whitespace =====

fn run_trailing_whitespace(root: &Path, paths: &[String], failures: &mut Vec<String>) {
    println!("==> trailing-whitespace");
    for rel in paths {
        let abs = root.join(rel);
        let Ok(bytes) = fs::read(&abs) else { continue };
        if looks_binary(&bytes) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let fixed = strip_trailing_whitespace(text);
        if fixed != text {
            if let Err(e) = fs::write(&abs, fixed.as_bytes()) {
                failures.push(format!("trailing-whitespace: write {rel}: {e}"));
            }
        }
    }
}

fn strip_trailing_whitespace(text: &str) -> String {
    // Preserve a final trailing newline if present; don't invent one.
    let had_trailing_newline = text.ends_with('\n');
    let mut out = String::with_capacity(text.len());
    let mut first = true;
    for line in text.split('\n') {
        if !first {
            out.push('\n');
        }
        first = false;
        out.push_str(line.trim_end_matches([' ', '\t']));
    }
    // `split('\n')` on text ending in '\n' yields an empty final segment, so
    // if the input ended with '\n' the loop already emitted the trailing '\n'.
    // If it didn't, we leave it alone.
    let _ = had_trailing_newline;
    out
}

// ===== Built-in: end-of-file-fixer =====

fn run_end_of_file_fixer(root: &Path, paths: &[String], failures: &mut Vec<String>) {
    println!("==> end-of-file-fixer");
    for rel in paths {
        let abs = root.join(rel);
        let Ok(bytes) = fs::read(&abs) else { continue };
        if bytes.is_empty() || looks_binary(&bytes) {
            continue;
        }
        let fixed = fix_eof(&bytes);
        if fixed != bytes {
            if let Err(e) = fs::write(&abs, &fixed) {
                failures.push(format!("end-of-file-fixer: write {rel}: {e}"));
            }
        }
    }
}

fn fix_eof(bytes: &[u8]) -> Vec<u8> {
    // Trim trailing \r and \n bytes, then append exactly one \n.
    let end = bytes
        .iter()
        .rposition(|b| *b != b'\n' && *b != b'\r')
        .map(|i| i + 1)
        .unwrap_or(0);
    let mut out = Vec::with_capacity(end + 1);
    out.extend_from_slice(&bytes[..end]);
    out.push(b'\n');
    out
}

// ===== Built-in: check-yaml =====

fn run_check_yaml(root: &Path, paths: &[String], failures: &mut Vec<String>) {
    let yamls: Vec<&String> = paths
        .iter()
        .filter(|p| p.ends_with(".yaml") || p.ends_with(".yml"))
        .collect();
    if yamls.is_empty() {
        return;
    }
    println!("==> check-yaml");
    for rel in yamls {
        let abs = root.join(rel);
        let Ok(bytes) = fs::read(&abs) else { continue };
        for (idx, doc) in serde_yaml_ng::Deserializer::from_slice(&bytes).enumerate() {
            if let Err(e) = serde_yaml_ng::Value::deserialize(doc) {
                failures.push(format!("check-yaml: {rel} (doc {idx}): {e}"));
                break;
            }
        }
    }
}

// ===== Built-in: check-json =====

fn run_check_json(root: &Path, paths: &[String], failures: &mut Vec<String>) {
    let jsons: Vec<&String> = paths.iter().filter(|p| p.ends_with(".json")).collect();
    if jsons.is_empty() {
        return;
    }
    println!("==> check-json");
    for rel in jsons {
        let abs = root.join(rel);
        let Ok(bytes) = fs::read(&abs) else { continue };
        if let Err(e) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            failures.push(format!("check-json: {rel}: {e}"));
        }
    }
}

// ===== Binary detection =====

fn looks_binary(bytes: &[u8]) -> bool {
    // Cheap heuristic used by git and pre-commit: NUL in the first 8 KiB
    // indicates binary content.
    let window = &bytes[..bytes.len().min(8192)];
    window.contains(&0)
}
