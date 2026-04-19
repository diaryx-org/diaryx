use crate::util::{require_env, run_checked, workspace_root};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

const USAGE: &str = "Usage: cargo xtask publish-macos <build-number>\n\n\
Builds the macOS App Store bundle (Tauri `apple` feature), signs it, packages a\n\
.pkg, and uploads to App Store Connect.\n\n\
  <build-number>  CFBundleVersion for this upload. Must be higher than the last\n                  \
uploaded build. Marketing version comes from tauri.conf.json.\n\n\
Required environment variables:\n  \
  API_KEY             App Store Connect API key ID\n  \
  API_ISSUER          App Store Connect issuer UUID\n  \
  APPLE_TEAM_ID       10-character Apple Developer team ID\n  \
  APP_SIGN_IDENTITY   codesign identity (e.g. \"Apple Distribution: ...\")\n  \
  PKG_SIGN_IDENTITY   productbuild identity (e.g. \"3rd Party Mac Developer Installer: ...\")\n";

pub fn run(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    if !cfg!(target_os = "macos") {
        return Err(
            "publish-macos is only supported on macOS (requires codesign, productbuild, xcrun)"
                .to_string(),
        );
    }

    let build_number = match args {
        [n] => n.clone(),
        _ => return Err(format!("expected exactly one argument\n\n{USAGE}")),
    };
    if !build_number.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("build number must be numeric: {build_number}"));
    }

    let root = workspace_root();

    let api_key = require_env("API_KEY")?;
    let api_issuer = require_env("API_ISSUER")?;
    let team_id = require_env("APPLE_TEAM_ID")?;
    let app_sign_identity = require_env("APP_SIGN_IDENTITY")?;
    let pkg_sign_identity = require_env("PKG_SIGN_IDENTITY")?;

    let tauri_dir = root.join("apps/tauri");
    let src_tauri = tauri_dir.join("src-tauri");
    let app_bundle = root.join("target/release/bundle/macos/Diaryx.app");
    let entitlements = src_tauri.join("Entitlements.plist");
    let provisioning_profile = src_tauri.join("embedded.provisionprofile");
    let pkg_output = root.join("Diaryx.pkg");

    println!("==> Building Diaryx.app...");
    let mut build = Command::new("cargo");
    build.current_dir(&tauri_dir).args([
        "tauri",
        "build",
        "--bundles",
        "app",
        "--",
        "--features",
        "apple",
    ]);
    run_checked(&mut build, "cargo tauri build")?;

    let binary = app_bundle.join("Contents/MacOS/diaryx_tauri");
    println!("==> Rewriting Nix dylib paths to system libraries (no-op off Nix)...");
    rewrite_nix_dylibs(&binary)?;

    println!("==> Setting CFBundleVersion to {build_number}...");
    let mut plist = Command::new("/usr/libexec/PlistBuddy");
    plist
        .arg("-c")
        .arg(format!("Set :CFBundleVersion {build_number}"))
        .arg(app_bundle.join("Contents/Info.plist"));
    run_checked(&mut plist, "PlistBuddy")?;

    println!("==> Embedding provisioning profile...");
    if !provisioning_profile.is_file() {
        return Err(format!(
            "{} not found. Download it from https://developer.apple.com/account/resources/profiles/list",
            provisioning_profile.display()
        ));
    }
    fs::copy(
        &provisioning_profile,
        app_bundle.join("Contents/embedded.provisionprofile"),
    )
    .map_err(|e| format!("copy provisioning profile: {e}"))?;
    let mut xattr = Command::new("xattr");
    xattr.arg("-cr").arg(&app_bundle);
    run_checked(&mut xattr, "xattr")?;

    let resolved_entitlements = root.join("target/xtask-entitlements.plist");
    write_resolved_entitlements(&entitlements, &resolved_entitlements, &team_id)?;

    println!("==> Signing Diaryx.app...");
    let mut sign = Command::new("codesign");
    sign.args(["--deep", "--force", "--options", "runtime", "--sign"])
        .arg(&app_sign_identity)
        .arg("--entitlements")
        .arg(&resolved_entitlements)
        .arg(&app_bundle);
    run_checked(&mut sign, "codesign")?;

    println!("==> Creating Diaryx.pkg...");
    let mut pkg = Command::new("productbuild");
    pkg.arg("--component")
        .arg(&app_bundle)
        .arg("/Applications")
        .arg("--sign")
        .arg(&pkg_sign_identity)
        .arg(&pkg_output);
    run_checked(&mut pkg, "productbuild")?;

    println!("==> Uploading to App Store Connect...");
    let mut upload = Command::new("xcrun");
    upload
        .args(["altool", "--upload-app", "--type", "macos", "--file"])
        .arg(&pkg_output)
        .args(["--apiKey", &api_key, "--apiIssuer", &api_issuer]);
    run_checked(&mut upload, "xcrun altool")?;

    let _ = fs::remove_file(&resolved_entitlements);

    println!("==> Done! Check App Store Connect for processing status.");
    Ok(())
}

fn rewrite_nix_dylibs(binary: &Path) -> Result<(), String> {
    let output = Command::new("otool")
        .arg("-L")
        .arg(binary)
        .output()
        .map_err(|e| format!("spawn otool: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "otool -L failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let trimmed = line.trim();
        let Some(first) = trimmed.split_whitespace().next() else {
            continue;
        };
        if !first.starts_with("/nix/store") && !first.starts_with("/Volumes/VOLUME") {
            continue;
        }
        let lib_name = Path::new(first)
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("cannot parse dylib path: {first}"))?;
        let system_path = format!("/usr/lib/{lib_name}");
        println!("    {first} -> {system_path}");
        let mut cmd = Command::new("install_name_tool");
        cmd.args(["-change", first, &system_path]).arg(binary);
        run_checked(&mut cmd, "install_name_tool")?;
    }
    Ok(())
}

fn write_resolved_entitlements(source: &Path, dest: &Path, team_id: &str) -> Result<(), String> {
    let content =
        fs::read_to_string(source).map_err(|e| format!("read {}: {e}", source.display()))?;
    let injection = format!(
        "    <key>com.apple.application-identifier</key>\n    <string>{team_id}.org.diaryx.desktop</string>\n</dict>"
    );
    let Some((head, tail)) = content.rsplit_once("</dict>") else {
        return Err(format!(
            "entitlements template missing </dict>: {}",
            source.display()
        ));
    };
    let resolved = format!("{head}{injection}{tail}");
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let mut f = fs::File::create(dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    f.write_all(resolved.as_bytes())
        .map_err(|e| format!("write {}: {e}", dest.display()))?;
    Ok(())
}
