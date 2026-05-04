//! `SyncToAsyncFs` — adapter for using a synchronous [`FileSystem`] in async
//! contexts.
//!
//! Most useful with [`InMemoryFs`](crate::InMemoryFs), where every operation
//! completes immediately.

use std::path::Path;

use crate::metadata::{DirEntry, Metadata};
#[allow(deprecated)]
use crate::traits::{AsyncFileSystem, BoxFuture, FileSystem};

/// Wrap a synchronous [`FileSystem`] so it satisfies [`AsyncFileSystem`].
///
/// Operations complete immediately; the futures resolve in a single poll.
#[derive(Clone)]
pub struct SyncToAsyncFs<F> {
    inner: F,
}

#[allow(deprecated)]
impl<F: FileSystem> SyncToAsyncFs<F> {
    /// Wrap a synchronous filesystem.
    pub fn new(fs: F) -> Self {
        Self { inner: fs }
    }

    /// Borrow the inner sync filesystem.
    pub fn inner(&self) -> &F {
        &self.inner
    }

    /// Unwrap and return the inner sync filesystem.
    pub fn into_inner(self) -> F {
        self.inner
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(deprecated)]
impl<F: FileSystem + Send + Sync> AsyncFileSystem for SyncToAsyncFs<F> {
    fn read<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<Vec<u8>>> {
        Box::pin(async move { self.inner.read(path) })
    }
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<String>> {
        Box::pin(async move { self.inner.read_to_string(path) })
    }
    fn read_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<Vec<DirEntry>>> {
        Box::pin(async move { self.inner.read_dir(path) })
    }
    fn write<'a>(
        &'a self,
        path: &'a Path,
        contents: &'a [u8],
    ) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.write(path, contents) })
    }
    fn create_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.create_dir(path) })
    }
    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.create_dir_all(path) })
    }
    fn remove_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.remove_file(path) })
    }
    fn remove_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.remove_dir(path) })
    }
    fn remove_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.remove_dir_all(path) })
    }
    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.rename(from, to) })
    }
    fn metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<Metadata>> {
        Box::pin(async move { self.inner.metadata(path) })
    }
    fn symlink_metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<Metadata>> {
        Box::pin(async move { self.inner.symlink_metadata(path) })
    }
    fn create_new<'a>(
        &'a self,
        path: &'a Path,
        contents: &'a [u8],
    ) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.create_new(path, contents) })
    }
}

#[cfg(target_arch = "wasm32")]
#[allow(deprecated)]
impl<F: FileSystem> AsyncFileSystem for SyncToAsyncFs<F> {
    fn read<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<Vec<u8>>> {
        Box::pin(async move { self.inner.read(path) })
    }
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<String>> {
        Box::pin(async move { self.inner.read_to_string(path) })
    }
    fn read_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<Vec<DirEntry>>> {
        Box::pin(async move { self.inner.read_dir(path) })
    }
    fn write<'a>(
        &'a self,
        path: &'a Path,
        contents: &'a [u8],
    ) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.write(path, contents) })
    }
    fn create_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.create_dir(path) })
    }
    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.create_dir_all(path) })
    }
    fn remove_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.remove_file(path) })
    }
    fn remove_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.remove_dir(path) })
    }
    fn remove_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.remove_dir_all(path) })
    }
    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.rename(from, to) })
    }
    fn metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<Metadata>> {
        Box::pin(async move { self.inner.metadata(path) })
    }
    fn symlink_metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, std::io::Result<Metadata>> {
        Box::pin(async move { self.inner.symlink_metadata(path) })
    }
    fn create_new<'a>(
        &'a self,
        path: &'a Path,
        contents: &'a [u8],
    ) -> BoxFuture<'a, std::io::Result<()>> {
        Box::pin(async move { self.inner.create_new(path, contents) })
    }
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;
    use crate::InMemoryFs;
    use std::future::Future;

    fn block_on<F: Future>(f: F) -> F::Output {
        use std::pin::pin;
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            |_| RawWaker::new(std::ptr::null(), &VTABLE),
            |_| {},
            |_| {},
            |_| {},
        );
        let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw_waker) };
        let mut cx = Context::from_waker(&waker);

        let mut pinned = pin!(f);
        loop {
            match pinned.as_mut().poll(&mut cx) {
                Poll::Ready(out) => return out,
                Poll::Pending => std::hint::spin_loop(),
            }
        }
    }

    #[test]
    fn write_then_read() {
        let fs = SyncToAsyncFs::new(InMemoryFs::new());

        block_on(fs.write(Path::new("hello.md"), b"hi")).unwrap();
        let s = block_on(fs.read_to_string(Path::new("hello.md"))).unwrap();
        assert_eq!(s, "hi");
        let bytes = block_on(fs.read(Path::new("hello.md"))).unwrap();
        assert_eq!(bytes, b"hi");
    }

    #[test]
    fn metadata_and_try_exists() {
        let fs = SyncToAsyncFs::new(InMemoryFs::new());
        block_on(fs.write(Path::new("file.md"), b"content")).unwrap();
        assert!(block_on(fs.try_exists(Path::new("file.md"))).unwrap());
        assert!(!block_on(fs.try_exists(Path::new("nope.md"))).unwrap());
        let m = block_on(fs.metadata(Path::new("file.md"))).unwrap();
        assert!(m.is_file());
        assert_eq!(m.len(), 7);
    }

    #[test]
    fn create_new_rejects_duplicate() {
        let fs = SyncToAsyncFs::new(InMemoryFs::new());
        block_on(fs.create_new(Path::new("created.md"), b"x")).unwrap();
        assert!(block_on(fs.create_new(Path::new("created.md"), b"y")).is_err());
    }

    #[test]
    fn directory_creation() {
        let fs = SyncToAsyncFs::new(InMemoryFs::new());
        block_on(fs.create_dir_all(Path::new("a/b/c"))).unwrap();
        assert!(block_on(fs.metadata(Path::new("a/b/c"))).unwrap().is_dir());
        assert!(block_on(fs.metadata(Path::new("a/b"))).unwrap().is_dir());
    }
}
