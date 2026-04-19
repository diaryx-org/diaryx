use crate::build_plugin;
use crate::util::{diaryx_app, run_checked, workspace_root};
use chrono::Utc;
use diaryx_core::YamlValue;
use indexmap::IndexMap;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::Command;
use toml_edit::DocumentMut;

pub fn run(args: &[String]) -> Result<(), String> {
    let root = workspace_root();

    let mut plugin: Option<String> = None;
    let mut upload = false;
    for arg in args {
        match arg.as_str() {
            "--upload" => upload = true,
            s if s.starts_with('-') => return Err(format!("unknown flag: {s}")),
            s if plugin.is_some() => {
                return Err(format!("unexpected positional arg: {s}"));
            }
            s => plugin = Some(s.to_string()),
        }
    }
    let plugin = plugin.ok_or_else(|| {
        "Usage: cargo xtask release-plugin <plugin-crate-name> [--upload]".to_string()
    })?;

    let crate_dir = root.join("crates/plugins").join(&plugin);
    if !crate_dir.is_dir() {
        return Err(format!(
            "crate directory not found at {}",
            crate_dir.display()
        ));
    }

    let version = read_plugin_version(&crate_dir, &root)?;
    println!("=== Releasing {plugin} v{version} ===");

    // Build release WASM (reuses the build-plugin xtask).
    build_plugin::run(&[plugin.clone(), "--release".to_string()])?;

    let wasm_src = root
        .join("target/wasm32-unknown-unknown/release")
        .join(format!("{plugin}.wasm"));
    let dist_dir = root.join("dist/plugins").join(&plugin);
    fs::create_dir_all(&dist_dir).map_err(|e| format!("mkdir {}: {e}", dist_dir.display()))?;

    let artifact_name = format!("{plugin}.wasm");
    let artifact_path = dist_dir.join(&artifact_name);

    fs::copy(&wasm_src, &artifact_path).map_err(|e| {
        format!(
            "copy {} → {}: {e}",
            wasm_src.display(),
            artifact_path.display()
        )
    })?;
    // Keep the host-loader filename around for local inspection and manual installs.
    fs::copy(&wasm_src, dist_dir.join("plugin.wasm"))
        .map_err(|e| format!("copy plugin.wasm: {e}"))?;

    let bytes =
        fs::read(&artifact_path).map_err(|e| format!("read {}: {e}", artifact_path.display()))?;
    let sha = format!("{:x}", Sha256::digest(&bytes));
    let size = bytes.len();

    println!();
    println!("=== Release artifact ready ===");
    println!("  Path:    {}", artifact_path.display());
    println!("  Version: {version}");
    println!("  Size:    {size} bytes");
    println!("  SHA256:  {sha}");

    let plugin_id = derive_plugin_id(&plugin);

    if upload {
        upload_release(
            &root,
            &plugin_id,
            &version,
            &artifact_path,
            &artifact_name,
            &sha,
            size,
        )?;
    } else {
        println!();
        println!("To upload, re-run with --upload:");
        println!("  cargo xtask release-plugin {plugin} --upload");
    }

    Ok(())
}

fn read_plugin_version(crate_dir: &Path, root: &Path) -> Result<String, String> {
    let cargo_path = crate_dir.join("Cargo.toml");
    let text = fs::read_to_string(&cargo_path)
        .map_err(|e| format!("read {}: {e}", cargo_path.display()))?;
    let doc: DocumentMut = text
        .parse()
        .map_err(|e| format!("parse {}: {e}", cargo_path.display()))?;

    // Look for `[package] version = "..."` — an actual string literal.
    let from_crate = doc
        .get("package")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(str::to_string);

    if let Some(v) = from_crate {
        return Ok(v);
    }

    // Otherwise (e.g. `version.workspace = true`), fall back to the workspace.
    let root_text = fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|e| format!("read workspace Cargo.toml: {e}"))?;
    let root_doc: DocumentMut = root_text
        .parse()
        .map_err(|e| format!("parse workspace Cargo.toml: {e}"))?;
    root_doc["workspace"]["package"]["version"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "no version in workspace Cargo.toml".to_string())
}

fn derive_plugin_id(plugin: &str) -> String {
    // diaryx_sync_extism → diaryx_sync → diaryx.sync
    let stripped = plugin.strip_suffix("_extism").unwrap_or(plugin);
    stripped.replacen('_', ".", 1)
}

fn capitalize_ascii(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn upload_release(
    root: &Path,
    plugin_id: &str,
    version: &str,
    artifact_path: &Path,
    artifact_name: &str,
    sha: &str,
    size: usize,
) -> Result<(), String> {
    let tag = format!("{plugin_id}/v{version}");
    let release_name = format!("{plugin_id} v{version}");

    println!();
    println!("=== Uploading {plugin_id} v{version} ===");

    // Previous release for this plugin (used as --notes-start-tag). Failure here
    // is non-fatal — we just pass an empty value, matching the old script.
    let prev_tag = find_previous_plugin_tag(plugin_id).unwrap_or_default();

    println!("Creating GitHub Release: {tag}");
    let mut cmd = Command::new("gh");
    cmd.args(["release", "create", &tag])
        .arg(artifact_path)
        .args(["--repo", "diaryx-org/diaryx"])
        .args(["--title", &release_name])
        .arg("--generate-notes")
        .args(["--notes-start-tag", &prev_tag]);
    run_checked(&mut cmd, "gh release create")?;

    let download_url =
        format!("https://github.com/diaryx-org/diaryx/releases/download/{tag}/{artifact_name}");
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    println!("Release created: {download_url}");
    println!("Updating plugin-registry...");

    let registry_dir = std::env::temp_dir().join(format!(
        "diaryx-registry-{}-{}",
        plugin_id.replace('.', "-"),
        version
    ));
    if registry_dir.exists() {
        fs::remove_dir_all(&registry_dir)
            .map_err(|e| format!("rm {}: {e}", registry_dir.display()))?;
    }

    let result = do_registry_update(
        root,
        &registry_dir,
        plugin_id,
        version,
        &tag,
        &download_url,
        sha,
        size,
        &now,
    );

    // Best-effort cleanup, regardless of success.
    let _ = fs::remove_dir_all(&registry_dir);

    let pr_url = result?;

    println!();
    println!("=== Upload complete ===");
    println!("  GitHub Release: https://github.com/diaryx-org/diaryx/releases/tag/{tag}");
    println!("  Registry PR:    {pr_url}");

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn do_registry_update(
    workspace_root: &Path,
    registry_dir: &Path,
    plugin_id: &str,
    version: &str,
    tag: &str,
    download_url: &str,
    sha: &str,
    size: usize,
    now: &str,
) -> Result<String, String> {
    // Shallow clone the plugin-registry repo.
    let mut cmd = Command::new("gh");
    cmd.args(["repo", "clone", "diaryx-org/plugin-registry"])
        .arg(registry_dir)
        .args(["--", "--depth", "1"]);
    run_checked(&mut cmd, "gh repo clone")?;

    // Inherit the workspace's git identity so the commit is attributed correctly.
    let name = capture_git_config(workspace_root, "user.name").unwrap_or_default();
    let email = capture_git_config(workspace_root, "user.email").unwrap_or_default();
    git_in(registry_dir, &["config", "user.name", &name])?;
    git_in(registry_dir, &["config", "user.email", &email])?;

    let plugin_file = registry_dir.join("plugins").join(format!("{plugin_id}.md"));
    let branch = format!("release/{plugin_id}-v{version}");

    let new_plugin = !plugin_file.exists();

    if new_plugin {
        if let Some(parent) = plugin_file.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        let title = capitalize_ascii(plugin_id.split('.').last().unwrap_or(plugin_id));
        let content = format!(
            "---\n\
             title: \"{title}\"\n\
             description: \"\"\n\
             id: \"{plugin_id}\"\n\
             version: \"{version}\"\n\
             author: \"Diaryx Team\"\n\
             license: \"PolyForm Shield 1.0.0\"\n\
             repository: \"https://github.com/diaryx-org/diaryx\"\n\
             categories: []\n\
             tags: []\n\
             capabilities: []\n\
             artifact:\n  \
             url: \"{download_url}\"\n  \
             sha256: \"{sha}\"\n  \
             size: {size}\n  \
             published_at: \"{now}\"\n\
             ---\n\
             \n\
             TODO: add description\n",
        );
        fs::write(&plugin_file, content)
            .map_err(|e| format!("write {}: {e}", plugin_file.display()))?;
    } else {
        update_existing_registry_entry(&plugin_file, version, download_url, sha, size, now)?;
    }

    git_in(registry_dir, &["checkout", "-b", &branch])?;
    let rel = plugin_file
        .strip_prefix(registry_dir)
        .unwrap_or(&plugin_file);
    git_in(registry_dir, &["add", &rel.to_string_lossy()])?;
    git_in(
        registry_dir,
        &["commit", "-m", &format!("release: {plugin_id} v{version}")],
    )?;
    git_in(registry_dir, &["push", "origin", &branch])?;

    let mut pr_body = format!(
        "Automated release from [diaryx {tag}](https://github.com/diaryx-org/diaryx/releases/tag/{tag})"
    );
    if new_plugin {
        pr_body.push_str(
            "\n\n> **New plugin** — description, categories, tags, capabilities, and UI slots need to be filled in before merging.",
        );
    }

    let pr_title = format!("release: {plugin_id} v{version}");
    let pr_url_raw = capture_cmd(
        "gh",
        &[
            "pr",
            "create",
            "--repo",
            "diaryx-org/plugin-registry",
            "--title",
            &pr_title,
            "--body",
            &pr_body,
            "--head",
            &branch,
        ],
        Some(registry_dir),
    )?;
    Ok(pr_url_raw.trim().to_string())
}

fn update_existing_registry_entry(
    plugin_file: &Path,
    version: &str,
    download_url: &str,
    sha: &str,
    size: usize,
    now: &str,
) -> Result<(), String> {
    let app = diaryx_app();
    let path_str = plugin_file
        .to_str()
        .ok_or_else(|| format!("non-UTF8 path: {}", plugin_file.display()))?;

    app.set_frontmatter_property(path_str, "version", YamlValue::String(version.to_string()))
        .map_err(|e| format!("set version: {e}"))?;

    let fm = app
        .get_all_frontmatter(path_str)
        .map_err(|e| format!("read frontmatter: {e}"))?;
    let mut artifact: IndexMap<String, YamlValue> = match fm.get("artifact") {
        Some(YamlValue::Mapping(m)) => m.clone(),
        _ => IndexMap::new(),
    };
    artifact.insert(
        "url".to_string(),
        YamlValue::String(download_url.to_string()),
    );
    artifact.insert("sha256".to_string(), YamlValue::String(sha.to_string()));
    artifact.insert("size".to_string(), YamlValue::Int(size as i64));
    artifact.insert(
        "published_at".to_string(),
        YamlValue::String(now.to_string()),
    );
    app.set_frontmatter_property(path_str, "artifact", YamlValue::Mapping(artifact))
        .map_err(|e| format!("set artifact: {e}"))?;

    Ok(())
}

fn find_previous_plugin_tag(plugin_id: &str) -> Option<String> {
    let q = format!("[.[] | select(.tagName | startswith(\"{plugin_id}/\"))][1].tagName // \"\"");
    let output = Command::new("gh")
        .args([
            "release",
            "list",
            "--repo",
            "diaryx-org/diaryx",
            "--json",
            "tagName",
            "-q",
        ])
        .arg(&q)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8(output.stdout).ok()?;
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn capture_git_config(dir: &Path, key: &str) -> Option<String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["config", "--get", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8(output.stdout).ok()?;
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn git_in(dir: &Path, args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir).args(args);
    run_checked(&mut cmd, &format!("git {}", args.join(" ")))
}

fn capture_cmd(program: &str, args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    let output = cmd.output().map_err(|e| format!("spawn {program}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
