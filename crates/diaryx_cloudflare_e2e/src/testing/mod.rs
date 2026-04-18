//! Shared fixtures for `diaryx_cloudflare_e2e` test binaries.
//!
//! - [`WranglerDev`] — spawns `bunx wrangler dev --env dev --local` against
//!   the `diaryx_cloudflare` crate directory on an OS-chosen port,
//!   idempotently applies local D1 migrations first, and kills the child
//!   on drop.
//! - [`sign_in_dev`] — drives the dev-mode magic-link → verify dance
//!   against any Diaryx server base URL (sync_server or cloudflare), using
//!   reqwest. Mirrors `diaryx_sync_server::testing::TestServer::sign_in_dev`
//!   but free-standing so consumers can point it at a live wrangler port.
//!
//! Both are intentionally live-process fixtures: the `tower::oneshot` /
//! `axum::serve` patterns used elsewhere can't host `diaryx_cloudflare`
//! (wasm32-only crate), so tests must drive a real Workers runtime.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const DEFAULT_PORT: u16 = 8789;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(180);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------------------
// WranglerDev
// ---------------------------------------------------------------------------

/// Owned `bunx wrangler dev --env dev --local` child. Dropped at the end of
/// each test run — we `kill` aggressively so a panicking test doesn't leave
/// wrangler lingering on the port.
pub struct WranglerDev {
    child: Child,
    port: u16,
}

impl WranglerDev {
    /// Name of the local D1 database declared in `wrangler.jsonc` under
    /// `env.dev.d1_databases[0].database_name`. Keep in sync.
    const LOCAL_D1_DB: &'static str = "diaryx-users-dev";

    /// Wrangler environment whose bindings (D1 / R2 / KV / DO) we target.
    const WRANGLER_ENV: &'static str = "dev";

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
    /// lives. Computed relative to this crate's manifest.
    pub fn cloudflare_crate_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace crates dir")
            .join("diaryx_cloudflare")
    }

    /// Apply D1 migrations to the local SQLite used by wrangler's local
    /// mode. Idempotent — wrangler tracks applied migrations per DB.
    fn apply_local_migrations(crate_dir: &std::path::Path) -> Result<(), String> {
        eprintln!(
            "wrangler e2e: applying D1 migrations to local {}…",
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

    /// Read the optional `DIARYX_CF_TEST_PORT` env override, falling back to
    /// [`DEFAULT_PORT`]. Distinct tests should set different ports (or run
    /// with `--test-threads=1`) to avoid collisions on the same wrangler
    /// process.
    pub fn env_port() -> u16 {
        std::env::var("DIARYX_CF_TEST_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_PORT)
    }

    /// Spawn wrangler and wait for `/api/health` to return 2xx. Returns
    /// `None` when `bunx` isn't available or `wrangler.jsonc` is missing —
    /// those are "environment not set up" skips. Panics on real failures
    /// (migrations apply, spawn, or readiness timeout).
    pub async fn spawn() -> Option<Self> {
        Self::spawn_on_port(Self::env_port()).await
    }

    /// Explicit-port variant so multiple tests in the same binary can use
    /// non-conflicting ports.
    pub async fn spawn_on_port(port: u16) -> Option<Self> {
        if !Self::is_bunx_available() {
            eprintln!(
                "wrangler e2e: skipping — bunx not found on PATH. \
                 Install Bun (https://bun.sh) and re-run."
            );
            return None;
        }

        let crate_dir = Self::cloudflare_crate_dir();
        if !crate_dir.join("wrangler.jsonc").exists() {
            eprintln!(
                "wrangler e2e: skipping — {} missing",
                crate_dir.join("wrangler.jsonc").display()
            );
            return None;
        }

        // Optional: wipe local D1 state to recover from inconsistent
        // migration state. Opt-in so we don't silently destroy dev data.
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
                    "wrangler e2e: DIARYX_CF_RESET_D1=1 — removing {}",
                    state_dir.display()
                );
                if let Err(e) = std::fs::remove_dir_all(&state_dir) {
                    panic!(
                        "wrangler e2e: failed to reset local D1 at {}: {e}",
                        state_dir.display()
                    );
                }
            }
        }

        if let Err(e) = Self::apply_local_migrations(&crate_dir) {
            panic!(
                "wrangler e2e: migrations apply failed: {e}\n\
                 If the error was \"duplicate column\" / \"already exists\", your \
                 local D1 is in an inconsistent state. Re-run with \
                 DIARYX_CF_RESET_D1=1 to wipe and reapply."
            );
        }

        eprintln!(
            "wrangler e2e: launching `bunx wrangler dev --env {} --local --port {port}` \
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
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .env("CI", "1");

        let child = cmd
            .spawn()
            .unwrap_or_else(|e| panic!("wrangler e2e: failed to spawn bunx: {e}"));
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
                    "wrangler e2e: timed out after {elapsed:?} waiting for \
                     {url} to return 2xx"
                );
            }
            if let Ok(resp) = client.get(&url).send().await {
                if resp.status().is_success() {
                    eprintln!(
                        "wrangler e2e: ready on port {port} after {:?}",
                        start.elapsed()
                    );
                    return Some(server);
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Origin URL — `http://127.0.0.1:PORT`, no path prefix. For HTTP
    /// clients that include `/api/...` in their paths (e.g.
    /// `ReqwestDispatcher` for the contract suite).
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// `http://127.0.0.1:PORT/api` — for consumers that expect the API
    /// prefix baked into the base, most notably `HttpNamespaceProvider`.
    pub fn api_base_url(&self) -> String {
        format!("http://127.0.0.1:{}/api", self.port)
    }
}

impl Drop for WranglerDev {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Dev-mode sign-in
// ---------------------------------------------------------------------------

/// Complete the dev-mode magic-link → verify dance against a running Diaryx
/// server and return a session token. Works against any adapter that honors
/// the dev-mode contract (empty `RESEND_API_KEY` on sync_server, or
/// `DEV_MODE=true` on the cloudflare worker).
///
/// Panics on any non-2xx response or malformed JSON so callers don't have
/// to thread `Result` through test code. `base_url` is the server **origin**
/// — e.g. `http://127.0.0.1:8789`. The `/api` prefix is added internally.
pub async fn sign_in_dev(client: &reqwest::Client, base_url: &str, email: &str) -> String {
    // POST /api/auth/magic-link
    let ml_url = format!("{}/api/auth/magic-link", base_url);
    let ml_resp: serde_json::Value = client
        .post(&ml_url)
        .json(&serde_json::json!({ "email": email }))
        .send()
        .await
        .unwrap_or_else(|e| panic!("sign_in_dev: magic-link POST failed: {e}"))
        .error_for_status()
        .unwrap_or_else(|e| panic!("sign_in_dev: magic-link non-2xx: {e}"))
        .json()
        .await
        .unwrap_or_else(|e| panic!("sign_in_dev: magic-link response parse: {e}"));

    let dev_link = ml_resp
        .get("dev_link")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            panic!(
                "sign_in_dev: dev_mode response missing dev_link. \
                 Is DEV_MODE=true / RESEND_API_KEY empty on the target? Got: {ml_resp}"
            )
        });

    // Extract ?token=... from dev_link.
    let token = dev_link
        .split("token=")
        .nth(1)
        .unwrap_or_else(|| panic!("sign_in_dev: dev_link missing token= param: {dev_link}"))
        .split('&')
        .next()
        .unwrap_or("")
        .to_string();

    // GET /api/auth/verify?token=<TOKEN>&device_name=e2e
    let verify_url = format!(
        "{}/api/auth/verify?token={}&device_name=e2e",
        base_url, token
    );
    let verify_resp: serde_json::Value = client
        .get(&verify_url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("sign_in_dev: verify GET failed: {e}"))
        .error_for_status()
        .unwrap_or_else(|e| panic!("sign_in_dev: verify non-2xx: {e}"))
        .json()
        .await
        .unwrap_or_else(|e| panic!("sign_in_dev: verify response parse: {e}"));

    verify_resp
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("sign_in_dev: verify response missing token: {verify_resp}"))
        .to_string()
}
