//! Y-sync v2 implementation using siphonophore.
//!
//! This module provides the sync backend using the siphonophore library,
//! with cloud-specific authentication and namespace management built
//! on top of the platform-agnostic sync primitives in `diaryx_server::sync`.
//!
//! ## Document Namespacing
//!
//! Documents use a namespacing scheme to distinguish workspace metadata from
//! file body content:
//!
//! - `workspace:<id>` - Workspace metadata CRDT
//! - `body:<workspace_id>/<path>` - File body content CRDT
//!
//! ## Hook-Based Features
//!
//! These features are implemented via siphonophore hooks rather than native support:
//! - Files-Ready handshake: via `on_before_sync` + `on_control_message`
//! - Peer join/leave notifications: via `on_peer_joined`/`on_peer_left` + `Handle::broadcast_text`
//! - Session joined confirmation: via `on_before_sync` `SendMessages` for guests

mod generic_hook;
mod handshake;
mod hooks;
mod server;
mod sqlite_storage;
mod storage_cache;

// Re-export shared protocol types (now provided by diaryx_server::sync::protocol)
pub use diaryx_server::sync::protocol::{
    AuthenticatedUser, ClientControlMessage, DocType, HandshakeState, ManifestFileEntry,
    ServerControlMessage,
};

// Re-export from local modules
pub use generic_hook::GenericNamespaceSyncHook;
pub use handshake::{ConnectionContext, handle_control_message, perform_handshake};
pub use hooks::{DiarySyncHook, SyncHookDelegate};
pub use server::{SyncV2Server, SyncV2State};
pub use storage_cache::StorageCache;
