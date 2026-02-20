#![doc = include_str!(concat!(env!("OUT_DIR"), "/crdt_README.md"))]

mod body_doc;
mod body_doc_manager;
#[cfg(all(not(target_arch = "wasm32"), feature = "git"))]
pub mod git;
mod history;
pub mod materialize;
mod memory_storage;
pub mod sanity;
pub mod self_healing;
#[cfg(all(not(target_arch = "wasm32"), feature = "crdt-sqlite"))]
mod sqlite_storage;
mod storage;
mod sync;
mod sync_handler;
mod sync_manager;
mod types;
mod workspace_doc;

// Protocol types shared across all platforms (native + WASM)
pub mod control_message;
mod sync_session;
mod sync_types;

// Native sync client modules (CLI, Tauri) — not available on WASM
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
mod sync_client;
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
mod tokio_transport;
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
mod transport;

pub use body_doc::BodyDoc;
pub use body_doc_manager::BodyDocManager;
pub use history::{ChangeType, FileDiff, HistoryEntry, HistoryManager};
pub use memory_storage::MemoryStorage;
#[cfg(all(not(target_arch = "wasm32"), feature = "crdt-sqlite"))]
pub use sqlite_storage::SqliteStorage;
pub use storage::{CrdtStorage, StorageResult};
pub use sync::{
    BodySyncProtocol, DocIdKind, SyncMessage, SyncProtocol, format_body_doc_id,
    format_workspace_doc_id, frame_body_message, frame_message_v2, parse_doc_id,
    unframe_body_message, unframe_message_v2,
};
pub use sync_handler::{GuestConfig, SyncHandler};
pub use sync_manager::{BodySyncResult, RustSyncManager, SyncMessageResult};
pub use types::{BinaryRef, CrdtUpdate, FileMetadata, UpdateOrigin};
pub use workspace_doc::WorkspaceCrdt;

// Re-exports for materialization, validation, and self-healing
pub use materialize::{
    MaterializationResult, MaterializedFile, materialize_workspace, parse_snapshot_markdown,
};
pub use sanity::{IssueKind, SanityIssue, SanityReport, validate_workspace};
pub use self_healing::{HealingAction, HealthTracker};

// Shared sync types (all platforms)
pub use control_message::ControlMessage;
pub use sync_session::{IncomingEvent, SessionAction, SyncSession};
pub use sync_types::{SyncEvent, SyncSessionConfig, SyncStatus};

// Native sync client re-exports
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
pub use sync_client::{ReconnectConfig, SyncClient, SyncClientConfig, SyncEventHandler, SyncStats};
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
pub use tokio_transport::{TokioConnector, TokioTransport};
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
pub use transport::{SyncTransport, TransportConnector, TransportError, WsMessage};
