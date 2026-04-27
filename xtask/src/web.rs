use crate::util::{run_checked, workspace_root};
use std::process::Command;

const USAGE: &str = "Usage: cargo xtask web [subcommand] [args...]

Wraps `bun run <script>` in apps/web. Defaults to `dev` when no subcommand
is given.

Subcommands (any package.json script in apps/web works; common ones):
  dev          (default) portless + vite dev server
  dev:bun      bun serve.ts
  build        bun run build:wasm && vite build
  preview      vite preview
  check        svelte-check
  test         vitest
  test:e2e     playwright test
";

pub fn run(args: &[String]) -> Result<(), String> {
    if matches!(
        args.first().map(String::as_str),
        Some("-h" | "--help" | "help")
    ) {
        println!("{USAGE}");
        return Ok(());
    }

    let mut script_args: Vec<String> = args.to_vec();
    if script_args.is_empty() {
        script_args.push("dev".into());
    }

    let web_dir = workspace_root().join("apps/web");
    let mut cmd = Command::new("bun");
    cmd.current_dir(&web_dir).arg("run").args(&script_args);
    run_checked(&mut cmd, "bun run")
}
