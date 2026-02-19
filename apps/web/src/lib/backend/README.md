---
title: Backend
description: Backend abstraction layer for WASM and Tauri
part_of: '[README](/apps/web/src/lib/README.md)'
attachments:
  - '[index.ts](/apps/web/src/lib/backend/index.ts)'
  - '[api.ts](/apps/web/src/lib/backend/api.ts)'
  - '[eventEmitter.ts](/apps/web/src/lib/backend/eventEmitter.ts)'
  - '[interface.ts](/apps/web/src/lib/backend/interface.ts)'
  - '[storageType.ts](/apps/web/src/lib/backend/storageType.ts)'
  - '[tauri.ts](/apps/web/src/lib/backend/tauri.ts)'
  - '[wasmWorkerNew.ts](/apps/web/src/lib/backend/wasmWorkerNew.ts)'
  - '[workerBackendNew.ts](/apps/web/src/lib/backend/workerBackendNew.ts)'
exclude:
  - '*.lock'
  - 'generated/**'
  - 'serde_json/**'
  - '*.test.ts'
---

# Backend

Backend abstraction layer supporting both WASM and Tauri environments.

## Files

| File | Purpose |
|------|---------|
| `api.ts` | High-level backend API |
| `eventEmitter.ts` | Event emission for file changes |
| `interface.ts` | Backend interface definition |
| `storageType.ts` | Storage type detection |
| `tauri.ts` | Tauri IPC implementation |
| `wasmWorkerNew.ts` | WASM worker implementation |
| `workerBackendNew.ts` | Worker-based backend |

The `generated/` directory contains TypeScript types generated from Rust.

## ZIP Import Memory Use

`workerBackendNew.ts` now imports ZIP files via a streaming reader (`@zip.js/zip.js`)
instead of loading full archives into a single `ArrayBuffer`. This significantly
reduces peak memory usage for large local imports by processing entries one at a time.
The import loop also yields periodically to the event loop and throttles progress
callbacks, improving UI responsiveness during very large imports.

## Native Sync (Tauri only)

The `TauriBackend` provides native sync methods that use the Rust sync client:

```typescript
// Check if native sync is available
if (backend.hasNativeSync?.()) {
  // Start native sync (uses Rust TokioTransport + SyncClient)
  await backend.startSync(serverUrl, docName, authToken);

  // Subscribe to sync events
  const unsubscribe = backend.onSyncEvent?.((event) => {
    console.log('Sync event:', event.type, event);
  });

  // Get sync status
  const status = await backend.getSyncStatus?.();

  // Stop sync
  await backend.stopSync?.();
}
```

Event types: `status-changed`, `files-changed`, `body-changed`, `progress`, `error`

## Link Parser Command

The backend command API now exposes Rust `link_parser` operations through
`Command::LinkParser` (parse, canonicalize, format, convert). Frontend callers
can use `api.ts` helpers (`runLinkParser`, `parseLink`, `canonicalizeLink`,
`formatLink`, `convertLink`) to avoid duplicating link parsing logic in
TypeScript.
