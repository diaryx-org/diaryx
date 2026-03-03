//! Host-bridged filesystem implementation for the publish Extism guest.
//!
//! `HostFs` implements `AsyncFileSystem` by delegating all operations to
//! host functions. Since host function calls are synchronous from the guest's
//! perspective, the async methods return immediately-ready futures.

use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use diaryx_core::fs::{AsyncFileSystem, BoxFuture};

use crate::host_bridge;

/// Filesystem backed by Extism host function calls.
#[derive(Clone)]
pub struct HostFs;

impl AsyncFileSystem for HostFs {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            host_bridge::read_file(&path_str).map_err(|e| Error::new(ErrorKind::Other, e))
        })
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            host_bridge::write_file(&path_str, content).map_err(|e| Error::new(ErrorKind::Other, e))
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            let exists =
                host_bridge::file_exists(&path_str).map_err(|e| Error::new(ErrorKind::Other, e))?;
            if exists {
                return Err(Error::new(ErrorKind::AlreadyExists, "File already exists"));
            }
            host_bridge::write_file(&path_str, content).map_err(|e| Error::new(ErrorKind::Other, e))
        })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            host_bridge::write_file(&path_str, "").map_err(|e| Error::new(ErrorKind::Other, e))
        })
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let prefix = dir.to_string_lossy();
            let files =
                host_bridge::list_files(&prefix).map_err(|e| Error::new(ErrorKind::Other, e))?;
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
            host_bridge::file_exists(&path_str).unwrap_or(false)
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
            let content =
                host_bridge::read_file(&from_str).map_err(|e| Error::new(ErrorKind::Other, e))?;
            host_bridge::write_file(&to_str, &content)
                .map_err(|e| Error::new(ErrorKind::Other, e))?;
            let _ = host_bridge::write_file(&from_str, "");
            Ok(())
        })
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            host_bridge::write_binary(&path_str, content)
                .map_err(|e| Error::new(ErrorKind::Other, e))
        })
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let prefix = dir.to_string_lossy();
            let files =
                host_bridge::list_files(&prefix).map_err(|e| Error::new(ErrorKind::Other, e))?;
            Ok(files.into_iter().map(PathBuf::from).collect())
        })
    }
}
