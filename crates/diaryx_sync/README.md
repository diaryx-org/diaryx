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
- **sqlite** — SQLite-backed CRDT storage (`SqliteStorage`)
- **server** — Siphonophore hooks, axum WebSocket server, `StorageCache`
- **native-sync** — Native sync transport (tokio-tungstenite WebSocket)
- **git** — Git-backed version history (commit, rebuild)

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
