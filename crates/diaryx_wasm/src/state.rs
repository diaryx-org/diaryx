//! Global filesystem state management.

use std::cell::RefCell;

use diaryx_core::fs::InMemoryFileSystem;

thread_local! {
    static FILESYSTEM: RefCell<InMemoryFileSystem> = RefCell::new(InMemoryFileSystem::new());
}

/// Execute a closure with read access to the global filesystem.
pub fn with_fs<F, R>(f: F) -> R
where
    F: FnOnce(&InMemoryFileSystem) -> R,
{
    FILESYSTEM.with(|fs| f(&fs.borrow()))
}

/// Execute a closure with access to the global filesystem.
///
/// Note: Uses immutable borrow because `InMemoryFileSystem` uses internal
/// mutability (`RefCell<HashMap>`). The `FileSystem` trait is implemented
/// for `&InMemoryFileSystem`, not `&mut InMemoryFileSystem`.
pub fn with_fs_mut<F, R>(f: F) -> R
where
    F: FnOnce(&InMemoryFileSystem) -> R,
{
    FILESYSTEM.with(|fs| f(&fs.borrow()))
}

/// Replace the entire filesystem with a new one.
///
/// Use this for operations that need to replace the whole filesystem
/// (e.g., loading from backup, initial load).
pub fn replace_fs(new_fs: InMemoryFileSystem) {
    FILESYSTEM.with(|fs| *fs.borrow_mut() = new_fs);
}

/// Reset the filesystem to a fresh state (for testing).
#[cfg(test)]
pub fn reset_filesystem() {
    FILESYSTEM.with(|fs| *fs.borrow_mut() = InMemoryFileSystem::new());
}
