---
title: Backend
description: Backend abstraction layer for WASM and Tauri
part_of: "[README](/apps/web/src/lib/README.md)"
attachments:
  - "[index.ts](/apps/web/src/lib/backend/index.ts)"
  - "[api.ts](/apps/web/src/lib/backend/api.ts)"
  - "[eventEmitter.ts](/apps/web/src/lib/backend/eventEmitter.ts)"
  - "[interface.ts](/apps/web/src/lib/backend/interface.ts)"
  - "[storageType.ts](/apps/web/src/lib/backend/storageType.ts)"
  - "[tauri.ts](/apps/web/src/lib/backend/tauri.ts)"
  - "[wasmWorkerNew.ts](/apps/web/src/lib/backend/wasmWorkerNew.ts)"
  - "[workerBackendNew.ts](/apps/web/src/lib/backend/workerBackendNew.ts)"
exclude:
  - "*.lock"
  - "generated/**"
  - "serde_json/**"
  - "*.test.ts"
---

# Backend

Backend abstraction layer supporting both WASM and Tauri environments.

## Files

| File                  | Purpose                         |
| --------------------- | ------------------------------- |
| `api.ts`              | High-level backend API          |
| `eventEmitter.ts`     | Event emission for file changes |
| `interface.ts`        | Backend interface definition    |
| `storageType.ts`      | Storage type detection          |
| `tauri.ts`            | Tauri IPC implementation        |
| `wasmWorkerNew.ts`    | WASM worker implementation      |
| `workerBackendNew.ts` | Worker-based backend            |

The `generated/` directory contains TypeScript types generated from Rust.

## ZIP Import Memory Use

`workerBackendNew.ts` now imports ZIP files via a streaming reader (`@zip.js/zip.js`)
instead of loading full archives into a single `ArrayBuffer`. This significantly
reduces peak memory usage for large local imports by processing entries one at a time.
The import loop also yields periodically to the event loop and throttles progress
callbacks, improving UI responsiveness during very large imports.

## Worker Startup Fail-Fast

`workerBackendNew.ts` now guards worker startup with error listeners and an
initialization timeout. If module worker loading is blocked (for example by
WebKit COEP/origin restrictions), backend initialization fails quickly with a
clear error instead of hanging indefinitely.

When worker startup fails due browser restrictions, `WorkerBackendNew` now
automatically falls back to an in-process (main-thread) WASM backend path so
local workspace flows can still proceed without plugins/worker-only features.

## Main-Thread WASM Fallback Loading

`wasmWorkerNew.ts` now prefers an explicit WASM asset URL
(`$lib/wasm/diaryx_wasm_bg.wasm`) when initializing `@diaryx/wasm` outside the
worker path. This avoids WebKit/dev fallback failures where wasm-pack's implicit
`import.meta.url` resolution can fail to fetch the `.wasm` binary.

## Sync Boundary (Plugin-Owned)

The web backend no longer initializes a host-side CRDT storage bridge.

- `wasmWorkerNew.ts` only initializes storage/runtime (OPFS, IndexedDB, FSA,
  plugin storage) and command execution.
- Sync/CRDT orchestration is plugin-owned (for example, sync plugin commands and
  plugin surfaces in settings/sidebar/status).
- `setupCrdtStorage()` remains a compatibility no-op in the worker API.

## Native Sync (Tauri only)

The `TauriBackend` provides native sync methods that use the Rust sync client:

```typescript
// Check if native sync is available
if (backend.hasNativeSync?.()) {
  // Start native sync (uses Rust TokioTransport + SyncClient)
  await backend.startSync(serverUrl, docName, authToken);

  // Subscribe to sync events
  const unsubscribe = backend.onSyncEvent?.((event) => {
    console.log("Sync event:", event.type, event);
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

## Native HTTP Proxy

`proxyFetch.ts` bridges HTTP through Tauri native networking when running in
Tauri. The bridge now maps null-body HTTP statuses (`204`, `205`, `304`, and
other fetch null-body statuses) to `Response` objects with `null` bodies so
no-content endpoints (for example, `DELETE /api/workspaces/{id}`) do not throw
`Response cannot have a body with the given status`.
