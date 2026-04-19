use crate::util::{require_env, run_checked, workspace_root};
use std::process::Command;

const USAGE: &str = "Usage: cargo xtask publish-ios\n\n\
Builds the iOS App Store export (Tauri `apple` feature) and uploads the IPA to\n\
App Store Connect.\n\n\
Required environment variables:\n  \
  API_KEY         App Store Connect API key ID\n  \
  API_ISSUER      App Store Connect issuer UUID\n  \
  API_KEY_PATH    Absolute path to the AuthKey_<ID>.p8 file\n";

pub fn run(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    if !args.is_empty() {
        return Err(format!(
            "unexpected argument: {}\n\n{USAGE}",
            args.join(" ")
        ));
    }
    if !cfg!(target_os = "macos") {
        return Err(
            "publish-ios is only supported on macOS (requires xcrun and cargo tauri ios build)"
                .to_string(),
        );
    }

    let root = workspace_root();

    let api_key = require_env("API_KEY")?;
    let api_issuer = require_env("API_ISSUER")?;
    let api_key_path = require_env("API_KEY_PATH")?;

    let tauri_dir = root.join("apps/tauri");

    println!("==> Building iOS app...");
    let mut build = Command::new("cargo");
    build
        .current_dir(&tauri_dir)
        .env("APPLE_API_KEY", &api_key)
        .env("APPLE_API_ISSUER", &api_issuer)
        .env("APPLE_API_KEY_PATH", &api_key_path)
        .args([
            "tauri",
            "ios",
            "build",
            "--export-method",
            "app-store-connect",
            "--",
            "--features",
            "apple",
        ]);
    run_checked(&mut build, "cargo tauri ios build")?;

    let ipa_dir = root.join("apps/tauri/src-tauri/gen/apple/build");
    let ipa = find_ipa(&ipa_dir)?;
    println!("==> Found IPA: {}", ipa.display());

    println!("==> Uploading to App Store Connect...");
    let mut upload = Command::new("xcrun");
    upload.args(["altool", "--upload-app", "--type", "ios", "--file"]);
    upload
        .arg(&ipa)
        .args(["--apiKey", &api_key, "--apiIssuer", &api_issuer]);
    run_checked(&mut upload, "xcrun altool")?;

    println!("==> Done! Check App Store Connect for processing status.");
    Ok(())
}

fn find_ipa(dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    if !dir.is_dir() {
        return Err(format!(
            "expected build output directory not found: {}",
            dir.display()
        ));
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(p) = stack.pop() {
        for entry in std::fs::read_dir(&p).map_err(|e| format!("read_dir {}: {e}", p.display()))? {
            let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("ipa") {
                return Ok(path);
            }
        }
    }
    Err(format!("could not find .ipa under {}", dir.display()))
}
