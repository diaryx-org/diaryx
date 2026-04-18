//! End-to-end sync plugin tests that drive two plugin WASM instances against
//! a real `diaryx_sync_server` over HTTP.
//!
//! The mock-based tests in `integration.rs` share an `Arc<MockNamespaceProvider>`
//! across two harnesses to simulate the server — fast and deterministic, but
//! blind to HTTP-transport, URL-encoding, SQLite persistence, and auth-flow
//! bugs. This file complements them by using:
//!
//! - A real `diaryx_sync_server::testing::TestServer` bound to `127.0.0.1:0`
//!   with `:memory:` SQLite + in-memory blob store.
//! - The dev-mode magic-link flow to obtain a real session token.
//! - Two `diaryx_extism::HttpNamespaceProvider` instances configured with
//!   that token, backing the `NamespaceProvider` trait for both harnesses.
//!
//! Scenario: device A links a workspace (creates a namespace, pushes two
//! files); device B downloads from the same namespace and verifies content
//! matches byte-for-byte. Asserts:
//!
//! - Link returns the expected namespace ID.
//! - Server-side object listing contains both files A pushed.
//! - B's disk has A's content (byte-exact), and the plugin manifest reflects
//!   the linked state.
//! - Content-hash dedup: identical bytes on both devices should share the
//!   same SHA-256 surface in `SyncManifest` clean entries.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use diaryx_extism::testing::{PluginTestHarness, PluginTestHarnessBuilder, RecordingStorage};
use diaryx_extism::{HttpNamespaceProvider, NamespaceProvider};
use diaryx_sync_extism::sync_manifest::SyncManifest;
use diaryx_sync_server::testing::TestServer;
use serde_json::{Value as JsonValue, json};

/// Absolute path to the built sync-plugin WASM (see [integration.rs] for
/// the rationale — relative paths silently skip because cargo's CWD is the
/// package dir, not the workspace root).
const WASM_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../target/wasm32-unknown-unknown/release/diaryx_sync_extism.wasm"
);

/// Early-return if the WASM file hasn't been built. Matches the convention
/// in `integration.rs` so the test set is skippable in the same way.
macro_rules! require_wasm {
    () => {
        if !std::path::Path::new(WASM_PATH).exists() {
            eprintln!(
                "Skipping: WASM not built. Run: cargo build -p diaryx_sync_extism \
                 --target wasm32-unknown-unknown --release"
            );
            return;
        }
    };
}

// ---------------------------------------------------------------------------
// Workspace helpers (reused from `integration.rs` patterns)
// ---------------------------------------------------------------------------

fn unique_temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "diaryx-sync-e2e-{label}-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("temp dir should be created");
    path
}

fn write_workspace_file(root_dir: &std::path::Path, relative_path: &str, contents: &str) {
    let path = root_dir.join(relative_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("workspace parent directories should exist");
    }
    std::fs::write(path, contents).expect("workspace file should be written");
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

/// Read the persisted `SyncManifest` from a device's `RecordingStorage`.
/// Returns `None` when the plugin hasn't written one yet.
fn read_manifest(storage: &RecordingStorage) -> Option<SyncManifest> {
    storage
        .data_snapshot()
        .into_iter()
        .find(|(key, _)| key.ends_with("sync_manifest"))
        .and_then(|(_, bytes)| serde_json::from_slice::<SyncManifest>(&bytes).ok())
}

fn manifest_clean_hashes(manifest: &SyncManifest) -> std::collections::BTreeMap<String, String> {
    manifest
        .files
        .iter()
        .map(|(key, entry)| (key.clone(), entry.content_hash.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// Scenario
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_devices_sync_via_real_http_server() {
    require_wasm!();

    // ---- 1. Spin up a real sync_server and sign in once. ----------------
    let server = TestServer::start().await;
    let token = server.sign_in_dev("e2e@example.com").await;

    // Shared provider. Both "devices" are the same authenticated user here —
    // the scenario is cross-device, not cross-user. Sync plugin doesn't see
    // the token; it only sees the NamespaceProvider interface.
    //
    // `HttpNamespaceProvider` expects the API prefix baked into the base URL
    // (it concatenates `/namespaces/...` directly), so use `api_base_url()`.
    let provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(token),
    ));

    // ---- 2. Device A: workspace with two notes; link + push. ------------
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
    let harness_a = build_harness(&workspace_a, storage_a.clone(), provider.clone());
    harness_a.init().await.expect("init A");

    let link_result: JsonValue = harness_a
        .command("LinkWorkspace", json!({ "name": "e2e workspace" }))
        .await
        .expect("LinkWorkspace should return Some")
        .expect("LinkWorkspace should succeed");

    let remote_id = link_result
        .get("remote_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("LinkWorkspace missing remote_id: {link_result}"))
        .to_string();

    assert_eq!(
        link_result.get("created_remote").and_then(|v| v.as_bool()),
        Some(true),
        "expected LinkWorkspace to have created a new remote. Got: {link_result}"
    );

    // ---- 3. Server-side assertions: A's objects landed in the namespace. -
    let server_objects = provider
        .list_objects(&remote_id, None, None, None)
        .expect("server list_objects should succeed");
    let server_keys: std::collections::BTreeSet<&str> =
        server_objects.iter().map(|o| o.key.as_str()).collect();
    assert!(
        server_keys.contains("files/hello.md"),
        "server namespace should contain files/hello.md; got: {server_keys:?}"
    );
    assert!(
        server_keys.contains("files/notes/day-1.md"),
        "server namespace should contain files/notes/day-1.md; got: {server_keys:?}"
    );

    // Manifest on A should show both files as clean, with non-empty hashes.
    let manifest_a = read_manifest(&storage_a).expect("manifest A should be persisted");
    let hashes_a = manifest_clean_hashes(&manifest_a);
    assert!(
        hashes_a.contains_key("files/hello.md") && !hashes_a["files/hello.md"].is_empty(),
        "manifest A should have a content hash for hello.md; got: {hashes_a:?}"
    );
    assert!(
        hashes_a.contains_key("files/notes/day-1.md")
            && !hashes_a["files/notes/day-1.md"].is_empty()
    );

    // ---- 4. Device B: empty workspace; download from the shared namespace.
    let workspace_b = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root B\n",
        &[],
    );
    let storage_b = Arc::new(RecordingStorage::new());
    let harness_b = build_harness(&workspace_b, storage_b.clone(), provider.clone());
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
        .expect("DownloadWorkspace should return Some")
        .expect("DownloadWorkspace should succeed");

    let files_imported = download_result
        .get("files_imported")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        files_imported >= 2,
        "device B should import at least 2 files (index.md + hello.md + nested); got: {download_result}"
    );

    // ---- 5. Disk assertions on B. -----------------------------------------
    let workspace_b_dir = workspace_b
        .parent()
        .expect("workspace B should have a parent dir");

    let hello_on_b = workspace_b_dir.join("hello.md");
    assert!(
        hello_on_b.exists(),
        "device B should have hello.md at {hello_on_b:?}"
    );
    let hello_contents = std::fs::read_to_string(&hello_on_b).expect("read hello.md");
    assert!(
        hello_contents.contains("Hello from A"),
        "device B's hello.md should match A:\n{hello_contents}"
    );

    let nested_on_b = workspace_b_dir.join("notes").join("day-1.md");
    assert!(
        nested_on_b.exists(),
        "device B should have the nested file at {nested_on_b:?}"
    );

    // ---- 6. Manifest parity: identical bytes → identical hashes. ----------
    let manifest_b = read_manifest(&storage_b).expect("manifest B should be persisted");
    let hashes_b = manifest_clean_hashes(&manifest_b);

    for key in ["files/hello.md", "files/notes/day-1.md"] {
        let ha = hashes_a
            .get(key)
            .unwrap_or_else(|| panic!("missing {key} hash on A: {hashes_a:?}"));
        let hb = hashes_b
            .get(key)
            .unwrap_or_else(|| panic!("missing {key} hash on B: {hashes_b:?}"));
        assert_eq!(
            ha, hb,
            "content hashes for {key} should match across devices. A: {ha} B: {hb}"
        );
    }

    // ---- 7. Namespace should be visible to the authenticated user. -------
    let my_namespaces = provider
        .list_namespaces()
        .expect("list_namespaces should succeed");
    assert!(
        my_namespaces.iter().any(|ns| ns.id == remote_id),
        "user's namespace listing should include the linked namespace. Got: {:?}",
        my_namespaces.iter().map(|n| &n.id).collect::<Vec<_>>()
    );

    // Explicit drop so the server shuts down before the test exits cleanly.
    drop(server);
}

// ---------------------------------------------------------------------------
// Multi-user isolation
// ---------------------------------------------------------------------------

/// Alice and Bob both sign in as distinct users. Alice creates a namespace
/// and pushes a file. Bob's `HttpNamespaceProvider` (with Bob's token)
/// must not be able to read from Alice's namespace, and must not see
/// Alice's namespace in his own listing.
///
/// This tests the server's authorization boundary — the sync plugin itself
/// can't enforce this; the server must. Both sync_server and cloudflare
/// must agree (contract suite candidate).
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

    // ---- Alice: create namespace + push one file via LinkWorkspace ------
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
        .command("LinkWorkspace", json!({ "name": "alice's workspace" }))
        .await
        .expect("LinkWorkspace Some")
        .expect("LinkWorkspace ok");
    let alice_ns = link_result
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("alice remote_id")
        .to_string();

    // Sanity: alice sees her own namespace & objects.
    let alice_list = alice_provider
        .list_namespaces()
        .expect("alice list_namespaces");
    assert!(
        alice_list.iter().any(|ns| ns.id == alice_ns),
        "alice should see her own namespace"
    );
    let alice_objs = alice_provider
        .list_objects(&alice_ns, None, None, None)
        .expect("alice list_objects");
    assert!(
        alice_objs.iter().any(|o| o.key == "files/secret.md"),
        "alice should see her own object"
    );

    // ---- Bob: try to see/read alice's namespace — should be denied. -----
    let bob_list = bob_provider
        .list_namespaces()
        .expect("bob list_namespaces should succeed (empty)");
    assert!(
        !bob_list.iter().any(|ns| ns.id == alice_ns),
        "bob should NOT see alice's namespace in his listing. Got: {:?}",
        bob_list.iter().map(|n| &n.id).collect::<Vec<_>>()
    );

    // Bob tries to list Alice's namespace objects directly.
    let bob_list_attempt = bob_provider.list_objects(&alice_ns, None, None, None);
    assert!(
        bob_list_attempt.is_err(),
        "bob should be denied listing objects in alice's namespace, got: {bob_list_attempt:?}"
    );

    // Bob tries to read Alice's secret object directly.
    let bob_get_attempt = bob_provider.get_object(&alice_ns, "files/secret.md");
    assert!(
        bob_get_attempt.is_err(),
        "bob should be denied reading alice's object, got {} bytes",
        bob_get_attempt.map(|b| b.len()).unwrap_or(0)
    );

    drop(server);
}

// ---------------------------------------------------------------------------
// URL-encoding fuzz through the plugin's namespace API
// ---------------------------------------------------------------------------

/// Keys that should round-trip byte-exact through the full stack:
/// plugin → `host_namespace` → [`HttpNamespaceProvider`] (URL-encoding) →
/// Axum path extractor → SQL → back.
///
/// Drawn from [`diaryx_server::contract::URL_KEY_CORPUS`], filtered to the
/// cases that should *succeed* — path-traversal / empty / dot-only entries
/// are handled separately (expected to be rejected or sanitized; see
/// the server-side `URL_KEY_CORPUS` comments).
///
/// This is the class of bug that commits `a03a0732` (CF path decoding) and
/// `044a78c9` (publish paths with spaces) fixed. A silent regression here
/// would surface as a test failure against sync_server *and* (once the
/// cloudflare variant lands) against `wrangler dev`.
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn url_corpus_keys_roundtrip_via_plugin_ns_api() {
    require_wasm!();

    let server = TestServer::start().await;
    let token = server.sign_in_dev("fuzz@example.com").await;
    let provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(token),
    ));

    // Workspace is irrelevant for this test — we drive the plugin's raw
    // namespace API commands directly, not the sync/link commands.
    let workspace = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root\n",
        &[],
    );
    let storage = Arc::new(RecordingStorage::new());
    let harness = build_harness(&workspace, storage, provider.clone());
    harness.init().await.expect("init");

    // Create a namespace through the plugin (exercises that path too).
    let create_result = harness
        .command("NsCreateNamespace", json!({ "name": "url fuzz" }))
        .await
        .expect("NsCreateNamespace Some")
        .expect("NsCreateNamespace ok");
    let ns_id = create_result
        .get("id")
        .and_then(|v| v.as_str())
        .expect("namespace id")
        .to_string();

    for key in VALID_FUZZ_KEYS {
        let body = format!("body for {key}");

        // PUT via plugin's NsPutObject (plugin accepts `body` as raw text).
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
            .expect(&format!("NsPutObject Some for key {key:?}"))
            .unwrap_or_else(|e| panic!("NsPutObject failed for key {key:?}: {e}"));
        assert_eq!(
            put_result.get("ok").and_then(|v| v.as_bool()),
            Some(true),
            "put result for {key:?}: {put_result}"
        );

        // GET via plugin's NsGetObject; body comes back as base64.
        let get_result = harness
            .command("NsGetObject", json!({ "namespace_id": ns_id, "key": key }))
            .await
            .expect(&format!("NsGetObject Some for key {key:?}"))
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

    // The server's listing should show every key we pushed.
    let listed = provider
        .list_objects(&ns_id, None, None, None)
        .expect("list_objects");
    let listed_keys: std::collections::BTreeSet<&str> =
        listed.iter().map(|o| o.key.as_str()).collect();
    for key in VALID_FUZZ_KEYS {
        assert!(
            listed_keys.contains(*key),
            "key {key:?} should appear in server listing. Got: {listed_keys:?}"
        );
    }

    drop(server);
}

// ---------------------------------------------------------------------------
// Bidirectional delta: edits on A propagate to B via Sync
// ---------------------------------------------------------------------------

/// After the initial A→B download, A edits `hello.md` locally and Syncs; B
/// then Syncs and should pull A's edit. Previous scenarios only covered the
/// first-sync path (link / download); this guards the incremental delta code
/// path that's hit on every subsequent Sync and is by far the most common
/// operation in day-to-day use.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edit_on_a_propagates_to_b_via_sync() {
    require_wasm!();

    let server = TestServer::start().await;
    let token = server.sign_in_dev("edit-prop@example.com").await;
    let provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(token),
    ));

    // ---- A: link + push initial v1 --------------------------------------
    let workspace_a = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - hello.md\n---\n\n# Root A\n",
        &[(
            "hello.md",
            "---\ntitle: Hello\npart_of: index.md\n---\n\n# v1 from A\n",
        )],
    );
    let storage_a = Arc::new(RecordingStorage::new());
    let harness_a = build_harness(&workspace_a, storage_a.clone(), provider.clone());
    harness_a.init().await.expect("init A");

    let link_result: JsonValue = harness_a
        .command("LinkWorkspace", json!({ "name": "edit-prop ws" }))
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link_result
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    // ---- B: download v1 --------------------------------------------------
    let workspace_b = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root B\n",
        &[],
    );
    let storage_b = Arc::new(RecordingStorage::new());
    let harness_b = build_harness(&workspace_b, storage_b.clone(), provider.clone());
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
        "precondition: B should have A's v1, got:\n{initial_b}"
    );

    // ---- A: edit hello.md locally, then Sync to push v2 ------------------
    let hello_on_a = workspace_a.parent().unwrap().join("hello.md");
    // Sleep >1s so the on-disk mtime is strictly newer than any recorded
    // manifest mtime (protects against coarse-grained FS clocks).
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
        "A's Sync should push the edit (pushed>=1). Got: {sync_a}"
    );

    // ---- B: Sync — should pull A's v2 ------------------------------------
    let sync_b: JsonValue = harness_b
        .command("Sync", json!({}))
        .await
        .expect("Sync B Some")
        .expect("Sync B ok");
    let pulled_b = sync_b.get("pulled").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(
        pulled_b >= 1,
        "B's Sync should pull at least 1 file (A's edit). Got: {sync_b}"
    );

    // ---- Disk assertion: B has v2 ----------------------------------------
    let final_b = std::fs::read_to_string(&hello_on_b).expect("read final B");
    assert!(
        final_b.contains("v2 from A, after edit"),
        "B should have A's v2 after Sync. Got:\n{final_b}"
    );
    assert!(
        !final_b.contains("v1 from A"),
        "B's v1 should be overwritten. Got:\n{final_b}"
    );

    drop(server);
}

// ---------------------------------------------------------------------------
// Delete propagation across devices via Sync
// ---------------------------------------------------------------------------

/// A deletes `hello.md` locally, Syncs, then B Syncs. B should observe the
/// deletion. This is a critical user-facing property — until this test,
/// our E2E coverage only exercised the add/update paths.
///
/// Note: the sync plugin detects local deletes by comparing the manifest
/// against on-disk state. `Sync` scans the filesystem, which is why we can
/// just `fs::remove_file` and let the next Sync pick it up.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delete_on_a_propagates_to_b_via_sync() {
    require_wasm!();

    let server = TestServer::start().await;
    let token = server.sign_in_dev("delete-prop@example.com").await;
    let provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(token),
    ));

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
        .command("LinkWorkspace", json!({ "name": "delete-prop ws" }))
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link_result
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    // B: download both files
    let workspace_b = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root B\n",
        &[],
    );
    let storage_b = Arc::new(RecordingStorage::new());
    let harness_b = build_harness(&workspace_b, storage_b, provider.clone());
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

    // A deletes remove.md from disk, then Syncs (should push the deletion).
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
        "A's Sync should delete remote file (deleted_remote>=1). Got: {sync_a}"
    );

    // B Syncs — should delete remove.md locally.
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
        "B's Sync should delete local file (deleted_local>=1). Got: {sync_b}"
    );

    assert!(
        !remove_on_b.exists(),
        "B's remove.md should be gone after Sync at {remove_on_b:?}"
    );

    // Sanity: the kept file is still there on B.
    let keep_on_b = workspace_b.parent().unwrap().join("keep.md");
    assert!(
        keep_on_b.exists(),
        "B's keep.md should still exist at {keep_on_b:?}"
    );

    drop(server);
}

// ---------------------------------------------------------------------------
// Idempotent Sync: calling Sync with no changes is a no-op
// ---------------------------------------------------------------------------

/// Regression guard: once a workspace is linked and pushed, a follow-up Sync
/// with no on-disk changes and no remote changes must report `pushed=0`,
/// `pulled=0`, `deleted_remote=0`, `deleted_local=0`.
///
/// Past regressions in this crate have seen Sync re-upload everything every
/// time due to manifest/hash mismatches — silently correct, but a bandwidth
/// and rate-limit hazard in production.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sync_with_no_changes_is_noop() {
    require_wasm!();

    let server = TestServer::start().await;
    let token = server.sign_in_dev("noop@example.com").await;
    let provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(token),
    ));

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
        .command("LinkWorkspace", json!({ "name": "noop ws" }))
        .await
        .expect("Link Some")
        .expect("Link ok");

    // `LinkWorkspace` rewrites root `index.md` frontmatter with the assigned
    // `workspace_id` AFTER the initial sync completes, so the next Sync will
    // push exactly 1 file (the mutated index.md). That's an expected artifact
    // of the link flow — not a violation of idempotence. Flush it here so
    // the subsequent assertions see a truly steady-state workspace.
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
        "post-link flush Sync should push at most 1 (the rewritten index.md). Got: {sync_flush}"
    );

    // Now the real assertion: another Sync with no changes is a full no-op.
    let sync_2: JsonValue = harness
        .command("Sync", json!({}))
        .await
        .expect("Sync Some")
        .expect("Sync ok");

    for field in ["pushed", "pulled", "deleted_remote", "deleted_local"] {
        let val = sync_2.get(field).and_then(|v| v.as_u64()).unwrap_or(999);
        assert_eq!(
            val, 0,
            "steady-state Sync should report {field}=0 (idempotent). Got {field}={val} in {sync_2}"
        );
    }

    // And a third time, for good measure.
    let sync_3: JsonValue = harness
        .command("Sync", json!({}))
        .await
        .expect("Sync Some")
        .expect("Sync ok");
    for field in ["pushed", "pulled", "deleted_remote", "deleted_local"] {
        let val = sync_3.get(field).and_then(|v| v.as_u64()).unwrap_or(999);
        assert_eq!(
            val, 0,
            "third Sync should also be a no-op. Got {field}={val} in {sync_3}"
        );
    }

    drop(server);
}

// ---------------------------------------------------------------------------
// Last-writer-wins (LWW) conflict resolution
// ---------------------------------------------------------------------------

/// Two devices both modify `hello.md` without syncing between them, then A
/// (with the strictly-later on-disk mtime) wins and B's v2 is overwritten by
/// A's v3 during B's next Sync.
///
/// **Previously `#[ignore]` because of an `index.md` entanglement.**
/// `LinkWorkspace` / `DownloadWorkspace` rewrite root `index.md` frontmatter
/// *after* their initial network sync, leaving each device's `index.md`
/// locally dirty. Before the follow-up `content_hash` fix in
/// `diaryx_cloudflare::handlers::list_objects` (which caused B to treat every
/// remote entry as unchanged), B would therefore see two dirty files against
/// an "unchanged" server and push both — pulling 0.
///
/// With the fix in place and a **flush Sync on both devices after the link
/// dance**, the link-path rewrite is settled before the conflict race, so
/// LWW for `hello.md` is evaluated cleanly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lww_resolves_conflict_in_favor_of_later_mtime() {
    require_wasm!();

    let server = TestServer::start().await;
    let token = server.sign_in_dev("lww@example.com").await;
    let provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(token),
    ));

    // ---- Device A: push v1 via Link ------------------------------------
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
        .command("LinkWorkspace", json!({ "name": "lww" }))
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    // ---- Device B: download v1 -----------------------------------------
    let workspace_b = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root B\n",
        &[],
    );
    let storage_b = Arc::new(RecordingStorage::new());
    let harness_b = build_harness(&workspace_b, storage_b, provider.clone());
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

    // ---- Flush the link/download frontmatter rewrites on both devices --
    // `LinkWorkspace` / `DownloadWorkspace` rewrite root `index.md` AFTER
    // their initial sync (adding `workspace_id` / link metadata). If we
    // didn't flush, A's next Sync would push `hello.md` + `index.md` and
    // B's sync would then be racing on *two* files instead of the one the
    // LWW test actually intends to exercise. One Sync per device settles
    // the rewrites so the subsequent race is purely about `hello.md`.
    for (label, harness) in [("A", &harness_a), ("B", &harness_b)] {
        let flush = harness
            .command("Sync", json!({}))
            .await
            .unwrap_or_else(|| panic!("flush Sync ({label}) Some"))
            .unwrap_or_else(|e| panic!("flush Sync ({label}) ok: {e}"));
        let pushed = flush.get("pushed").and_then(|v| v.as_u64()).unwrap_or(0);
        assert!(
            pushed <= 1,
            "flush Sync on {label} should push at most 1 (post-link index.md rewrite). Got: {flush}"
        );
    }

    // ---- B writes v2 locally (will be the LOSER of the LWW race) -------
    let hello_on_b = workspace_b
        .parent()
        .expect("workspace B parent")
        .join("hello.md");
    std::fs::write(
        &hello_on_b,
        "---\ntitle: Hello\npart_of: index.md\n---\n\n# v2_from_B\n",
    )
    .expect("write v2 to B");

    // Sleep > 1s so A's next write gets a strictly later filesystem mtime
    // (APFS/ext4 handle sub-second mtimes but some CI filesystems round to
    // seconds; 1.1s is the defensive floor).
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    // ---- A writes v3 locally (LATER mtime — should win LWW) ------------
    let hello_on_a = workspace_a
        .parent()
        .expect("workspace A parent")
        .join("hello.md");
    std::fs::write(
        &hello_on_a,
        "---\ntitle: Hello\npart_of: index.md\n---\n\n# v3_from_A\n",
    )
    .expect("write v3 to A");

    // ---- A syncs first — server now has v3 -----------------------------
    let sync_a = harness_a
        .command("Sync", json!({}))
        .await
        .expect("Sync A Some")
        .expect("Sync A ok");
    let pushed_a = sync_a.get("pushed").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(pushed_a >= 1, "A's Sync should push v3. Got: {sync_a}");

    // ---- B syncs — server's v3 (newer) should win and overwrite B -----
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

    // ---- Verify disk state: B ends up with v3_from_A -------------------
    let final_b = std::fs::read_to_string(&hello_on_b).expect("read B's hello.md");
    assert!(
        final_b.contains("v3_from_A"),
        "B should have A's newer content after LWW. Got:\n{final_b}"
    );
    assert!(
        !final_b.contains("v2_from_B"),
        "B's older content should be gone. Got:\n{final_b}"
    );

    drop(server);
}

// ---------------------------------------------------------------------------
// Bidirectional edits on non-overlapping files — both devices converge
// ---------------------------------------------------------------------------

/// A edits `hello.md` locally; B edits `world.md` locally (no overlap). After
/// each Syncs twice (A→server, B→server→A), both workspaces should end up
/// with *both* edits.
///
/// This exercises the merge path where each device has *independent* local
/// changes the other hasn't seen yet — a different codepath from the
/// single-writer `edit_on_a_propagates_to_b_via_sync` case, and a realistic
/// model of two people collaborating in the same workspace.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bidirectional_edits_converge() {
    require_wasm!();

    let server = TestServer::start().await;
    let token = server.sign_in_dev("bidir@example.com").await;
    let provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(token),
    ));

    // Both devices start with hello.md + world.md.
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
    let link_result: JsonValue = harness_a
        .command("LinkWorkspace", json!({ "name": "bidir ws" }))
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
    let harness_b = build_harness(&workspace_b, storage_b, provider.clone());
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

    // Flush the link-path rewrites on both devices before the non-overlapping
    // edits; see the LWW test for the rationale.
    for harness in [&harness_a, &harness_b] {
        harness.command("Sync", json!({})).await.unwrap().unwrap();
    }

    // ---- A edits hello.md; B edits world.md (disjoint keys) --------------
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let hello_on_a = workspace_a.parent().unwrap().join("hello.md");
    std::fs::write(
        &hello_on_a,
        "---\ntitle: Hello\npart_of: index.md\n---\n\n# hello v2 (from A)\n",
    )
    .expect("write hello v2 on A");

    let world_on_b = workspace_b.parent().unwrap().join("world.md");
    std::fs::write(
        &world_on_b,
        "---\ntitle: World\npart_of: index.md\n---\n\n# world v2 (from B)\n",
    )
    .expect("write world v2 on B");

    // A syncs first (pushes hello v2). Then B syncs (should push world v2
    // AND pull hello v2). Then A syncs again (should pull world v2).
    harness_a
        .command("Sync", json!({}))
        .await
        .expect("Sync A Some")
        .expect("Sync A ok");
    harness_b
        .command("Sync", json!({}))
        .await
        .expect("Sync B Some")
        .expect("Sync B ok");
    harness_a
        .command("Sync", json!({}))
        .await
        .expect("Sync A2 Some")
        .expect("Sync A2 ok");

    // ---- Both devices should now have both edits -------------------------
    let hello_on_b = workspace_b.parent().unwrap().join("hello.md");
    let world_on_a = workspace_a.parent().unwrap().join("world.md");

    let a_hello = std::fs::read_to_string(&hello_on_a).expect("read A hello");
    let a_world = std::fs::read_to_string(&world_on_a).expect("read A world");
    let b_hello = std::fs::read_to_string(&hello_on_b).expect("read B hello");
    let b_world = std::fs::read_to_string(&world_on_b).expect("read B world");

    assert!(a_hello.contains("hello v2 (from A)"), "A hello:\n{a_hello}");
    assert!(
        a_world.contains("world v2 (from B)"),
        "A should have pulled B's world edit. Got:\n{a_world}"
    );
    assert!(
        b_hello.contains("hello v2 (from A)"),
        "B should have pulled A's hello edit. Got:\n{b_hello}"
    );
    assert!(b_world.contains("world v2 (from B)"), "B world:\n{b_world}");

    drop(server);
}

// ---------------------------------------------------------------------------
// Multi-change catch-up — B goes "offline" while A makes N changes
// ---------------------------------------------------------------------------

/// Exercises the common "I haven't opened this device in a week" path:
/// A makes **several** independent changes between B's sync checkpoints, then
/// B Syncs once and must catch up on all of them in a single pass. Verifies
/// that `compute_diff` handles accumulated remote state correctly (not just
/// one-change-at-a-time).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_change_catchup_in_single_sync() {
    require_wasm!();

    let server = TestServer::start().await;
    let token = server.sign_in_dev("catchup@example.com").await;
    let provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(token),
    ));

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
        .command("LinkWorkspace", json!({ "name": "catchup" }))
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
    let harness_b = build_harness(&workspace_b, storage_b, provider.clone());
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

    // Flush link/download rewrites on both.
    for harness in [&harness_a, &harness_b] {
        harness.command("Sync", json!({})).await.unwrap().unwrap();
    }

    // ---- A makes 3 changes: edit a.md, edit b.md, add new c.md. Each
    // Syncs so they all land on the server before B's catch-up sync. -------
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
    // Also extend index.md so the tree-walk picks up c.md; otherwise the
    // plugin's scan may skip it.
    std::fs::write(
        a_root.join("index.md"),
        "---\ntitle: Root\ncontents:\n  - a.md\n  - b.md\n  - c.md\n---\n\n# Root\n",
    )
    .expect("update index");
    harness_a.command("Sync", json!({})).await.unwrap().unwrap();

    // ---- B's single catch-up Sync should pull ALL 3 (a, b, c) + index ---
    let sync_b: JsonValue = harness_b
        .command("Sync", json!({}))
        .await
        .expect("Sync B Some")
        .expect("Sync B ok");
    let pulled = sync_b.get("pulled").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(
        pulled >= 3,
        "B's catch-up Sync should pull at least 3 changed files (a, b, c). Got: {sync_b}"
    );

    // Disk assertions: B should see all 3 new contents.
    let b_root = workspace_b.parent().unwrap();
    let a_contents = std::fs::read_to_string(b_root.join("a.md")).expect("read B a");
    let b_contents = std::fs::read_to_string(b_root.join("b.md")).expect("read B b");
    let c_contents = std::fs::read_to_string(b_root.join("c.md")).expect("read B c");
    assert!(a_contents.contains("a v2"), "B a.md:\n{a_contents}");
    assert!(b_contents.contains("b v2"), "B b.md:\n{b_contents}");
    assert!(
        c_contents.contains("c v1 (new)"),
        "B c.md should be newly created by catch-up Sync:\n{c_contents}"
    );

    drop(server);
}

// ---------------------------------------------------------------------------
// Binary-file round-trip via plugin NsPutObject / NsGetObject
// ---------------------------------------------------------------------------

/// Push a non-UTF8 binary payload through the plugin's namespace API and
/// assert byte-exact return. The existing URL-corpus test only exercises
/// text bodies; this guards against accidental UTF-8-only paths in the
/// encode/decode chain (e.g. storing attachments).
///
/// Uses the standard PNG signature + some arbitrary bytes including 0x00
/// and 0xFF to catch any naive `String::from_utf8` assumption.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn binary_file_roundtrip_via_plugin() {
    require_wasm!();

    let server = TestServer::start().await;
    let token = server.sign_in_dev("binary@example.com").await;
    let provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(token),
    ));

    let workspace = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root\n",
        &[],
    );
    let storage = Arc::new(RecordingStorage::new());
    let harness = build_harness(&workspace, storage, provider.clone());
    harness.init().await.expect("init");

    let create = harness
        .command("NsCreateNamespace", json!({ "name": "binary fuzz" }))
        .await
        .expect("NsCreateNamespace Some")
        .expect("NsCreateNamespace ok");
    let ns_id = create
        .get("id")
        .and_then(|v| v.as_str())
        .expect("namespace id")
        .to_string();

    // PNG signature + IHDR-ish bytes. Includes 0x00 and 0xFF and high
    // bytes above 0x7F. Must NOT be valid UTF-8.
    let binary_bytes: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG magic
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR start
        0xFF, 0xFE, 0x80, 0x81, 0x00, 0xC0, 0xC1, 0xDE, // arbitrary high bytes
    ];
    assert!(
        std::str::from_utf8(&binary_bytes).is_err(),
        "test precondition: payload must not be valid UTF-8"
    );

    // NsPutObject accepts `body` as a UTF-8 string OR `body_base64` for raw
    // bytes (see the plugin's command router). Use base64 for the binary
    // case; that's also how real attachment uploads work.
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

    // Round-trip via NsGetObject — body returned as base64.
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
        "binary payload should round-trip byte-exact"
    );

    // Double-check via the provider's raw get_object (skips the plugin's
    // base64 layer entirely — directly hits HTTP).
    let raw = provider
        .get_object(&ns_id, "attachment.bin")
        .expect("provider get_object");
    assert_eq!(
        raw, binary_bytes,
        "provider-level get should also return byte-exact binary"
    );

    drop(server);
}

// ---------------------------------------------------------------------------
// Reconnect / harness-reconstruction — manifest survives across sessions
// ---------------------------------------------------------------------------

/// Simulates closing and reopening the app: one session links the workspace
/// and pushes everything; the harness is dropped; a second session is built
/// with the *same* `RecordingStorage` and verifies Sync sees a clean
/// workspace (no spurious re-push, no spurious re-pull).
///
/// This is the test that would fail if the plugin accidentally regressed its
/// manifest persistence — e.g. forgetting to write the manifest, using a
/// different storage key on reload, or rebuilding from scratch on init.
/// Catch-all for "do we actually remember what we synced?"
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sync_state_survives_harness_reconstruction() {
    require_wasm!();

    let server = TestServer::start().await;
    let token = server.sign_in_dev("reconnect@example.com").await;
    let provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(token),
    ));

    // Shared storage — first and second harness both read/write here.
    let storage = Arc::new(RecordingStorage::new());
    let workspace = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - hello.md\n---\n\n# Root\n",
        &[(
            "hello.md",
            "---\ntitle: Hello\npart_of: index.md\n---\n\n# initial\n",
        )],
    );

    // ---- Session 1: link + flush, then drop the harness ----------------
    let ns_id = {
        let harness = build_harness(&workspace, storage.clone(), provider.clone());
        harness.init().await.expect("init session 1");
        let link = harness
            .command("LinkWorkspace", json!({ "name": "reconnect" }))
            .await
            .expect("Link Some")
            .expect("Link ok");
        // Flush the post-link index.md rewrite so the manifest persisted to
        // `storage` reflects a fully clean state.
        harness.command("Sync", json!({})).await.unwrap().unwrap();
        link.get("remote_id")
            .and_then(|v| v.as_str())
            .expect("remote_id")
            .to_string()
        // `harness` drops here (end of block).
    };

    // Sanity: storage now holds a `sync_manifest` entry. Without it the
    // second session would rescan from scratch and push everything as "new".
    let snapshot = storage.data_snapshot();
    assert!(
        snapshot.iter().any(|(k, _)| k.ends_with("sync_manifest")),
        "session 1 should persist a sync_manifest to storage. Got keys: {:?}",
        snapshot.keys().collect::<Vec<_>>()
    );

    // ---- Session 2: rebuild harness with same storage, expect no-op ----
    let harness_2 = build_harness(&workspace, storage.clone(), provider.clone());
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
            "reconstructed harness should be a no-op ({field}=0); \
             a non-zero value means the manifest wasn't restored from storage. \
             Got {field}={val} in {sync_2}"
        );
    }

    // ---- Session 2: edit a file, Sync, verify it reaches the server ----
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
        "session 2's Sync should push the post-reconnect edit. Got: {sync_edit}"
    );

    // Confirm via provider (bypasses plugin state) that the edit landed.
    let remote = provider
        .get_object(&ns_id, "files/hello.md")
        .expect("provider get_object");
    let remote_text = String::from_utf8(remote).expect("remote hello is utf8");
    assert!(
        remote_text.contains("edited after reconnect"),
        "server should have the post-reconnect edit. Got:\n{remote_text}"
    );

    drop(server);
}

// ---------------------------------------------------------------------------
// Concurrent Sync — two devices fire Sync simultaneously, no corruption
// ---------------------------------------------------------------------------

/// A and B each make a local edit on disjoint files, then invoke `Sync`
/// *concurrently* via `tokio::join!`. Both must succeed, the server must not
/// drop either write, and after one more Sync each they must converge.
///
/// The target here is the server-side write path: while sync_server's
/// in-process `Arc<Mutex<Connection>>` serialises everything internally,
/// cloudflare's D1 can genuinely interleave requests. A bug like "read meta,
/// decide to overwrite, write — without transaction" would surface only
/// when two clients race.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_syncs_from_two_devices_converge() {
    require_wasm!();

    let server = TestServer::start().await;
    let token = server.sign_in_dev("concurrent@example.com").await;
    let provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(token),
    ));

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
        .command("LinkWorkspace", json!({ "name": "concurrent" }))
        .await
        .expect("Link Some")
        .expect("Link ok");
    let _ns_id = link
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
    let harness_b = build_harness(&workspace_b, storage_b, provider.clone());
    harness_b.init().await.expect("init B");
    harness_b
        .command(
            "DownloadWorkspace",
            json!({
                "remote_id": _ns_id,
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

    // Each device edits a distinct file, then Syncs concurrently.
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

    // `tokio::join!` drives both futures on the same task cooperatively.
    // Each harness's `Sync` issues its HTTP calls independently, so the
    // server sees interleaved PUT requests — which is the bit we care about.
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
        "both devices should have pushed their edits (sum >= 2). Got: A={sync_a} B={sync_b}"
    );

    // Converge: each device syncs once more to pick up the other's change.
    harness_a.command("Sync", json!({})).await.unwrap().unwrap();
    harness_b.command("Sync", json!({})).await.unwrap().unwrap();
    harness_a.command("Sync", json!({})).await.unwrap().unwrap();

    let a_world = std::fs::read_to_string(workspace_a.parent().unwrap().join("world.md"))
        .expect("read A world");
    let b_hello = std::fs::read_to_string(workspace_b.parent().unwrap().join("hello.md"))
        .expect("read B hello");
    assert!(
        a_world.contains("world v2 (from B)"),
        "A should have B's world edit after convergence. Got:\n{a_world}"
    );
    assert!(
        b_hello.contains("hello v2 (from A)"),
        "B should have A's hello edit after convergence. Got:\n{b_hello}"
    );

    drop(server);
}

// ---------------------------------------------------------------------------
// Large binary round-trip — a non-trivial payload through the full stack
// ---------------------------------------------------------------------------

/// 512 KiB of pseudorandom binary bytes through `NsPutObject` /
/// `NsGetObject`. The previous `binary_file_roundtrip_via_plugin` used ~24
/// bytes; this guards against anything that might only bite for real
/// attachment-sized payloads — inline-vs-blob transition thresholds, base64
/// buffer sizing, R2 streaming quirks on the cloudflare side.
///
/// 512 KiB balances "big enough to stress the path" against "small enough
/// to run in CI in under a second" — larger sizes can hit Extism plugin
/// memory caps in debug builds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn large_binary_file_roundtrips_via_plugin() {
    require_wasm!();

    let server = TestServer::start().await;
    let token = server.sign_in_dev("large@example.com").await;
    let provider: Arc<dyn NamespaceProvider> = Arc::new(HttpNamespaceProvider::new(
        server.api_base_url(),
        Some(token),
    ));

    let workspace = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root\n",
        &[],
    );
    let storage = Arc::new(RecordingStorage::new());
    let harness = build_harness(&workspace, storage, provider.clone());
    harness.init().await.expect("init");

    let create = harness
        .command("NsCreateNamespace", json!({ "name": "large fuzz" }))
        .await
        .expect("NsCreateNamespace Some")
        .expect("NsCreateNamespace ok");
    let ns_id = create
        .get("id")
        .and_then(|v| v.as_str())
        .expect("namespace id")
        .to_string();

    // 512 KiB deterministic binary payload — arithmetic over u8 ensures
    // the full 0x00–0xFF range appears, including bytes that would break
    // any latent `String::from_utf8_lossy` / UTF-8-only path.
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
        .unwrap_or_else(|| {
            panic!(
                "NsGetObject missing body_base64 (key len={}): {get_result}",
                large_bytes.len()
            )
        });
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(returned_b64)
        .expect("base64 decode");
    assert_eq!(decoded.len(), SIZE, "decoded length mismatch");
    assert_eq!(
        decoded, large_bytes,
        "512 KiB payload should round-trip byte-exact"
    );

    // Also verify at the provider level (no plugin base64 layer).
    let raw = provider
        .get_object(&ns_id, "big.bin")
        .expect("provider get_object");
    assert_eq!(raw.len(), SIZE, "provider raw length mismatch");
    assert_eq!(raw, large_bytes, "provider raw get should be byte-exact");

    // Listing should report the full size_bytes (exercises the size header
    // propagation path end-to-end).
    let listed = provider
        .list_objects(&ns_id, None, None, None)
        .expect("list_objects");
    let entry = listed
        .iter()
        .find(|o| o.key == "big.bin")
        .unwrap_or_else(|| panic!("big.bin missing from listing: {listed:?}"));
    let reported_size = entry.size_bytes.unwrap_or(0);
    assert_eq!(
        reported_size as usize, SIZE,
        "list_objects should report size_bytes={SIZE}, got {reported_size}"
    );

    drop(server);
}
