//! Runs the shared [`diaryx_server::contract`] suite against
//! `diaryx_cloudflare` served by `wrangler dev --local`.
//!
//! This is the seam that turns the contract suite into a cross-adapter drift
//! detector: any test in [`diaryx_server::contract`] runs here too, and
//! divergences between this adapter and `diaryx_sync_server` surface as test
//! failures rather than production incidents (commits `a03a0732`,
//! `044a78c9`, and `9927b5f9` are the kind of bugs this is meant to catch).
//!
//! # Running
//!
//! Tests are `#[ignore]` by default because they:
//!
//! - Require `bunx` (Bun) on `PATH`. We use `bunx wrangler` so no global
//!   wrangler install is needed, but Bun itself must be available.
//! - Spawn a real `wrangler dev --local` process, which runs `worker-build`
//!   and boots miniflare — **first run can take over a minute** while the
//!   worker compiles.
//!
//! To run:
//!
//! ```bash
//! cargo test -p diaryx_cloudflare_e2e --test contract -- --ignored --nocapture
//! ```
//!
//! Override the port with `DIARYX_CF_TEST_PORT=9999`.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use diaryx_server::contract::{self, http::ReqwestDispatcher};

const DEFAULT_PORT: u16 = 8789;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(180);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Owned `bunx wrangler dev --local` child. Dropped at the end of the test
/// run — we kill aggressively so a panicking test doesn't leave wrangler
/// lingering on the port.
struct WranglerDev {
    child: Child,
    port: u16,
}

impl WranglerDev {
    fn is_bunx_available() -> bool {
        Command::new("bunx")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Path to the `diaryx_cloudflare` crate directory, where `wrangler.jsonc`
    /// lives. Computed relative to this test crate's manifest.
    fn cloudflare_crate_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace crates dir")
            .join("diaryx_cloudflare")
    }

    /// Name of the local D1 database as declared in `wrangler.jsonc` under
    /// `env.dev.d1_databases[0].database_name`. Keep in sync.
    const LOCAL_D1_DB: &'static str = "diaryx-users-dev";

    /// Name of the wrangler environment whose bindings we target. `env.dev`
    /// points at `diaryx-users-dev`, local-safe URLs, and `DEV_MODE=true`
    /// (which makes the magic-link handler return the token directly in the
    /// response body). Without `--env dev`, wrangler starts the worker with
    /// no bindings and DB access 500s with "Binding `diaryx_users` is
    /// undefined".
    const WRANGLER_ENV: &'static str = "dev";

    /// Apply D1 migrations to the local (on-disk sqlite) D1 database. Safe
    /// to call on every run — wrangler records applied migrations in the
    /// local D1 state so subsequent invocations are no-ops.
    ///
    /// Without this step, local D1 starts empty and the first request that
    /// touches SQLite fails with "no such table: users".
    fn apply_local_migrations(crate_dir: &std::path::Path) -> Result<(), String> {
        eprintln!(
            "wrangler contract tests: applying D1 migrations to local {}…",
            Self::LOCAL_D1_DB
        );
        let status = Command::new("bunx")
            .current_dir(crate_dir)
            .arg("wrangler")
            .arg("d1")
            .arg("migrations")
            .arg("apply")
            .arg(Self::LOCAL_D1_DB)
            .arg("--local")
            .arg("--env")
            .arg(Self::WRANGLER_ENV)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .env("CI", "1")
            .status()
            .map_err(|e| format!("failed to spawn wrangler d1 migrations apply: {e}"))?;
        if !status.success() {
            return Err(format!(
                "wrangler d1 migrations apply exited with status {status}"
            ));
        }
        Ok(())
    }

    /// Attempt to spawn wrangler and wait until `/api/health` returns 2xx.
    ///
    /// Returns `None` when `bunx` isn't available (tests skip) or when
    /// wrangler fails to become ready within the timeout. Either outcome is
    /// logged to stderr so `--nocapture` runs make the skip visible.
    async fn spawn() -> Option<Self> {
        if !Self::is_bunx_available() {
            eprintln!(
                "wrangler contract tests: skipping — bunx not found on PATH. \
                 Install Bun (https://bun.sh) and re-run."
            );
            return None;
        }

        let crate_dir = Self::cloudflare_crate_dir();
        if !crate_dir.join("wrangler.jsonc").exists() {
            eprintln!(
                "wrangler contract tests: skipping — {} missing",
                crate_dir.join("wrangler.jsonc").display()
            );
            return None;
        }

        let port = std::env::var("DIARYX_CF_TEST_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_PORT);

        // Optional: wipe local D1 state before applying migrations. Useful
        // when a prior partial run left the DB in an inconsistent state
        // (e.g. a schema mid-flight or a column created outside the
        // migrations table). Opt-in via `DIARYX_CF_RESET_D1=1` so it doesn't
        // silently destroy dev data.
        if std::env::var("DIARYX_CF_RESET_D1")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        {
            let state_dir = crate_dir
                .join(".wrangler")
                .join("state")
                .join("v3")
                .join("d1");
            if state_dir.exists() {
                eprintln!(
                    "wrangler contract tests: DIARYX_CF_RESET_D1=1 — removing {}",
                    state_dir.display()
                );
                if let Err(e) = std::fs::remove_dir_all(&state_dir) {
                    panic!(
                        "wrangler contract tests: failed to reset local D1 at {}: {e}",
                        state_dir.display()
                    );
                }
            }
        }

        // Apply migrations first — wrangler dev starts an empty local D1 if
        // the DB hasn't been initialized, and requests 500 when tables are
        // missing. This step is idempotent. A failure here is a real test
        // failure, not a reason to skip — bunx is clearly available (we
        // checked above), so migrations failing means something is wrong
        // with the repo/config, not the environment.
        //
        // If migrations fail with "duplicate column" or similar, the local
        // D1 state is inconsistent; re-run with `DIARYX_CF_RESET_D1=1` to
        // wipe `.wrangler/state/v3/d1/` and apply from scratch.
        if let Err(e) = Self::apply_local_migrations(&crate_dir) {
            panic!(
                "wrangler contract tests: migrations apply failed: {e}\n\
                 If the error was \"duplicate column\" / \"already exists\", your \
                 local D1 is in an inconsistent state. Re-run with \
                 DIARYX_CF_RESET_D1=1 to wipe and reapply."
            );
        }

        eprintln!(
            "wrangler contract tests: launching `bunx wrangler dev --env {} --local --port {port}` \
             in {} (first run may take over a minute while worker-build runs)…",
            Self::WRANGLER_ENV,
            crate_dir.display()
        );

        let mut cmd = Command::new("bunx");
        cmd.current_dir(&crate_dir)
            .arg("wrangler")
            .arg("dev")
            .arg("--env")
            .arg(Self::WRANGLER_ENV)
            .arg("--local")
            .arg("--port")
            .arg(port.to_string())
            // Forward wrangler's output so users see build progress under --nocapture.
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            // Discourage wrangler from asking interactive questions.
            .env("CI", "1");

        let child = cmd
            .spawn()
            .unwrap_or_else(|e| panic!("wrangler contract tests: failed to spawn bunx: {e}"));
        let server = Self { child, port };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("reqwest client");
        let url = format!("http://127.0.0.1:{port}/api/health");
        let start = Instant::now();

        loop {
            if start.elapsed() > STARTUP_TIMEOUT {
                let elapsed = start.elapsed();
                drop(server);
                panic!(
                    "wrangler contract tests: timed out after {elapsed:?} waiting for \
                     {url} to return 2xx"
                );
            }
            if let Ok(resp) = client.get(&url).send().await {
                if resp.status().is_success() {
                    eprintln!(
                        "wrangler contract tests: ready on port {port} after {:?}",
                        start.elapsed()
                    );
                    return Some(server);
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for WranglerDev {
    fn drop(&mut self) {
        // Best-effort termination. `kill` sends SIGKILL on Unix; wrangler's
        // miniflare child may linger briefly but the OS reaps it.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// All contract tests run inside ONE `#[tokio::test]` against a single
/// wrangler instance — spinning up wrangler once per test would push total
/// runtime into tens of minutes. Individual contract-test function names
/// are logged via `eprintln!` so a failure surfaces which scenario failed.
#[tokio::test]
#[ignore]
async fn cloudflare_contract_suite() {
    let Some(server) = WranglerDev::spawn().await else {
        // Skip rather than fail — keeps the ignored-by-default behavior
        // consistent whether or not bunx is installed.
        return;
    };
    let dispatcher = ReqwestDispatcher::new(server.base_url());

    eprintln!("▶ test_health_endpoint_returns_200_ok");
    contract::test_health_endpoint_returns_200_ok(&dispatcher).await;

    eprintln!("▶ test_magic_link_dev_mode_returns_dev_credentials");
    contract::test_magic_link_dev_mode_returns_dev_credentials(&dispatcher).await;

    eprintln!("▶ test_magic_link_rejects_invalid_email");
    contract::test_magic_link_rejects_invalid_email(&dispatcher).await;

    eprintln!("✓ all contract tests passed");
}
