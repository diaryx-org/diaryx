use crate::util::{run_checked, which, workspace_root};
use std::fs;
use std::path::Path;
use std::process::Command;

pub fn run(args: &[String]) -> Result<(), String> {
    let root = workspace_root();

    let mut plugin: Option<String> = None;
    let mut release = false;
    let mut passthrough: Vec<String> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--release" => release = true,
            s if s.starts_with('-') => passthrough.push(s.to_string()),
            s if plugin.is_some() => {
                return Err(format!("unexpected positional arg: {s}"));
            }
            s => plugin = Some(s.to_string()),
        }
    }

    let Some(plugin) = plugin else {
        print_usage(&root);
        return Err("no plugin specified".to_string());
    };

    let profile = if release { "release" } else { "debug" };
    println!("Building {plugin} (target: wasm32-unknown-unknown, profile: {profile})");

    let mut cmd = Command::new("cargo");
    cmd.current_dir(&root)
        .args(["build", "--target", "wasm32-unknown-unknown", "-p", &plugin]);
    if release {
        cmd.arg("--release");
    }
    for p in &passthrough {
        cmd.arg(p);
    }
    run_checked(&mut cmd, "cargo build")?;

    let wasm_file = root
        .join("target/wasm32-unknown-unknown")
        .join(profile)
        .join(format!("{plugin}.wasm"));

    if !wasm_file.exists() {
        return Err(format!(
            "expected output not found at {}",
            wasm_file.display()
        ));
    }

    let size = fs::metadata(&wasm_file)
        .map_err(|e| format!("stat {}: {e}", wasm_file.display()))?
        .len();
    println!("Built: {} ({size} bytes)", wasm_file.display());

    if release {
        if let Some(wasm_opt) = which("wasm-opt") {
            println!("Running wasm-opt -Oz...");
            let mut c = Command::new(&wasm_opt);
            c.args(["-Oz", "-o"]).arg(&wasm_file).arg(&wasm_file);
            run_checked(&mut c, "wasm-opt")?;
            let opt_size = fs::metadata(&wasm_file)
                .map_err(|e| format!("stat: {e}"))?
                .len();
            println!("Optimized: {} ({opt_size} bytes)", wasm_file.display());
        }
    }

    Ok(())
}

fn print_usage(root: &Path) {
    eprintln!("Usage: cargo xtask build-plugin <plugin-crate-name> [--release]\n");
    eprintln!("Available plugins:");
    let plugins_dir = root.join("crates/plugins");
    match fs::read_dir(&plugins_dir) {
        Ok(entries) => {
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n != "diaryx_plugin_sdk")
                .collect();
            names.sort();
            for n in names {
                eprintln!("  {n}");
            }
        }
        Err(e) => eprintln!("  (could not read {}: {e})", plugins_dir.display()),
    }
}
