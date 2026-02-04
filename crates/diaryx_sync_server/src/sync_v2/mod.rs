//! Y-sync v2 implementation using siphonophore.
//!
//! This module provides an alternative sync backend using the siphonophore
//! library for simpler use cases. It's designed to coexist with the existing
//! `/sync` endpoint during migration.
//!
//! ## When to Use sync_v2
//!
//! **Use `/sync2` (siphonophore) for:**
//! - Simple document sync without Files-Ready handshake
//! - Use cases where native multiplexing is beneficial
//! - Testing and evaluation
//!
//! **Use `/sync` (v1) for:**
//! - Full feature support (Files-Ready handshake, focus tracking)
//! - Session/guest collaboration with peer events
//! - Production workloads until sync_v2 is fully tested
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
//! Siphonophore uses a slightly different wire format than v1:
//! - `[doc_id_len: u8][doc_id: bytes][yjs_payload: bytes]`
//!
//! This is similar to v1 format but uses a single byte for length
//! instead of varuint encoding.
//!
//! ## Limitations
//!
//! Siphonophore doesn't support:
//! - Custom pre-sync handshakes (Files-Ready protocol)
//! - Custom control messages beyond Leave/Save
//! - Focus tracking or peer events
//!
//! These features require the v1 `/sync` endpoint.

mod handshake;
mod hooks;
mod server;

pub use handshake::{
    ClientControlMessage, ConnectionContext, HandshakeState, ManifestFileEntry,
    ServerControlMessage, handle_control_message, perform_handshake,
};
pub use hooks::{AuthenticatedUser, DiaryxHook, DocType};
pub use server::{SyncV2Server, SyncV2State};
