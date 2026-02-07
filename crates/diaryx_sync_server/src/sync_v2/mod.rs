//! Y-sync v2 implementation using siphonophore.
//!
//! This module provides the sync backend using the siphonophore library.
//!
//! ## Siphonophore Features
//!
//! - Native document multiplexing over a single WebSocket
//! - Actor-based document management via Kameo
//! - Hook system for authentication, persistence, and change handling
//!
//! ## Document Namespacing
//!
//! Documents use a namespacing scheme to distinguish workspace metadata from
//! file body content:
//!
//! - `workspace:<id>` - Workspace metadata CRDT
//! - `body:<workspace_id>/<path>` - File body content CRDT
//!
//! ## Wire Protocol
//!
//! Siphonophore uses a slightly different wire format than the legacy v1:
//! - `[doc_id_len: u8][doc_id: bytes][yjs_payload: bytes]`
//!
//! ## Hook-Based Features
//!
//! These features are implemented via siphonophore hooks rather than native support:
//! - Files-Ready handshake: via `on_before_sync` + `on_control_message`
//! - Peer join/leave notifications: via `on_peer_joined`/`on_peer_left` + `Handle::broadcast_text`
//! - Session joined confirmation: via `on_before_sync` `SendMessages` for guests
//!
//! Not yet supported:
//! - Focus tracking broadcast (focus/unfocus messages are received but not relayed)

mod handshake;
mod hooks;
mod server;
mod store;

pub use handshake::{
    ClientControlMessage, ConnectionContext, HandshakeState, ManifestFileEntry,
    ServerControlMessage, handle_control_message, perform_handshake,
};
pub use hooks::{AuthenticatedUser, DiaryxHook, DocType};
pub use server::{SyncV2Server, SyncV2State};
pub use store::{
    SnapshotError, SnapshotImportMode, SnapshotImportResult, StorageCache, WorkspaceStore,
};
