//! End-to-end sync plugin tests against an in-process
//! `diaryx_sync_server::testing::TestServer`.
//!
//! Each `#[tokio::test]` is a thin wrapper that:
//!   1. Spawns a fresh in-memory `TestServer`.
//!   2. Signs in (dev-mode magic-link).
//!   3. Builds an `HttpNamespaceProvider` against the server.
//!   4. Calls into `diaryx_sync_extism::e2e_scenarios::*` to run the actual
//!      scenario body.
//!
//! The scenario bodies are shared with `crates/diaryx_cloudflare_e2e/tests/
//! sync_plugin_e2e.rs`, which runs the *same* suite against a `wrangler dev`
//! instance of `diaryx_cloudflare`. Bugs that surface in only one backend
//! point to a sync_server / cloudflare drift; bugs that surface in both
//! point to a sync-plugin issue.
//!
//! Skips with a printed message when the plugin WASM hasn't been built —
//! run `cargo build -p diaryx_sync_extism --target wasm32-unknown-unknown
//! --release` first.

use std::sync::Arc;

use diaryx_extism::{HttpNamespaceProvider, NamespaceProvider};
use diaryx_sync_extism::e2e_scenarios as scenarios;
use diaryx_sync_server::testing::TestServer;

/// Early-return if the WASM file hasn't been built. Prints a hint so the
/// skip is actionable rather than silent.
macro_rules! require_wasm {
    () => {
        if !scenarios::wasm_available() {
            eprintln!(
                "Skipping: sync plugin WASM not built. Run: \
                 cargo build -p diaryx_sync_extism --target wasm32-unknown-unknown --release"
            );
            return;
        }
    };
}

/// Spin up a `TestServer`, sign in as `email`, and build a provider against
/// `/api`. Returned `TestServer` must be kept alive until the test ends.
async fn setup(email: &str) -> (TestServer, Arc<dyn NamespaceProvider>) {
    let server = TestServer::start().await;
    let token = server.sign_in_dev(email).await;
    let provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(token),
    ));
    (server, provider)
}

// ---------------------------------------------------------------------------
// Wrappers — one per scenario. Naming kept stable for CI history continuity.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_devices_sync_via_real_http_server() {
    require_wasm!();
    let (server, provider) = setup("e2e@example.com").await;
    scenarios::two_devices_round_trip(provider, "roundtrip").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bob_cannot_access_alices_namespace() {
    require_wasm!();
    let server = TestServer::start().await;
    let alice_token = server.sign_in_dev("alice@example.com").await;
    let bob_token = server.sign_in_dev("bob@example.com").await;
    let alice_provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(alice_token),
    ));
    let bob_provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(bob_token),
    ));
    scenarios::multi_user_isolation(alice_provider, bob_provider, "isolation").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn url_corpus_keys_roundtrip_via_plugin_ns_api() {
    require_wasm!();
    let (server, provider) = setup("fuzz@example.com").await;
    scenarios::url_corpus_roundtrip(provider, "urlfuzz").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edit_on_a_propagates_to_b_via_sync() {
    require_wasm!();
    let (server, provider) = setup("edit-prop@example.com").await;
    scenarios::edit_propagates(provider, "edit").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delete_on_a_propagates_to_b_via_sync() {
    require_wasm!();
    let (server, provider) = setup("delete-prop@example.com").await;
    scenarios::delete_propagates(provider, "delete").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sync_with_no_changes_is_noop() {
    require_wasm!();
    let (server, provider) = setup("noop@example.com").await;
    scenarios::sync_noop(provider, "noop").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lww_resolves_conflict_in_favor_of_later_mtime() {
    require_wasm!();
    let (server, provider) = setup("lww@example.com").await;
    scenarios::lww_resolves_conflict(provider, "lww").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bidirectional_edits_converge() {
    require_wasm!();
    let (server, provider) = setup("bidir@example.com").await;
    scenarios::bidirectional_edits_converge(provider, "bidir").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_change_catchup_in_single_sync() {
    require_wasm!();
    let (server, provider) = setup("catchup@example.com").await;
    scenarios::multi_change_catchup(provider, "catchup").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn binary_file_roundtrip_via_plugin() {
    require_wasm!();
    let (server, provider) = setup("binary@example.com").await;
    scenarios::binary_file_roundtrip(provider, "binary").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sync_state_survives_harness_reconstruction() {
    require_wasm!();
    let (server, provider) = setup("reconnect@example.com").await;
    scenarios::state_survives_reconstruction(provider, "reconnect").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_syncs_from_two_devices_converge() {
    require_wasm!();
    let (server, provider) = setup("concurrent@example.com").await;
    scenarios::concurrent_syncs_converge(provider, "concurrent").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn large_binary_file_roundtrips_via_plugin() {
    require_wasm!();
    let (server, provider) = setup("large@example.com").await;
    scenarios::large_binary_roundtrip(provider, "large").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rename_round_trips_to_other_device() {
    require_wasm!();
    let (server, provider) = setup("rename@example.com").await;
    scenarios::rename_round_trip(provider, "rename").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unlink_then_relink_same_namespace_is_idempotent() {
    require_wasm!();
    let (server, provider) = setup("relink@example.com").await;
    scenarios::unlink_then_relink_idempotent(provider, "relink").await;
    drop(server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn large_workspace_paginates_correctly() {
    require_wasm!();
    let (server, provider) = setup("pagination@example.com").await;
    scenarios::large_workspace_paginates(provider, "pagination").await;
    drop(server);
}
