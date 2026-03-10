---
title: diaryx_sync
author: adammharris
part_of: '[Crates README](/crates/README.md)'
---
# diaryx_sync

Sync engine for Diaryx — CRDT types, sync protocol, and server infrastructure.

This crate owns all CRDT and sync functionality. It provides WASM-compatible core types and protocol modules, with optional features for server, native sync, SQLite storage, and git integration.

## Feature Flags

- **default** — CRDT types and sync protocol only (WASM-compatible)
- **typescript** — TypeScript binding derives for CRDT-facing types
- **sqlite** — SQLite-backed CRDT storage (`SqliteStorage`)
- **server** — Siphonophore hooks, axum WebSocket server, `StorageCache`
- **native-sync** — Native sync transport (tokio-tungstenite WebSocket)
- **git** — Git-backed version history (commit, rebuild)

For non-browser WASM guests (for example Extism on `wasm32-unknown-unknown`),
sync timestamping uses a monotonic fallback clock in `time.rs` instead of
`SystemTime::now()` to avoid runtime panics in environments without host clock APIs.

## Core Modules (always available)

- **types** — `FileMetadata`, `BinaryRef`, `CrdtUpdate`, `UpdateOrigin`
- **workspace_doc** — `WorkspaceCrdt` — Y.Map-backed workspace metadata
- **body_doc** / **body_doc_manager** — `BodyDoc`, `BodyDocManager` — Y.Text body documents
- **memory_storage** — `MemoryStorage` — in-memory CrdtStorage for tests and WASM
- **sync_protocol** — `SyncProtocol`, `SyncMessage`, wire format (frame/unframe, varint)
- **sync_handler** — `SyncHandler` — remote update application with filesystem effects
- **sync_manager** — `RustSyncManager` — unified sync orchestration
- **sync_session** — `SyncSession` — platform-agnostic sync session state machine
- **sync_types** — `SyncEvent`, `SyncStatus`, `SyncSessionConfig`
- **control_message** — `ControlMessage` — handshake and control protocol
- **history** — `HistoryManager`, `HistoryEntry`, `FileDiff`
- **materialize** — `materialize_workspace()` — snapshot export from CRDT state
- **sanity** — `validate_workspace()` — CRDT consistency checks
- **self_healing** — `HealthTracker` — auto-repair for CRDT issues
- **crdt_fs** — `CrdtFs` — filesystem decorator that transparently updates CRDTs
- **decorator_stack** — `DecoratedFsBuilder` — composable FS decorator builder
- **sync_plugin** — `SyncPlugin<FS>` — `WorkspacePlugin` that owns all CRDT state and handles ~50 sync commands

The sync plugin manifest also exposes an explicit `WorkspaceProvider`
contribution so hosts can surface Diaryx Cloud as a provider without inferring
it from command names.

`InitializeWorkspaceCrdt` rebuilds metadata from the logical workspace tree and
then backfills any additional on-disk markdown files under the workspace root.
That keeps live sync resilient when local create/rename/move operations briefly
leave `contents` or `part_of` links stale.
Late body updates are ignored for files whose workspace metadata is already
tombstoned, so a just-deleted file cannot be recreated on disk by delayed body
packets that arrive after the delete metadata sync.

## Feature-Gated Modules

- **sqlite_storage** (feature: `sqlite`) — `SqliteStorage`
- **git/** (feature: `git`) — `commit_workspace()`, `rebuild_crdt_from_git()`
- **sync_client** (feature: `native-sync`) — `SyncClient`, `TokioTransport`
- **hooks** (feature: `server`) — `DiarySyncHook`, `SyncHookDelegate`
- **server** (feature: `server`) — `SyncServer` builder
- **storage** (feature: `server`) — `StorageCache` per-workspace connection cache
- **local** (feature: `server`) — Local sync server for CLI web editing
- **protocol** (feature: `server`) — `DocType`, `AuthenticatedUser`, handshake types

## Usage

### WASM (default features)
```toml
diaryx_sync = { workspace = true }
```

### Server
```toml
diaryx_sync = { workspace = true, features = ["server", "git"] }
```

### CLI
```toml
diaryx_sync = { workspace = true, features = ["server"] }
```
