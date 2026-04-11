//! Host-bridged filesystem implementation for the publish Extism guest.
//!
//! `HostFs` implements `AsyncFileSystem` by delegating all operations to
//! host functions. Since host function calls are synchronous from the guest's
//! perspective, the async methods return immediately-ready futures.

use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use diaryx_core::fs::{AsyncFileSystem, BoxFuture};
use diaryx_plugin_sdk::host;

/// Map a host-fs error string into an `io::Error`, preserving `NotFound`
/// when the underlying message indicates a missing file. This lets callers
/// like the publisher skip missing attachments via `e.kind() == NotFound`.
fn host_err(msg: String) -> Error {
    let lower = msg.to_lowercase();
    let kind = if lower.contains("no such file")
        || lower.contains("os error 2")
        || lower.contains("not found")
    {
        ErrorKind::NotFound
    } else {
        ErrorKind::Other
    };
    Error::new(kind, msg)
}

/// Filesystem backed by Extism host function calls.
#[derive(Clone)]
pub struct HostFs;

impl AsyncFileSystem for HostFs {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            host::fs::read_file(&path_str).map_err(host_err)
        })
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            host::fs::write_file(&path_str, content).map_err(host_err)
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            let exists = host::fs::file_exists(&path_str).map_err(host_err)?;
            if exists {
                return Err(Error::new(ErrorKind::AlreadyExists, "File already exists"));
            }
            host::fs::write_file(&path_str, content).map_err(host_err)
        })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            host::fs::write_file(&path_str, "").map_err(host_err)
        })
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let prefix = dir.to_string_lossy();
            let files = host::fs::list_dir(&prefix).map_err(host_err)?;
            Ok(files
                .into_iter()
                .filter(|f| f.ends_with(".md"))
                .map(PathBuf::from)
                .collect())
        })
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            host::fs::file_exists(&path_str).unwrap_or(false)
        })
    }

    fn create_dir_all<'a>(&'a self, _path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { Ok(()) })
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move { path.extension().is_none() })
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let from_str = from.to_string_lossy();
            let to_str = to.to_string_lossy();
            let content = host::fs::read_file(&from_str).map_err(host_err)?;
            host::fs::write_file(&to_str, &content).map_err(host_err)?;
            let _ = host::fs::write_file(&from_str, "");
            Ok(())
        })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            host::fs::read_binary(&path_str).map_err(host_err)
        })
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            host::fs::write_binary(&path_str, content).map_err(host_err)
        })
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let prefix = dir.to_string_lossy();
            let files = host::fs::list_files(&prefix).map_err(host_err)?;
            Ok(files.into_iter().map(PathBuf::from).collect())
        })
    }
}
