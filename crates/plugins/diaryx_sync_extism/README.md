---
title: "Sync"
description: "Real-time multi-device sync across Diaryx workspaces"
id: "diaryx.sync"
version: "0.1.4"
author: "Diaryx Team"
license: "PolyForm Shield 1.0.0"
repository: "https://github.com/diaryx-org/diaryx"
categories: ["sync", "collaboration"]
tags: ["sync", "lww", "realtime"]
capabilities: ["workspace_events", "file_events", "custom_commands"]
artifact:
  url: ""
  sha256: ""
  size: 0
  published_at: ""
ui:
  - slot: SettingsTab
    id: sync-settings
    label: "Sync"
  - slot: StatusBarItem
    id: sync-status
    label: "Sync"
  - slot: WorkspaceProvider
    id: diaryx.sync
    label: "Diaryx Sync"
cli:
  - name: sync
    about: "Sync workspace across devices"
requested_permissions:
  defaults:
    plugin_storage:
      include: ["all"]
    read_files:
      include: ["all"]
    edit_files:
      include: ["all"]
    create_files:
      include: ["all"]
    delete_files:
      include: ["all"]
  reasons:
    plugin_storage: "Store sync configuration and manifest state."
    read_files: "Read workspace files for hashing, diffing, and upload."
    edit_files: "Apply remote changes to existing workspace files."
    create_files: "Create files received from remote sync or snapshot restore."
    delete_files: "Delete files removed by remote sync or snapshot restore."
---

# diaryx_sync_extism

Extism guest plugin for LWW (last-writer-wins) file sync across devices.

Syncs workspace files to a remote namespace object store using hash-based
diffing and timestamp-based conflict resolution. No CRDTs — each file is
treated as an opaque blob identified by its SHA-256 content hash.

## How it works

1. **Scan** — Walk the workspace file set, hash each file, compare against the
   local sync manifest (a persisted map of `key → {hash, size, modified_at, state}`).
2. **Diff** — Fetch the server's object listing for the namespace and compute a
   plan: push, pull, delete-remote, delete-local.
3. **Resolve conflicts** — When both local and remote changed since the last
   sync, the most recent `modified_at` timestamp wins (LWW).
4. **Execute** — Push local files, pull remote files, delete as needed, then
   update the manifest.

### DownloadWorkspace (large-workspace path)

`DownloadWorkspace` uses a streaming pull tuned for unreliable links and
multi-thousand-file workspaces. Instead of "list everything → pull
everything → save manifest" it interleaves the work:

- **Page-by-page listing.** Server objects are paginated (500/page) and
  filtered against the existing manifest as each page arrives. Files
  already present with matching hashes are skipped — this is what makes
  the operation **resumable**: a re-run after a crash, network drop, or
  user cancellation only pulls what's actually missing.
- **Concurrent batches.** Each "wave" is `concurrency × batch_size` files
  fanned out as parallel HTTP requests via the new
  `host_namespace_get_objects_batches_concurrent` host fn. The WASM guest
  is single-threaded, so concurrency lives entirely on the host side.
- **Adaptive sizing.** After every wave the plugin tracks elapsed time
  and error count, then ramps `batch_size`/`concurrency` up on fast
  clean runs and backs off on slow runs or per-batch failures. The state
  is persisted per-namespace under `download_adaptive::<ns>` so the next
  run starts where the last one stabilised.
- **Per-wave checkpoint.** The manifest is `save()`d after every wave —
  the worst-case loss from a hard crash is one in-flight wave.
- **Cooperative cancellation.** Callers pass `cancel_token` in the
  command params; the plugin polls `host::cancellation::is_cancelled`
  between waves. On cancel it persists progress and returns
  `"DownloadWorkspace cancelled"`. Re-invoking with the same `remote_id`
  resumes from the manifest.

The host UI (`apps/web/src/lib/sync/workspaceProviderService.ts`)
generates the cancel token, exposes a handle for cancel buttons, and
preserves the partial workspace on cancel so resume "just works."

## Build

```bash
cargo build --target wasm32-unknown-unknown -p diaryx_sync_extism --release
# Output: target/wasm32-unknown-unknown/release/diaryx_sync_extism.wasm

# Optional optimization:
wasm-opt -Oz target/wasm32-unknown-unknown/release/diaryx_sync_extism.wasm -o dist/diaryx_sync.wasm
```

Or via the workspace xtask:
```bash
cargo xtask build-wasm
```

## Exports

### JSON (standard Extism protocol)

| Export | Description |
|--------|-------------|
| `manifest()` | Plugin metadata, UI contributions, CLI subcommands, permissions |
| `init(params)` | Initialize with workspace config (root, server URL, auth token) |
| `shutdown()` | Persist manifest and clean up |
| `handle_command(request)` | Structured command dispatcher (see Commands below) |
| `on_event(event)` | Filesystem events from the host (saved, created, deleted, renamed) |
| `get_config()` / `set_config()` | Plugin configuration (server URL, auth token, workspace ID) |
| `execute_typed_command(request)` | Alternative typed command interface |

### Commands

| Command | Description |
|---------|-------------|
| `Sync` | Full push + pull cycle |
| `SyncPush` | Push local changes to server |
| `SyncPull` | Pull remote changes to local |
| `SyncStatus` | Status with filesystem scan |
| `GetSyncStatus` | Cached status (no scan) |
| `GetProviderStatus` | Check if sync can reach the namespace service with the current host session |
| `LinkWorkspace` | Link workspace to a remote namespace |
| `UnlinkWorkspace` | Unlink workspace from namespace |
| `DownloadWorkspace` | Download entire remote workspace |
| `UploadWorkspaceSnapshot` | Upload all local files as initial snapshot |
| `ListRemoteWorkspaces` | List user-owned namespaces |
| `NsCreateNamespace` | Create a new namespace |
| `NsListNamespaces` | List namespaces |
| `NsPutObject` | Store object in namespace |
| `NsGetObject` | Retrieve object from namespace |
| `NsDeleteObject` | Delete object from namespace |
| `NsListObjects` | List objects in namespace |

### CLI subcommands

```
diaryx sync              # Full push + pull
diaryx sync status       # Show sync status
diaryx sync push         # Push local → remote
diaryx sync pull         # Pull remote → local
diaryx sync link         # Link to namespace (--namespace-id or --name)
diaryx sync unlink       # Unlink from namespace
diaryx sync config       # Show/set config (--server, --workspace-id, --show)
```

## Host Functions Required

| Function | Description |
|----------|-------------|
| `host_log` | Log a message |
| `host_read_file` | Read a workspace file |
| `host_read_binary` | Read a binary file |
| `host_list_dir` | List direct children of a directory |
| `host_list_files` | List files under a prefix |
| `host_file_exists` | Check file existence |
| `host_file_metadata` | Get file size and modification time |
| `host_write_file` | Write a text file |
| `host_write_binary` | Write a binary file |
| `host_delete_file` | Delete a file |
| `host_emit_event` | Emit sync events to host |
| `host_storage_get` | Load persisted manifest/config |
| `host_storage_set` | Persist manifest/config |
| `host_get_timestamp` | Get current timestamp |
| `host_hash_file` | Compute SHA-256 hash of a file |
| `host_workspace_file_set` | Get all file paths in the workspace tree |
| `host_get_runtime_context` | Get runtime context (server URL, auth token, workspace path) |
| `host_namespace_create` | Create a sync namespace |
| `host_namespace_list` | List user-owned namespaces |
| `host_namespace_list_objects` | List namespace objects with hashes and timestamps |
| `host_namespace_get_object` | Download object bytes |
| `host_namespace_get_objects_batch` | Download many objects in one request (server-parallel) |
| `host_namespace_get_objects_batches_concurrent` | Fan out N batch requests host-side in parallel |
| `host_namespace_put_object` | Upload object bytes |
| `host_namespace_delete_object` | Delete namespace object |
| `host_is_cancelled` | Poll whether the host has flagged an operation token as cancelled |

All host functions use the Extism string ABI (`String -> String`).

## UI Contributions

- **Settings tab** — Auth status, server URL, status check button
- **Status bar item** — Sync status indicator (right-aligned)
- **Workspace provider** — Registers "Diaryx Sync" as a workspace provider
- **Command palette** — Sync, Sync Push, Sync Pull, Sync Status

## Sync Manifest

The plugin persists a local manifest (via `host_storage_set`) tracking:

- **files** — Map of `key → {content_hash, size_bytes, modified_at, state}` where
  state is `Clean` (matches last sync) or `Dirty` (modified locally)
- **pending_deletes** — Files deleted locally that need server-side deletion on
  next sync
- **namespace_id** — The linked remote namespace
- **last_sync_at** — Timestamp of last successful sync

## Configuration

Two layers:

1. **Device-local** (host storage) — `server_url`, `auth_token`, `workspace_id`
2. **Workspace frontmatter** (syncs across devices) — `workspace_id` stored in
   the root index file under `plugins."diaryx.sync".workspace_id`

## Key behaviors

- **Ghost cleanup** — If the server listing reports an object that returns 404
  on fetch, the plugin deletes the ghost entry and does not report an error.
- **Retryable deletes** — Local rename/delete tombstones stay in the manifest
  until the corresponding remote delete succeeds. A remote 404 is treated as an
  acknowledged delete because the desired state is already reached.
- **Fail-fast bootstrap** — `LinkWorkspace` and `UploadWorkspaceSnapshot`
  return command errors on partial upload/delete failures instead of reporting
  success with an incomplete remote snapshot.
- **Percent-encoding normalization** — Server keys may use `+` or `%20` for
  spaces; `decode_server_key` normalizes both forms so local/remote comparisons
  work regardless of which client uploaded.
- **Safe manifest updates** — The catch-all that marks untracked files as
  `Clean` only applies to files confirmed present on the server, preventing
  phantom entries that could cause incorrect local deletions.

## Testing

Integration tests live in [tests/integration.rs](tests/integration.rs) and
load the real built WASM via `PluginTestHarness` from `diaryx_extism::testing`,
with in-process mocks for host APIs (`MockNamespaceProvider`,
`RecordingStorage`, `RecordingEventEmitter`).

Run:

```bash
cargo build -p diaryx_sync_extism --target wasm32-unknown-unknown --release
cargo test -p diaryx_sync_extism --test integration
```

Multi-device scenarios share a single `Arc<MockNamespaceProvider>` across two
harnesses — each harness has its own workspace directory and storage, but
every push/pull lands in the same namespace object store. See
`two_devices_share_namespace_via_link_and_download` for the pattern.

### End-to-end layer: [tests/sync_e2e.rs](tests/sync_e2e.rs)

Complements the mocks with real-HTTP coverage. Spawns
`diaryx_sync_server::testing::TestServer` on `127.0.0.1:0`, signs in via
the dev-mode magic-link flow, and drives plugin WASM instances through
`diaryx_extism::HttpNamespaceProvider` (sync `ureq` impl of
`NamespaceProvider`).

All scenarios run against a real TCP listener + in-memory SQLite. They're
fast (full suite is ~5s) because there's no external process boot — but
they exercise every plugin code path that uses the HTTP transport.

Scenarios (13 total, all passing, none `#[ignore]`d):

**Core happy paths**
- `two_devices_sync_via_real_http_server` — A links, B downloads;
  byte-exact content, manifest hash parity, server listing.
- `edit_on_a_propagates_to_b_via_sync` — A edits a file, Syncs; B's next
  Sync pulls the edit. Validates incremental delta (not just first sync).
- `delete_on_a_propagates_to_b_via_sync` — A removes a file, Syncs; B's
  next Sync removes it locally. Validates delete propagation.

**Conflict resolution**
- `lww_resolves_conflict_in_favor_of_later_mtime` — both devices edit
  the same file independently; the one with the strictly-later mtime
  wins. Found and fixed a units-mismatch bug in `sync_engine::compute_diff`
  (local mtime in ms was compared against server mtime in s — biased
  ~1000× toward push). See the regression unit test
  `conflict_lww_does_not_confuse_ms_and_s` in `sync_engine.rs`.
- `bidirectional_edits_converge` — A and B edit non-overlapping files
  concurrently; after a round-trip of Syncs both devices have both edits.
- `concurrent_syncs_from_two_devices_converge` — A and B call `Sync`
  *simultaneously* via `tokio::join!`; both succeed, the server doesn't
  drop either write, and a final catch-up Sync converges. Meaningful
  mostly against cloudflare (D1 can genuinely interleave) — sync_server's
  `Arc<Mutex<Connection>>` serialises internally.

**Idempotence / state preservation**
- `sync_with_no_changes_is_noop` — a second Sync with no local/remote
  changes reports `pushed=pulled=deleted_*=0`. (First post-link Sync is
  expected to push 1 — the frontmatter rewrite — so the assertion uses
  a flush pattern.)
- `sync_state_survives_harness_reconstruction` — session 1 links + flushes
  then drops the harness; session 2 rebuilds with the same
  `RecordingStorage` and a Sync returns a full no-op. Guards against
  manifest-persistence regressions.
- `multi_change_catchup_in_single_sync` — A makes 3 independent changes
  (edit a, edit b, add c + update index); B's *single* Sync pulls them
  all.

**Authorization**
- `bob_cannot_access_alices_namespace` — two distinct users; Bob's token
  must not see Alice's namespace in listings and is denied on direct
  list/get. Server-side authz test.

**Payload / encoding fuzz**
- `url_corpus_keys_roundtrip_via_plugin_ns_api` — the safe subset of
  `diaryx_server::contract::URL_KEY_CORPUS` through `NsPutObject` /
  `NsGetObject`, asserting byte-exact body + server listing. Sibling of
  the contract-level fuzz.
- `binary_file_roundtrip_via_plugin` — small non-UTF-8 payload (PNG
  signature + assorted high bytes) via base64; proves the encode/decode
  chain doesn't assume UTF-8.
- `large_binary_file_roundtrips_via_plugin` — 512 KiB deterministic
  pseudorandom bytes through the full stack, plus `list_objects`
  `size_bytes` parity check. Stresses R2/SQLite blob paths that might
  only bite for real-attachment-sized payloads.

Run:

```bash
cargo build -p diaryx_sync_extism --target wasm32-unknown-unknown --release
cargo test -p diaryx_sync_extism --test sync_e2e
```

### Cloudflare variant: [`diaryx_cloudflare_e2e` / `tests/sync_plugin_e2e.rs`](../../diaryx_cloudflare_e2e)

Every scenario above mirrored into a single multi-scenario `#[ignore]`-gated
runner (`cloudflare_sync_plugin_suite`) that boots `bunx wrangler dev --env
dev --local` once, executes each scenario under `catch_unwind` so a single
drift doesn't abort the rest, and panics at the end with a summary.
Amortises the ~60s wrangler boot across all scenarios.

Run:

```bash
cargo test -p diaryx_cloudflare_e2e --test sync_plugin_e2e -- --ignored --nocapture
```

This layer catches adapter-specific bugs (URL routing in worker-rs, D1
persistence, R2 vs SQLite blob semantics) that the sync_server E2E can't
see. It already surfaced:

- Missing `/batch/objects/multipart` endpoint on cloudflare (fixed — now
  mirrors the sync_server implementation byte-for-byte).
- `list_objects` dropping `content_hash` from its JSON response —
  silently broke cross-device pulls because `sync_engine::compute_diff`
  treats a missing hash as "server unchanged". Fixed + added a
  contract-level regression assertion so any future drop is caught at
  the faster layer.

### Gotcha: WASM path

Both test files use an absolute `WASM_PATH` built via
`concat!(env!("CARGO_MANIFEST_DIR"), "/../../../target/...")`. Earlier
versions used a path relative to cargo's run-time CWD — which defaults to
the package dir, not the workspace root, so `require_wasm!()` silently
returned "skip" and the tests reported as passing without actually
exercising the WASM. Keep the absolute form.
