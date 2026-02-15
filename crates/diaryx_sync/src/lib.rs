//! # Diaryx Sync Protocol Engine
//!
//! Shared sync protocol layer for Diaryx, built on [`siphonophore`].
//!
//! This crate provides:
//! - **Protocol types**: Document type parsing, control messages, handshake state
//! - **Storage**: Per-workspace CRDT storage cache
//! - **Hooks**: Pluggable sync hook system via [`SyncHookDelegate`] trait
//! - **Local server**: Lightweight server for CLI-based web editing
//!
//! Used by both `diaryx_sync_server` (cloud) and `diaryx` CLI (local editing).

pub mod hooks;
pub mod local;
pub mod protocol;
pub mod server;
pub mod storage;
