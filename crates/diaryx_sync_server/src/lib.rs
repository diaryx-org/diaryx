#![doc = include_str!(concat!(env!("OUT_DIR"), "/README.md"))]

//! Diaryx Sync Server
//!
//! A multi-device sync server for Diaryx with magic link authentication.
//!
//! ## Features
//!
//! - **Magic link authentication**: Passwordless login via email
//! - **Real-time sync**: WebSocket-based Y-sync protocol using diaryx_core's CRDT
//! - **Multi-device support**: Track and manage connected devices
//! - **Persistent storage**: SQLite-based storage for user data and CRDT state
//! - **Generic namespace API**: Namespace-scoped object store, audiences, and sessions

pub mod adapters;
pub mod auth;
pub mod blob_store;
pub mod config;
pub mod db;
pub mod email;
pub mod handlers;
pub mod proxy_adapters;
pub mod rate_limit;
pub mod sync_v2;
pub mod testing;
pub mod tokens;

pub use config::Config;
