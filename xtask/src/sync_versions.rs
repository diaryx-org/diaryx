use crate::util::{diaryx_app, workspace_root};
use std::fs;
use std::path::Path;
use toml_edit::{DocumentMut, value};

pub fn run(_args: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let readme = root.join("README.md");
    let readme_str = readme
        .to_str()
        .ok_or_else(|| format!("non-UTF8 path: {}", readme.display()))?;

    let app = diaryx_app();
    let raw = app
        .get_frontmatter_property(readme_str, "version")
        .map_err(|e| format!("read README.md frontmatter: {e}"))?
        .as_ref()
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| "version not found or not a string in README.md".to_string())?;
    let version = raw.trim().trim_start_matches('v');
    if version.is_empty() {
        return Err("Could not find version in README.md frontmatter".to_string());
    }
    println!("Syncing version: {version}");

    update_cargo_toml(&root.join("Cargo.toml"), version)?;
    update_json_version(&root.join("apps/tauri/src-tauri/tauri.conf.json"), version)?;
    update_json_version(&root.join("apps/web/package.json"), version)?;
    update_flake_nix(&root.join("flake.nix"), version)?;

    println!("Version synced to {version} in all files");
    Ok(())
}

fn update_cargo_toml(path: &Path, version: &str) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e| format!("parse {}: {e}", path.display()))?;

    // [workspace.package].version
    doc["workspace"]["package"]["version"] = value(version);

    // [workspace.dependencies].<crate>.version for every internal crate that
    // is version-locked to the workspace. Extism guest plugins under
    // crates/plugins/*_extism ship on independent release cycles to the
    // marketplace, so they are intentionally not listed here.
    for name in [
        "diaryx_core",
        "diaryx_native",
        "diaryx_server",
        "diaryx_extism",
        "diaryx_plugin_sdk",
    ] {
        let dep = &mut doc["workspace"]["dependencies"][name];
        if dep.is_none() {
            continue;
        }
        dep["version"] = value(version);
    }

    fs::write(path, doc.to_string()).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(())
}

fn update_json_version(path: &Path, version: &str) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let new_text = replace_json_version(&text, version);
    fs::write(path, new_text).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(())
}

/// Replace every `"version": "..."` occurrence in the file with the new
/// version. Matches the behavior of the old sed-based script.
fn replace_json_version(text: &str, version: &str) -> String {
    let key = "\"version\"";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(idx) = rest.find(key) {
        out.push_str(&rest[..idx]);
        out.push_str(key);
        let after_key = &rest[idx + key.len()..];

        let Some(colon) = after_key.find(':') else {
            out.push_str(after_key);
            return out;
        };
        out.push_str(&after_key[..=colon]);
        let after_colon = &after_key[colon + 1..];

        let Some(open) = after_colon.find('"') else {
            out.push_str(after_colon);
            return out;
        };
        out.push_str(&after_colon[..open]);
        out.push('"');
        out.push_str(version);
        out.push('"');

        let after_open = &after_colon[open + 1..];
        let Some(close) = after_open.find('"') else {
            // Unterminated — abandon rewriting, keep original tail
            out.push_str(after_open);
            return out;
        };
        rest = &after_open[close + 1..];
    }
    out.push_str(rest);
    out
}

fn update_flake_nix(path: &Path, version: &str) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut out = text;
    for pname in [
        "diaryx",
        "diaryx-sync-server",
        "ts-bindings",
        "wasm-package",
    ] {
        out = replace_nix_pname_version(&out, pname, version);
    }
    fs::write(path, out).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(())
}

/// Find `pname = "<pname>";` literally and replace the subsequent
/// `version = "..."` value. Matches the perl-based behavior of the old script.
fn replace_nix_pname_version(text: &str, pname: &str, version: &str) -> String {
    let needle = format!("pname = \"{pname}\";");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(idx) = rest.find(&needle) {
        let head_end = idx + needle.len();
        out.push_str(&rest[..head_end]);
        let after = &rest[head_end..];

        let ver_key = "version = \"";
        let Some(ver_rel) = after.find(ver_key) else {
            out.push_str(after);
            return out;
        };
        out.push_str(&after[..ver_rel + ver_key.len()]);
        let after_open = &after[ver_rel + ver_key.len()..];

        let Some(close) = after_open.find('"') else {
            out.push_str(after_open);
            return out;
        };
        out.push_str(version);
        out.push('"');
        rest = &after_open[close + 1..];
    }
    out.push_str(rest);
    out
}
