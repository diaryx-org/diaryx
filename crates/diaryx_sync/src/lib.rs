//! # Diaryx Sync Engine
//!
//! Sync engine for Diaryx, providing CRDT types, sync protocol, and server infrastructure.
//!
//! ## Feature flags
//!
//! - **default** — CRDT types and protocol only (WASM-compatible)
//! - **sqlite** — SQLite-backed CRDT storage
//! - **server** — Siphonophore hooks, axum WebSocket server, StorageCache
//! - **native-sync** — Native sync transport (tokio-tungstenite)
//! - **git** — Git-backed version history

// ==================== CRDT core (always available, WASM-compatible) ====================

mod body_doc;
mod body_doc_manager;
pub mod control_message;
mod history;
pub mod materialize;
mod memory_storage;
pub mod sanity;
pub mod self_healing;
mod sync_handler;
mod sync_manager;
mod sync_protocol;
mod sync_session;
mod sync_types;
mod workspace_doc;

// Filesystem decorators (CRDT-aware FS layer)
mod crdt_fs;
mod decorator_stack;

// ==================== Feature-gated modules ====================

// SqliteStorage: re-exported from diaryx_core during transition period.
// The local copy (sqlite_storage.rs) is kept but not compiled.

#[cfg(all(not(target_arch = "wasm32"), feature = "git"))]
pub mod git;

// Native sync client (CLI, Tauri) — not available on WASM
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
mod sync_client;
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
mod tokio_transport;
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
mod transport;

// Server infrastructure (siphonophore hooks, axum WebSocket)
#[cfg(feature = "server")]
pub mod hooks;
#[cfg(feature = "server")]
pub mod local;
#[cfg(feature = "server")]
pub mod protocol;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
pub mod storage;

// ==================== Re-exports ====================
//
// During the transition period, shared traits and types are re-exported from
// diaryx_core::crdt so that all crates use the same trait identity.
// Once diaryx_core::crdt is removed (Step 6), these will become primary definitions.

// Core types — re-export from diaryx_core::crdt for trait coherence
pub use diaryx_core::crdt::{BinaryRef, CrdtUpdate, FileMetadata, UpdateOrigin};
pub use diaryx_core::crdt::{CrdtStorage, StorageResult};

// Body documents
pub use body_doc::BodyDoc;
pub use body_doc_manager::BodyDocManager;

// Workspace CRDT
pub use workspace_doc::WorkspaceCrdt;

// Storage implementations
#[cfg(all(not(target_arch = "wasm32"), feature = "sqlite"))]
pub use diaryx_core::crdt::SqliteStorage;
pub use memory_storage::MemoryStorage;

// Sync protocol
pub use sync_protocol::{
    BodySyncProtocol, DocIdKind, SyncMessage, SyncProtocol, format_body_doc_id,
    format_workspace_doc_id, frame_body_message, frame_message_v2, parse_doc_id,
    unframe_body_message, unframe_message_v2,
};

// Sync handler + manager
pub use sync_handler::{GuestConfig, SyncHandler};
pub use sync_manager::{BodySyncResult, RustSyncManager, SyncMessageResult};

// History, materialization, validation, self-healing
pub use history::{ChangeType, FileDiff, HistoryEntry, HistoryManager};
pub use materialize::{
    MaterializationResult, MaterializedFile, materialize_workspace, parse_snapshot_markdown,
};
pub use sanity::{IssueKind, SanityIssue, SanityReport, validate_workspace};
pub use self_healing::{HealingAction, HealthTracker};

// Shared sync types (all platforms)
pub use control_message::ControlMessage;
pub use sync_session::{IncomingEvent, SessionAction, SyncSession};
pub use sync_types::{SyncEvent, SyncSessionConfig, SyncStatus};

// CrdtFs and decorator stack
pub use crdt_fs::CrdtFs;
pub use decorator_stack::DecoratedFsBuilder;

// Native sync client re-exports
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
pub use sync_client::{ReconnectConfig, SyncClient, SyncClientConfig, SyncEventHandler, SyncStats};
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
pub use tokio_transport::{TokioConnector, TokioTransport};
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
pub use transport::{SyncTransport, TransportConnector, TransportError, WsMessage};
