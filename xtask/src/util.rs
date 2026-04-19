use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask manifest has parent")
        .to_path_buf()
}

pub fn which(program: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

pub fn apply_macos_env(cmd: &mut Command) {
    if !cfg!(target_os = "macos") {
        return;
    }
    if which("xcode-select").is_none() || which("xcrun").is_none() {
        return;
    }
    if env::var_os("DEVELOPER_DIR").is_none() {
        if let Some(dir) = capture("xcode-select", &["-p"]) {
            cmd.env("DEVELOPER_DIR", dir);
        }
    }
    if env::var_os("SDKROOT").is_none() {
        let sdk = capture("xcrun", &["--sdk", "macosx", "--show-sdk-path"])
            .or_else(|| capture("xcrun", &["--show-sdk-path"]));
        if let Some(sdk) = sdk {
            cmd.env("SDKROOT", sdk);
        }
    }
    cmd.env("CC", "/usr/bin/cc")
        .env("CXX", "/usr/bin/c++")
        .env("AR", "/usr/bin/ar")
        .env("CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER", "/usr/bin/cc");
}

pub fn capture(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8(output.stdout).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn run_checked(cmd: &mut Command, what: &str) -> Result<(), String> {
    let status = cmd
        .status()
        .map_err(|e| format!("failed to spawn {what}: {e}"))?;
    if !status.success() {
        return Err(format!("{what} failed with status {status}"));
    }
    Ok(())
}

pub fn diaryx_app() -> diaryx_core::entry::DiaryxAppSync<diaryx_core::fs::RealFileSystem> {
    diaryx_core::entry::DiaryxAppSync::new(diaryx_core::fs::RealFileSystem)
}

/// Load simple `KEY=VALUE` pairs from a dotenv-style file into the current
/// process environment. Existing env vars are NOT overwritten, so secrets
/// injected by CI take precedence over any checked-out local file.
///
/// Supports `#` comments, blank lines, and optional single or double quotes
/// around values. Does NOT support shell interpolation — unlike `source`, this
/// is a pure parser, so `.env.publish` files must use literal values.
pub fn load_env_file(path: &Path) -> Result<(), String> {
    let contents = fs::read_to_string(path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    for (i, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let rest = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = rest.split_once('=') else {
            return Err(format!(
                "{}:{}: expected KEY=VALUE",
                path.display(),
                i + 1
            ));
        };
        let key = key.trim();
        let value = strip_quotes(value.trim());
        if env::var_os(key).is_none() {
            // SAFETY: xtask is single-threaded at this point.
            unsafe { env::set_var(key, value) };
        }
    }
    Ok(())
}

fn strip_quotes(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Read a required env var, returning a descriptive error that hints at the
/// dotenv file it usually lives in.
pub fn require_env(key: &str) -> Result<String, String> {
    env::var(key).map_err(|_| {
        format!(
            "missing env var {key} (set it directly or in scripts/.env.publish)"
        )
    })
}
