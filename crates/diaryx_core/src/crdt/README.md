---
title: CRDT Synchronization
description: Conflict-free replicated data types for real-time collaboration
part_of: "[README](/crates/diaryx_core/src/README.md)"
audience:
  - developers
attachments:
  - "[mod.rs](/crates/diaryx_core/src/crdt/mod.rs)"
  - "[body_doc.rs](/crates/diaryx_core/src/crdt/body_doc.rs)"
  - "[control_message.rs](/crates/diaryx_core/src/crdt/control_message.rs)"
  - "[body_doc_manager.rs](/crates/diaryx_core/src/crdt/body_doc_manager.rs)"
  - "[history.rs](/crates/diaryx_core/src/crdt/history.rs)"
  - "[memory_storage.rs](/crates/diaryx_core/src/crdt/memory_storage.rs)"
  - "[sqlite_storage.rs](/crates/diaryx_core/src/crdt/sqlite_storage.rs)"
  - "[storage.rs](/crates/diaryx_core/src/crdt/storage.rs)"
  - "[sync.rs](/crates/diaryx_core/src/crdt/sync.rs)"
  - "[sync_client.rs](/crates/diaryx_core/src/crdt/sync_client.rs)"
  - "[sync_handler.rs](/crates/diaryx_core/src/crdt/sync_handler.rs)"
  - "[sync_manager.rs](/crates/diaryx_core/src/crdt/sync_manager.rs)"
  - "[sync_session.rs](/crates/diaryx_core/src/crdt/sync_session.rs)"
  - "[sync_types.rs](/crates/diaryx_core/src/crdt/sync_types.rs)"
  - "[tokio_transport.rs](/crates/diaryx_core/src/crdt/tokio_transport.rs)"
  - "[transport.rs](/crates/diaryx_core/src/crdt/transport.rs)"
  - "[types.rs](/crates/diaryx_core/src/crdt/types.rs)"
  - "[workspace_doc.rs](/crates/diaryx_core/src/crdt/workspace_doc.rs)"
  - "[materialize.rs](/crates/diaryx_core/src/crdt/materialize.rs)"
  - "[sanity.rs](/crates/diaryx_core/src/crdt/sanity.rs)"
  - "[self_healing.rs](/crates/diaryx_core/src/crdt/self_healing.rs)"
  - "[git/mod.rs](/crates/diaryx_core/src/crdt/git/mod.rs)"
  - "[git/commit.rs](/crates/diaryx_core/src/crdt/git/commit.rs)"
  - "[git/rebuild.rs](/crates/diaryx_core/src/crdt/git/rebuild.rs)"
  - "[git/repo.rs](/crates/diaryx_core/src/crdt/git/repo.rs)"
exclude:
  - "*.lock"
---

# CRDT Synchronization

This module provides conflict-free replicated data types (CRDTs) for real-time
collaboration, built on [yrs](https://docs.rs/yrs) (the Rust port of Yjs).

## Feature Flags

This module requires the `crdt` feature:

```toml
[dependencies]
diaryx_core = { version = "...", features = ["crdt"] }

# For SQLite-based persistent storage (native only)
diaryx_core = { version = "...", features = ["crdt", "crdt-sqlite"] }

# For native WebSocket sync client (CLI, Tauri)
diaryx_core = { version = "...", features = ["native-sync"] }
```

## Architecture

The CRDT system has several layers, from low-level to high-level:

```text
                    +-----------------+
                    |  SyncProtocol   |  Y-sync for Hocuspocus server
                    +--------+--------+
                             |
          +------------------+------------------+
          |                                     |
+---------v----------+             +-----------v---------+
|   WorkspaceCrdt    |             |    BodyDocManager   |
| (file hierarchy)   |             | (document content)  |
+---------+----------+             +-----------+---------+
          |                                     |
          |              +-------------+        |
          +------------->| CrdtStorage |<-------+
                         +------+------+
                                |
               +----------------+----------------+
               |                                 |
      +--------v--------+              +---------v--------+
      |  MemoryStorage  |              |  SqliteStorage   |
      +-----------------+              +------------------+
```

1. **Types** (`types.rs`): Core data structures like `FileMetadata` and `BinaryRef`
2. **Storage** (`storage.rs`): `CrdtStorage` trait for persisting CRDT state
3. **WorkspaceCrdt** (`workspace_doc.rs`): Y.Doc for workspace file hierarchy
4. **BodyDoc** (`body_doc.rs`): Per-file Y.Doc for document content
5. **BodyDocManager** (`body_doc_manager.rs`): Manages multiple BodyDocs
6. **SyncProtocol** (`sync.rs`): Y-sync protocol for Hocuspocus server
7. **HistoryManager** (`history.rs`): Version history and time travel

## Frontmatter timestamps

When converting frontmatter to `FileMetadata`, the `updated` property is parsed
as either a numeric timestamp (milliseconds) or an RFC3339/ISO8601 string, and
mapped to `modified_at`. When writing frontmatter back to disk, `updated` is
emitted as an RFC3339 string for readability.

CRDT metadata paths are canonicalized to workspace-relative form during ingest
(`part_of`, `contents`, `attachments`). Plain-canonical workspaces are handled
with a link-format hint so ambiguous plain links (`Folder/file.md`) resolve as
workspace-root references when appropriate.

## WorkspaceCrdt

Manages the workspace file hierarchy as a CRDT. Files are keyed by stable
document IDs (UUIDs), making renames and moves trivial property updates.

### Doc-ID Based Architecture

```rust,ignore
use diaryx_core::crdt::{WorkspaceCrdt, MemoryStorage, FileMetadata};
use std::sync::Arc;

let storage = Arc::new(MemoryStorage::new());
let workspace = WorkspaceCrdt::new(storage);

// Create a file with auto-generated UUID
let metadata = FileMetadata::with_filename(
    "my-note.md".to_string(),
    Some("My Note".to_string())
);
let doc_id = workspace.create_file(metadata).unwrap();

// Derive filesystem path from doc_id (walks parent chain)
let path = workspace.get_path(&doc_id); // Some("my-note.md")

// Find doc_id by path
let found_id = workspace.find_by_path(Path::new("my-note.md"));

// Renames and moves are trivial - doc_id is stable!
workspace.rename_file(&doc_id, "new-name.md").unwrap();
workspace.move_file(&doc_id, Some(&parent_doc_id)).unwrap();
```

### Legacy Path-Based API

For backward compatibility, path-based operations are still supported:

```rust,ignore
workspace.set_file("notes/my-note.md", metadata);
let meta = workspace.get_file("notes/my-note.md");
workspace.remove_file("notes/my-note.md");
```

### Migration

Workspaces using the legacy path-based format can be migrated:

```rust,ignore
if workspace.needs_migration() {
    let count = workspace.migrate_to_doc_ids().unwrap();
    println!("Migrated {} files", count);
}
```

## BodyDoc

Manages individual document content with collaborative editing support:

```rust,ignore
use diaryx_core::crdt::{BodyDoc, MemoryStorage};
use std::sync::Arc;

let storage = Arc::new(MemoryStorage::new());
let doc = BodyDoc::new("notes/my-note.md", storage);

// Body content operations
doc.set_body("# Hello World\n\nThis is my note.");
let content = doc.get_body();

// Collaborative editing
doc.insert_at(0, "Prefix: ");
doc.delete_range(0, 8);

// Frontmatter operations
doc.set_frontmatter("title", "My Note");
let title = doc.get_frontmatter("title");
doc.remove_frontmatter("audience");
```

Body sync observer registration and per-update logs are emitted at trace level
to avoid log spam during large downloads. Enable trace logging only when
diagnosing body sync issues.

BodyDoc sync observers are registered once per document. Repeated calls to
`set_sync_callback` for the same doc are ignored to avoid duplicate observers
and unnecessary overhead during bulk downloads.

When a file is renamed, an existing BodyDoc now emits sync updates using the
current doc name (not the originally captured name), so post-rename edits
continue syncing under the renamed path.

## Initial Sync Readiness

`SyncSession` marks initial sync as ready only after:

1. Workspace metadata handshake is complete, and
2. The active body-doc bootstrap set has completed.

This avoids reporting `synced` before file bodies are actually available.
For compatibility with older servers, the session still includes a bounded
fallback path when explicit `sync_complete` signaling is absent.

During active sync, the session now re-scans workspace file paths after
workspace metadata updates and queues missing body `SyncStep1` messages for any
newly discovered files. This prevents cases where metadata arrives first but
body bootstrap for those files is never initiated.

All path keys used by sync tracking (`body_synced`, echo maps, focus sets) are
canonicalized (`./` and leading `/` stripped, slash-normalized). This ensures
`README.md`, `./README.md`, and `/README.md` are treated as one logical file.

Rename reconciliation is now resilient to transient filesystem gaps: if a
rename is detected from CRDT metadata but both old/new paths are momentarily
missing on disk, the sync handler still emits a logical `FileRenamed` event and
suppresses re-materializing the old path. This keeps active-entry remapping and
sidebar state consistent during OPFS timing races.

Remote workspace renames now also migrate in-memory body-doc keys and all
related sync-tracking maps (`body_synced`, echo caches, state-vector caches,
focus set). This keeps post-rename body sync on a single canonical path instead
of splitting into "old path vs new path" streams.

`SqliteStorage::rename_doc` now treats rename as an overwrite migration:
destination doc/update rows are replaced by source rows in one transaction, and
missing-source renames are a no-op. This prevents stale destination state from
surviving a rename and causing duplicate body streams on reconnect.

For legacy path-key workspaces (where CRDT keys are file paths, not UUID doc
IDs), `CrdtFs::move_file` now forces delete+create key migration on rename
instead of filename-only metadata mutation. This ensures workspace metadata
path keys track the renamed canonical path, so remote clients can reconcile
rename events and body-doc routing on the same path.

## Integrity Audit and Self-heal

Before reporting fully synced, `SyncSession` runs an integrity audit over active
workspace files:

- Drops stale pending-body entries for files no longer in the workspace set.
- Requeues missing `SyncStep1` body bootstrap for active files that are not yet
  synced or not loaded in memory.
- When `write_to_disk` is enabled, checks whether each active file exists on
  disk; missing files are marked unsynced and requeued for bootstrap.

This bounded reconciliation pass prevents false `synced` states and repairs
cases where transient filesystem failures or timing races leave metadata and
body sync out of alignment.

## BodyDocManager

Manages multiple BodyDocs with lazy loading:

```rust,ignore
use diaryx_core::crdt::{BodyDocManager, MemoryStorage};
use std::sync::Arc;

let storage = Arc::new(MemoryStorage::new());
let manager = BodyDocManager::new(storage);

// Get or create a BodyDoc for a file
let doc = manager.get_or_create("notes/my-note.md");
doc.set_body("Content here");

// Check if a doc exists
if manager.has_doc("notes/my-note.md") {
    // ...
}

// Remove a doc from the manager
manager.remove_doc("notes/my-note.md");
```

## Sync Protocol

The sync module implements Y-sync protocol for real-time collaboration with
Hocuspocus or other Y.js-compatible servers:

```rust,ignore
use diaryx_core::crdt::{WorkspaceCrdt, MemoryStorage};
use std::sync::Arc;

let storage = Arc::new(MemoryStorage::new());
let workspace = WorkspaceCrdt::new("workspace", storage);

// Get sync state for initial handshake
let state_vector = workspace.get_sync_state();

// Apply remote update from server
let remote_update: Vec<u8> = /* from WebSocket */;
workspace.apply_update(&remote_update);

// Encode state for sending to server
let full_state = workspace.encode_state();

// Encode incremental update since a state vector
let diff = workspace.encode_state_as_update(&remote_state_vector);
```

## Version History

All local changes are automatically recorded, enabling version history and
time travel:

```rust,ignore
use diaryx_core::crdt::{WorkspaceCrdt, MemoryStorage, HistoryEntry};
use std::sync::Arc;

let storage = Arc::new(MemoryStorage::new());
let workspace = WorkspaceCrdt::new("workspace", storage.clone());

// Make some changes
workspace.set_file("file1.md", metadata1);
workspace.set_file("file2.md", metadata2);

// Get version history
let history: Vec<HistoryEntry> = storage.get_all_updates("workspace").unwrap();
for entry in &history {
    println!("Version {} at {:?}: {} bytes",
             entry.version, entry.timestamp, entry.update.len());
}

// Time travel to a specific version
workspace.restore_to_version(1);
```

## Storage Backends

### MemoryStorage

In-memory storage for WASM/web and testing:

```rust,ignore
use diaryx_core::crdt::MemoryStorage;
use std::sync::Arc;

let storage = Arc::new(MemoryStorage::new());
```

### SqliteStorage

Persistent storage using SQLite (requires `crdt-sqlite` feature, native only):

```rust,ignore
use diaryx_core::crdt::SqliteStorage;
use std::sync::Arc;

let storage = Arc::new(SqliteStorage::open("crdt.db").unwrap());
```

`SqliteStorage` recovers from poisoned connection mutexes (for example, after a
panic in another runtime task while holding the lock) and logs a warning
instead of panicking future CRDT operations. This prevents reconnect loops in
long-running sync servers from crashing due to lock poison propagation.

Compaction now handles `keep_updates = 0` explicitly (keep no incremental
updates, retain only the reconstructed snapshot) without integer underflow.
This avoids panic-on-compact in sync server maintenance paths.

## Integration with Command API

CRDT operations are available through the unified command API for WASM/Tauri:

```rust,ignore
use diaryx_core::{Diaryx, Command, CommandResult};

let diaryx = Diaryx::with_crdt(fs, crdt_storage);

// Execute CRDT commands
let result = diaryx.execute(Command::GetSyncState {
    doc_type: "workspace".to_string(),
    doc_name: None,
});

let result = diaryx.execute(Command::SetFileMetadata {
    path: "notes/my-note.md".to_string(),
    metadata: file_metadata,
});

let result = diaryx.execute(Command::GetHistory {
    doc_type: "workspace".to_string(),
    doc_name: None,
});
```

Link parser operations are also available through the command API via
`Command::LinkParser`, enabling frontend callers to parse/canonicalize/format/
convert links using the same Rust logic as core filesystem and workspace code.

## Sync Client

### SyncSession (all platforms)

`SyncSession` (`sync_session.rs`) is a message-driven protocol handler shared
by all platforms (native and WASM). It encapsulates the entire sync protocol:
message framing, binary routing, control message parsing, handshake state
machine, and body SyncStep1 loop.

```text
         Platform-specific layer
         ┌──────────────┬──────────────┐
         │ SyncClient   │ WasmSyncCli  │  ← owns transport, reconnection
         │ (tokio)      │ (JS bridge)  │
         └──────┬───────┴──────┬───────┘
                │              │
                └──────┬───────┘
                       ▼
              ┌──────────────────┐
              │   SyncSession    │  ← message-driven protocol handler
              │  (diaryx_core)   │    handshake, routing, framing, control messages
              └────────┬─────────┘
                       ▼
              ┌──────────────────┐
              │ RustSyncManager  │  ← Y-sync protocol, CRDT operations
              └──────────────────┘
```

`SyncSession` uses an inject/process pattern: the platform feeds `IncomingEvent`s
(Connected, BinaryMessage, TextMessage, Disconnected, etc.) and receives back a
`Vec<SessionAction>` (SendBinary, SendText, Emit, DownloadSnapshot) that the
platform executes using its own transport.

Shared types (`sync_types.rs`) define `SyncEvent` and `SyncStatus`, available
on all platforms without feature gates.

### Native (CLI, Tauri)

`SyncClient` (requires `native-sync` feature) wraps `SyncSession` with a
pluggable WebSocket transport, reconnection with exponential backoff, and
ping/keepalive.

`SyncClient` is generic over `C: TransportConnector`, allowing different
WebSocket backends. The built-in `TokioConnector` uses `tokio-tungstenite`
with rustls TLS. Custom connectors can be implemented for platform-specific
networking (e.g., Apple's `URLSessionWebSocketTask` on iOS).

Frontends implement `SyncEventHandler` to receive status changes, progress
updates, and file change notifications.

```rust,ignore
use diaryx_core::crdt::{
    SyncClient, SyncClientConfig, SyncEvent, SyncEventHandler, TokioConnector,
};

struct MyHandler;
impl SyncEventHandler for MyHandler {
    fn on_event(&self, event: SyncEvent) {
        match event {
            SyncEvent::StatusChanged(status) => println!("Status: {:?}", status),
            SyncEvent::Progress { completed, total } => println!("{}/{}", completed, total),
            _ => {}
        }
    }
}

let config = SyncClientConfig {
    server_url: "https://sync.example.com".to_string(),
    workspace_id: "my-workspace".to_string(),
    auth_token: Some("token".to_string()),
    reconnect: Default::default(),
};

let client = SyncClient::new(config, sync_manager, Arc::new(MyHandler), TokioConnector);

// Persistent sync with reconnection (Tauri, CLI `sync start`)
client.run_persistent(running).await;

// One-shot push/pull (CLI `sync push`, `sync pull`)
let stats = client.run_one_shot().await?;
```

#### Custom Transport

To use a different WebSocket backend, implement `TransportConnector` and
`SyncTransport`:

```rust,ignore
use diaryx_core::crdt::{TransportConnector, SyncTransport, TransportError, WsMessage};

struct MyConnector;

#[async_trait::async_trait]
impl TransportConnector for MyConnector {
    type Transport = MyTransport;
    async fn connect(&self, url: &str) -> Result<MyTransport, TransportError> {
        // ... establish connection
    }
}

struct MyTransport { /* ... */ }

#[async_trait::async_trait]
impl SyncTransport for MyTransport {
    async fn send_binary(&mut self, data: Vec<u8>) -> Result<(), TransportError> { /* ... */ }
    async fn send_text(&mut self, text: String) -> Result<(), TransportError> { /* ... */ }
    async fn send_ping(&mut self) -> Result<(), TransportError> { /* ... */ }
    async fn recv(&mut self) -> Option<Result<WsMessage, TransportError>> { /* ... */ }
    async fn close(&mut self) -> Result<(), TransportError> { /* ... */ }
}

// Then use with SyncClient:
let client = SyncClient::new(config, sync_manager, handler, MyConnector);
```

### WASM (Web)

`WasmSyncClient` (in `diaryx_wasm`) wraps `SyncSession` for the browser.
The WebSocket lives on the main thread; `WasmSyncClient` lives in a Web Worker.
The main thread injects messages via `onBinaryMessage()`/`onTextMessage()`, then
drains outgoing data via `syncDrain()` (a batched poll that returns all queued
binary, text, and JSON event messages in a single worker round-trip).

TypeScript's `UnifiedSyncTransport` handles only WebSocket lifecycle, reconnection,
and snapshot download (HTTP). All protocol logic (framing, routing, handshake)
is handled by `SyncSession` in Rust.

## Git-Backed Version History

The `git` submodule (requires `git` feature, native only) provides commit, compact,
and rebuild operations that use git as the authoritative history store.

```toml
[dependencies]
diaryx_core = { version = "...", features = ["git"] }
```

### Architecture

After each commit, CRDT update logs can be compacted since the workspace state
is captured in git. Git becomes the authoritative history; CRDT becomes a thin
sync buffer.

- **Materialize** (`materialize.rs`): Extracts CRDT state into file content
  (frontmatter YAML + body markdown).
- **Validate** (`sanity.rs`): Checks for empty body docs, broken parent chains,
  orphan body docs, missing children.
- **Self-Healing** (`self_healing.rs`): Tracks consecutive validation failures.
  After 3 failures, recommends CRDT rebuild from git.
- **Commit** (`git/commit.rs`): Materializes workspace, validates, builds git tree,
  creates commit, compacts all CRDT docs.
- **Rebuild** (`git/rebuild.rs`): Clears CRDT state and repopulates from a git commit.
  Used for self-healing and restore operations.
- **Repo** (`git/repo.rs`): Creates and opens Standard or Bare git repositories.

### Usage

```rust,ignore
use diaryx_core::crdt::git::{
    CommitOptions, commit_workspace, init_repo, open_repo, RepoKind,
    rebuild_crdt_from_git,
};
use diaryx_core::crdt::self_healing::HealthTracker;

// Open or init a repo
let repo = open_repo(workspace_root)
    .unwrap_or_else(|_| init_repo(workspace_root, RepoKind::Standard).unwrap());

// Commit workspace state
let mut tracker = HealthTracker::new();
let result = commit_workspace(
    &storage, &workspace, &body_docs, &repo,
    workspace_id, &CommitOptions::default(), &mut tracker,
).unwrap();

// Rebuild CRDT from git (self-healing)
let file_count = rebuild_crdt_from_git(&repo, &storage, workspace_id, None).unwrap();
```

## Relationship to Cloud Sync

The CRDT module handles **real-time collaboration** (character-by-character edits),
while the [`sync`](../sync/README.md) module handles **file-level cloud sync**
(S3, Google Drive). They work together:

- CRDT tracks fine-grained changes within documents
- Cloud sync uploads/downloads whole files to/from storage providers
- Both use the same `WorkspaceCrdt` metadata for consistency

## End-to-End Parity Tests

The integration suite at `crates/diaryx_core/tests/crdt_sync_parity.rs`
asserts that markdown files on disk and CRDT state stay in lockstep:

- local write/move/delete operations through the decorated filesystem
- remote workspace/body updates with disk side effects (including rename/delete)

For each active file, the tests verify:

- path set parity (disk files == active CRDT files)
- frontmatter metadata parity (parsed file metadata == workspace CRDT metadata)
- body parity (parsed file body == body-doc CRDT content)

Run this suite with:

```bash
cargo test -p diaryx_core --features crdt --test crdt_sync_parity
```
