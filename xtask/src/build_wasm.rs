use crate::util::{apply_macos_env, run_checked, which, workspace_root};
use std::path::Path;
use std::process::Command;

pub fn run(_args: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let out_dir = root.join("apps/web/src/lib/wasm");

    println!("Building WASM from workspace: {}", root.display());

    let wasm_pack = which("wasm-pack").ok_or_else(|| {
        "wasm-pack not found on PATH. Install it with: cargo install wasm-pack".to_string()
    })?;
    println!("Using wasm-pack at: {}", wasm_pack.display());
    println!(
        "Building in directory: {}/crates/diaryx_wasm",
        root.display()
    );

    let mut cmd = Command::new(&wasm_pack);
    cmd.current_dir(&root)
        .args([
            "build",
            "crates/diaryx_wasm",
            "--target",
            "web",
            "--out-dir",
        ])
        .arg(&out_dir);
    apply_macos_env(&mut cmd);
    run_checked(&mut cmd, "wasm-pack")?;

    let wasm_file = out_dir.join("diaryx_wasm_bg.wasm");
    if let Some(wasm_opt) = which("wasm-opt") {
        println!("Running wasm-opt -Oz on {}", wasm_file.display());
        let mut c = Command::new(&wasm_opt);
        c.args(["-Oz", "-o"]).arg(&wasm_file).arg(&wasm_file);
        run_checked(&mut c, "wasm-opt")?;
    } else {
        println!("wasm-opt not found, skipping additional size optimization");
    }

    let bindings_dir = root.join("crates/diaryx_core/bindings");
    if bindings_dir.is_dir() {
        trim_trailing_whitespace_ts(&bindings_dir)?;
        println!("Cleaned trailing whitespace in ts-rs bindings");
    }

    Ok(())
}

fn trim_trailing_whitespace_ts(dir: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|e| format!("file_type: {e}"))?;
        if file_type.is_dir() {
            trim_trailing_whitespace_ts(&path)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("ts") {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("read {}: {e}", path.display()))?;
            let cleaned: String = content
                .split('\n')
                .map(|line| line.trim_end_matches([' ', '\t']))
                .collect::<Vec<_>>()
                .join("\n");
            if cleaned != content {
                std::fs::write(&path, cleaned)
                    .map_err(|e| format!("write {}: {e}", path.display()))?;
            }
        }
    }
    Ok(())
}
