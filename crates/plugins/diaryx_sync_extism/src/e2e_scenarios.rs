//! Backend-agnostic end-to-end scenario bodies for the sync plugin.
//!
//! Each scenario takes an already-authenticated `Arc<dyn NamespaceProvider>`
//! (so the caller decides whether the backend is `diaryx_sync_server`
//! in-process or `wrangler dev` over HTTP) and runs the same plugin WASM
//! through a real workspace lifecycle. Scenarios `panic!` on failure so they
//! work with both `#[tokio::test]` and the boot-amortized `catch_unwind`
//! runner used by the Cloudflare suite.
//!
//! See `crates/plugins/diaryx_sync_extism/tests/sync_e2e.rs` and
//! `crates/diaryx_cloudflare_e2e/tests/sync_plugin_e2e.rs` for the two
//! consumer entry points.
//!
//! ## Naming
//!
//! Each scenario takes a `label: &str` that gets folded into temp-dir paths
//! and workspace names. Pass something unique per call when running against
//! persistent state (Cloudflare's local D1) so scenarios don't collide;
//! against an in-process server the label can be the scenario name itself.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use diaryx_extism::NamespaceProvider;
use diaryx_extism::testing::{PluginTestHarness, PluginTestHarnessBuilder, RecordingStorage};
use serde_json::{Value as JsonValue, json};

use crate::sync_manifest::SyncManifest;

// ---------------------------------------------------------------------------
// Plugin WASM path + skip helper
// ---------------------------------------------------------------------------

/// Absolute path to the built sync-plugin WASM. Computed via
/// `CARGO_MANIFEST_DIR` so it resolves to the same workspace `target/`
/// regardless of which test crate calls into this module — both
/// `diaryx_sync_extism/tests/` and `diaryx_cloudflare_e2e/tests/` reach the
/// same artifact, since this constant is baked in when *this* crate is
/// compiled.
pub const WASM_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../target/wasm32-unknown-unknown/release/diaryx_sync_extism.wasm"
);

/// True when the plugin WASM has been built and the scenarios can run.
/// Wrappers should call this and skip (return / log) when false; building
/// the WASM is a separate prerequisite from running the tests.
pub fn wasm_available() -> bool {
    std::path::Path::new(WASM_PATH).exists()
}

// ---------------------------------------------------------------------------
// Workspace + harness helpers
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

fn create_workspace(
    label: &str,
    root_filename: &str,
    root_contents: &str,
    files: &[(&str, &str)],
) -> PathBuf {
    let root_dir = unique_temp_dir(label);
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
// Scenario 1 — two-device link + download (byte-parity round-trip)
// ---------------------------------------------------------------------------

/// Device A links a workspace (creates a namespace, pushes two files);
/// device B downloads from the same namespace and verifies content matches
/// byte-for-byte.
pub async fn two_devices_round_trip(provider: Arc<dyn NamespaceProvider>, label: &str) {
    let workspace_a = create_workspace(
        &format!("{label}-a"),
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
        .command(
            "LinkWorkspace",
            json!({ "name": format!("{label} workspace") }),
        )
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

    let workspace_b = create_workspace(
        &format!("{label}-b"),
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
        "device B should import at least 2 files; got: {download_result}"
    );

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

    let my_namespaces = provider
        .list_namespaces()
        .expect("list_namespaces should succeed");
    assert!(
        my_namespaces.iter().any(|ns| ns.id == remote_id),
        "user's namespace listing should include the linked namespace. Got: {:?}",
        my_namespaces.iter().map(|n| &n.id).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Scenario 2 — multi-user isolation (alice / bob)
// ---------------------------------------------------------------------------

/// Bob's provider must not be able to read from Alice's namespace, and must
/// not see Alice's namespace in his own listing. Tests the server's
/// authorization boundary; the sync plugin itself can't enforce this.
pub async fn multi_user_isolation(
    alice_provider: Arc<dyn NamespaceProvider>,
    bob_provider: Arc<dyn NamespaceProvider>,
    label: &str,
) {
    let workspace_alice = create_workspace(
        &format!("{label}-alice"),
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
        .command(
            "LinkWorkspace",
            json!({ "name": format!("{label} alice's workspace") }),
        )
        .await
        .expect("LinkWorkspace Some")
        .expect("LinkWorkspace ok");
    let alice_ns = link_result
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("alice remote_id")
        .to_string();

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

    let bob_list = bob_provider
        .list_namespaces()
        .expect("bob list_namespaces should succeed (empty)");
    assert!(
        !bob_list.iter().any(|ns| ns.id == alice_ns),
        "bob should NOT see alice's namespace in his listing. Got: {:?}",
        bob_list.iter().map(|n| &n.id).collect::<Vec<_>>()
    );

    let bob_list_attempt = bob_provider.list_objects(&alice_ns, None, None, None);
    assert!(
        bob_list_attempt.is_err(),
        "bob should be denied listing objects in alice's namespace, got: {bob_list_attempt:?}"
    );

    let bob_get_attempt = bob_provider.get_object(&alice_ns, "files/secret.md");
    assert!(
        bob_get_attempt.is_err(),
        "bob should be denied reading alice's object, got {} bytes",
        bob_get_attempt.map(|b| b.len()).unwrap_or(0)
    );
}

// ---------------------------------------------------------------------------
// Scenario 3 — URL key corpus round-trip via the plugin's NS commands
// ---------------------------------------------------------------------------

/// Keys that should round-trip byte-exact through plugin → host_namespace →
/// HttpNamespaceProvider URL-encoding → server path-decoder → SQL → back.
/// Same corpus for both backends so any drift in URL handling surfaces.
pub const VALID_FUZZ_KEYS: &[&str] = &[
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

pub async fn url_corpus_roundtrip(provider: Arc<dyn NamespaceProvider>, label: &str) {
    let workspace = create_workspace(
        &format!("{label}-url"),
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root\n",
        &[],
    );
    let storage = Arc::new(RecordingStorage::new());
    let harness = build_harness(&workspace, storage, provider.clone());
    harness.init().await.expect("init");

    let create_result = harness
        .command(
            "NsCreateNamespace",
            json!({ "name": format!("{label} url fuzz") }),
        )
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
            "key {key:?} should appear in server listing. Got: {listed_keys:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario 4 — edit on A propagates to B via Sync
// ---------------------------------------------------------------------------

pub async fn edit_propagates(provider: Arc<dyn NamespaceProvider>, label: &str) {
    let workspace_a = create_workspace(
        &format!("{label}-edit-a"),
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
        .command(
            "LinkWorkspace",
            json!({ "name": format!("{label} edit-prop") }),
        )
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link_result
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    let workspace_b = create_workspace(
        &format!("{label}-edit-b"),
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
        "precondition: B should have A's v1, got:\n{initial_b}"
    );

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

    let final_b = std::fs::read_to_string(&hello_on_b).expect("read final B");
    assert!(
        final_b.contains("v2 from A, after edit"),
        "B should have A's v2 after Sync. Got:\n{final_b}"
    );
    assert!(
        !final_b.contains("v1 from A"),
        "B's v1 should be overwritten. Got:\n{final_b}"
    );
}

// ---------------------------------------------------------------------------
// Scenario 5 — delete propagation
// ---------------------------------------------------------------------------

pub async fn delete_propagates(provider: Arc<dyn NamespaceProvider>, label: &str) {
    let workspace_a = create_workspace(
        &format!("{label}-del-a"),
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
        .command(
            "LinkWorkspace",
            json!({ "name": format!("{label} delete-prop") }),
        )
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link_result
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    let workspace_b = create_workspace(
        &format!("{label}-del-b"),
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
        "A's Sync should delete remote file (deleted_remote>=1). Got: {sync_a}"
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
        "B's Sync should delete local file (deleted_local>=1). Got: {sync_b}"
    );

    assert!(
        !remove_on_b.exists(),
        "B's remove.md should be gone after Sync at {remove_on_b:?}"
    );

    let keep_on_b = workspace_b.parent().unwrap().join("keep.md");
    assert!(
        keep_on_b.exists(),
        "B's keep.md should still exist at {keep_on_b:?}"
    );
}

// ---------------------------------------------------------------------------
// Scenario 6 — idempotent Sync (no-op when nothing changed)
// ---------------------------------------------------------------------------

pub async fn sync_noop(provider: Arc<dyn NamespaceProvider>, label: &str) {
    let workspace = create_workspace(
        &format!("{label}-noop"),
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
        .command("LinkWorkspace", json!({ "name": format!("{label} noop") }))
        .await
        .expect("Link Some")
        .expect("Link ok");

    // `LinkWorkspace` rewrites root `index.md` frontmatter with the assigned
    // `workspace_id` AFTER the initial sync completes, so the next Sync will
    // push exactly 1 file (the mutated index.md). That's an expected artifact
    // of the link flow — flush it before asserting steady-state idempotence.
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
}

// ---------------------------------------------------------------------------
// Scenario 7 — last-writer-wins (LWW) conflict resolution
// ---------------------------------------------------------------------------

pub async fn lww_resolves_conflict(provider: Arc<dyn NamespaceProvider>, label: &str) {
    let workspace_a = create_workspace(
        &format!("{label}-lww-a"),
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
        .command("LinkWorkspace", json!({ "name": format!("{label} lww") }))
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    let workspace_b = create_workspace(
        &format!("{label}-lww-b"),
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

    // Flush link/download index.md rewrites so LWW races only on hello.md.
    for (lbl, harness) in [("A", &harness_a), ("B", &harness_b)] {
        let flush = harness
            .command("Sync", json!({}))
            .await
            .unwrap_or_else(|| panic!("flush Sync ({lbl}) Some"))
            .unwrap_or_else(|e| panic!("flush Sync ({lbl}) ok: {e}"));
        let pushed = flush.get("pushed").and_then(|v| v.as_u64()).unwrap_or(0);
        assert!(
            pushed <= 1,
            "flush Sync on {lbl} should push at most 1 (post-link index.md rewrite). Got: {flush}"
        );
    }

    let hello_on_b = workspace_b
        .parent()
        .expect("workspace B parent")
        .join("hello.md");
    std::fs::write(
        &hello_on_b,
        "---\ntitle: Hello\npart_of: index.md\n---\n\n# v2_from_B\n",
    )
    .expect("write v2 to B");

    // Sleep > 1s so A's next write gets a strictly later filesystem mtime.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let hello_on_a = workspace_a
        .parent()
        .expect("workspace A parent")
        .join("hello.md");
    std::fs::write(
        &hello_on_a,
        "---\ntitle: Hello\npart_of: index.md\n---\n\n# v3_from_A\n",
    )
    .expect("write v3 to A");

    let sync_a = harness_a
        .command("Sync", json!({}))
        .await
        .expect("Sync A Some")
        .expect("Sync A ok");
    let pushed_a = sync_a.get("pushed").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(pushed_a >= 1, "A's Sync should push v3. Got: {sync_a}");

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
// Scenario 8 — bidirectional non-overlapping edits converge
// ---------------------------------------------------------------------------

pub async fn bidirectional_edits_converge(provider: Arc<dyn NamespaceProvider>, label: &str) {
    let workspace_a = create_workspace(
        &format!("{label}-bidir-a"),
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
        .command("LinkWorkspace", json!({ "name": format!("{label} bidir") }))
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link_result
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    let workspace_b = create_workspace(
        &format!("{label}-bidir-b"),
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
}

// ---------------------------------------------------------------------------
// Scenario 9 — multi-change catch-up in a single Sync
// ---------------------------------------------------------------------------

pub async fn multi_change_catchup(provider: Arc<dyn NamespaceProvider>, label: &str) {
    let workspace_a = create_workspace(
        &format!("{label}-catch-a"),
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
        .command(
            "LinkWorkspace",
            json!({ "name": format!("{label} catchup") }),
        )
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    let workspace_b = create_workspace(
        &format!("{label}-catch-b"),
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
}

// ---------------------------------------------------------------------------
// Scenario 10 — small binary file round-trip (24 bytes)
// ---------------------------------------------------------------------------

pub async fn binary_file_roundtrip(provider: Arc<dyn NamespaceProvider>, label: &str) {
    let workspace = create_workspace(
        &format!("{label}-bin"),
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root\n",
        &[],
    );
    let storage = Arc::new(RecordingStorage::new());
    let harness = build_harness(&workspace, storage, provider.clone());
    harness.init().await.expect("init");

    let create = harness
        .command(
            "NsCreateNamespace",
            json!({ "name": format!("{label} binary") }),
        )
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
        "binary payload should round-trip byte-exact"
    );

    let raw = provider
        .get_object(&ns_id, "attachment.bin")
        .expect("provider get_object");
    assert_eq!(
        raw, binary_bytes,
        "provider-level get should also return byte-exact binary"
    );
}

// ---------------------------------------------------------------------------
// Scenario 11 — sync state survives harness reconstruction
// ---------------------------------------------------------------------------

pub async fn state_survives_reconstruction(provider: Arc<dyn NamespaceProvider>, label: &str) {
    let storage = Arc::new(RecordingStorage::new());
    let workspace = create_workspace(
        &format!("{label}-recon"),
        "index.md",
        "---\ntitle: Root\ncontents:\n  - hello.md\n---\n\n# Root\n",
        &[(
            "hello.md",
            "---\ntitle: Hello\npart_of: index.md\n---\n\n# initial\n",
        )],
    );

    let ns_id = {
        let harness = build_harness(&workspace, storage.clone(), provider.clone());
        harness.init().await.expect("init session 1");
        let link = harness
            .command(
                "LinkWorkspace",
                json!({ "name": format!("{label} reconnect") }),
            )
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
        "session 1 should persist a sync_manifest to storage. Got keys: {:?}",
        snapshot.keys().collect::<Vec<_>>()
    );

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

    let remote = provider
        .get_object(&ns_id, "files/hello.md")
        .expect("provider get_object");
    let remote_text = String::from_utf8(remote).expect("remote hello is utf8");
    assert!(
        remote_text.contains("edited after reconnect"),
        "server should have the post-reconnect edit. Got:\n{remote_text}"
    );
}

// ---------------------------------------------------------------------------
// Scenario 12 — concurrent Sync from two devices
// ---------------------------------------------------------------------------

pub async fn concurrent_syncs_converge(provider: Arc<dyn NamespaceProvider>, label: &str) {
    let workspace_a = create_workspace(
        &format!("{label}-conc-a"),
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
        .command(
            "LinkWorkspace",
            json!({ "name": format!("{label} concurrent") }),
        )
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    let workspace_b = create_workspace(
        &format!("{label}-conc-b"),
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
}

// ---------------------------------------------------------------------------
// Scenario 13 — large binary file (512 KiB)
// ---------------------------------------------------------------------------

pub async fn large_binary_roundtrip(provider: Arc<dyn NamespaceProvider>, label: &str) {
    let workspace = create_workspace(
        &format!("{label}-large"),
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root\n",
        &[],
    );
    let storage = Arc::new(RecordingStorage::new());
    let harness = build_harness(&workspace, storage, provider.clone());
    harness.init().await.expect("init");

    let create = harness
        .command(
            "NsCreateNamespace",
            json!({ "name": format!("{label} large") }),
        )
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

    let raw = provider
        .get_object(&ns_id, "big.bin")
        .expect("provider get_object");
    assert_eq!(raw.len(), SIZE, "provider raw length mismatch");
    assert_eq!(raw, large_bytes, "provider raw get should be byte-exact");

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
}

// ---------------------------------------------------------------------------
// Scenario 14 — rename round-trip
// ---------------------------------------------------------------------------

/// `on_event("file_renamed")` (lib.rs) records `delete(old) + dirty(new)`
/// in the manifest — rename is *not* a first-class identity operation, it's
/// a delete-then-add. This guards: server holds only the new key after A
/// renames + Syncs; B's local `hello.md` is removed and `greeting.md`
/// exists with A's content; an edit to the renamed file on A propagates to
/// B at the new path.
///
/// Note: a manifest-dirty entry alone doesn't push — Sync's tree-walk only
/// discovers files reachable through `index.md`'s `contents` list, so a
/// realistic rename also requires the host to update `index.md`. This
/// scenario does that.
pub async fn rename_round_trip(provider: Arc<dyn NamespaceProvider>, label: &str) {
    let workspace_a = create_workspace(
        &format!("{label}-ren-a"),
        "index.md",
        "---\ntitle: Root\ncontents:\n  - hello.md\n---\n\n# Root\n",
        &[(
            "hello.md",
            "---\ntitle: Hello\npart_of: index.md\n---\n\n# v1 from A\n",
        )],
    );
    let storage_a = Arc::new(RecordingStorage::new());
    let harness_a = build_harness(&workspace_a, storage_a, provider.clone());
    harness_a.init().await.expect("init A");
    let link = harness_a
        .command(
            "LinkWorkspace",
            json!({ "name": format!("{label} rename") }),
        )
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    let workspace_b = create_workspace(
        &format!("{label}-ren-b"),
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

    for harness in [&harness_a, &harness_b] {
        harness.command("Sync", json!({})).await.unwrap().unwrap();
    }

    let a_root = workspace_a.parent().expect("A root");
    let hello_on_a = a_root.join("hello.md");
    let greeting_on_a = a_root.join("greeting.md");
    std::fs::rename(&hello_on_a, &greeting_on_a).expect("rename on A");

    harness_a
        .send_file_moved(
            hello_on_a.to_string_lossy().as_ref(),
            greeting_on_a.to_string_lossy().as_ref(),
        )
        .await;

    let index_on_a = a_root.join("index.md");
    std::fs::write(
        &index_on_a,
        "---\ntitle: Root\ncontents:\n  - greeting.md\n---\n\n# Root\n",
    )
    .expect("update index on A");
    harness_a
        .send_file_saved(index_on_a.to_string_lossy().as_ref())
        .await;

    let sync_a: JsonValue = harness_a
        .command("Sync", json!({}))
        .await
        .expect("Sync A Some")
        .expect("Sync A ok");
    let pushed_a = sync_a.get("pushed").and_then(|v| v.as_u64()).unwrap_or(0);
    let deleted_remote = sync_a
        .get("deleted_remote")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        pushed_a >= 1,
        "A's Sync after rename should push greeting.md. Got: {sync_a}"
    );
    assert!(
        deleted_remote >= 1,
        "A's Sync after rename should delete hello.md from server. Got: {sync_a}"
    );

    let server_objs = provider
        .list_objects(&ns_id, None, None, None)
        .expect("list_objects after rename");
    let server_keys: std::collections::BTreeSet<&str> =
        server_objs.iter().map(|o| o.key.as_str()).collect();
    assert!(
        server_keys.contains("files/greeting.md"),
        "server should have files/greeting.md after rename. Got: {server_keys:?}"
    );
    assert!(
        !server_keys.contains("files/hello.md"),
        "server should NOT have files/hello.md after rename. Got: {server_keys:?}"
    );

    let sync_b: JsonValue = harness_b
        .command("Sync", json!({}))
        .await
        .expect("Sync B Some")
        .expect("Sync B ok");
    let pulled_b = sync_b.get("pulled").and_then(|v| v.as_u64()).unwrap_or(0);
    let deleted_local = sync_b
        .get("deleted_local")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        pulled_b >= 1,
        "B's Sync should pull greeting.md. Got: {sync_b}"
    );
    assert!(
        deleted_local >= 1,
        "B's Sync should delete hello.md locally. Got: {sync_b}"
    );

    let b_root = workspace_b.parent().expect("B root");
    assert!(
        !b_root.join("hello.md").exists(),
        "B's hello.md should be removed after rename Sync"
    );
    let greeting_on_b = b_root.join("greeting.md");
    assert!(
        greeting_on_b.exists(),
        "B's greeting.md should exist after rename Sync"
    );
    let greeting_contents = std::fs::read_to_string(&greeting_on_b).expect("read greeting");
    assert!(
        greeting_contents.contains("v1 from A"),
        "B's greeting.md should contain A's content. Got:\n{greeting_contents}"
    );

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    std::fs::write(
        &greeting_on_a,
        "---\ntitle: Hello\npart_of: index.md\n---\n\n# v2 after rename\n",
    )
    .expect("write v2 to greeting on A");
    harness_a
        .send_file_saved(greeting_on_a.to_string_lossy().as_ref())
        .await;

    harness_a
        .command("Sync", json!({}))
        .await
        .expect("Sync A2 Some")
        .expect("Sync A2 ok");
    harness_b
        .command("Sync", json!({}))
        .await
        .expect("Sync B2 Some")
        .expect("Sync B2 ok");

    let final_b = std::fs::read_to_string(&greeting_on_b).expect("read final B greeting");
    assert!(
        final_b.contains("v2 after rename"),
        "B's renamed file should reflect the post-rename edit. Got:\n{final_b}"
    );
}

// ---------------------------------------------------------------------------
// Scenario 15 — unlink + relink to the same namespace is idempotent
// ---------------------------------------------------------------------------

pub async fn unlink_then_relink_idempotent(provider: Arc<dyn NamespaceProvider>, label: &str) {
    let storage = Arc::new(RecordingStorage::new());
    let workspace = create_workspace(
        &format!("{label}-relink"),
        "index.md",
        "---\ntitle: Root\ncontents:\n  - hello.md\n  - notes/day-1.md\n---\n\n# Root\n",
        &[
            (
                "hello.md",
                "---\ntitle: Hello\npart_of: index.md\n---\n\n# stable\n",
            ),
            (
                "notes/day-1.md",
                "---\ntitle: Day 1\npart_of: index.md\n---\n\n# also stable\n",
            ),
        ],
    );

    let harness = build_harness(&workspace, storage.clone(), provider.clone());
    harness.init().await.expect("init");

    let link = harness
        .command(
            "LinkWorkspace",
            json!({ "name": format!("{label} relink") }),
        )
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    harness.command("Sync", json!({})).await.unwrap().unwrap();

    let unlink = harness
        .command("UnlinkWorkspace", json!({}))
        .await
        .expect("Unlink Some")
        .expect("Unlink ok");
    assert_eq!(
        unlink.get("ok").and_then(|v| v.as_bool()),
        Some(true),
        "Unlink should report ok=true. Got: {unlink}"
    );

    let snapshot_after_unlink = storage.data_snapshot();
    assert!(
        snapshot_after_unlink
            .iter()
            .any(|(k, _)| k.ends_with("sync_manifest")),
        "post-unlink storage should still contain the sync_manifest \
         (relink relies on it). Got keys: {:?}",
        snapshot_after_unlink.keys().collect::<Vec<_>>()
    );

    let server_objs = provider
        .list_objects(&ns_id, None, None, None)
        .expect("list_objects post-unlink");
    let server_keys: std::collections::BTreeSet<&str> =
        server_objs.iter().map(|o| o.key.as_str()).collect();
    assert!(
        server_keys.contains("files/hello.md") && server_keys.contains("files/notes/day-1.md"),
        "post-unlink server should still hold A's files. Got: {server_keys:?}"
    );

    let relink = harness
        .command(
            "LinkWorkspace",
            json!({ "remote_id": ns_id, "name": format!("{label} relink") }),
        )
        .await
        .expect("Relink Some")
        .expect("Relink ok");
    assert_eq!(
        relink.get("remote_id").and_then(|v| v.as_str()),
        Some(ns_id.as_str()),
        "Relink should reattach to the same namespace ID. Got: {relink}"
    );
    assert_eq!(
        relink.get("created_remote").and_then(|v| v.as_bool()),
        Some(false),
        "Relink should NOT create a new remote namespace. Got: {relink}"
    );

    // Like LinkWorkspace, relink rewrites `workspace_id` into index.md
    // frontmatter AFTER its initial sync, so the immediately-following Sync
    // may push exactly 1 file (the rewritten index.md). Mirrors the flush
    // pattern in `sync_noop`.
    let flush: JsonValue = harness
        .command("Sync", json!({}))
        .await
        .expect("flush Sync Some")
        .expect("flush Sync ok");
    let flushed = flush.get("pushed").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(
        flushed <= 1,
        "post-relink flush Sync should push at most 1 (rewritten index.md). \
         Got: {flush}"
    );

    let sync_after_flush: JsonValue = harness
        .command("Sync", json!({}))
        .await
        .expect("Sync Some")
        .expect("Sync ok");
    for field in ["pushed", "pulled", "deleted_remote", "deleted_local"] {
        let val = sync_after_flush
            .get(field)
            .and_then(|v| v.as_u64())
            .unwrap_or(999);
        assert_eq!(
            val, 0,
            "post-relink steady-state Sync should be a no-op ({field}=0). \
             Got {field}={val} in {sync_after_flush}"
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario 16 — large workspace pagination
// ---------------------------------------------------------------------------

pub async fn large_workspace_paginates(provider: Arc<dyn NamespaceProvider>, label: &str) {
    const FILE_COUNT: usize = 600;
    let mut contents_yaml = String::new();
    for i in 0..FILE_COUNT {
        contents_yaml.push_str(&format!("  - notes/n-{i:04}.md\n"));
    }
    let root_md = format!("---\ntitle: Root\ncontents:\n{contents_yaml}---\n\n# Root\n");

    let mut child_files: Vec<(String, String)> = Vec::with_capacity(FILE_COUNT);
    for i in 0..FILE_COUNT {
        child_files.push((
            format!("notes/n-{i:04}.md"),
            format!("---\ntitle: N{i}\npart_of: index.md\n---\n\n# note {i}\n"),
        ));
    }
    let child_refs: Vec<(&str, &str)> = child_files
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let workspace_a = create_workspace(
        &format!("{label}-page-a"),
        "index.md",
        &root_md,
        &child_refs,
    );
    let storage_a = Arc::new(RecordingStorage::new());
    let harness_a = build_harness(&workspace_a, storage_a, provider.clone());
    harness_a.init().await.expect("init A");

    let link = harness_a
        .command(
            "LinkWorkspace",
            json!({ "name": format!("{label} pagination") }),
        )
        .await
        .expect("Link Some")
        .expect("Link ok");
    let ns_id = link
        .get("remote_id")
        .and_then(|v| v.as_str())
        .expect("remote_id")
        .to_string();

    // Sanity: server actually received >500 keys. The server's default
    // list_objects page is 100 and applies `prefix` *after* pagination
    // (objects.rs filter runs on the post-`limit` slice), so we paginate
    // on the unfiltered listing and apply the prefix client-side.
    let mut server_note_count = 0usize;
    let mut offset: u32 = 0;
    let page: u32 = 500;
    loop {
        let chunk = provider
            .list_objects(&ns_id, None, Some(page), Some(offset))
            .expect("list_objects post-link");
        let returned = chunk.len();
        server_note_count += chunk
            .iter()
            .filter(|o| o.key.starts_with("files/notes/"))
            .count();
        if (returned as u32) < page {
            break;
        }
        offset += page;
    }
    assert_eq!(
        server_note_count, FILE_COUNT,
        "server should hold all {FILE_COUNT} note files after Link. \
         Got: {server_note_count}"
    );

    let workspace_b = create_workspace(
        &format!("{label}-page-b"),
        "index.md",
        "---\ntitle: Root\ncontents: []\n---\n\n# Root B\n",
        &[],
    );
    let storage_b = Arc::new(RecordingStorage::new());
    let harness_b = build_harness(&workspace_b, storage_b, provider);
    harness_b.init().await.expect("init B");

    let download: JsonValue = harness_b
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
    let imported = download
        .get("files_imported")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        imported >= FILE_COUNT as u64,
        "B should import all {FILE_COUNT} note files (plus index.md). \
         Got files_imported={imported} in {download}"
    );

    // Spot-check first / mid / last-of-first-page / first-of-second-page /
    // last. Catches off-by-one bugs at page boundaries; reading every file
    // wouldn't add coverage.
    let b_root = workspace_b.parent().expect("B root");
    for i in [0usize, 250, 499, 500, 599] {
        let path = b_root.join(format!("notes/n-{i:04}.md"));
        assert!(
            path.exists(),
            "B should have notes/n-{i:04}.md after pagination download. \
             Path: {path:?}"
        );
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read notes/n-{i:04}.md: {e}"));
        assert!(
            body.contains(&format!("# note {i}\n")),
            "notes/n-{i:04}.md should contain its own marker body. Got:\n{body}"
        );
    }
}
