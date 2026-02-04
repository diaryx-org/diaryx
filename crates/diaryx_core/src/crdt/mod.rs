#![doc = include_str!(concat!(env!("OUT_DIR"), "/crdt_README.md"))]

mod body_doc;
mod body_doc_manager;
mod history;
mod memory_storage;
#[cfg(all(not(target_arch = "wasm32"), feature = "crdt-sqlite"))]
mod sqlite_storage;
mod storage;
mod sync;
mod sync_client;
mod sync_handler;
mod sync_manager;
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
mod tokio_transport;
mod transport;
mod types;
mod workspace_doc;

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
pub use sync_client::{
    OutgoingSender, OutgoingSyncMessage, SyncClient, SyncClientConfig, SyncEvent, SyncEventBridge,
    SyncEventCallback, create_sync_event_bridge,
};
pub use sync_handler::{GuestConfig, SyncHandler};
pub use sync_manager::{BodySyncResult, RustSyncManager, SyncMessageResult};
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
pub use tokio_transport::TokioTransport;
pub use transport::{ConnectionStatus, MessageCallback, StatusCallback, SyncConfig, SyncTransport};
pub use types::{BinaryRef, CrdtUpdate, FileMetadata, UpdateOrigin};
pub use workspace_doc::WorkspaceCrdt;
