//! Filesystem abstraction.
//!
//! Diaryx-specific re-export shim around the [`crossfs`] crate plus the
//! Diaryx event-emitting decorator. New code should depend on `crossfs`
//! directly.
//!
//! # Migration
//!
//! The `FileSystem` (sync) and `AsyncFileSystem` (async) traits live in
//! `crossfs` and have method names aligned with [`std::fs`] / [`tokio::fs`].
//! Legacy method names (`write_file`, `delete_file`, `move_file`,
//! `read_binary`, `is_dir`, `list_files`, `get_modified_time`, …) remain on
//! the trait as `#[deprecated]` aliases so existing call sites continue to
//! compile during the transition.
//!
//! ```ignore
//! use diaryx_core::fs::{AsyncFileSystem, InMemoryFileSystem, SyncToAsyncFs};
//!
//! let fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
//! fs.write(std::path::Path::new("file.md"), b"hello").await?;
//! ```

mod callback_registry;
mod event_fs;
mod events;

#[allow(deprecated)]
pub use crossfs::FileSystem;
pub use crossfs::{AsyncFileSystem, BoxFuture, DirEntry, FileType, Metadata, SyncToAsyncFs};

/// Backward-compatible alias for [`crossfs::InMemoryFs`].
pub use crossfs::InMemoryFs as InMemoryFileSystem;

pub use callback_registry::{CallbackRegistry, EventCallback, SubscriptionId};
pub use event_fs::EventEmittingFs;
pub use events::FileSystemEvent;

/// Returns true if the path refers to a temporary file created by the
/// metadata writer's safe-write process (`.tmp`, `.bak`, `.swap` extensions).
/// Excluded from workspace trees and sync.
///
/// This is a Diaryx convention and is not part of `crossfs`.
pub fn is_temp_file(path: &str) -> bool {
    path.ends_with(".tmp") || path.ends_with(".bak") || path.ends_with(".swap")
}

/// Test helper: drive a future to completion on the current thread.
///
/// Used by `diaryx_core`'s in-tree tests for sync wrapping of async APIs.
/// Production code should use a real async runtime.
#[cfg(test)]
pub(crate) fn block_on_test<F: std::future::Future>(f: F) -> F::Output {
    futures_lite::future::block_on(f)
}

/// Compute the SHA-256 hash of a file (asynchronously) and return it as
/// lowercase hex.
///
/// Diaryx-specific helper. `crossfs` does not include hashing in its trait
/// surface; consumers that need it should depend on `sha2` directly or use
/// this helper through `diaryx_core`.
pub async fn hash_file<F: AsyncFileSystem + ?Sized>(
    fs: &F,
    path: &std::path::Path,
) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};

    let bytes = fs.read(path).await?;
    let hash = Sha256::digest(&bytes);
    Ok(hash.iter().fold(String::with_capacity(64), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
        s
    }))
}
