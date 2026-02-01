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

CRDT synchronization bridge connecting the Rust WASM CRDT to the sync server.

## Files

| File | Purpose |
|------|---------|
| `multiplexedBodySync.ts` | Multiplexed body document sync |
| `rustCrdtApi.ts` | TypeScript API for Rust CRDT |
| `syncHelpers.ts` | Sync utility functions |
| `syncTransport.ts` | WebSocket transport layer |
| `types.ts` | TypeScript type definitions |
| `workspaceCrdtBridge.ts` | Bridge between CRDT and stores |
