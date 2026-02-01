---
title: CRDT
description: CRDT synchronization bridge
part_of: '[README](/apps/web/src/lib/README.md)'
attachments:
  - '[index.ts](/apps/web/src/lib/crdt/index.ts)'
  - '[multiplexedBodySync.ts](/apps/web/src/lib/crdt/multiplexedBodySync.ts)'
  - '[rustCrdtApi.ts](/apps/web/src/lib/crdt/rustCrdtApi.ts)'
  - '[syncHelpers.ts](/apps/web/src/lib/crdt/syncHelpers.ts)'
  - '[syncTransport.ts](/apps/web/src/lib/crdt/syncTransport.ts)'
  - '[types.ts](/apps/web/src/lib/crdt/types.ts)'
  - '[workspaceCrdtBridge.ts](/apps/web/src/lib/crdt/workspaceCrdtBridge.ts)'
exclude:
  - '*.lock'
  - '*.test.ts'
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

## Sync Status Tracking

Sync status is tracked at two levels to ensure accurate UI representation:

1. **Metadata sync** (`syncStatus`): Tracks workspace metadata (file list, titles, hierarchy)
2. **Body sync** (`bodySyncStatus`): Tracks file content synchronization

The `effectiveSyncStatus` getter in `collaborationStore` combines these, only showing "synced" when BOTH metadata AND body content are fully synchronized. This prevents UI issues where the indicator shows completion before file content has actually downloaded.

### Per-file tracking

`MultiplexedBodySync` tracks per-file sync completion via:
- `receivedData`: Whether actual sync data was received for this file
- `synced`: Whether the file has been marked as synced (via `sync_complete` or data receipt)

## Files

| File | Purpose |
|------|---------|
| `multiplexedBodySync.ts` | Multiplexed body document sync with per-file tracking |
| `rustCrdtApi.ts` | TypeScript API for Rust CRDT |
| `syncHelpers.ts` | Sync utility functions |
| `syncTransport.ts` | WebSocket transport layer |
| `types.ts` | TypeScript type definitions |
| `workspaceCrdtBridge.ts` | Bridge between CRDT and stores |
