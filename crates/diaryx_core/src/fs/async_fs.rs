//! Async filesystem abstraction module.
//!
//! This module provides the `AsyncFileSystem` trait for abstracting async filesystem operations,
//! allowing different implementations for native and WASM targets.
//!
//! This is particularly useful for:
//! - WASM environments where JavaScript APIs (like IndexedDB) are inherently async
//! - Native environments using async runtimes like tokio
//! - Code that needs to await filesystem operations

use std::future::Future;
use std::io::Result;
use std::path::{Path, PathBuf};
use std::pin::Pin;

/// A boxed future that is Send.
/// Used for recursive async methods where we need type erasure.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Async abstraction over filesystem operations.
///
/// This trait mirrors `FileSystem` but with async methods, making it suitable
/// for environments where filesystem operations may be asynchronous (e.g., WASM
/// with IndexedDB, or native code using async I/O).
///
/// # Example
///
/// ```ignore
/// use diaryx_core::fs::AsyncFileSystem;
///
/// async fn example<F: AsyncFileSystem>(fs: &F) {
///     let content = fs.read_to_string(Path::new("file.md")).await.unwrap();
///     fs.write_file(Path::new("output.md"), &content).await.unwrap();
/// }
/// ```
pub trait AsyncFileSystem: Send + Sync {
    /// Reads the file content as a string.
    fn read_to_string(&self, path: &Path) -> impl Future<Output = Result<String>> + Send;

    /// Overwrites an existing file with new content.
    fn write_file(&self, path: &Path, content: &str) -> impl Future<Output = Result<()>> + Send;

    /// Creates a file ONLY if it doesn't exist.
    /// Should return an error if file exists.
    fn create_new(&self, path: &Path, content: &str) -> impl Future<Output = Result<()>> + Send;

    /// Deletes a file.
    fn delete_file(&self, path: &Path) -> impl Future<Output = Result<()>> + Send;

    /// Finds markdown files in a folder.
    fn list_md_files(&self, dir: &Path) -> impl Future<Output = Result<Vec<PathBuf>>> + Send;

    /// Checks if a file or directory exists.
    fn exists(&self, path: &Path) -> impl Future<Output = bool> + Send;

    /// Creates a directory and all parent directories.
    fn create_dir_all(&self, path: &Path) -> impl Future<Output = Result<()>> + Send;

    /// Checks if a path is a directory.
    fn is_dir(&self, path: &Path) -> impl Future<Output = bool> + Send;

    /// Move/rename a file from `from` to `to`.
    ///
    /// Implementations should treat this as an atomic-ish move when possible,
    /// and should error if the source does not exist or if the destination already exists.
    fn move_file(&self, from: &Path, to: &Path) -> impl Future<Output = Result<()>> + Send;

    // ==================== Binary File Methods ====================
    // These methods support binary files (attachments) without base64 overhead

    /// Read binary file content.
    fn read_binary(&self, path: &Path) -> impl Future<Output = Result<Vec<u8>>> + Send {
        async move {
            // Default implementation: read as string and convert to bytes
            self.read_to_string(path).await.map(|s| s.into_bytes())
        }
    }

    /// Write binary content to a file.
    fn write_binary(&self, _path: &Path, _content: &[u8]) -> impl Future<Output = Result<()>> + Send {
        async move {
            // Default implementation: not supported
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Binary write not supported",
            ))
        }
    }

    /// List all files in a directory (not recursive).
    fn list_files(&self, _dir: &Path) -> impl Future<Output = Result<Vec<PathBuf>>> + Send {
        async move {
            // Default: return empty
            Ok(vec![])
        }
    }

    /// Recursively list all markdown files in a directory and its subdirectories.
    ///
    /// This method returns a boxed future to allow for recursive calls.
    fn list_md_files_recursive(&self, dir: &Path) -> BoxFuture<'_, Result<Vec<PathBuf>>>
    where
        Self: Sized,
    {
        let dir = dir.to_path_buf();
        Box::pin(async move {
            let mut all_files = self.list_md_files(&dir).await?;

            // Get subdirectories and recurse
            if let Ok(entries) = self.list_files(&dir).await {
                for entry in entries {
                    if self.is_dir(&entry).await {
                        if let Ok(subdir_files) = self.list_md_files_recursive(&entry).await {
                            all_files.extend(subdir_files);
                        }
                    }
                }
            }

            Ok(all_files)
        })
    }

    /// Recursively list ALL files and directories in a directory.
    ///
    /// This method returns a boxed future to allow for recursive calls.
    fn list_all_files_recursive(&self, dir: &Path) -> BoxFuture<'_, Result<Vec<PathBuf>>>
    where
        Self: Sized,
    {
        let dir = dir.to_path_buf();
        Box::pin(async move {
            let mut all_entries = Vec::new();

            if let Ok(entries) = self.list_files(&dir).await {
                for entry in entries {
                    all_entries.push(entry.clone());
                    if self.is_dir(&entry).await {
                        if let Ok(subdir_entries) = self.list_all_files_recursive(&entry).await {
                            all_entries.extend(subdir_entries);
                        }
                    }
                }
            }

            Ok(all_entries)
        })
    }
}

// ============================================================================
// Adapter: Sync FileSystem -> AsyncFileSystem
// ============================================================================

use super::FileSystem;

/// Wrapper that adapts a synchronous `FileSystem` to `AsyncFileSystem`.
///
/// This is useful for wrapping `InMemoryFileSystem` or other sync implementations
/// to be used in async contexts. The operations complete immediately since the
/// underlying implementation is synchronous.
///
/// # Example
///
/// ```ignore
/// use diaryx_core::fs::{InMemoryFileSystem, SyncToAsyncFs, AsyncFileSystem};
///
/// let sync_fs = InMemoryFileSystem::new();
/// let async_fs = SyncToAsyncFs::new(sync_fs);
///
/// // Now you can use async_fs in async code
/// async {
///     let content = async_fs.read_to_string(Path::new("file.md")).await;
/// };
/// ```
#[derive(Clone)]
pub struct SyncToAsyncFs<F: FileSystem> {
    inner: F,
}

impl<F: FileSystem> SyncToAsyncFs<F> {
    /// Create a new async wrapper around a synchronous filesystem.
    pub fn new(fs: F) -> Self {
        Self { inner: fs }
    }

    /// Get a reference to the inner synchronous filesystem.
    pub fn inner(&self) -> &F {
        &self.inner
    }

    /// Unwrap and return the inner synchronous filesystem.
    pub fn into_inner(self) -> F {
        self.inner
    }
}

impl<F: FileSystem + Send + Sync> AsyncFileSystem for SyncToAsyncFs<F> {
    fn read_to_string(&self, path: &Path) -> impl Future<Output = Result<String>> + Send {
        let result = self.inner.read_to_string(path);
        async move { result }
    }

    fn write_file(&self, path: &Path, content: &str) -> impl Future<Output = Result<()>> + Send {
        let result = self.inner.write_file(path, content);
        async move { result }
    }

    fn create_new(&self, path: &Path, content: &str) -> impl Future<Output = Result<()>> + Send {
        let result = self.inner.create_new(path, content);
        async move { result }
    }

    fn delete_file(&self, path: &Path) -> impl Future<Output = Result<()>> + Send {
        let result = self.inner.delete_file(path);
        async move { result }
    }

    fn list_md_files(&self, dir: &Path) -> impl Future<Output = Result<Vec<PathBuf>>> + Send {
        let result = self.inner.list_md_files(dir);
        async move { result }
    }

    fn exists(&self, path: &Path) -> impl Future<Output = bool> + Send {
        let result = self.inner.exists(path);
        async move { result }
    }

    fn create_dir_all(&self, path: &Path) -> impl Future<Output = Result<()>> + Send {
        let result = self.inner.create_dir_all(path);
        async move { result }
    }

    fn is_dir(&self, path: &Path) -> impl Future<Output = bool> + Send {
        let result = self.inner.is_dir(path);
        async move { result }
    }

    fn move_file(&self, from: &Path, to: &Path) -> impl Future<Output = Result<()>> + Send {
        let result = self.inner.move_file(from, to);
        async move { result }
    }

    fn read_binary(&self, path: &Path) -> impl Future<Output = Result<Vec<u8>>> + Send {
        let result = self.inner.read_binary(path);
        async move { result }
    }

    fn write_binary(&self, path: &Path, content: &[u8]) -> impl Future<Output = Result<()>> + Send {
        let result = self.inner.write_binary(path, content);
        async move { result }
    }

    fn list_files(&self, dir: &Path) -> impl Future<Output = Result<Vec<PathBuf>>> + Send {
        let result = self.inner.list_files(dir);
        async move { result }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFileSystem;

    #[test]
    fn test_sync_to_async_wrapper() {
        let sync_fs = InMemoryFileSystem::new();

        // Write a file using sync API
        sync_fs
            .write_file(Path::new("test.md"), "# Hello")
            .unwrap();

        // Wrap in async adapter
        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Use futures::executor to run the async code in a test
        // Note: In real async code, you'd use an async runtime
        let content = futures_lite_test_block_on(async_fs.read_to_string(Path::new("test.md")));
        assert_eq!(content.unwrap(), "# Hello");

        let exists = futures_lite_test_block_on(async_fs.exists(Path::new("test.md")));
        assert!(exists);

        let not_exists = futures_lite_test_block_on(async_fs.exists(Path::new("nonexistent.md")));
        assert!(!not_exists);
    }

    #[test]
    fn test_async_write_and_read() {
        let sync_fs = InMemoryFileSystem::new();
        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Write using async API
        let write_result =
            futures_lite_test_block_on(async_fs.write_file(Path::new("new.md"), "New content"));
        assert!(write_result.is_ok());

        // Read it back
        let content = futures_lite_test_block_on(async_fs.read_to_string(Path::new("new.md")));
        assert_eq!(content.unwrap(), "New content");
    }

    #[test]
    fn test_async_create_new() {
        let sync_fs = InMemoryFileSystem::new();
        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Create new file
        let result =
            futures_lite_test_block_on(async_fs.create_new(Path::new("created.md"), "Created!"));
        assert!(result.is_ok());

        // Try to create again - should fail
        let result2 =
            futures_lite_test_block_on(async_fs.create_new(Path::new("created.md"), "Again!"));
        assert!(result2.is_err());
    }

    #[test]
    fn test_async_directory_operations() {
        let sync_fs = InMemoryFileSystem::new();
        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Create directory
        let result = futures_lite_test_block_on(async_fs.create_dir_all(Path::new("a/b/c")));
        assert!(result.is_ok());

        // Check it's a directory
        let is_dir = futures_lite_test_block_on(async_fs.is_dir(Path::new("a/b/c")));
        assert!(is_dir);

        // Check parent is also a directory
        let parent_is_dir = futures_lite_test_block_on(async_fs.is_dir(Path::new("a/b")));
        assert!(parent_is_dir);
    }

    #[test]
    fn test_inner_access() {
        let sync_fs = InMemoryFileSystem::new();
        sync_fs
            .write_file(Path::new("test.md"), "content")
            .unwrap();

        let async_fs = SyncToAsyncFs::new(sync_fs);

        // Access inner
        assert!(async_fs.inner().exists(Path::new("test.md")));

        // Unwrap
        let recovered = async_fs.into_inner();
        assert!(recovered.exists(Path::new("test.md")));
    }

    /// Simple blocking executor for tests only.
    /// In production, use a proper async runtime.
    fn futures_lite_test_block_on<F: Future>(f: F) -> F::Output {
        use std::pin::pin;
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        // Create a no-op waker
        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            |_| RawWaker::new(std::ptr::null(), &VTABLE), // clone
            |_| {},                                       // wake
            |_| {},                                       // wake_by_ref
            |_| {},                                       // drop
        );

        let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw_waker) };
        let mut cx = Context::from_waker(&waker);

        let mut pinned = pin!(f);
        loop {
            match pinned.as_mut().poll(&mut cx) {
                Poll::Ready(output) => return output,
                Poll::Pending => {
                    // For our sync-wrapped futures, this should never happen
                    // But we handle it anyway by spinning
                    std::hint::spin_loop();
                }
            }
        }
    }
}