//! End-to-end sync plugin suite against `diaryx_cloudflare` served by
//! `wrangler dev --env dev --local`.
//!
//! Mirrors the scenarios in
//! `crates/plugins/diaryx_sync_extism/tests/sync_e2e.rs` but points the same
//! `HttpNamespaceProvider` at the cloudflare worker instead of a locally-
//! spawned `diaryx_sync_server`. Adapter-level bugs (URL-encoding in worker
//! routing, D1 persistence quirks, R2 vs SQLite blob semantics) surface here
//! but not in the sync_server E2E.
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
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use diaryx_cloudflare_e2e::testing::{WranglerDev, sign_in_dev};
use diaryx_extism::testing::{PluginTestHarness, PluginTestHarnessBuilder, RecordingStorage};
use diaryx_extism::{HttpNamespaceProvider, NamespaceProvider};
use futures::FutureExt;
use serde_json::{Value as JsonValue, json};

/// Workspace-root-relative path to the built sync-plugin WASM. Must be an
/// absolute compile-time path — cargo's CWD at test-run time is the package
/// dir, so a relative path silently misses and every scenario would skip.
const WASM_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/wasm32-unknown-unknown/release/diaryx_sync_extism.wasm"
);

const PORT: u16 = 8790;

// ---------------------------------------------------------------------------
// Workspace / harness helpers (mirrors sync_e2e.rs)
// ---------------------------------------------------------------------------

fn unique_temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "diaryx-cf-sync-e2e-{label}-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("temp dir");
    path
}

fn write_workspace_file(root_dir: &std::path::Path, relative_path: &str, contents: &str) {
    let path = root_dir.join(relative_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("workspace parent dir");
    }
    std::fs::write(path, contents).expect("write workspace file");
}

fn create_workspace(root_filename: &str, root_contents: &str, files: &[(&str, &str)]) -> PathBuf {
    let root_dir = unique_temp_dir(root_filename.trim_end_matches(".md"));
    write_workspace_file(&root_dir, root_filename, root_contents);
    for (relative_path, contents) in files {
        write_workspace_file(&root_dir, relative_path, contents);
    }
    root_dir.join(root_filename)
}

fn build_harness(
    workspace_root: &std::path::Path,
    storage: Arc<RecordingStorage>,
    provider: Arc<dyn NamespaceProvider>,
) -> PluginTestHarness {
    PluginTestHarnessBuilder::new(WASM_PATH)
        .with_storage(storage)
        .with_workspace_root(workspace_root)
        .with_namespace_provider(provider)
        .build()
        .expect("Failed to load sync plugin WASM")
}

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

// ---------------------------------------------------------------------------
// The suite
// ---------------------------------------------------------------------------

/// Drive every sync-plugin scenario against one wrangler dev instance. The
/// scenarios are sequenced (not parallel) because each needs its own
/// workspace directories and we don't want to fight for FS state — but they
/// share the authenticated *server*, so each creates its own unique email /
/// namespace to stay isolated.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn cloudflare_sync_plugin_suite() {
    if !std::path::Path::new(WASM_PATH).exists() {
        eprintln!(
            "cloudflare sync-plugin suite: skipping — WASM not built at {WASM_PATH}. \
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

    // -- Scenario 1: two-device round-trip (Link / Download byte parity) ---
    {
        let email = unique_email("roundtrip");
        let token = sign_in_dev(&reqwest_client, &base_url, &email).await;
        let provider: Arc<dyn NamespaceProvider> =
            Arc::new(HttpNamespaceProvider::new(&api_base_url, Some(token)));
        run_scenario(
            "two_devices_sync_link_and_download",
            || scenario_two_devices_link_download(provider),
            &mut failures,
        )
        .await;
    }

    // -- Scenario 2: multi-user isolation (alice / bob) --------------------
    {
        let alice_email = unique_email("alice");
        let bob_email = unique_email("bob");
        let alice_token = sign_in_dev(&reqwest_client, &base_url, &alice_email).await;
        let bob_token = sign_in_dev(&reqwest_client, &base_url, &bob_email).await;
        let alice_provider: Arc<dyn NamespaceProvider> =
            Arc::new(HttpNamespaceProvider::new(&api_base_url, Some(alice_token)));
        let bob_provider: Arc<dyn NamespaceProvider> =
            Arc::new(HttpNamespaceProvider::new(&api_base_url, Some(bob_token)));
        run_scenario(
            "bob_cannot_access_alices_namespace",
            || scenario_multi_user_isolation(alice_provider, bob_provider),
            &mut failures,
        )
        .await;
    }

    // -- Scenario 3: URL-encoding corpus fuzz through plugin NsPutObject ---
    {
        let email = unique_email("urlfuzz");
        let token = sign_in_dev(&reqwest_client, &base_url, &email).await;
        let provider: Arc<dyn NamespaceProvider> =
            Arc::new(HttpNamespaceProvider::new(&api_base_url, Some(token)));
        run_scenario(
            "url_corpus_keys_roundtrip_via_plugin_ns_api",
            || scenario_url_corpus_roundtrip(provider),
            &mut failures,
        )
        .await;
    }

    // -- Scenario 4: edit on A propagates to B via Sync --------------------
    {
        let email = unique_email("edit");
        let token = sign_in_dev(&reqwest_client, &base_url, &email).await;
        let provider: Arc<dyn NamespaceProvider> =
            Arc::new(HttpNamespaceProvider::new(&api_base_url, Some(token)));
        run_scenario(
            "edit_on_a_propagates_to_b_via_sync",
            || scenario_edit_propagates(provider),
            &mut failures,
        )
        .await;
    }

    // -- Scenario 5: delete on A propagates to B via Sync ------------------
    {
        let email = unique_email("delete");
        let token = sign_in_dev(&reqwest_client, &base_url, &email).await;
        let provider: Arc<dyn NamespaceProvider> =
            Arc::new(HttpNamespaceProvider::new(&api_base_url, Some(token)));
        run_scenario(
            "delete_on_a_propagates_to_b_via_sync",
            || scenario_delete_propagates(provider),
            &mut failures,
        )
        .await;
    }

    // -- Scenario 6: idempotent Sync ---------------------------------------
    {
        let email = unique_email("noop");
        let token = sign_in_dev(&reqwest_client, &base_url, &email).await;
        let provider: Arc<dyn NamespaceProvider> =
            Arc::new(HttpNamespaceProvider::new(&api_base_url, Some(token)));
        run_scenario(
            "sync_with_no_changes_is_noop",
            || scenario_sync_noop(provider),
            &mut failures,
        )
        .await;
    }

    // -- Scenario 7: LWW conflict resolves to newer mtime ------------------
    {
        let email = unique_email("lww");
        let token = sign_in_dev(&reqwest_client, &base_url, &email).await;
        let provider: Arc<dyn NamespaceProvider> =
            Arc::new(HttpNamespaceProvider::new(&api_base_url, Some(token)));
        run_scenario(
            "lww_resolves_conflict_in_favor_of_later_mtime",
            || scenario_lww(provider),
            &mut failures,
        )
        .await;
    }

    // -- Scenario 8: bidirectional non-overlapping edits converge ---------
    {
        let email = unique_email("bidir");
        let token = sign_in_dev(&reqwest_client, &base_url, &email).await;
        let provider: Arc<dyn NamespaceProvider> =
            Arc::new(HttpNamespaceProvider::new(&api_base_url, Some(token)));
        run_scenario(
            "bidirectional_edits_converge",
            || scenario_bidirectional(provider),
            &mut failures,
        )
        .await;
    }

    // -- Scenario 9: multi-change catch-up in a single Sync ---------------
    {
        let email = unique_email("catchup");
        let token = sign_in_dev(&reqwest_client, &base_url, &email).await;
        let provider: Arc<dyn NamespaceProvider> =
            Arc::new(HttpNamespaceProvider::new(&api_base_url, Some(token)));
        run_scenario(
            "multi_change_catchup_in_single_sync",
            || scenario_catchup(provider),
            &mut failures,
        )
        .await;
    }

    // -- Scenario 10: binary file round-trip via plugin NsPut/NsGet -------
    {
        let email = unique_email("binary");
        let token = sign_in_dev(&reqwest_client, &base_url, &email).await;
        let provider: Arc<dyn NamespaceProvider> =
            Arc::new(HttpNamespaceProvider::new(&api_base_url, Some(token)));
        run_scenario(
            "binary_file_roundtrip_via_plugin",
            || scenario_binary(provider),
            &mut failures,
        )
        .await;
    }

    // -- Scenario 11: reconnect — manifest survives harness reconstruction -
    {
        let email = unique_email("reconnect");
        let token = sign_in_dev(&reqwest_client, &base_url, &email).await;
        let provider: Arc<dyn NamespaceProvider> =
            Arc::new(HttpNamespaceProvider::new(&api_base_url, Some(token)));
        run_scenario(
            "sync_state_survives_harness_reconstruction",
            || scenario_reconnect(provider),
            &mut failures,
        )
        .await;
    }

    // -- Scenario 12: concurrent Sync from two devices ---------------------
    {
        let email = unique_email("concurrent");
        let token = sign_in_dev(&reqwest_client, &base_url, &email).await;
        let provider: Arc<dyn NamespaceProvider> =
            Arc::new(HttpNamespaceProvider::new(&api_base_url, Some(token)));
        run_scenario(
            "concurrent_syncs_from_two_devices_converge",
            || scenario_concurrent_sync(provider),
            &mut failures,
        )
        .await;
    }

    // -- Scenario 13: large binary file (512 KiB) --------------------------
    {
        let email = unique_email("large");
        let token = sign_in_dev(&reqwest_client, &base_url, &email).await;
        let provider: Arc<dyn NamespaceProvider> =
            Arc::new(HttpNamespaceProvider::new(&api_base_url, Some(token)));
        run_scenario(
            "large_binary_file_roundtrips_via_plugin",
            || scenario_large_binary(provider),
            &mut failures,
        )
        .await;
    }

    // -- Summary -----------------------------------------------------------
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

// ---------------------------------------------------------------------------
// Scenario implementations
// ---------------------------------------------------------------------------

async fn scenario_two_devices_link_download(provider: Arc<dyn NamespaceProvider>) {
    let workspace_a = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - hello.md\n  - notes/day-1.md\n---\n\n# Root\n",
        &[
            (
                "hello.md",
                "---\ntitle: Hello\npart_of: index.md\n---\n\n# Hello from A\n",
            ),
            (
                "notes/day-1.md",
                "---\ntitle: Day 1\npart_of: index.md\n---\n\n# Nested note\n",
            ),
        ],
    );
    let storage_a = Arc::new(RecordingStorage::new());
    let harness_a = build_harness(&workspace_a, storage_a, provider.clone());
    harness_a.init().await.expect("init A");

    let link_result: JsonValue = harness_a
        .command("LinkWorkspace", json!({ "name": "cf e2e roundtrip" }))
        .await
        .expect("Link Some")
        .expect("Link ok");
    let remote_id = link_result
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    let server_objects = provider
        .list_objects(&remote_id, None, None, None)
        .expect("list_objects");
    let server_keys: std::collections::BTreeSet<&str> =
        server_objects.iter().map(|o| o.key.as_str()).collect();
    assert!(
        server_keys.contains("files/hello.md"),
        "cloudflare namespace should contain files/hello.md. Got: {server_keys:?}"
    );
    assert!(
        server_keys.contains("files/notes/day-1.md"),
        "cloudflare namespace should contain files/notes/day-1.md. Got: {server_keys:?}"
    );

    // Device B: empty workspace; download.
    let workspace_b = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root B\n",
        &[],
    );
    let storage_b = Arc::new(RecordingStorage::new());
    let harness_b = build_harness(&workspace_b, storage_b, provider);
    harness_b.init().await.expect("init B");

    let download_result: JsonValue = harness_b
        .command(
            "DownloadWorkspace",
            json!({
                "remote_id": remote_id,
                "workspace_root": workspace_b.to_string_lossy(),
                "link": true,
            }),
        )
        .await
        .expect("Download Some")
        .expect("Download ok");
    let files_imported = download_result
        .get("files_imported")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        files_imported >= 2,
        "device B should import at least 2 files; got: {download_result}"
    );

    let workspace_b_dir = workspace_b.parent().expect("workspace B parent");
    let hello_on_b = workspace_b_dir.join("hello.md");
    assert!(
        hello_on_b.exists(),
        "cloudflare: device B should have hello.md at {hello_on_b:?}"
    );
    let contents = std::fs::read_to_string(&hello_on_b).expect("read hello.md");
    assert!(
        contents.contains("Hello from A"),
        "cloudflare: device B's hello.md missing A's content:\n{contents}"
    );
}

async fn scenario_multi_user_isolation(
    alice_provider: Arc<dyn NamespaceProvider>,
    bob_provider: Arc<dyn NamespaceProvider>,
) {
    let workspace_alice = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - secret.md\n---\n\n# Alice's root\n",
        &[(
            "secret.md",
            "---\ntitle: Secret\npart_of: index.md\n---\n\n# top secret\n",
        )],
    );
    let storage_alice = Arc::new(RecordingStorage::new());
    let harness_alice = build_harness(&workspace_alice, storage_alice, alice_provider.clone());
    harness_alice.init().await.expect("init alice");

    let link_result = harness_alice
        .command("LinkWorkspace", json!({ "name": "alice's cf workspace" }))
        .await
        .expect("Link Some")
        .expect("Link ok");
    let alice_ns = link_result
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    // Alice sees her own.
    let alice_list = alice_provider
        .list_namespaces()
        .expect("alice list_namespaces");
    assert!(
        alice_list.iter().any(|ns| ns.id == alice_ns),
        "alice should see her own namespace"
    );

    // Bob must NOT see alice's in his listing.
    let bob_list = bob_provider.list_namespaces().expect("bob list_namespaces");
    assert!(
        !bob_list.iter().any(|ns| ns.id == alice_ns),
        "bob should NOT see alice's namespace. Got: {:?}",
        bob_list.iter().map(|n| &n.id).collect::<Vec<_>>()
    );

    // Direct list attempt by bob must fail.
    let bob_list_attempt = bob_provider.list_objects(&alice_ns, None, None, None);
    assert!(
        bob_list_attempt.is_err(),
        "bob should be denied listing alice's objects, got: {bob_list_attempt:?}"
    );

    // Direct get attempt by bob must fail.
    let bob_get_attempt = bob_provider.get_object(&alice_ns, "files/secret.md");
    assert!(
        bob_get_attempt.is_err(),
        "bob should be denied reading alice's object, got {} bytes",
        bob_get_attempt.map(|b| b.len()).unwrap_or(0)
    );
}

/// URL-encoding corpus — a subset of
/// [`diaryx_server::contract::URL_KEY_CORPUS`] filtered to values that should
/// succeed. Matches the equivalent list in `sync_e2e.rs` so both adapters are
/// exercised by the same keys.
const VALID_FUZZ_KEYS: &[&str] = &[
    "hello.md",
    "hello world.md",
    "hello%20world.md",
    "hello+world.md",
    "notes/today.md",
    "a/b/c.md",
    "emoji-🎉.md",
    "café.md",
    ".hidden",
];

async fn scenario_url_corpus_roundtrip(provider: Arc<dyn NamespaceProvider>) {
    let workspace = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root\n",
        &[],
    );
    let storage = Arc::new(RecordingStorage::new());
    let harness = build_harness(&workspace, storage, provider.clone());
    harness.init().await.expect("init");

    let create_result = harness
        .command("NsCreateNamespace", json!({ "name": "cf url fuzz" }))
        .await
        .expect("NsCreateNamespace Some")
        .expect("NsCreateNamespace ok");
    let ns_id = create_result
        .get("id")
        .and_then(|v| v.as_str())
        .expect("namespace id")
        .to_string();

    for key in VALID_FUZZ_KEYS {
        let body = format!("cf body for {key}");

        let put_result = harness
            .command(
                "NsPutObject",
                json!({
                    "namespace_id": ns_id,
                    "key": key,
                    "content_type": "text/plain",
                    "body": body.clone(),
                }),
            )
            .await
            .unwrap_or_else(|| panic!("NsPutObject Some for key {key:?}"))
            .unwrap_or_else(|e| panic!("NsPutObject failed for key {key:?}: {e}"));
        assert_eq!(
            put_result.get("ok").and_then(|v| v.as_bool()),
            Some(true),
            "put result for {key:?}: {put_result}"
        );

        let get_result = harness
            .command("NsGetObject", json!({ "namespace_id": ns_id, "key": key }))
            .await
            .unwrap_or_else(|| panic!("NsGetObject Some for key {key:?}"))
            .unwrap_or_else(|e| panic!("NsGetObject failed for key {key:?}: {e}"));

        let b64 = get_result
            .get("body_base64")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                panic!("NsGetObject missing body_base64 for key {key:?}: {get_result}")
            });
        use base64::Engine as _;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap_or_else(|e| panic!("base64 decode failed for key {key:?}: {e}"));
        assert_eq!(
            decoded,
            body.as_bytes(),
            "round-trip byte mismatch for key {key:?}: sent {body:?}, got {}",
            String::from_utf8_lossy(&decoded)
        );
    }

    let listed = provider
        .list_objects(&ns_id, None, None, None)
        .expect("list_objects");
    let listed_keys: std::collections::BTreeSet<&str> =
        listed.iter().map(|o| o.key.as_str()).collect();
    for key in VALID_FUZZ_KEYS {
        assert!(
            listed_keys.contains(*key),
            "key {key:?} should appear in cloudflare listing. Got: {listed_keys:?}"
        );
    }
}

async fn scenario_edit_propagates(provider: Arc<dyn NamespaceProvider>) {
    let workspace_a = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - hello.md\n---\n\n# Root A\n",
        &[(
            "hello.md",
            "---\ntitle: Hello\npart_of: index.md\n---\n\n# v1 from A\n",
        )],
    );
    let storage_a = Arc::new(RecordingStorage::new());
    let harness_a = build_harness(&workspace_a, storage_a, provider.clone());
    harness_a.init().await.expect("init A");
    let link_result: JsonValue = harness_a
        .command("LinkWorkspace", json!({ "name": "cf edit-prop ws" }))
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link_result
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    let workspace_b = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root B\n",
        &[],
    );
    let storage_b = Arc::new(RecordingStorage::new());
    let harness_b = build_harness(&workspace_b, storage_b, provider);
    harness_b.init().await.expect("init B");
    harness_b
        .command(
            "DownloadWorkspace",
            json!({
                "remote_id": ns_id,
                "workspace_root": workspace_b.to_string_lossy(),
                "link": true,
            }),
        )
        .await
        .expect("Download Some")
        .expect("Download ok");

    let hello_on_b = workspace_b.parent().unwrap().join("hello.md");
    let initial_b = std::fs::read_to_string(&hello_on_b).expect("read initial B");
    assert!(
        initial_b.contains("v1 from A"),
        "precondition: B should have A's v1. Got:\n{initial_b}"
    );

    let hello_on_a = workspace_a.parent().unwrap().join("hello.md");
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    std::fs::write(
        &hello_on_a,
        "---\ntitle: Hello\npart_of: index.md\n---\n\n# v2 from A, after edit\n",
    )
    .expect("write v2 to A");

    let sync_a: JsonValue = harness_a
        .command("Sync", json!({}))
        .await
        .expect("Sync A Some")
        .expect("Sync A ok");
    let pushed_a = sync_a.get("pushed").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(
        pushed_a >= 1,
        "A's Sync should push the edit. Got: {sync_a}"
    );

    let sync_b: JsonValue = harness_b
        .command("Sync", json!({}))
        .await
        .expect("Sync B Some")
        .expect("Sync B ok");
    let pulled_b = sync_b.get("pulled").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(
        pulled_b >= 1,
        "B's Sync should pull A's edit. Got: {sync_b}"
    );

    let final_b = std::fs::read_to_string(&hello_on_b).expect("read final B");
    assert!(
        final_b.contains("v2 from A, after edit"),
        "B should have A's v2 after Sync. Got:\n{final_b}"
    );
}

async fn scenario_delete_propagates(provider: Arc<dyn NamespaceProvider>) {
    let workspace_a = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - keep.md\n  - remove.md\n---\n\n# Root\n",
        &[
            (
                "keep.md",
                "---\ntitle: Keep\npart_of: index.md\n---\n\n# keep me\n",
            ),
            (
                "remove.md",
                "---\ntitle: Remove\npart_of: index.md\n---\n\n# delete me\n",
            ),
        ],
    );
    let storage_a = Arc::new(RecordingStorage::new());
    let harness_a = build_harness(&workspace_a, storage_a, provider.clone());
    harness_a.init().await.expect("init A");
    let link_result: JsonValue = harness_a
        .command("LinkWorkspace", json!({ "name": "cf delete-prop ws" }))
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link_result
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    let workspace_b = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root B\n",
        &[],
    );
    let storage_b = Arc::new(RecordingStorage::new());
    let harness_b = build_harness(&workspace_b, storage_b, provider);
    harness_b.init().await.expect("init B");
    harness_b
        .command(
            "DownloadWorkspace",
            json!({
                "remote_id": ns_id,
                "workspace_root": workspace_b.to_string_lossy(),
                "link": true,
            }),
        )
        .await
        .expect("Download Some")
        .expect("Download ok");

    let remove_on_b = workspace_b.parent().unwrap().join("remove.md");
    assert!(
        remove_on_b.exists(),
        "precondition: B should have remove.md before deletion"
    );

    let remove_on_a = workspace_a.parent().unwrap().join("remove.md");
    std::fs::remove_file(&remove_on_a).expect("delete remove.md on A");

    let sync_a: JsonValue = harness_a
        .command("Sync", json!({}))
        .await
        .expect("Sync A Some")
        .expect("Sync A ok");
    let deleted_remote = sync_a
        .get("deleted_remote")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        deleted_remote >= 1,
        "A's Sync should delete remote file (>=1). Got: {sync_a}"
    );

    let sync_b: JsonValue = harness_b
        .command("Sync", json!({}))
        .await
        .expect("Sync B Some")
        .expect("Sync B ok");
    let deleted_local = sync_b
        .get("deleted_local")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        deleted_local >= 1,
        "B's Sync should delete local file (>=1). Got: {sync_b}"
    );

    assert!(
        !remove_on_b.exists(),
        "B's remove.md should be gone after Sync at {remove_on_b:?}"
    );

    let keep_on_b = workspace_b.parent().unwrap().join("keep.md");
    assert!(keep_on_b.exists(), "B's keep.md should still exist");
}

async fn scenario_sync_noop(provider: Arc<dyn NamespaceProvider>) {
    let workspace = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - hello.md\n---\n\n# Root\n",
        &[(
            "hello.md",
            "---\ntitle: Hello\npart_of: index.md\n---\n\n# stable\n",
        )],
    );
    let storage = Arc::new(RecordingStorage::new());
    let harness = build_harness(&workspace, storage, provider);
    harness.init().await.expect("init");

    harness
        .command("LinkWorkspace", json!({ "name": "cf noop ws" }))
        .await
        .expect("Link Some")
        .expect("Link ok");

    // `LinkWorkspace` rewrites root `index.md` frontmatter with the assigned
    // `workspace_id` *after* the initial sync, so the first follow-up Sync
    // pushes exactly 1 file. Flush it here so the subsequent assertions see
    // a truly steady-state workspace. (Same caveat applies to sync_server —
    // see `sync_e2e.rs::sync_with_no_changes_is_noop`.)
    let sync_flush: JsonValue = harness
        .command("Sync", json!({}))
        .await
        .expect("Sync Some")
        .expect("Sync ok");
    let flushed = sync_flush
        .get("pushed")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        flushed <= 1,
        "post-link flush Sync should push at most 1. Got: {sync_flush}"
    );

    let sync_2: JsonValue = harness
        .command("Sync", json!({}))
        .await
        .expect("Sync Some")
        .expect("Sync ok");
    for field in ["pushed", "pulled", "deleted_remote", "deleted_local"] {
        let val = sync_2.get(field).and_then(|v| v.as_u64()).unwrap_or(999);
        assert_eq!(
            val, 0,
            "steady-state Sync should report {field}=0. Got {field}={val} in {sync_2}"
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario 7: LWW conflict resolution
// ---------------------------------------------------------------------------
//
// Two devices edit `hello.md` without syncing between them. A's edit is
// strictly later in wall-clock time, so A wins: B's Sync should pull A's
// version. Guards against the units-mismatch bug where local mtime (ms)
// was compared against server mtime (s) — the comparison was biased ~1000×
// toward the local side, making pulls impossible in LWW conflicts.
async fn scenario_lww(provider: Arc<dyn NamespaceProvider>) {
    let workspace_a = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - hello.md\n---\n\n# Root\n",
        &[(
            "hello.md",
            "---\ntitle: Hello\npart_of: index.md\n---\n\n# v1\n",
        )],
    );
    let storage_a = Arc::new(RecordingStorage::new());
    let harness_a = build_harness(&workspace_a, storage_a, provider.clone());
    harness_a.init().await.expect("init A");
    let link = harness_a
        .command("LinkWorkspace", json!({ "name": "cf lww" }))
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    let workspace_b = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root B\n",
        &[],
    );
    let storage_b = Arc::new(RecordingStorage::new());
    let harness_b = build_harness(&workspace_b, storage_b, provider);
    harness_b.init().await.expect("init B");
    harness_b
        .command(
            "DownloadWorkspace",
            json!({
                "remote_id": ns_id,
                "workspace_root": workspace_b.to_string_lossy(),
                "link": true,
            }),
        )
        .await
        .expect("Download Some")
        .expect("Download ok");

    // Flush link/download rewrites so LWW races only on hello.md.
    for harness in [&harness_a, &harness_b] {
        harness.command("Sync", json!({})).await.unwrap().unwrap();
    }

    // B writes v2 (older), then sleep 1.1s, then A writes v3 (newer).
    let hello_on_b = workspace_b.parent().unwrap().join("hello.md");
    std::fs::write(
        &hello_on_b,
        "---\ntitle: Hello\npart_of: index.md\n---\n\n# v2_from_B\n",
    )
    .expect("write v2 to B");

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let hello_on_a = workspace_a.parent().unwrap().join("hello.md");
    std::fs::write(
        &hello_on_a,
        "---\ntitle: Hello\npart_of: index.md\n---\n\n# v3_from_A\n",
    )
    .expect("write v3 to A");

    // A syncs first — server now has v3.
    let sync_a = harness_a
        .command("Sync", json!({}))
        .await
        .expect("Sync A Some")
        .expect("Sync A ok");
    let pushed_a = sync_a.get("pushed").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(pushed_a >= 1, "A's Sync should push v3. Got: {sync_a}");

    // B syncs — LWW should resolve in A's favor (pull).
    let sync_b = harness_b
        .command("Sync", json!({}))
        .await
        .expect("Sync B Some")
        .expect("Sync B ok");
    let pulled_b = sync_b.get("pulled").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(
        pulled_b >= 1,
        "B's Sync should pull A's newer v3. Got: {sync_b}"
    );

    let final_b = std::fs::read_to_string(&hello_on_b).expect("read B's hello.md");
    assert!(
        final_b.contains("v3_from_A"),
        "B should have A's newer content after LWW. Got:\n{final_b}"
    );
    assert!(
        !final_b.contains("v2_from_B"),
        "B's older content should be gone. Got:\n{final_b}"
    );
}

// ---------------------------------------------------------------------------
// Scenario 8: bidirectional edits on non-overlapping files converge
// ---------------------------------------------------------------------------

async fn scenario_bidirectional(provider: Arc<dyn NamespaceProvider>) {
    let workspace_a = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - hello.md\n  - world.md\n---\n\n# Root\n",
        &[
            (
                "hello.md",
                "---\ntitle: Hello\npart_of: index.md\n---\n\n# hello v1\n",
            ),
            (
                "world.md",
                "---\ntitle: World\npart_of: index.md\n---\n\n# world v1\n",
            ),
        ],
    );
    let storage_a = Arc::new(RecordingStorage::new());
    let harness_a = build_harness(&workspace_a, storage_a, provider.clone());
    harness_a.init().await.expect("init A");
    let link = harness_a
        .command("LinkWorkspace", json!({ "name": "cf bidir" }))
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    let workspace_b = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root B\n",
        &[],
    );
    let storage_b = Arc::new(RecordingStorage::new());
    let harness_b = build_harness(&workspace_b, storage_b, provider);
    harness_b.init().await.expect("init B");
    harness_b
        .command(
            "DownloadWorkspace",
            json!({
                "remote_id": ns_id,
                "workspace_root": workspace_b.to_string_lossy(),
                "link": true,
            }),
        )
        .await
        .expect("Download Some")
        .expect("Download ok");

    for harness in [&harness_a, &harness_b] {
        harness.command("Sync", json!({})).await.unwrap().unwrap();
    }

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    std::fs::write(
        workspace_a.parent().unwrap().join("hello.md"),
        "---\ntitle: Hello\npart_of: index.md\n---\n\n# hello v2 (from A)\n",
    )
    .expect("write A hello v2");
    std::fs::write(
        workspace_b.parent().unwrap().join("world.md"),
        "---\ntitle: World\npart_of: index.md\n---\n\n# world v2 (from B)\n",
    )
    .expect("write B world v2");

    harness_a.command("Sync", json!({})).await.unwrap().unwrap();
    harness_b.command("Sync", json!({})).await.unwrap().unwrap();
    harness_a.command("Sync", json!({})).await.unwrap().unwrap();

    let a_world = std::fs::read_to_string(workspace_a.parent().unwrap().join("world.md"))
        .expect("read A world");
    let b_hello = std::fs::read_to_string(workspace_b.parent().unwrap().join("hello.md"))
        .expect("read B hello");
    assert!(
        a_world.contains("world v2 (from B)"),
        "A should have pulled B's world edit. Got:\n{a_world}"
    );
    assert!(
        b_hello.contains("hello v2 (from A)"),
        "B should have pulled A's hello edit. Got:\n{b_hello}"
    );
}

// ---------------------------------------------------------------------------
// Scenario 9: multi-change catch-up in a single Sync
// ---------------------------------------------------------------------------

async fn scenario_catchup(provider: Arc<dyn NamespaceProvider>) {
    let workspace_a = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - a.md\n  - b.md\n---\n\n# Root\n",
        &[
            ("a.md", "---\ntitle: A\npart_of: index.md\n---\n\n# a v1\n"),
            ("b.md", "---\ntitle: B\npart_of: index.md\n---\n\n# b v1\n"),
        ],
    );
    let storage_a = Arc::new(RecordingStorage::new());
    let harness_a = build_harness(&workspace_a, storage_a, provider.clone());
    harness_a.init().await.expect("init A");
    let link = harness_a
        .command("LinkWorkspace", json!({ "name": "cf catchup" }))
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    let workspace_b = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root B\n",
        &[],
    );
    let storage_b = Arc::new(RecordingStorage::new());
    let harness_b = build_harness(&workspace_b, storage_b, provider);
    harness_b.init().await.expect("init B");
    harness_b
        .command(
            "DownloadWorkspace",
            json!({
                "remote_id": ns_id,
                "workspace_root": workspace_b.to_string_lossy(),
                "link": true,
            }),
        )
        .await
        .expect("Download Some")
        .expect("Download ok");

    for harness in [&harness_a, &harness_b] {
        harness.command("Sync", json!({})).await.unwrap().unwrap();
    }

    // A accumulates 3 changes, syncing each.
    let a_root = workspace_a.parent().unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    std::fs::write(
        a_root.join("a.md"),
        "---\ntitle: A\npart_of: index.md\n---\n\n# a v2\n",
    )
    .expect("edit a");
    harness_a.command("Sync", json!({})).await.unwrap().unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    std::fs::write(
        a_root.join("b.md"),
        "---\ntitle: B\npart_of: index.md\n---\n\n# b v2\n",
    )
    .expect("edit b");
    harness_a.command("Sync", json!({})).await.unwrap().unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    std::fs::write(
        a_root.join("c.md"),
        "---\ntitle: C\npart_of: index.md\n---\n\n# c v1 (new)\n",
    )
    .expect("add c");
    std::fs::write(
        a_root.join("index.md"),
        "---\ntitle: Root\ncontents:\n  - a.md\n  - b.md\n  - c.md\n---\n\n# Root\n",
    )
    .expect("update index");
    harness_a.command("Sync", json!({})).await.unwrap().unwrap();

    // B's single catch-up Sync should pull all 3.
    let sync_b: JsonValue = harness_b
        .command("Sync", json!({}))
        .await
        .expect("Sync B Some")
        .expect("Sync B ok");
    let pulled = sync_b.get("pulled").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(
        pulled >= 3,
        "B's catch-up Sync should pull at least 3 files. Got: {sync_b}"
    );

    let b_root = workspace_b.parent().unwrap();
    let a_contents = std::fs::read_to_string(b_root.join("a.md")).expect("read B a");
    let b_contents = std::fs::read_to_string(b_root.join("b.md")).expect("read B b");
    let c_contents = std::fs::read_to_string(b_root.join("c.md")).expect("read B c");
    assert!(a_contents.contains("a v2"), "B a.md:\n{a_contents}");
    assert!(b_contents.contains("b v2"), "B b.md:\n{b_contents}");
    assert!(c_contents.contains("c v1 (new)"), "B c.md:\n{c_contents}");
}

// ---------------------------------------------------------------------------
// Scenario 10: binary file round-trip via plugin NsPut/NsGet
// ---------------------------------------------------------------------------

async fn scenario_binary(provider: Arc<dyn NamespaceProvider>) {
    let workspace = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root\n",
        &[],
    );
    let storage = Arc::new(RecordingStorage::new());
    let harness = build_harness(&workspace, storage, provider.clone());
    harness.init().await.expect("init");

    let create = harness
        .command("NsCreateNamespace", json!({ "name": "cf binary" }))
        .await
        .expect("NsCreateNamespace Some")
        .expect("NsCreateNamespace ok");
    let ns_id = create
        .get("id")
        .and_then(|v| v.as_str())
        .expect("namespace id")
        .to_string();

    let binary_bytes: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0xFF, 0xFE, 0x80, 0x81, 0x00, 0xC0, 0xC1, 0xDE,
    ];
    assert!(
        std::str::from_utf8(&binary_bytes).is_err(),
        "test precondition: payload must not be valid UTF-8"
    );

    use base64::Engine as _;
    let body_b64 = base64::engine::general_purpose::STANDARD.encode(&binary_bytes);

    let put_result = harness
        .command(
            "NsPutObject",
            json!({
                "namespace_id": ns_id,
                "key": "attachment.bin",
                "content_type": "application/octet-stream",
                "body_base64": body_b64,
            }),
        )
        .await
        .expect("NsPutObject Some")
        .expect("NsPutObject ok");
    assert_eq!(
        put_result.get("ok").and_then(|v| v.as_bool()),
        Some(true),
        "put result: {put_result}"
    );

    let get_result = harness
        .command(
            "NsGetObject",
            json!({ "namespace_id": ns_id, "key": "attachment.bin" }),
        )
        .await
        .expect("NsGetObject Some")
        .expect("NsGetObject ok");
    let returned_b64 = get_result
        .get("body_base64")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("NsGetObject missing body_base64: {get_result}"));
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(returned_b64)
        .expect("base64 decode");
    assert_eq!(
        decoded, binary_bytes,
        "binary payload should round-trip byte-exact through cloudflare"
    );

    let raw = provider
        .get_object(&ns_id, "attachment.bin")
        .expect("provider get_object");
    assert_eq!(
        raw, binary_bytes,
        "provider-level get should return byte-exact binary"
    );
}

// ---------------------------------------------------------------------------
// Scenario 11: reconnect — manifest survives harness reconstruction
// ---------------------------------------------------------------------------

async fn scenario_reconnect(provider: Arc<dyn NamespaceProvider>) {
    let storage = Arc::new(RecordingStorage::new());
    let workspace = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - hello.md\n---\n\n# Root\n",
        &[(
            "hello.md",
            "---\ntitle: Hello\npart_of: index.md\n---\n\n# initial\n",
        )],
    );

    // Session 1: link + flush, then drop harness.
    let ns_id = {
        let harness = build_harness(&workspace, storage.clone(), provider.clone());
        harness.init().await.expect("init session 1");
        let link = harness
            .command("LinkWorkspace", json!({ "name": "cf reconnect" }))
            .await
            .expect("Link Some")
            .expect("Link ok");
        harness.command("Sync", json!({})).await.unwrap().unwrap();
        link.get("remote_id")
            .and_then(|v| v.as_str())
            .expect("remote_id")
            .to_string()
    };

    let snapshot = storage.data_snapshot();
    assert!(
        snapshot.iter().any(|(k, _)| k.ends_with("sync_manifest")),
        "session 1 should persist sync_manifest. Keys: {:?}",
        snapshot.keys().collect::<Vec<_>>()
    );

    // Session 2: rebuild with same storage, expect no-op.
    let harness_2 = build_harness(&workspace, storage, provider.clone());
    harness_2.init().await.expect("init session 2");

    let sync_2: JsonValue = harness_2
        .command("Sync", json!({}))
        .await
        .expect("Sync session 2 Some")
        .expect("Sync session 2 ok");
    for field in ["pushed", "pulled", "deleted_remote", "deleted_local"] {
        let val = sync_2.get(field).and_then(|v| v.as_u64()).unwrap_or(999);
        assert_eq!(
            val, 0,
            "reconstructed harness should be a no-op ({field}=0). Got {field}={val} in {sync_2}"
        );
    }

    // Session 2: edit a file, Sync, verify it lands on the server.
    let hello_on_disk = workspace.parent().unwrap().join("hello.md");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    std::fs::write(
        &hello_on_disk,
        "---\ntitle: Hello\npart_of: index.md\n---\n\n# edited after reconnect\n",
    )
    .expect("write edit");
    let sync_edit: JsonValue = harness_2
        .command("Sync", json!({}))
        .await
        .expect("Sync edit Some")
        .expect("Sync edit ok");
    let pushed = sync_edit
        .get("pushed")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        pushed >= 1,
        "session 2 Sync should push the edit. Got: {sync_edit}"
    );

    let remote = provider
        .get_object(&ns_id, "files/hello.md")
        .expect("provider get_object");
    let remote_text = String::from_utf8(remote).expect("utf8");
    assert!(
        remote_text.contains("edited after reconnect"),
        "server should have post-reconnect edit. Got:\n{remote_text}"
    );
}

// ---------------------------------------------------------------------------
// Scenario 12: concurrent Sync — two devices race, no corruption
// ---------------------------------------------------------------------------

async fn scenario_concurrent_sync(provider: Arc<dyn NamespaceProvider>) {
    let workspace_a = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - hello.md\n  - world.md\n---\n\n# Root\n",
        &[
            (
                "hello.md",
                "---\ntitle: Hello\npart_of: index.md\n---\n\n# hello v1\n",
            ),
            (
                "world.md",
                "---\ntitle: World\npart_of: index.md\n---\n\n# world v1\n",
            ),
        ],
    );
    let storage_a = Arc::new(RecordingStorage::new());
    let harness_a = build_harness(&workspace_a, storage_a, provider.clone());
    harness_a.init().await.expect("init A");
    let link = harness_a
        .command("LinkWorkspace", json!({ "name": "cf concurrent" }))
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    let workspace_b = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root B\n",
        &[],
    );
    let storage_b = Arc::new(RecordingStorage::new());
    let harness_b = build_harness(&workspace_b, storage_b, provider);
    harness_b.init().await.expect("init B");
    harness_b
        .command(
            "DownloadWorkspace",
            json!({
                "remote_id": ns_id,
                "workspace_root": workspace_b.to_string_lossy(),
                "link": true,
            }),
        )
        .await
        .expect("Download Some")
        .expect("Download ok");

    for harness in [&harness_a, &harness_b] {
        harness.command("Sync", json!({})).await.unwrap().unwrap();
    }

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    std::fs::write(
        workspace_a.parent().unwrap().join("hello.md"),
        "---\ntitle: Hello\npart_of: index.md\n---\n\n# hello v2 (from A)\n",
    )
    .expect("write A hello v2");
    std::fs::write(
        workspace_b.parent().unwrap().join("world.md"),
        "---\ntitle: World\npart_of: index.md\n---\n\n# world v2 (from B)\n",
    )
    .expect("write B world v2");

    // Fire both Syncs concurrently — interleaves PUT requests at the server.
    let (res_a, res_b) = tokio::join!(
        harness_a.command("Sync", json!({})),
        harness_b.command("Sync", json!({})),
    );
    let sync_a = res_a.expect("Sync A Some").expect("Sync A ok");
    let sync_b = res_b.expect("Sync B Some").expect("Sync B ok");
    let pushed_a = sync_a.get("pushed").and_then(|v| v.as_u64()).unwrap_or(0);
    let pushed_b = sync_b.get("pushed").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(
        pushed_a + pushed_b >= 2,
        "both devices should push (sum>=2). Got A={sync_a} B={sync_b}"
    );

    // Converge.
    harness_a.command("Sync", json!({})).await.unwrap().unwrap();
    harness_b.command("Sync", json!({})).await.unwrap().unwrap();
    harness_a.command("Sync", json!({})).await.unwrap().unwrap();

    let a_world = std::fs::read_to_string(workspace_a.parent().unwrap().join("world.md"))
        .expect("read A world");
    let b_hello = std::fs::read_to_string(workspace_b.parent().unwrap().join("hello.md"))
        .expect("read B hello");
    assert!(
        a_world.contains("world v2 (from B)"),
        "A should have B's world edit after concurrent sync + converge. Got:\n{a_world}"
    );
    assert!(
        b_hello.contains("hello v2 (from A)"),
        "B should have A's hello edit after concurrent sync + converge. Got:\n{b_hello}"
    );
}

// ---------------------------------------------------------------------------
// Scenario 13: large binary file (512 KiB through R2)
// ---------------------------------------------------------------------------

async fn scenario_large_binary(provider: Arc<dyn NamespaceProvider>) {
    let workspace = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root\n",
        &[],
    );
    let storage = Arc::new(RecordingStorage::new());
    let harness = build_harness(&workspace, storage, provider.clone());
    harness.init().await.expect("init");

    let create = harness
        .command("NsCreateNamespace", json!({ "name": "cf large" }))
        .await
        .expect("NsCreateNamespace Some")
        .expect("NsCreateNamespace ok");
    let ns_id = create
        .get("id")
        .and_then(|v| v.as_str())
        .expect("namespace id")
        .to_string();

    const SIZE: usize = 512 * 1024;
    let large_bytes: Vec<u8> = (0..SIZE)
        .map(|i| i.wrapping_mul(31).wrapping_add(7) as u8)
        .collect();
    assert_eq!(large_bytes.len(), SIZE);

    use base64::Engine as _;
    let body_b64 = base64::engine::general_purpose::STANDARD.encode(&large_bytes);

    let put_result = harness
        .command(
            "NsPutObject",
            json!({
                "namespace_id": ns_id,
                "key": "big.bin",
                "content_type": "application/octet-stream",
                "body_base64": body_b64,
            }),
        )
        .await
        .expect("NsPutObject Some")
        .expect("NsPutObject ok");
    assert_eq!(
        put_result.get("ok").and_then(|v| v.as_bool()),
        Some(true),
        "put result: {put_result}"
    );

    let get_result = harness
        .command(
            "NsGetObject",
            json!({ "namespace_id": ns_id, "key": "big.bin" }),
        )
        .await
        .expect("NsGetObject Some")
        .expect("NsGetObject ok");
    let returned_b64 = get_result
        .get("body_base64")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("NsGetObject missing body_base64: {get_result}"));
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(returned_b64)
        .expect("base64 decode");
    assert_eq!(decoded.len(), SIZE, "decoded length mismatch");
    assert_eq!(
        decoded, large_bytes,
        "512 KiB payload through R2 should round-trip byte-exact"
    );

    let raw = provider
        .get_object(&ns_id, "big.bin")
        .expect("provider get_object");
    assert_eq!(raw.len(), SIZE);
    assert_eq!(raw, large_bytes);

    let listed = provider
        .list_objects(&ns_id, None, None, None)
        .expect("list_objects");
    let entry = listed
        .iter()
        .find(|o| o.key == "big.bin")
        .unwrap_or_else(|| panic!("big.bin missing: {listed:?}"));
    let reported = entry.size_bytes.unwrap_or(0);
    assert_eq!(
        reported as usize, SIZE,
        "cloudflare list_objects size_bytes should match. Got {reported}"
    );
}
