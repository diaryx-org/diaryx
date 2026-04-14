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

## Build

```bash
cargo build --target wasm32-unknown-unknown -p diaryx_sync_extism --release
# Output: target/wasm32-unknown-unknown/release/diaryx_sync_extism.wasm

# Optional optimization:
wasm-opt -Oz target/wasm32-unknown-unknown/release/diaryx_sync_extism.wasm -o dist/diaryx_sync.wasm
```

Or via the build script:
```bash
./scripts/build-wasm.sh
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
| `host_namespace_put_object` | Upload object bytes |
| `host_namespace_delete_object` | Delete namespace object |

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
