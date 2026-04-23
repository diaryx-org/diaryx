use crate::util::workspace_root;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const USAGE: &str = "Usage: cargo xtask install-hooks [--force]

Installs git hooks that invoke `cargo xtask pre-commit`. Refuses to
overwrite an existing hook unless --force is passed.
";

const HOOK_MARKER: &str = "# Managed by `cargo xtask install-hooks`";

pub fn run(args: &[String]) -> Result<(), String> {
    let mut force = false;
    for arg in args {
        match arg.as_str() {
            "--force" | "-f" => force = true,
            "-h" | "--help" | "help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => {
                return Err(format!(
                    "unknown flag for install-hooks: {other}\n\n{USAGE}"
                ));
            }
        }
    }

    let root = workspace_root();
    let hooks_dir = git_hooks_dir(&root)?;
    fs::create_dir_all(&hooks_dir).map_err(|e| format!("create {}: {e}", hooks_dir.display()))?;

    let hook_path = hooks_dir.join("pre-commit");
    let contents = format!(
        "#!/bin/sh\n\
         {HOOK_MARKER}\n\
         exec cargo xtask pre-commit \"$@\"\n"
    );

    if hook_path.exists() && !force {
        let existing = fs::read_to_string(&hook_path).unwrap_or_default();
        if !existing.contains(HOOK_MARKER) {
            return Err(format!(
                "{} exists and was not written by this tool; re-run with --force to overwrite",
                hook_path.display()
            ));
        }
    }

    fs::write(&hook_path, &contents).map_err(|e| format!("write {}: {e}", hook_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&hook_path)
            .map_err(|e| format!("stat {}: {e}", hook_path.display()))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&hook_path, perms)
            .map_err(|e| format!("chmod {}: {e}", hook_path.display()))?;
    }

    println!("Installed {}", hook_path.display());
    Ok(())
}

fn git_hooks_dir(root: &std::path::Path) -> Result<PathBuf, String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--git-path", "hooks"])
        .output()
        .map_err(|e| format!("git rev-parse --git-path hooks: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git rev-parse --git-path hooks failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let path = PathBuf::from(&raw);
    Ok(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}
