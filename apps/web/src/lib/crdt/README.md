---
title: CRDT
description: CRDT synchronization bridge
part_of: "[README](/apps/web/src/lib/README.md)"
attachments:
  - "[index.ts](/apps/web/src/lib/crdt/index.ts)"
  - "[multiplexedBodySync.ts](/apps/web/src/lib/crdt/multiplexedBodySync.ts)"
  - "[rustCrdtApi.ts](/apps/web/src/lib/crdt/rustCrdtApi.ts)"
  - "[syncHelpers.ts](/apps/web/src/lib/crdt/syncHelpers.ts)"
  - "[syncTransport.ts](/apps/web/src/lib/crdt/syncTransport.ts)"
  - "[types.ts](/apps/web/src/lib/crdt/types.ts)"
  - "[workspaceCrdtBridge.ts](/apps/web/src/lib/crdt/workspaceCrdtBridge.ts)"
exclude:
  - "*.lock"
  - "*.test.ts"
---

# CRDT

CRDT synchronization bridge connecting the Rust CRDT to the sync server.

## Platform-specific Sync

The sync layer supports two modes depending on the platform:

### Native Sync (Tauri)

For the Tauri desktop app, sync uses the native Rust `SyncClient` with `TokioTransport`. This provides:

- Direct WebSocket connections from Rust
- Better performance (no JS↔Rust bridge overhead for sync messages)
- Automatic reconnection via the Rust sync client
- Event bridge connects local CRDT changes to the WebSocket

The `Backend.startSync()` / `stopSync()` methods are used to control native sync.

### Browser Sync (WASM/Web)

For the web app, sync uses JavaScript WebSockets via `SyncTransport`. The flow is:

1. Frontend creates `SyncTransport` (browser WebSocket)
2. Local CRDT changes emit `SendSyncMessage` events from Rust
3. `workspaceCrdtBridge.ts` handles these events and sends them via `syncBridge.sendRawMessage()`

`SyncTransport` queues workspace sync messages while disconnected and flushes them after reconnect,
so local metadata updates aren’t dropped during transient network outages.

## Sync Status Tracking

Sync status is tracked at two levels to ensure accurate UI representation:

1. **Metadata sync** (`syncStatus`): Tracks workspace metadata (file list, titles, hierarchy)
2. **Body sync** (`bodySyncStatus`): Tracks file content synchronization

The `effectiveSyncStatus` getter in `collaborationStore` combines these, only showing "synced" when BOTH metadata AND body content are fully synchronized. This prevents UI issues where the indicator shows completion before file content has actually downloaded.

`waitForInitialSync()` follows the same rule: it resolves only when metadata
and active body bootstrap are ready.

`UnifiedSyncTransport` now treats Rust `statusChanged: synced` as authoritative
for workspace readiness. `SyncSession` emits this only after metadata and
pending body bootstrap are complete, so the UI no longer waits on an extra
fallback timer.

`sync_complete` (when present) is still surfaced for metrics/progress callbacks,
but it is not required to mark the workspace as synced.

## Canonical Path Resolution for Sync Events

`workspaceCrdtBridge.ts` resolves canonical paths for sync decisions via
backend `GetCanonicalPath` (`syncHelpers.getCanonicalPath`) when available, and
falls back to local normalization only if backend canonicalization is
unavailable.

For metadata mutations addressed by filesystem path, the bridge resolves the
underlying CRDT storage key (`doc_id` in doc-ID mode) before calling `Get/SetCrdtFile`,
so updates (including `BinaryRef.hash` attachment metadata) land on the canonical
record instead of a legacy path key.

For hierarchy metadata (`part_of` / `contents`), `workspaceCrdtBridge.ts` now
uses backend `LinkParser` commands when available, so tree resolution follows
the same Rust link semantics (including plain-canonical handling). A local
resolver remains as fallback.

`App.svelte` uses the async bridge method when matching metadata/body events to
the currently open entry. This avoids alias mismatches such as `./README.md`
vs `README.md` and prevents duplicate or missed UI updates.

`workspaceCrdtBridge.ts` also exposes `onFileRenamed(oldPath, newPath)` so UI
state can remap the currently open entry path immediately when a rename arrives.
For `FileRenamed` events, the bridge fetches fresh metadata for `new_path` from
Rust and emits it through `onFileChange`, which keeps sidebar properties in sync
after remote renames.

When `SendSyncMessage` events arrive before the transport is ready (for
example, during reconnect/setup transitions), `workspaceCrdtBridge` queues the
local updates and flushes them after connect so local saves are not dropped.

For snapshot bootstrap flows (`load_server`, and `sync_local` after successful
snapshot upload), `SyncSetupWizard` explicitly discards queued pre-connect local
updates before opening the WebSocket. This prevents replaying stale bootstrap
create/delete/body events on top of already-imported snapshot state.

`workspaceCrdtBridge.ts` also wires the incremental attachment sync queue:

- configures queue auth/server/workspace context as sync state changes
- provides backend access for local attachment reads/writes
- indexes `BinaryRef` attachment metadata and queues missing-blob downloads on metadata updates
- normalizes filesystem-event frontmatter before callbacks/queueing so missing
  `attachments` fields are treated as `[]` instead of crashing event handlers

To keep queue state consistent, bridge code and controllers import the same
`attachmentSyncService` module path (single singleton instance), and queue
context is refreshed during `initWorkspace` after `workspaceId/serverUrl` are set.

### Per-file tracking

`MultiplexedBodySync` tracks per-file sync completion via:

- `receivedData`: Whether actual sync data was received for this file
- `synced`: Whether the file has been marked as synced (via `sync_complete` or data receipt)

### Focus list behavior

`MultiplexedBodySync` tracks the local focus list and re-sends it after reconnects,
so focus updates aren’t dropped if the WebSocket is temporarily disconnected.

### Deferred body subscriptions

If a file is opened before sync configuration is ready (workspace ID or server URL),
the body subscription is queued and flushed once sync initializes. This prevents
missed body updates during late auth or workspace setup.

## Files

| File                     | Purpose                                               |
| ------------------------ | ----------------------------------------------------- |
| `multiplexedBodySync.ts` | Multiplexed body document sync with per-file tracking |
| `rustCrdtApi.ts`         | TypeScript API for Rust CRDT                          |
| `syncHelpers.ts`         | Sync utility functions                                |
| `syncTransport.ts`       | WebSocket transport layer                             |
| `types.ts`               | TypeScript type definitions                           |
| `workspaceCrdtBridge.ts` | Bridge between CRDT and stores                        |
