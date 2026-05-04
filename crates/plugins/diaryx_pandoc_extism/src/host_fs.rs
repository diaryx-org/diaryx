//! Host-bridged filesystem implementation for the pandoc Extism guest.

use std::io::{Error, ErrorKind, Result};
use std::path::Path;

use diaryx_core::fs::{AsyncFileSystem, BoxFuture, DirEntry, FileType, Metadata};
use diaryx_plugin_sdk::host;

fn host_err(e: String) -> Error {
    Error::new(ErrorKind::Other, e)
}

/// Filesystem backed by Extism host function calls.
#[derive(Clone)]
pub struct HostFs;

impl AsyncFileSystem for HostFs {
    fn read<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            host::fs::read_binary(&path_str).map_err(host_err)
        })
    }

    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            host::fs::read_file(&path_str).map_err(host_err)
        })
    }

    fn read_dir<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<DirEntry>>> {
        Box::pin(async move {
            let prefix = dir.to_string_lossy();
            let files = host::fs::list_files(&prefix).map_err(host_err)?;
            Ok(files
                .into_iter()
                .map(|f| {
                    let p = std::path::PathBuf::from(f);
                    let ft = if p.extension().is_none() {
                        FileType::dir()
                    } else {
                        FileType::file()
                    };
                    DirEntry::new(p, ft)
                })
                .collect())
        })
    }

    fn write<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            if let Ok(s) = std::str::from_utf8(contents) {
                host::fs::write_file(&path_str, s).map_err(host_err)
            } else {
                host::fs::write_binary(&path_str, contents).map_err(host_err)
            }
        })
    }

    fn create_dir<'a>(&'a self, _path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { Ok(()) })
    }

    fn create_dir_all<'a>(&'a self, _path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { Ok(()) })
    }

    fn remove_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            host::fs::write_file(&path_str, "").map_err(host_err)
        })
    }

    fn remove_dir<'a>(&'a self, _path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            Err(Error::new(
                ErrorKind::Unsupported,
                "remove_dir not supported in pandoc plugin",
            ))
        })
    }

    fn remove_dir_all<'a>(&'a self, _path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            Err(Error::new(
                ErrorKind::Unsupported,
                "remove_dir_all not supported in pandoc plugin",
            ))
        })
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let from_str = from.to_string_lossy();
            let to_str = to.to_string_lossy();
            let content = host::fs::read_file(&from_str).map_err(host_err)?;
            host::fs::write_file(&to_str, &content).map_err(host_err)?;
            let _ = host::fs::write_file(&from_str, "");
            Ok(())
        })
    }

    fn metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Metadata>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            let exists = host::fs::file_exists(&path_str).map_err(host_err)?;
            if !exists {
                return Err(Error::new(ErrorKind::NotFound, "Not found"));
            }
            let ft = if path.extension().is_none() {
                FileType::dir()
            } else {
                FileType::file()
            };
            Ok(Metadata::new(ft, 0, None))
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let path_str = path.to_string_lossy();
            let exists = host::fs::file_exists(&path_str).map_err(host_err)?;
            if exists {
                return Err(Error::new(ErrorKind::AlreadyExists, "File already exists"));
            }
            if let Ok(s) = std::str::from_utf8(contents) {
                host::fs::write_file(&path_str, s).map_err(host_err)
            } else {
                host::fs::write_binary(&path_str, contents).map_err(host_err)
            }
        })
    }
}
