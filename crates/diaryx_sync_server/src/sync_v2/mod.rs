//! Y-sync v2 implementation using siphonophore.
//!
//! This module provides the sync backend using the siphonophore library,
//! with cloud-specific authentication and namespace management built
//! on top of the shared `diaryx_sync` protocol engine.
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
mod server;
mod store;

// Re-export from diaryx_sync (shared protocol types)
pub use diaryx_sync::protocol::{
    AuthenticatedUser, ClientControlMessage, DocType, HandshakeState, ManifestFileEntry,
    ServerControlMessage,
};

// Re-export from local modules
pub use generic_hook::GenericNamespaceSyncHook;
pub use handshake::{ConnectionContext, handle_control_message, perform_handshake};
pub use server::{SyncV2Server, SyncV2State};
pub use store::StorageCache;
