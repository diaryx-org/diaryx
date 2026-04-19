use crate::util::workspace_root;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub fn run(_args: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let bindings_root = root.join("crates/diaryx_core/bindings");
    let generated_dir = root.join("apps/web/src/lib/backend/generated");

    if !bindings_root.is_dir() {
        return Err(format!(
            "bindings directory not found at {}\nRun 'cargo test -p diaryx_core' to generate bindings first.",
            bindings_root.display()
        ));
    }

    fs::create_dir_all(&generated_dir)
        .map_err(|e| format!("mkdir {}: {e}", generated_dir.display()))?;

    // Symlink bindings/bindings/*.ts → generated/*.ts
    let src_bindings = bindings_root.join("bindings");
    sync_directory(&src_bindings, &generated_dir)?;

    // Symlink bindings/serde_json/*.ts → generated/serde_json/*.ts
    let src_serde = bindings_root.join("serde_json");
    if src_serde.is_dir() {
        let dest_serde = generated_dir.join("serde_json");
        fs::create_dir_all(&dest_serde)
            .map_err(|e| format!("mkdir {}: {e}", dest_serde.display()))?;
        sync_directory(&src_serde, &dest_serde)?;
    }

    // Step 4: auto-generate index.ts
    let index = generated_dir.join("index.ts");
    let mut body = String::new();
    body.push_str("// Auto-generated barrel file — do not edit manually.\n");
    body.push_str("// Run `cargo xtask sync-bindings` to regenerate.\n\n");

    let mut count = 0usize;

    let mut top_names: Vec<String> = list_ts_files(&generated_dir)?
        .into_iter()
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
        .filter(|n| n != "index")
        .collect();
    top_names.sort();
    for n in &top_names {
        let _ = writeln!(body, "export type {{ {n} }} from './{n}';");
        count += 1;
    }

    let serde_dir = generated_dir.join("serde_json");
    if serde_dir.is_dir() {
        let mut sub_names: Vec<String> = list_ts_files(&serde_dir)?
            .into_iter()
            .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
            .collect();
        sub_names.sort();
        for n in &sub_names {
            let _ = writeln!(body, "export type {{ {n} }} from './serde_json/{n}';");
            count += 1;
        }
    }

    fs::write(&index, body).map_err(|e| format!("write {}: {e}", index.display()))?;
    println!("Synced bindings: {count} types exported in index.ts");
    Ok(())
}

/// Sync `dest_dir`'s `.ts` files to mirror `src_dir`:
///   - orphaned `.ts` files (no matching source) are removed
///   - missing / wrong symlinks are (re)created via `link_file`
///   - correct existing symlinks are left untouched so mtimes don't bump
///
/// `index.ts` in `dest_dir` is preserved (it's the auto-generated barrel).
fn sync_directory(src_dir: &Path, dest_dir: &Path) -> Result<(), String> {
    let wanted_srcs = list_ts_files(src_dir)?;
    let wanted: HashSet<OsString> = wanted_srcs
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_os_string()))
        .collect();

    if dest_dir.is_dir() {
        for entry in
            fs::read_dir(dest_dir).map_err(|e| format!("read_dir {}: {e}", dest_dir.display()))?
        {
            let entry = entry.map_err(|e| format!("{e}"))?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("ts") {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            if name == "index.ts" {
                continue;
            }
            if wanted.contains(name) {
                continue;
            }
            fs::remove_file(&path).map_err(|e| format!("rm {}: {e}", path.display()))?;
        }
    }

    for src in wanted_srcs {
        let name = src.file_name().expect("ts file has name").to_os_string();
        link_file(&src, &dest_dir.join(&name))?;
    }
    Ok(())
}

fn list_ts_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("{e}"))?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("ts") {
            files.push(path);
        }
    }
    Ok(files)
}

fn link_file(src: &Path, dest: &Path) -> Result<(), String> {
    let dest_dir = dest
        .parent()
        .ok_or_else(|| format!("dest has no parent: {}", dest.display()))?;
    let rel = relative_path(src, dest_dir);

    // If the correct symlink already exists, nothing to do.
    if let Ok(existing) = fs::read_link(dest) {
        if existing == rel {
            return Ok(());
        }
    }

    if dest.exists() || fs::symlink_metadata(dest).is_ok() {
        fs::remove_file(dest).map_err(|e| format!("rm {}: {e}", dest.display()))?;
    }

    make_symlink(&rel, dest)
        .map_err(|e| format!("symlink {} → {}: {e}", dest.display(), rel.display()))?;
    println!("  linked: {}", dest.file_name().unwrap().to_string_lossy());
    Ok(())
}

#[cfg(unix)]
fn make_symlink(rel: &Path, dest: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(rel, dest)
}

#[cfg(not(unix))]
fn make_symlink(rel: &Path, dest: &Path) -> std::io::Result<()> {
    // Windows fallback: plain file copy. Resolve `rel` against dest's parent.
    let absolute = dest.parent().unwrap_or(Path::new(".")).join(rel);
    std::fs::copy(&absolute, dest).map(|_| ())
}

fn relative_path(target: &Path, base: &Path) -> PathBuf {
    let target_abs = absolutize(target);
    let base_abs = absolutize(base);

    let mut t_comps = target_abs.components().peekable();
    let mut b_comps = base_abs.components().peekable();

    while t_comps.peek() == b_comps.peek() && t_comps.peek().is_some() {
        t_comps.next();
        b_comps.next();
    }

    let mut result = PathBuf::new();
    for _ in b_comps {
        result.push("..");
    }
    for c in t_comps {
        match c {
            Component::Normal(s) => result.push(s),
            Component::CurDir => {}
            Component::ParentDir => result.push(".."),
            Component::RootDir | Component::Prefix(_) => {
                return target_abs;
            }
        }
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

fn absolutize(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}
