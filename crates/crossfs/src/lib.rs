//! `crossfs` — a cross-platform filesystem abstraction.
//!
//! Mirrors the surface of [`std::fs`] / [`tokio::fs`], with backends for the
//! native OS, in-memory testing, and browser storage (OPFS, IndexedDB, the
//! File System Access API).
//!
//! See [`AsyncFileSystem`] for the primary trait. [`InMemoryFs`] is always
//! available and is useful for tests and sandboxes. Backend structs
//! (`StdFs`, `OpfsFs`, etc.) live in this crate behind cargo features and
//! will land in subsequent commits.
//!
//! # Status
//!
//! Pre-0.1, in-tree only. The API is being aligned with `std::fs` naming and
//! is not yet stable. Legacy method names (`write_file`, `delete_file`,
//! `move_file`, `read_binary`, `is_dir`, …) remain on the trait as
//! `#[deprecated]` aliases so existing call sites in the Diaryx workspace
//! continue to compile.

#![deny(rust_2018_idioms)]

mod adapter;
pub mod error;
mod memory;
mod metadata;
mod traits;

pub use adapter::SyncToAsyncFs;
pub use memory::InMemoryFs;
pub use metadata::{DirEntry, FileType, Metadata};
#[allow(deprecated)]
pub use traits::FileSystem;
pub use traits::{AsyncFileSystem, BoxFuture};
