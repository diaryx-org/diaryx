//! CRDT-based sync primitives shared by Diaryx server adapters
//! (native siphonophore-based server and Cloudflare Workers DO).
//!
//! This module is intentionally platform-agnostic: no tokio, no axum,
//! no rusqlite, no siphonophore. Native-specific impls live in
//! `diaryx_sync_server`. Wasm32 adapters (Cloudflare) get everything here.

mod crdt_storage;
mod doc_manager;
mod memory_storage;
pub mod protocol;
mod time;
mod workspace_doc;

pub use crdt_storage::{CrdtStorage, CrdtUpdate, StorageResult, UpdateOrigin};
pub use doc_manager::SyncDocManager;
pub use memory_storage::MemoryStorage;
pub use workspace_doc::WorkspaceCrdt;
