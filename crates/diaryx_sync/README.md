---
title: diaryx_sync
author: adammharris
part_of: '[Crates README](/crates/README.md)'
---
# diaryx_sync

Sync protocol engine for Diaryx.

This crate contains the shared sync protocol layer used by both the cloud sync server (`diaryx_sync_server`) and the CLI's local web editing mode.

## Modules

- **protocol** — Document type parsing, control messages, handshake types, wire format utilities
- **storage** — Per-workspace `SqliteStorage` cache
- **hooks** — `SyncHookDelegate` trait and generic `DiarySyncHook<D>` implementing siphonophore's `Hook`
- **server** — Generic `SyncServer` builder wrapping siphonophore + axum
- **local** — `LocalSyncHook` and `start_local_server()` for CLI-based web editing

## Usage

The cloud server implements `SyncHookDelegate` with JWT auth and multi-workspace isolation.
The CLI implements it with no-op auth for single-workspace local editing.
