//! The `FileSystem` and `AsyncFileSystem` traits.
//!
//! Method names are aligned with [`std::fs`] / [`tokio::fs`]. Backends
//! implement the canonical methods; legacy method names from previous
//! versions remain as `#[deprecated]` default impls that delegate, so
//! existing call sites keep compiling during the migration to v0.1.

// Allowing `deprecated` inside this file: the legacy methods and the sync
// `FileSystem` trait are marked `#[deprecated]` to warn external callers, but
// internal default impls and blanket forwards must reference them and would
// otherwise trip the lint on every line.
#![allow(deprecated)]

use std::future::Future;
use std::io;
use std::path::Path;
use std::pin::Pin;

use crate::metadata::{DirEntry, Metadata};

/// A boxed future for object-safe async methods.
///
/// On native targets, futures are `Send` for compatibility with multi-threaded
/// runtimes. On WASM, the `Send` bound is dropped since JavaScript is
/// single-threaded.
#[cfg(not(target_arch = "wasm32"))]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A boxed future for object-safe async methods (WASM).
#[cfg(target_arch = "wasm32")]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

// ============================================================================
// AsyncFileSystem
// ============================================================================

/// Async filesystem abstraction.
///
/// This trait mirrors the surface of [`std::fs`] / [`tokio::fs`] and is
/// designed to be object-safe (`dyn AsyncFileSystem`) — every method returns
/// a [`BoxFuture`].
///
/// Backends implement the canonical methods listed under "required methods"
/// and "default-implemented methods" below. The methods marked
/// `#[deprecated]` are legacy aliases retained for source compatibility with
/// pre-`crossfs` Diaryx code; new code should use the canonical names.
#[cfg(not(target_arch = "wasm32"))]
pub trait AsyncFileSystem: Send + Sync {
    // ---- canonical: read ----

    /// Read the entire contents of a file as bytes. Mirrors [`std::fs::read`].
    fn read<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Vec<u8>>>;

    /// Read the entire contents of a file as a string. Mirrors
    /// [`std::fs::read_to_string`].
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<String>>;

    /// Return the entries in a directory (non-recursive). Mirrors
    /// [`std::fs::read_dir`], but returns a `Vec` rather than an iterator
    /// since async iterators are not yet stabilized.
    fn read_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Vec<DirEntry>>>;

    // ---- canonical: write ----

    /// Write a file, replacing it if it already exists. Mirrors
    /// [`std::fs::write`].
    fn write<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, io::Result<()>>;

    /// Create a new directory. Mirrors [`std::fs::create_dir`].
    fn create_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>>;

    /// Create a directory and all parent directories. Mirrors
    /// [`std::fs::create_dir_all`].
    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>>;

    // ---- canonical: mutate ----

    /// Remove a regular file. Mirrors [`std::fs::remove_file`].
    fn remove_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>>;

    /// Remove an empty directory. Mirrors [`std::fs::remove_dir`].
    fn remove_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>>;

    /// Recursively remove a directory and all its contents. Mirrors
    /// [`std::fs::remove_dir_all`].
    fn remove_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>>;

    /// Rename or move a file or directory. Mirrors [`std::fs::rename`].
    /// Should error if the source does not exist.
    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, io::Result<()>>;

    // ---- canonical: inspect ----

    /// Return metadata about the entry at `path`. Mirrors
    /// [`std::fs::metadata`]. Follows symlinks.
    fn metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Metadata>>;

    /// Like [`metadata`](Self::metadata), but does **not** follow symlinks.
    /// Mirrors [`std::fs::symlink_metadata`].
    ///
    /// Backends without symlink support need not override; the default
    /// delegates to `metadata`.
    fn symlink_metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Metadata>> {
        self.metadata(path)
    }

    /// Returns `Ok(true)` if the path exists, `Ok(false)` if it doesn't, and
    /// `Err(_)` if the existence check itself failed. Mirrors
    /// [`std::fs::try_exists`].
    fn try_exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<bool>> {
        Box::pin(async move {
            match self.symlink_metadata(path).await {
                Ok(_) => Ok(true),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(e) => Err(e),
            }
        })
    }

    // ---- canonical: extras (not in std::fs but useful) ----

    /// Create a file only if it does not already exist. Returns
    /// `ErrorKind::AlreadyExists` if it does.
    ///
    /// In `std::fs` this is `OpenOptions::new().create_new(true).write(true)`;
    /// since `crossfs` v0.1 does not yet ship `OpenOptions`, this is provided
    /// as a top-level method. Backends should implement this atomically when
    /// the underlying storage supports it.
    fn create_new<'a>(
        &'a self,
        path: &'a Path,
        contents: &'a [u8],
    ) -> BoxFuture<'a, io::Result<()>>;

    /// Remove every entry inside `dir` but leave `dir` itself in place.
    fn clear_dir<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        Box::pin(async move {
            let entries = self.read_dir(dir).await?;
            for entry in entries {
                let ft = entry.file_type()?;
                if ft.is_dir() {
                    self.remove_dir_all(entry.path()).await?;
                } else {
                    self.remove_file(entry.path()).await?;
                }
            }
            Ok(())
        })
    }

    // ============================================================================
    // Legacy / deprecated method aliases
    // ============================================================================
    //
    // These exist so that pre-`crossfs` Diaryx call sites continue compiling.
    // New code should use the canonical method above each one. They are all
    // default-implemented in terms of the canonical methods.

    /// Deprecated alias for [`write`](Self::write) accepting `&str`.
    #[deprecated(since = "0.1.0", note = "use `write(path, content.as_bytes())`")]
    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, io::Result<()>> {
        Box::pin(async move { self.write(path, content.as_bytes()).await })
    }

    /// Deprecated alias for [`read`](Self::read).
    #[deprecated(since = "0.1.0", note = "use `read`")]
    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Vec<u8>>> {
        self.read(path)
    }

    /// Deprecated alias for [`write`](Self::write).
    #[deprecated(since = "0.1.0", note = "use `write`")]
    fn write_binary<'a>(
        &'a self,
        path: &'a Path,
        content: &'a [u8],
    ) -> BoxFuture<'a, io::Result<()>> {
        self.write(path, content)
    }

    /// Deprecated alias for [`remove_file`](Self::remove_file).
    #[deprecated(since = "0.1.0", note = "use `remove_file`")]
    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        self.remove_file(path)
    }

    /// Deprecated alias for [`rename`](Self::rename).
    #[deprecated(since = "0.1.0", note = "use `rename`")]
    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        self.rename(from, to)
    }

    /// Deprecated. Use [`read_dir`](Self::read_dir) and inspect each
    /// `DirEntry::path()`.
    #[deprecated(since = "0.1.0", note = "use `read_dir` and map to paths")]
    fn list_files<'a>(
        &'a self,
        dir: &'a Path,
    ) -> BoxFuture<'a, io::Result<Vec<std::path::PathBuf>>> {
        Box::pin(async move {
            let entries = self.read_dir(dir).await?;
            Ok(entries
                .into_iter()
                .map(|e| e.path().to_path_buf())
                .collect())
        })
    }

    /// Deprecated. Domain-specific helper retained for source compatibility.
    /// Use `read_dir` and filter by extension at the call site.
    #[deprecated(
        since = "0.1.0",
        note = "use `read_dir` and filter for `.md` extension at the call site"
    )]
    fn list_md_files<'a>(
        &'a self,
        dir: &'a Path,
    ) -> BoxFuture<'a, io::Result<Vec<std::path::PathBuf>>> {
        Box::pin(async move {
            let entries = self.read_dir(dir).await?;
            Ok(entries
                .into_iter()
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
                .map(|e| e.path().to_path_buf())
                .collect())
        })
    }

    /// Deprecated. Domain-specific helper retained for source compatibility.
    #[deprecated(
        since = "0.1.0",
        note = "use `read_dir` recursively and filter for `.md` extension"
    )]
    fn list_md_files_recursive<'a>(
        &'a self,
        dir: &'a Path,
    ) -> BoxFuture<'a, io::Result<Vec<std::path::PathBuf>>> {
        Box::pin(async move {
            let mut all = self.list_md_files(dir).await?;
            if let Ok(entries) = self.read_dir(dir).await {
                for entry in entries {
                    if entry.file_type()?.is_dir()
                        && let Ok(sub) = self.list_md_files_recursive(entry.path()).await
                    {
                        all.extend(sub);
                    }
                }
            }
            Ok(all)
        })
    }

    /// Deprecated. Use `read_dir` recursively at the call site.
    #[deprecated(since = "0.1.0", note = "use `read_dir` recursively")]
    fn list_all_files_recursive<'a>(
        &'a self,
        dir: &'a Path,
    ) -> BoxFuture<'a, io::Result<Vec<std::path::PathBuf>>> {
        Box::pin(async move {
            let mut all = Vec::new();
            if let Ok(entries) = self.read_dir(dir).await {
                for entry in entries {
                    let path = entry.path().to_path_buf();
                    all.push(path.clone());
                    if entry.file_type()?.is_dir()
                        && let Ok(sub) = self.list_all_files_recursive(&path).await
                    {
                        all.extend(sub);
                    }
                }
            }
            Ok(all)
        })
    }

    /// Deprecated. Use [`try_exists`](Self::try_exists), or — to match
    /// [`std::path::Path::exists`] semantics that swallow errors — call
    /// `try_exists(path).await.unwrap_or(false)`.
    #[deprecated(since = "0.1.0", note = "use `try_exists`")]
    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move { self.try_exists(path).await.unwrap_or(false) })
    }

    /// Deprecated. Use `metadata(path).await?.is_dir()`.
    #[deprecated(since = "0.1.0", note = "use `metadata(path).await?.is_dir()`")]
    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            self.metadata(path)
                .await
                .map(|m| m.is_dir())
                .unwrap_or(false)
        })
    }

    /// Deprecated. Use `symlink_metadata(path).await?.is_symlink()`.
    #[deprecated(
        since = "0.1.0",
        note = "use `symlink_metadata(path).await?.is_symlink()`"
    )]
    fn is_symlink<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            self.symlink_metadata(path)
                .await
                .map(|m| m.is_symlink())
                .unwrap_or(false)
        })
    }

    /// Deprecated. Use `metadata(path).await?.modified()` and convert to
    /// the time format you need.
    #[deprecated(since = "0.1.0", note = "use `metadata(path).await?.modified()`")]
    fn get_modified_time<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        Box::pin(async move {
            let meta = self.metadata(path).await.ok()?;
            let modified = meta.modified().ok()?;
            let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
            i64::try_from(dur.as_millis()).ok()
        })
    }

    /// Deprecated. Use `metadata(path).await?.len()`.
    #[deprecated(since = "0.1.0", note = "use `metadata(path).await?.len()`")]
    fn get_file_size<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<u64>> {
        Box::pin(async move { self.metadata(path).await.ok().map(|m| m.len()) })
    }
}

/// Async filesystem abstraction (WASM build).
///
/// Identical to the native trait but without the `Send + Sync` bound, since
/// JavaScript is single-threaded.
#[cfg(target_arch = "wasm32")]
pub trait AsyncFileSystem {
    fn read<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Vec<u8>>>;
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<String>>;
    fn read_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Vec<DirEntry>>>;
    fn write<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, io::Result<()>>;
    fn create_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>>;
    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>>;
    fn remove_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>>;
    fn remove_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>>;
    fn remove_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>>;
    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, io::Result<()>>;
    fn metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Metadata>>;

    fn symlink_metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Metadata>> {
        self.metadata(path)
    }

    fn try_exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<bool>> {
        Box::pin(async move {
            match self.symlink_metadata(path).await {
                Ok(_) => Ok(true),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(e) => Err(e),
            }
        })
    }

    fn create_new<'a>(
        &'a self,
        path: &'a Path,
        contents: &'a [u8],
    ) -> BoxFuture<'a, io::Result<()>>;

    fn clear_dir<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        Box::pin(async move {
            let entries = self.read_dir(dir).await?;
            for entry in entries {
                let ft = entry.file_type()?;
                if ft.is_dir() {
                    self.remove_dir_all(entry.path()).await?;
                } else {
                    self.remove_file(entry.path()).await?;
                }
            }
            Ok(())
        })
    }

    #[deprecated(since = "0.1.0", note = "use `write(path, content.as_bytes())`")]
    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, io::Result<()>> {
        Box::pin(async move { self.write(path, content.as_bytes()).await })
    }

    #[deprecated(since = "0.1.0", note = "use `read`")]
    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Vec<u8>>> {
        self.read(path)
    }

    #[deprecated(since = "0.1.0", note = "use `write`")]
    fn write_binary<'a>(
        &'a self,
        path: &'a Path,
        content: &'a [u8],
    ) -> BoxFuture<'a, io::Result<()>> {
        self.write(path, content)
    }

    #[deprecated(since = "0.1.0", note = "use `remove_file`")]
    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        self.remove_file(path)
    }

    #[deprecated(since = "0.1.0", note = "use `rename`")]
    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        self.rename(from, to)
    }

    #[deprecated(since = "0.1.0", note = "use `read_dir` and map to paths")]
    fn list_files<'a>(
        &'a self,
        dir: &'a Path,
    ) -> BoxFuture<'a, io::Result<Vec<std::path::PathBuf>>> {
        Box::pin(async move {
            let entries = self.read_dir(dir).await?;
            Ok(entries
                .into_iter()
                .map(|e| e.path().to_path_buf())
                .collect())
        })
    }

    #[deprecated(
        since = "0.1.0",
        note = "use `read_dir` and filter for `.md` extension at the call site"
    )]
    fn list_md_files<'a>(
        &'a self,
        dir: &'a Path,
    ) -> BoxFuture<'a, io::Result<Vec<std::path::PathBuf>>> {
        Box::pin(async move {
            let entries = self.read_dir(dir).await?;
            Ok(entries
                .into_iter()
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
                .map(|e| e.path().to_path_buf())
                .collect())
        })
    }

    #[deprecated(
        since = "0.1.0",
        note = "use `read_dir` recursively and filter for `.md` extension"
    )]
    fn list_md_files_recursive<'a>(
        &'a self,
        dir: &'a Path,
    ) -> BoxFuture<'a, io::Result<Vec<std::path::PathBuf>>> {
        Box::pin(async move {
            let mut all = self.list_md_files(dir).await?;
            if let Ok(entries) = self.read_dir(dir).await {
                for entry in entries {
                    if entry.file_type()?.is_dir()
                        && let Ok(sub) = self.list_md_files_recursive(entry.path()).await
                    {
                        all.extend(sub);
                    }
                }
            }
            Ok(all)
        })
    }

    #[deprecated(since = "0.1.0", note = "use `read_dir` recursively")]
    fn list_all_files_recursive<'a>(
        &'a self,
        dir: &'a Path,
    ) -> BoxFuture<'a, io::Result<Vec<std::path::PathBuf>>> {
        Box::pin(async move {
            let mut all = Vec::new();
            if let Ok(entries) = self.read_dir(dir).await {
                for entry in entries {
                    let path = entry.path().to_path_buf();
                    all.push(path.clone());
                    if entry.file_type()?.is_dir()
                        && let Ok(sub) = self.list_all_files_recursive(&path).await
                    {
                        all.extend(sub);
                    }
                }
            }
            Ok(all)
        })
    }

    #[deprecated(since = "0.1.0", note = "use `try_exists`")]
    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move { self.try_exists(path).await.unwrap_or(false) })
    }

    #[deprecated(since = "0.1.0", note = "use `metadata(path).await?.is_dir()`")]
    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            self.metadata(path)
                .await
                .map(|m| m.is_dir())
                .unwrap_or(false)
        })
    }

    #[deprecated(
        since = "0.1.0",
        note = "use `symlink_metadata(path).await?.is_symlink()`"
    )]
    fn is_symlink<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            self.symlink_metadata(path)
                .await
                .map(|m| m.is_symlink())
                .unwrap_or(false)
        })
    }

    #[deprecated(since = "0.1.0", note = "use `metadata(path).await?.modified()`")]
    fn get_modified_time<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        Box::pin(async move {
            let meta = self.metadata(path).await.ok()?;
            let modified = meta.modified().ok()?;
            let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
            i64::try_from(dur.as_millis()).ok()
        })
    }

    #[deprecated(since = "0.1.0", note = "use `metadata(path).await?.len()`")]
    fn get_file_size<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<u64>> {
        Box::pin(async move { self.metadata(path).await.ok().map(|m| m.len()) })
    }
}

// Blanket impl for references — native.
#[cfg(not(target_arch = "wasm32"))]
#[allow(deprecated)]
impl<T: AsyncFileSystem + ?Sized> AsyncFileSystem for &T {
    fn read<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Vec<u8>>> {
        (*self).read(path)
    }
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<String>> {
        (*self).read_to_string(path)
    }
    fn read_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Vec<DirEntry>>> {
        (*self).read_dir(path)
    }
    fn write<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, io::Result<()>> {
        (*self).write(path, contents)
    }
    fn create_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        (*self).create_dir(path)
    }
    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        (*self).create_dir_all(path)
    }
    fn remove_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        (*self).remove_file(path)
    }
    fn remove_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        (*self).remove_dir(path)
    }
    fn remove_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        (*self).remove_dir_all(path)
    }
    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        (*self).rename(from, to)
    }
    fn metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Metadata>> {
        (*self).metadata(path)
    }
    fn symlink_metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Metadata>> {
        (*self).symlink_metadata(path)
    }
    fn try_exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<bool>> {
        (*self).try_exists(path)
    }
    fn create_new<'a>(
        &'a self,
        path: &'a Path,
        contents: &'a [u8],
    ) -> BoxFuture<'a, io::Result<()>> {
        (*self).create_new(path, contents)
    }
    fn clear_dir<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        (*self).clear_dir(dir)
    }
}

// Blanket impl for references — wasm.
#[cfg(target_arch = "wasm32")]
#[allow(deprecated)]
impl<T: AsyncFileSystem + ?Sized> AsyncFileSystem for &T {
    fn read<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Vec<u8>>> {
        (*self).read(path)
    }
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<String>> {
        (*self).read_to_string(path)
    }
    fn read_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Vec<DirEntry>>> {
        (*self).read_dir(path)
    }
    fn write<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, io::Result<()>> {
        (*self).write(path, contents)
    }
    fn create_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        (*self).create_dir(path)
    }
    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        (*self).create_dir_all(path)
    }
    fn remove_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        (*self).remove_file(path)
    }
    fn remove_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        (*self).remove_dir(path)
    }
    fn remove_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        (*self).remove_dir_all(path)
    }
    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        (*self).rename(from, to)
    }
    fn metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Metadata>> {
        (*self).metadata(path)
    }
    fn symlink_metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<Metadata>> {
        (*self).symlink_metadata(path)
    }
    fn try_exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, io::Result<bool>> {
        (*self).try_exists(path)
    }
    fn create_new<'a>(
        &'a self,
        path: &'a Path,
        contents: &'a [u8],
    ) -> BoxFuture<'a, io::Result<()>> {
        (*self).create_new(path, contents)
    }
    fn clear_dir<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, io::Result<()>> {
        (*self).clear_dir(dir)
    }
}

// ============================================================================
// FileSystem (sync)
// ============================================================================

/// Synchronous filesystem abstraction.
///
/// **Deprecated.** Use [`AsyncFileSystem`] instead. This trait is retained
/// only because parts of the Diaryx workspace still depend on it directly;
/// it will be removed in a future version.
///
/// Method names are aligned with [`std::fs`]. Legacy method names from
/// pre-`crossfs` Diaryx are retained as `#[deprecated]` default impls.
#[deprecated(
    since = "0.1.0",
    note = "use `AsyncFileSystem`; the sync trait will be removed in a future version"
)]
pub trait FileSystem: Send + Sync {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>>;
    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()>;
    fn create_dir(&self, path: &Path) -> io::Result<()>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn remove_dir(&self, path: &Path) -> io::Result<()>;
    fn remove_dir_all(&self, path: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn metadata(&self, path: &Path) -> io::Result<Metadata>;

    fn symlink_metadata(&self, path: &Path) -> io::Result<Metadata> {
        self.metadata(path)
    }

    fn try_exists(&self, path: &Path) -> io::Result<bool> {
        match self.symlink_metadata(path) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn create_new(&self, path: &Path, contents: &[u8]) -> io::Result<()>;

    // ---- legacy aliases ----

    #[deprecated(since = "0.1.0", note = "use `write(path, content.as_bytes())`")]
    fn write_file(&self, path: &Path, content: &str) -> io::Result<()> {
        self.write(path, content.as_bytes())
    }

    #[deprecated(since = "0.1.0", note = "use `read`")]
    fn read_binary(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.read(path)
    }

    #[deprecated(since = "0.1.0", note = "use `write`")]
    fn write_binary(&self, path: &Path, content: &[u8]) -> io::Result<()> {
        self.write(path, content)
    }

    #[deprecated(since = "0.1.0", note = "use `remove_file`")]
    fn delete_file(&self, path: &Path) -> io::Result<()> {
        self.remove_file(path)
    }

    #[deprecated(since = "0.1.0", note = "use `rename`")]
    fn move_file(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.rename(from, to)
    }

    #[deprecated(since = "0.1.0", note = "use `read_dir` and map to paths")]
    fn list_files(&self, dir: &Path) -> io::Result<Vec<std::path::PathBuf>> {
        let entries = self.read_dir(dir)?;
        Ok(entries
            .into_iter()
            .map(|e| e.path().to_path_buf())
            .collect())
    }

    #[deprecated(
        since = "0.1.0",
        note = "use `read_dir` and filter for `.md` extension at the call site"
    )]
    fn list_md_files(&self, dir: &Path) -> io::Result<Vec<std::path::PathBuf>> {
        let entries = self.read_dir(dir)?;
        Ok(entries
            .into_iter()
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
            .map(|e| e.path().to_path_buf())
            .collect())
    }

    #[deprecated(
        since = "0.1.0",
        note = "use `read_dir` recursively and filter for `.md` extension"
    )]
    fn list_md_files_recursive(&self, dir: &Path) -> io::Result<Vec<std::path::PathBuf>> {
        let mut all = self.list_md_files(dir)?;
        if let Ok(entries) = self.read_dir(dir) {
            for entry in entries {
                if entry.file_type()?.is_dir()
                    && let Ok(sub) = self.list_md_files_recursive(entry.path())
                {
                    all.extend(sub);
                }
            }
        }
        Ok(all)
    }

    #[deprecated(since = "0.1.0", note = "use `read_dir` recursively")]
    fn list_all_files_recursive(&self, dir: &Path) -> io::Result<Vec<std::path::PathBuf>> {
        let mut all = Vec::new();
        if let Ok(entries) = self.read_dir(dir) {
            for entry in entries {
                let path = entry.path().to_path_buf();
                all.push(path.clone());
                if entry.file_type()?.is_dir()
                    && let Ok(sub) = self.list_all_files_recursive(&path)
                {
                    all.extend(sub);
                }
            }
        }
        Ok(all)
    }

    #[deprecated(since = "0.1.0", note = "use `try_exists`")]
    fn exists(&self, path: &Path) -> bool {
        self.try_exists(path).unwrap_or(false)
    }

    #[deprecated(since = "0.1.0", note = "use `metadata(path)?.is_dir()`")]
    fn is_dir(&self, path: &Path) -> bool {
        self.metadata(path).map(|m| m.is_dir()).unwrap_or(false)
    }

    #[deprecated(since = "0.1.0", note = "use `symlink_metadata(path)?.is_symlink()`")]
    fn is_symlink(&self, path: &Path) -> bool {
        self.symlink_metadata(path)
            .map(|m| m.is_symlink())
            .unwrap_or(false)
    }

    #[deprecated(since = "0.1.0", note = "use `metadata(path)?.modified()`")]
    fn get_modified_time(&self, path: &Path) -> Option<i64> {
        let meta = self.metadata(path).ok()?;
        let modified = meta.modified().ok()?;
        let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
        i64::try_from(dur.as_millis()).ok()
    }

    #[deprecated(since = "0.1.0", note = "use `metadata(path)?.len()`")]
    fn get_file_size(&self, path: &Path) -> Option<u64> {
        self.metadata(path).ok().map(|m| m.len())
    }
}

#[allow(deprecated)]
impl<T: FileSystem + ?Sized> FileSystem for &T {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        (*self).read(path)
    }
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        (*self).read_to_string(path)
    }
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        (*self).read_dir(path)
    }
    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        (*self).write(path, contents)
    }
    fn create_dir(&self, path: &Path) -> io::Result<()> {
        (*self).create_dir(path)
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        (*self).create_dir_all(path)
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        (*self).remove_file(path)
    }
    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        (*self).remove_dir(path)
    }
    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        (*self).remove_dir_all(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        (*self).rename(from, to)
    }
    fn metadata(&self, path: &Path) -> io::Result<Metadata> {
        (*self).metadata(path)
    }
    fn symlink_metadata(&self, path: &Path) -> io::Result<Metadata> {
        (*self).symlink_metadata(path)
    }
    fn try_exists(&self, path: &Path) -> io::Result<bool> {
        (*self).try_exists(path)
    }
    fn create_new(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        (*self).create_new(path, contents)
    }
}
