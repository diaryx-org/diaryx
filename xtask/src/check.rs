use crate::util::{run_checked, workspace_root};
use std::process::Command;
use std::thread;

const USAGE: &str = "Usage: cargo xtask check [--fix]

Runs Rust checks (cargo fmt + clippy) concurrently with the web check
(svelte-check in apps/web). Matches the commands used by pre-commit and CI.

Flags:
  --fix    Apply autofixes on the Rust lane: runs `cargo clippy --fix
           --allow-dirty -p diaryx_core` first, then `cargo fmt --all`.
           svelte-check has no autofix and still runs to surface issues.
";

pub fn run(args: &[String]) -> Result<(), String> {
    let mut fix = false;
    for arg in args {
        match arg.as_str() {
            "--fix" => fix = true,
            "-h" | "--help" | "help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown flag for check: {other}\n\n{USAGE}")),
        }
    }

    let root = workspace_root();

    let web_handle = {
        let web_dir = root.join("apps/web");
        thread::spawn(move || -> Result<(), String> {
            println!("==> svelte-check (apps/web)");
            let mut cmd = Command::new("bun");
            cmd.current_dir(&web_dir).args(["run", "check"]);
            run_checked(&mut cmd, "bun run check")
        })
    };

    let rust_result: Result<(), String> = (|| {
        if fix {
            println!("==> cargo clippy --fix --allow-dirty -p diaryx_core");
            let mut clippy = Command::new("cargo");
            clippy.current_dir(&root).args([
                "clippy",
                "--fix",
                "--allow-dirty",
                "-p",
                "diaryx_core",
                "--",
                "-D",
                "warnings",
            ]);
            run_checked(&mut clippy, "cargo clippy --fix")?;

            println!("==> cargo fmt --all");
            let mut fmt = Command::new("cargo");
            fmt.current_dir(&root).args(["fmt", "--all"]);
            run_checked(&mut fmt, "cargo fmt")?;
        } else {
            println!("==> cargo fmt --all -- --check");
            let mut fmt = Command::new("cargo");
            fmt.current_dir(&root)
                .args(["fmt", "--all", "--", "--check"]);
            run_checked(&mut fmt, "cargo fmt --check")?;

            println!("==> cargo clippy -p diaryx_core -- -D warnings");
            let mut clippy = Command::new("cargo");
            clippy
                .current_dir(&root)
                .args(["clippy", "-p", "diaryx_core", "--", "-D", "warnings"]);
            run_checked(&mut clippy, "cargo clippy")?;
        }
        Ok(())
    })();

    let web_result = web_handle
        .join()
        .map_err(|_| "svelte-check thread panicked".to_string())?;

    match (rust_result, web_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(r), Err(w)) => Err(format!("rust lane: {r}\nweb lane: {w}")),
        (Err(e), Ok(())) => Err(format!("rust lane: {e}")),
        (Ok(()), Err(e)) => Err(format!("web lane: {e}")),
    }
}
