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
  - "[workspaceAccess.ts](/apps/web/src/lib/backend/workspaceAccess.ts)"
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
| `workspaceAccess.ts`  | Tauri workspace-access bridge   |
| `wasmWorkerNew.ts`    | WASM worker implementation      |
| `workerBackendNew.ts` | Worker-based backend            |

The `generated/` directory contains TypeScript types generated from Rust.
Plugin manifest/editor-extension bindings are generated from `diaryx_core`, so
after Rust-side manifest changes the repo should refresh them via
`cargo test -p diaryx_core` followed by `scripts/sync-bindings.sh`.

## Tauri Runtime Detection

`interface.ts` treats Tauri v2 as present when either `globalThis.isTauri` or
`window.__TAURI_INTERNALS__` exists. That matches the real runtime markers used
by `@tauri-apps/api` and avoids iOS builds silently falling back to browser
code paths just because `window.__TAURI__` is absent.

## Attachment Upload Path

`api.ts` now uploads entry attachments over the backend binary channel:

- resolve the attachment storage path with `ResolveAttachmentPath`
- write bytes with `writeBinary(...)`
- register the attachment ref with the lightweight `RegisterAttachment` command

This avoids base64 encoding/decoding and keeps large media uploads off the
JSON command path in both web and Tauri runtimes.

## Live Sync Event Emission

`api.ts` emits browser plugin file events for both body edits and frontmatter
edits. `setFrontmatterProperty()` and `removeFrontmatterProperty()` always
dispatch `file_saved` for the effective entry path, even when the change is not
a title rename.

That event contract is required for provider-owned live sync. The sync guest
uses `file_saved` to rebuild workspace metadata from disk and propagate
description, audience, `part_of`, `contents`, and other frontmatter changes
across connected clients without relying on host-side CRDT refresh logic.

For image previews on Tauri, the frontend can also prefer native `asset:`
URLs (`convertFileSrc`) for local verified attachment files. When native
loading is unavailable or out-of-scope, the preview path falls back to the
shared blob resolver.

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

## OPFS Runtime Probe

`storageType.ts` now distinguishes between nominal OPFS support and actual
runtime usability. Async callers can resolve `opfs` to `indexeddb` up front
when browsers expose `navigator.storage.getDirectory()` but the storage backend
is unavailable in the current mode (for example Safari private browsing).

`workerBackendNew.ts` uses that probe before both worker and main-thread
initialization, which avoids misleading "Initializing with storage: opfs" logs
and reduces restore/create noise before the backend falls back to IndexedDB.

`localWorkspaceRegistry.svelte.ts` also uses the same probe before scanning the
OPFS root, so unsupported runtimes skip best-effort workspace discovery instead
of logging transient OPFS discovery warnings on every startup.

## Main-Thread WASM Fallback Loading

`wasmWorkerNew.ts` now prefers an explicit WASM asset URL
(`$lib/wasm/diaryx_wasm_bg.wasm`) when initializing `$wasm` outside the
worker path. This avoids WebKit/dev fallback failures where wasm-pack's implicit
`import.meta.url` resolution can fail to fetch the `.wasm` binary.

## Preview Mock Backend

`mockBackend.ts` backs preview/onboarding iframe flows with an in-memory
backend. It now explicitly implements `GetFilesystemTree`, including subtree
selection, hidden-file filtering, and depth pruning, so preview mode satisfies
the same `Response::Tree` contract used by workspace asset storage and browser
plugin startup.

Unsupported mock commands now throw instead of silently returning `Ok`. That
keeps preview/test failures localized to the missing command rather than
surfacing later as confusing response-type mismatches.

## Sync Boundary (Plugin-Owned)

The web backend no longer initializes a host-side CRDT storage bridge.

- `wasmWorkerNew.ts` only initializes storage/runtime (OPFS, IndexedDB, FSA,
  plugin storage) and command execution.
- Sync/CRDT orchestration is plugin-owned (for example, sync plugin commands and
  plugin surfaces in settings/sidebar/status).
- Runtime context passed to Extism guests includes generic provider-link
  metadata for the current workspace, not just sync-plugin-specific IDs.
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

Marketplace plugin registry fetches and `.wasm` artifact downloads now also go
through `proxyFetch` on Tauri, which keeps TestFlight iOS plugin installs off
the WKWebView network path and routes them through native `reqwest` instead.

## Plugin Host Parity

The shared frontend now uses the same plugin inspection and permission-review
flow for browser and Tauri installs.

- `TauriBackend` exposes `inspectPlugin()` so local `.wasm` installs can read
  requested permissions before installation, matching browser plugin review.
- `api.ts` now also treats Tauri Extism "Permission not configured" plugin
  errors like the browser host does: it normalizes the requested target,
  triggers the shared permission banner UI, and retries the plugin command or
  component render once after the user allows it.
- `api.ts` also exposes `resolveWorkspaceRootIndexPath(...)`, which normalizes
  workspace-directory vs root-index-file inputs before frontmatter or workspace
  config flows read `README.md` / `index.md`. Shared frontend callers use that
  helper to avoid attempting `GetFrontmatter`/`GetWorkspaceConfig` directly on
  a directory path in Tauri/App Store/TestFlight builds.
- `workspaceAccess.ts` now does the same kind of app-wide bridging for
  sandboxed workspace picks: shared folder-picker flows call the native
  `authorize_workspace_path` command immediately after selection so TestFlight
  and App Store builds persist security-scoped bookmarks before those paths are
  stored in the local workspace registry.
- Tauri plugin install/inspect calls now also log stage-specific failures on
  both the JS and Rust sides, so mobile/TestFlight install issues show up in
  the Debug log panel with enough context to tell whether the failure happened
  during inspection, native install, or workspace file writes.
- `TauriBackend` and browser Extism runtimes now also expose direct
  `get_component_html` loading for plugin iframe surfaces, with a
  `PluginCommand("get_component_html")` fallback for older guests.
- `TauriBackend` also exposes `executePluginCommandWithFiles(...)` so settings
  commands that rely on temporary `host_request_file` payloads can pass raw
  bytes to native Extism plugins just like browser-loaded plugins.
- Browser plugin loading now enforces the same protocol-version compatibility
  range as the native Extism loader and preserves guest CLI declarations in the
  normalized manifest.
- Browser Extism host file writes/deletes now propagate storage failures back to
  the guest instead of logging and returning success. Provider-owned restore
  flows therefore fail fast with a surfaced error when workspace writes are not
  possible, rather than stalling indefinitely behind the launch overlay.

## Reveal In File Manager

`TauriBackend` also exposes an optional `revealInFileManager(path)` helper that
invokes a Tauri desktop command backed by `tauri-plugin-opener`'s
`reveal_item_in_dir`. `LeftSidebar.svelte` uses that to offer a desktop-only
"Show in Finder" / "Show in Explorer" / "Show in File Manager" action for tree
entries. The control is hidden on iOS and Android because Tauri does not expose
reveal support there.

## Desktop App Updates

`TauriBackend` now also exposes optional `checkForAppUpdate()` and
`installAppUpdate()` helpers. Those commands are backed by the Tauri updater
plugin only in direct desktop distribution builds (`desktop-updater` feature),
and they return `null`/`false` in App Store or mobile builds so shared frontend
code can probe update support without branching on target-specific imports.
