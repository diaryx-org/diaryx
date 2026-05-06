//! File handles, [`OpenOptions`], and the [`AsyncFile`] super-trait.
//!
//! This module mirrors the open-file half of [`std::fs`] / [`tokio::fs`]:
//!
//! * [`OpenOptions`] — builder for configuring how a file should be opened.
//! * [`AsyncFile`] — auto-implemented super-trait for any `AsyncRead +
//!   AsyncWrite + AsyncSeek` value.
//! * [`File`] — boxed, object-safe handle returned by
//!   [`AsyncFileSystem::open`](crate::AsyncFileSystem::open).
//!
//! Streaming traits come from [`futures_io`]; tokio interop is left to the
//! caller (typically via `tokio_util::compat`).
//!
//! # Default `open` impl
//!
//! Backends that don't override `open` get a [`MemFile`]-backed implementation
//! that loads the entire file into memory and writes it back when the handle
//! is closed. **It does not write back on drop** — callers must drive the
//! handle to completion (e.g. via `AsyncWriteExt::close`) or the bytes are
//! lost. Backends with native streaming (OS files, OPFS sync access handles)
//! should override `open` to avoid the whole-file buffering.

use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_io::{AsyncRead, AsyncSeek, AsyncWrite};

use crate::traits::{AsyncFileSystem, BoxFuture};

// ============================================================================
// AsyncFile super-trait + File alias
// ============================================================================

/// Marker super-trait for async file handles.
///
/// Auto-implemented for any `AsyncRead + AsyncWrite + AsyncSeek + Unpin`
/// value (and `+ Send` on native targets). Most code never names this
/// directly — it works through [`File`].
#[cfg(not(target_arch = "wasm32"))]
pub trait AsyncFile: AsyncRead + AsyncWrite + AsyncSeek + Send + Unpin {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: AsyncRead + AsyncWrite + AsyncSeek + Send + Unpin + ?Sized> AsyncFile for T {}

/// Marker super-trait for async file handles (WASM build, no `Send`).
#[cfg(target_arch = "wasm32")]
pub trait AsyncFile: AsyncRead + AsyncWrite + AsyncSeek + Unpin {}
#[cfg(target_arch = "wasm32")]
impl<T: AsyncRead + AsyncWrite + AsyncSeek + Unpin + ?Sized> AsyncFile for T {}

/// Owned, object-safe async file handle returned by
/// [`AsyncFileSystem::open`](crate::AsyncFileSystem::open).
///
/// The lifetime parameter borrows from the originating filesystem, so the
/// handle cannot outlive the `&FS` it was opened from. Backends that own
/// their state independently of the filesystem reference (e.g. an OS file
/// descriptor, an OPFS sync access handle) can box a `'static` handle and
/// the `'static: 'a` coercion makes it fit.
pub type File<'a> = Pin<Box<dyn AsyncFile + 'a>>;

// ============================================================================
// OpenOptions
// ============================================================================

/// Options for opening a file. Mirrors [`std::fs::OpenOptions`] minus
/// `append`.
///
/// `append` is intentionally omitted: browser backends cannot offer atomic
/// append semantics. Callers that need it should `seek(End(0))` then
/// `write`.
#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    create: bool,
    create_new: bool,
    truncate: bool,
}

impl OpenOptions {
    /// Construct a fresh `OpenOptions` with all flags unset.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the file for reading.
    pub fn read(mut self, yes: bool) -> Self {
        self.read = yes;
        self
    }

    /// Open the file for writing.
    pub fn write(mut self, yes: bool) -> Self {
        self.write = yes;
        self
    }

    /// Create the file if it does not exist. Requires `write`.
    pub fn create(mut self, yes: bool) -> Self {
        self.create = yes;
        self
    }

    /// Atomically create a new file, failing if it already exists. Requires
    /// `write`.
    pub fn create_new(mut self, yes: bool) -> Self {
        self.create_new = yes;
        self
    }

    /// Truncate the file to length zero on open. Requires `write`.
    pub fn truncate(mut self, yes: bool) -> Self {
        self.truncate = yes;
        self
    }

    /// Whether `read` was set.
    pub fn is_read(&self) -> bool {
        self.read
    }
    /// Whether `write` was set.
    pub fn is_write(&self) -> bool {
        self.write
    }
    /// Whether `create` was set.
    pub fn is_create(&self) -> bool {
        self.create
    }
    /// Whether `create_new` was set.
    pub fn is_create_new(&self) -> bool {
        self.create_new
    }
    /// Whether `truncate` was set.
    pub fn is_truncate(&self) -> bool {
        self.truncate
    }

    /// Open `path` against `fs` using these options. Equivalent to
    /// `fs.open(path, self)`.
    pub fn open<'a, F: AsyncFileSystem + ?Sized>(
        self,
        fs: &'a F,
        path: &'a Path,
    ) -> BoxFuture<'a, io::Result<File<'a>>> {
        fs.open(path, self)
    }

    /// Validate that the flag combination is internally consistent. Returns
    /// `InvalidInput` for nonsensical combinations.
    pub fn validate(&self) -> io::Result<()> {
        if !self.read && !self.write {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "OpenOptions: at least one of `read` or `write` must be set",
            ));
        }
        if (self.create || self.create_new || self.truncate) && !self.write {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "OpenOptions: `create`, `create_new`, and `truncate` require `write`",
            ));
        }
        Ok(())
    }
}

// ============================================================================
// MemFile — default in-memory backing for `open`
// ============================================================================

/// Default `File` impl: loads the entire file into memory and writes it back
/// to the underlying filesystem on close.
///
/// Used as the default impl of [`AsyncFileSystem::open`](crate::AsyncFileSystem::open).
/// Backends with real streaming should override `open` to skip the
/// whole-file buffering.
///
/// Bytes are **not** persisted on drop — drive the handle to completion via
/// `AsyncWriteExt::close` (or `flush` followed by `close`) to commit
/// changes. This matches `tokio::fs::File`'s behavior.
pub struct MemFile<'a, F: AsyncFileSystem + ?Sized> {
    fs: &'a F,
    path: PathBuf,
    cursor: Cursor<Vec<u8>>,
    write: bool,
    dirty: bool,
    closed: bool,
    flushing: Option<BoxFuture<'a, io::Result<()>>>,
}

impl<'a, F: AsyncFileSystem + ?Sized> MemFile<'a, F> {
    /// Construct a `MemFile` with pre-loaded contents.
    pub fn new(fs: &'a F, path: PathBuf, contents: Vec<u8>, write: bool) -> Self {
        Self {
            fs,
            path,
            cursor: Cursor::new(contents),
            write,
            dirty: false,
            closed: false,
            flushing: None,
        }
    }

    fn poll_writeback(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.closed || !self.write || !self.dirty {
            return Poll::Ready(Ok(()));
        }
        if self.flushing.is_none() {
            // Capture by value so the future doesn't borrow self — only fs.
            let bytes = std::mem::take(self.cursor.get_mut());
            let path = self.path.clone();
            let fs: &'a F = self.fs;
            self.flushing = Some(Box::pin(async move { fs.write(&path, &bytes).await }));
        }
        let fut = self.flushing.as_mut().expect("just set above");
        match fut.as_mut().poll(cx) {
            Poll::Ready(result) => {
                self.flushing = None;
                if result.is_ok() {
                    self.dirty = false;
                }
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<'a, F: AsyncFileSystem + ?Sized> AsyncRead for MemFile<'a, F> {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = Pin::into_inner(self);
        if this.closed {
            return Poll::Ready(Err(io::Error::other("file is closed")));
        }
        let n = std::io::Read::read(&mut this.cursor, buf)?;
        Poll::Ready(Ok(n))
    }
}

impl<'a, F: AsyncFileSystem + ?Sized> AsyncWrite for MemFile<'a, F> {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = Pin::into_inner(self);
        if this.closed {
            return Poll::Ready(Err(io::Error::other("file is closed")));
        }
        if !this.write {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "file not opened with write access",
            )));
        }
        let n = std::io::Write::write(&mut this.cursor, buf)?;
        if n > 0 {
            this.dirty = true;
        }
        Poll::Ready(Ok(n))
    }

    /// Flushes pending writes. For `MemFile`, this is a no-op — the writeback
    /// happens on `close`. Backends with real streaming should buffer
    /// differently and flush eagerly.
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = Pin::into_inner(self);
        match this.poll_writeback(cx) {
            Poll::Ready(Ok(())) => {
                this.closed = true;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<'a, F: AsyncFileSystem + ?Sized> AsyncSeek for MemFile<'a, F> {
    fn poll_seek(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        pos: io::SeekFrom,
    ) -> Poll<io::Result<u64>> {
        let this = Pin::into_inner(self);
        if this.closed {
            return Poll::Ready(Err(io::Error::other("file is closed")));
        }
        let n = std::io::Seek::seek(&mut this.cursor, pos)?;
        Poll::Ready(Ok(n))
    }
}

// ============================================================================
// default_open — wired in by the trait's default `open` method
// ============================================================================

/// Default implementation of `AsyncFileSystem::open` used when the backend
/// does not override it. Loads (or creates) the file and returns a
/// [`MemFile`] handle.
pub(crate) async fn default_open<'a, F: AsyncFileSystem + ?Sized>(
    fs: &'a F,
    path: &'a Path,
    options: OpenOptions,
) -> io::Result<File<'a>> {
    options.validate()?;

    // create_new: fail if the file exists.
    if options.is_create_new() {
        // create_new is atomic on backends that support it.
        fs.create_new(path, b"").await?;
    } else if options.is_create() {
        // Touch: create if missing, leave existing alone.
        match fs.try_exists(path).await? {
            true => {}
            false => fs.write(path, b"").await?,
        }
    }

    let initial: Vec<u8> = if options.is_truncate() {
        Vec::new()
    } else {
        // Read existing contents. If create wasn't requested and file is
        // missing, this returns NotFound — which matches std::fs.
        match fs.read(path).await {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound && options.is_create_new() => {
                // We just created it — should not happen, but be safe.
                Vec::new()
            }
            Err(e) => return Err(e),
        }
    };

    let mem = MemFile::new(fs, path.to_path_buf(), initial, options.is_write());
    Ok(Box::pin(mem))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InMemoryFs, SyncToAsyncFs};
    use std::future::Future;
    use std::io::SeekFrom;

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
    fn validate_rejects_no_access() {
        let opts = OpenOptions::new();
        assert!(opts.validate().is_err());
    }

    #[test]
    fn validate_rejects_create_without_write() {
        let opts = OpenOptions::new().read(true).create(true);
        assert!(opts.validate().is_err());
    }

    #[test]
    fn validate_accepts_read_only() {
        assert!(OpenOptions::new().read(true).validate().is_ok());
    }

    #[test]
    fn validate_accepts_write_create() {
        assert!(
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn open_read_existing_file() {
        use futures_io::AsyncRead;

        let fs = SyncToAsyncFs::new(InMemoryFs::new());
        block_on(fs.write(Path::new("hello.md"), b"hello world")).unwrap();

        let mut file = block_on(
            OpenOptions::new()
                .read(true)
                .open(&fs, Path::new("hello.md")),
        )
        .expect("open should succeed");

        // Drive AsyncRead manually since we don't depend on futures-util.
        let mut buf = vec![0u8; 32];
        let read = block_on(std::future::poll_fn(|cx| {
            std::pin::Pin::new(&mut file).poll_read(cx, &mut buf)
        }))
        .unwrap();
        assert_eq!(read, 11);
        assert_eq!(&buf[..read], b"hello world");
    }

    #[test]
    fn open_write_create_then_close_persists() {
        use futures_io::{AsyncSeek, AsyncWrite};

        let fs = SyncToAsyncFs::new(InMemoryFs::new());
        let mut file = block_on(
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&fs, Path::new("out.md")),
        )
        .expect("open should succeed");

        // Write some bytes.
        let n = block_on(std::future::poll_fn(|cx| {
            std::pin::Pin::new(&mut file).poll_write(cx, b"persisted")
        }))
        .unwrap();
        assert_eq!(n, 9);

        // Seek to start and overwrite a prefix.
        let pos = block_on(std::future::poll_fn(|cx| {
            std::pin::Pin::new(&mut file).poll_seek(cx, SeekFrom::Start(0))
        }))
        .unwrap();
        assert_eq!(pos, 0);
        let n = block_on(std::future::poll_fn(|cx| {
            std::pin::Pin::new(&mut file).poll_write(cx, b"PERSISTED")
        }))
        .unwrap();
        assert_eq!(n, 9);

        // Close to commit.
        block_on(std::future::poll_fn(|cx| {
            std::pin::Pin::new(&mut file).poll_close(cx)
        }))
        .unwrap();
        drop(file);

        let on_disk = block_on(fs.read(Path::new("out.md"))).unwrap();
        assert_eq!(&on_disk, b"PERSISTED");
    }

    #[test]
    fn open_write_without_close_loses_changes() {
        use futures_io::AsyncWrite;

        let fs = SyncToAsyncFs::new(InMemoryFs::new());
        block_on(fs.write(Path::new("file.md"), b"original")).unwrap();

        let mut file = block_on(
            OpenOptions::new()
                .write(true)
                .open(&fs, Path::new("file.md")),
        )
        .expect("open should succeed");
        let _ = block_on(std::future::poll_fn(|cx| {
            std::pin::Pin::new(&mut file).poll_write(cx, b"DIRTY")
        }))
        .unwrap();
        // No close → bytes are dropped.
        drop(file);

        let on_disk = block_on(fs.read(Path::new("file.md"))).unwrap();
        assert_eq!(&on_disk, b"original");
    }

    #[test]
    fn open_create_new_fails_if_exists() {
        let fs = SyncToAsyncFs::new(InMemoryFs::new());
        block_on(fs.write(Path::new("dup.md"), b"x")).unwrap();
        let res = block_on(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&fs, Path::new("dup.md")),
        );
        assert!(res.is_err());
        assert_eq!(res.err().unwrap().kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn open_read_missing_file_fails() {
        let fs = SyncToAsyncFs::new(InMemoryFs::new());
        let res = block_on(
            OpenOptions::new()
                .read(true)
                .open(&fs, Path::new("missing.md")),
        );
        assert!(res.is_err());
        assert_eq!(res.err().unwrap().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn write_to_read_only_handle_fails() {
        use futures_io::AsyncWrite;

        let fs = SyncToAsyncFs::new(InMemoryFs::new());
        block_on(fs.write(Path::new("ro.md"), b"x")).unwrap();
        let mut file =
            block_on(OpenOptions::new().read(true).open(&fs, Path::new("ro.md"))).unwrap();
        let res = block_on(std::future::poll_fn(|cx| {
            std::pin::Pin::new(&mut file).poll_write(cx, b"y")
        }));
        assert!(res.is_err());
        assert_eq!(res.err().unwrap().kind(), io::ErrorKind::PermissionDenied);
    }
}
