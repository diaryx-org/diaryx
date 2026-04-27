//! End-to-end sync plugin suite against `diaryx_cloudflare` served by
//! `wrangler dev --env dev --local`.
//!
//! Mirrors `crates/plugins/diaryx_sync_extism/tests/sync_e2e.rs` but points
//! the same `HttpNamespaceProvider` at the cloudflare worker instead of an
//! in-process `diaryx_sync_server`. Adapter-level bugs (URL-encoding in
//! worker routing, D1 persistence quirks, R2 vs SQLite blob semantics)
//! surface here but not in the sync_server E2E.
//!
//! # Source of truth
//!
//! Scenario bodies live in `diaryx_sync_extism::e2e_scenarios` and are
//! shared with `sync_e2e.rs`. To add or fix a scenario, edit it once there
//! and add a call site below.
//!
//! # Boot amortization
//!
//! `wrangler dev` takes ~60s to spin up on a cold cache. Instead of paying
//! that boot per `#[test]` function, we consolidate every scenario into a
//! single `cloudflare_sync_plugin_suite` function that boots wrangler once,
//! runs each scenario under `catch_unwind`, and panics at the end with a
//! summary of every failure. Same pattern as `tests/contract.rs`.
//!
//! # Running
//!
//! ```bash
//! # Build the sync plugin WASM first (release target).
//! cargo build -p diaryx_sync_extism --target wasm32-unknown-unknown --release
//!
//! # Then run the suite (ignored by default).
//! cargo test -p diaryx_cloudflare_e2e --test sync_plugin_e2e -- --ignored --nocapture
//! ```
//!
//! Prerequisites: `bunx` on PATH, wrangler secrets wired for `--env dev`
//! (handled by `WranglerDev`), sync plugin WASM built.
//!
//! # Port isolation
//!
//! Defaults to port **8790** so it can run alongside `contract.rs` (8789)
//! without colliding. Override via `DIARYX_CF_TEST_PORT`.

#![cfg(not(target_arch = "wasm32"))]

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use diaryx_cloudflare_e2e::testing::{WranglerDev, sign_in_dev};
use diaryx_extism::{HttpNamespaceProvider, NamespaceProvider};
use diaryx_sync_extism::e2e_scenarios as scenarios;
use futures::FutureExt;

const PORT: u16 = 8790;

/// Nanos-suffixed email to avoid colliding with persistent D1 state across
/// runs. (Cloudflare `--local` state persists on disk unless the caller sets
/// `DIARYX_CF_RESET_D1=1`.)
fn unique_email(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    format!("cf-sync-{label}-{nanos}@example.com")
}

/// Build a provider authenticated as a freshly-signed-in user. Each call
/// mints a unique email so D1 state from prior runs doesn't collide.
async fn provider_for(
    client: &reqwest::Client,
    base_url: &str,
    api_base_url: &str,
    label: &str,
) -> Arc<dyn NamespaceProvider> {
    let token = sign_in_dev(client, base_url, &unique_email(label)).await;
    Arc::new(HttpNamespaceProvider::new(api_base_url, Some(token)))
}

/// Run a single scenario, capturing any panic so the suite keeps going.
/// Same pattern as `contract.rs`. `make_fut` is `FnOnce` because we pass the
/// already-built harnesses / providers by move.
async fn run_scenario<F, Fut>(name: &str, make_fut: F, failures: &mut Vec<String>)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    eprintln!("▶ {name}");
    let fut = AssertUnwindSafe(make_fut());
    match fut.catch_unwind().await {
        Ok(()) => eprintln!("  ✓ {name}"),
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic payload>".to_string()
            };
            let first_line = msg.lines().next().unwrap_or("").to_string();
            eprintln!("  ✗ {name}: {first_line}");
            failures.push(format!("{name}: {first_line}"));
        }
    }
}

/// Drive every shared sync-plugin scenario against one wrangler dev
/// instance. Scenarios are sequenced (not parallel) because each owns its
/// own workspace temp dirs and we don't want to fight for FS state. They
/// share the authenticated *server*, with each scenario minting its own
/// unique email/namespace to stay isolated against persistent D1 state.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn cloudflare_sync_plugin_suite() {
    if !scenarios::wasm_available() {
        eprintln!(
            "cloudflare sync-plugin suite: skipping — sync plugin WASM not built. \
             Run: cargo build -p diaryx_sync_extism --target wasm32-unknown-unknown --release"
        );
        return;
    }

    let port = std::env::var("DIARYX_CF_TEST_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(PORT);

    let Some(server) = WranglerDev::spawn_on_port(port).await else {
        return; // bunx missing → skip
    };

    let base_url = server.base_url();
    let api_base_url = server.api_base_url();

    // Shared reqwest client for sign-ins — plugin itself uses its own ureq
    // inside `HttpNamespaceProvider`; this one is just for the auth dance.
    let reqwest_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("reqwest client");

    let mut failures: Vec<String> = Vec::new();

    // Scenario 1: two-device round-trip.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "roundtrip").await;
        run_scenario(
            "two_devices_round_trip",
            || scenarios::two_devices_round_trip(provider, "cf-roundtrip"),
            &mut failures,
        )
        .await;
    }

    // Scenario 2: multi-user isolation.
    {
        let alice_provider = provider_for(&reqwest_client, &base_url, &api_base_url, "alice").await;
        let bob_provider = provider_for(&reqwest_client, &base_url, &api_base_url, "bob").await;
        run_scenario(
            "multi_user_isolation",
            || scenarios::multi_user_isolation(alice_provider, bob_provider, "cf-isolation"),
            &mut failures,
        )
        .await;
    }

    // Scenario 3: URL-corpus round-trip via plugin Ns API.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "urlfuzz").await;
        run_scenario(
            "url_corpus_roundtrip",
            || scenarios::url_corpus_roundtrip(provider, "cf-urlfuzz"),
            &mut failures,
        )
        .await;
    }

    // Scenario 4: edit propagates.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "edit").await;
        run_scenario(
            "edit_propagates",
            || scenarios::edit_propagates(provider, "cf-edit"),
            &mut failures,
        )
        .await;
    }

    // Scenario 5: delete propagates.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "delete").await;
        run_scenario(
            "delete_propagates",
            || scenarios::delete_propagates(provider, "cf-delete"),
            &mut failures,
        )
        .await;
    }

    // Scenario 6: idempotent Sync.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "noop").await;
        run_scenario(
            "sync_noop",
            || scenarios::sync_noop(provider, "cf-noop"),
            &mut failures,
        )
        .await;
    }

    // Scenario 7: LWW conflict.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "lww").await;
        run_scenario(
            "lww_resolves_conflict",
            || scenarios::lww_resolves_conflict(provider, "cf-lww"),
            &mut failures,
        )
        .await;
    }

    // Scenario 8: bidirectional non-overlapping edits converge.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "bidir").await;
        run_scenario(
            "bidirectional_edits_converge",
            || scenarios::bidirectional_edits_converge(provider, "cf-bidir"),
            &mut failures,
        )
        .await;
    }

    // Scenario 9: multi-change catch-up.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "catchup").await;
        run_scenario(
            "multi_change_catchup",
            || scenarios::multi_change_catchup(provider, "cf-catchup"),
            &mut failures,
        )
        .await;
    }

    // Scenario 10: small binary file round-trip.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "binary").await;
        run_scenario(
            "binary_file_roundtrip",
            || scenarios::binary_file_roundtrip(provider, "cf-binary"),
            &mut failures,
        )
        .await;
    }

    // Scenario 11: state survives harness reconstruction.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "reconnect").await;
        run_scenario(
            "state_survives_reconstruction",
            || scenarios::state_survives_reconstruction(provider, "cf-reconnect"),
            &mut failures,
        )
        .await;
    }

    // Scenario 12: concurrent Sync from two devices.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "concurrent").await;
        run_scenario(
            "concurrent_syncs_converge",
            || scenarios::concurrent_syncs_converge(provider, "cf-concurrent"),
            &mut failures,
        )
        .await;
    }

    // Scenario 13: 512 KiB binary through R2.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "large").await;
        run_scenario(
            "large_binary_roundtrip",
            || scenarios::large_binary_roundtrip(provider, "cf-large"),
            &mut failures,
        )
        .await;
    }

    // Scenario 14 (NEW vs the previous cf suite): rename round-trip.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "rename").await;
        run_scenario(
            "rename_round_trip",
            || scenarios::rename_round_trip(provider, "cf-rename"),
            &mut failures,
        )
        .await;
    }

    // Scenario 15 (NEW): unlink + relink to same namespace is idempotent.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "relink").await;
        run_scenario(
            "unlink_then_relink_idempotent",
            || scenarios::unlink_then_relink_idempotent(provider, "cf-relink"),
            &mut failures,
        )
        .await;
    }

    // Scenario 16 (NEW): large-workspace pagination through fetch_server_manifest.
    {
        let provider = provider_for(&reqwest_client, &base_url, &api_base_url, "pagination").await;
        run_scenario(
            "large_workspace_paginates",
            || scenarios::large_workspace_paginates(provider, "cf-pagination"),
            &mut failures,
        )
        .await;
    }

    if failures.is_empty() {
        eprintln!("✓ all cloudflare sync-plugin scenarios passed");
    } else {
        eprintln!(
            "\n=== Cloudflare sync-plugin failures ({}) ===",
            failures.len()
        );
        for f in &failures {
            eprintln!("  ✗ {f}");
        }
        panic!(
            "{} cloudflare sync-plugin scenarios failed; see panic messages above each ✗ line.",
            failures.len()
        );
    }

    drop(server);
}
