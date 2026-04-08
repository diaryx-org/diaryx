---
title: "Sync"
description: "Real-time multi-device sync across Diaryx workspaces"
id: "diaryx.sync"
version: "0.1.4"
author: "Diaryx Team"
license: "PolyForm Shield 1.0.0"
repository: "https://github.com/diaryx-org/diaryx"
categories: ["sync", "collaboration"]
tags: ["sync", "crdt", "realtime"]
capabilities: ["workspace_events", "file_events", "crdt_commands", "sync_transport", "custom_commands"]
artifact:
  url: ""
  sha256: ""
  size: 0
  published_at: ""
ui:
  - slot: SettingsTab
    id: sync-settings
    label: "Sync"
  - slot: SidebarTab
    id: snapshots
    label: "Snapshots"
  - slot: SidebarTab
    id: history
    label: "History"
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
    plugin_storage: "Store sync configuration and CRDT state."
    read_files: "Read workspace files for snapshotting, reconciliation, and sync."
    edit_files: "Apply remote changes to existing workspace files."
    create_files: "Create files received from remote sync or restored from snapshots."
    delete_files: "Delete files removed by remote sync or snapshot restore operations."
---

# diaryx_sync_extism

Extism guest plugin wrapping `diaryx_sync` for on-demand CRDT sync.

This repo pins `diaryx_core` / `diaryx_extism` from the main
`https://github.com/diaryx-org/diaryx.git` repository via git refs rather than
local filesystem patches, so standalone builds track a pushed Diaryx commit.

## Overview

This crate compiles to a `.wasm` module that can be loaded by the Extism host runtime:
- **Native** (Tauri/CLI): via `diaryx_extism` (wasmtime)
- **Web**: via `@extism/extism` JS SDK

The Sync settings tab uses Diaryx's declarative plugin UI surfaces.
Snapshot/history panels remain iframe-backed plugin HTML.

The plugin owns all CRDT state (WorkspaceCrdt, BodyDocManager) in its own WASM sandbox and is loaded on demand when sync is enabled.
Workspace namespace creation, listing, object upload/download/delete, and manifest listing all go through the host SDK's typed namespace API instead of direct `/namespaces` HTTP fetches.

## Exports

### JSON (standard Extism protocol)

| Export | Description |
|--------|-------------|
| `manifest()` | Plugin metadata + UI contributions (sync settings, Snapshots/History tabs, status bar) |
| `init(params)` | Initialize with workspace config |
| `shutdown()` | Persist state and clean up |
| `handle_command(request)` | Structured commands (CRDT ops, sync state, etc.) |
| `on_event(event)` | Filesystem events from the host |
| `get_config()` / `set_config()` | Plugin configuration |

### Binary (hot path)

| Export | Description |
|--------|-------------|
| `handle_binary_message(bytes)` | Framed v2 sync message, returns action envelope |
| `handle_text_message(text)` | Control/handshake messages, returns action envelope |
| `on_connected(params)` | Connection established, returns initial sync messages |
| `on_disconnected()` | Connection lost |
| `queue_local_update(params)` | Local CRDT change, returns sync messages to send |
| `on_snapshot_imported()` | Snapshot downloaded and imported |
| `sync_body_files(params)` | Request body sync for specific files |

## Host Functions Required

| Function | Description |
|----------|-------------|
| `host_log` | Log a message |
| `host_read_file` | Read a workspace file |
| `host_list_files` | List files under a prefix |
| `host_file_exists` | Check file existence |
| `host_write_file` | Write a text file |
| `host_delete_file` | Delete a file |
| `host_write_binary` | Write a binary file |
| `host_emit_event` | Emit sync events to host |
| `host_storage_get` | Load persisted CRDT state |
| `host_storage_set` | Persist CRDT state |
| `host_get_timestamp` | Get current timestamp |
| `host_ws_request` | Generic websocket transport bridge used by the guest's sync runtime |
| `host_namespace_create` | Create a sync workspace namespace |
| `host_namespace_list` | List user-owned namespaces |
| `host_namespace_list_objects` | List namespace object metadata, including sync hashes and timestamps |
| `host_namespace_get_object` | Download namespace object bytes |
| `host_namespace_put_object` | Upload owner-only sync objects |
| `host_namespace_delete_object` | Delete namespace objects removed locally |

All host functions use the Extism string ABI (`String -> String`), so side-effect
functions should still return an empty string (`""`) from the host.

State access inside the guest uses non-panicking `try_borrow`/`try_borrow_mut`
paths for both binary hot-path handlers and JSON/lifecycle exports
(`init`/`shutdown`/`handle_command`/`on_event`/`get_config`/`set_config`), so
transient borrow conflicts are surfaced as warnings/errors instead of crashing
the plugin with `RefCell` borrow panics.

Namespace host calls include explicit per-request deadlines in the browser host
path, so `DownloadWorkspace` fails with a concrete request error instead of
hanging indefinitely on a stalled object fetch. The pull loop also logs
start/progress/finish messages to make restore stalls easier to distinguish
from slow-but-active sequential downloads.

## Build

```bash
cargo build --target wasm32-unknown-unknown -p diaryx_sync_extism --release
# Output: target/wasm32-unknown-unknown/release/diaryx_sync_extism.wasm

# Optional optimization:
wasm-opt -Oz target/wasm32-unknown-unknown/release/diaryx_sync_extism.wasm -o dist/diaryx_sync.wasm
```

Or via the build script which includes this step:
```bash
./scripts/build-wasm.sh
```

If browser loading fails with unresolved `__wbindgen_*` imports, the served
`apps/web/public/plugins/diaryx_sync.wasm` file is stale or incorrect. Re-run
`./scripts/build-wasm.sh` so the Extism guest artifact from this crate is copied
to `public/plugins`.

## Binary Action Envelope

Binary exports return an action list encoded as:
```
[u16: num_actions]
for each action:
  [u8: action_type]   // 0=SendBinary, 1=SendText, 2=EmitEvent, 3=DownloadSnapshot
  [u32: payload_len]
  [payload_bytes]
```
