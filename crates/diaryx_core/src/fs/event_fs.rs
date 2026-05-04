//! Event-emitting filesystem decorator.
//!
//! [`EventEmittingFs`] wraps an [`AsyncFileSystem`] and fires
//! [`FileSystemEvent`]s on writes / deletes / renames. Used by the Diaryx
//! UI to react to filesystem mutations.
//!
//! ```text
//! Operation → EventEmittingFs → Inner FS → emit Event → CallbackRegistry
//! ```
//!
//! Decorator is Diaryx-specific (the event vocabulary references
//! frontmatter, sync, etc.) and lives in `diaryx_core`, not in `crossfs`.

use std::io::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::frontmatter;
use crate::link_parser;
use crossfs::{AsyncFileSystem, BoxFuture, DirEntry, Metadata};

use super::callback_registry::{CallbackRegistry, EventCallback, SubscriptionId};
use super::events::FileSystemEvent;

/// Filesystem decorator that emits [`FileSystemEvent`]s on mutations.
pub struct EventEmittingFs<FS: AsyncFileSystem> {
    inner: FS,
    registry: Arc<CallbackRegistry>,
    enabled: AtomicBool,
}

impl<FS: AsyncFileSystem> EventEmittingFs<FS> {
    /// Wrap a filesystem.
    pub fn new(inner: FS) -> Self {
        Self {
            inner,
            registry: Arc::new(CallbackRegistry::new()),
            enabled: AtomicBool::new(true),
        }
    }

    /// Wrap a filesystem with a shared callback registry.
    pub fn with_registry(inner: FS, registry: Arc<CallbackRegistry>) -> Self {
        Self {
            inner,
            registry,
            enabled: AtomicBool::new(true),
        }
    }

    /// Whether event emission is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Toggle event emission. Disabled decorators still delegate writes;
    /// only the event side-effects are skipped.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    /// Subscribe to filesystem events. Returns a subscription ID.
    pub fn on_event(&self, callback: EventCallback) -> SubscriptionId {
        self.registry.subscribe(callback)
    }

    /// Unsubscribe. Returns `true` if the subscription existed.
    pub fn off_event(&self, id: SubscriptionId) -> bool {
        self.registry.unsubscribe(id)
    }

    /// Borrow the shared callback registry, e.g. to attach it to another
    /// decorator instance via [`with_registry`](Self::with_registry).
    pub fn registry(&self) -> &Arc<CallbackRegistry> {
        &self.registry
    }

    /// Borrow the wrapped filesystem.
    pub fn inner(&self) -> &FS {
        &self.inner
    }

    fn emit(&self, event: FileSystemEvent) {
        if self.is_enabled() {
            self.registry.emit(&event);
        }
    }

    fn extract_frontmatter(&self, content: &str) -> Option<serde_json::Value> {
        frontmatter::parse_or_empty(content)
            .ok()
            .and_then(|parsed| serde_json::to_value(&parsed.frontmatter).ok())
    }

    fn get_parent_from_content(&self, file_path: &Path, content: &str) -> Option<PathBuf> {
        frontmatter::parse_or_empty(content)
            .ok()
            .and_then(|parsed| {
                parsed
                    .frontmatter
                    .get("part_of")
                    .and_then(|v| v.as_str())
                    .map(|raw| {
                        let parsed = link_parser::parse_link(raw);
                        PathBuf::from(link_parser::to_canonical(&parsed, file_path))
                    })
            })
    }
}

impl<FS: AsyncFileSystem + Clone> Clone for EventEmittingFs<FS> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            registry: Arc::clone(&self.registry),
            enabled: AtomicBool::new(self.enabled.load(Ordering::SeqCst)),
        }
    }
}

impl<FS: AsyncFileSystem> std::fmt::Debug for EventEmittingFs<FS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventEmittingFs")
            .field("enabled", &self.is_enabled())
            .field("registry", &self.registry)
            .finish()
    }
}

// Native impl
#[cfg(not(target_arch = "wasm32"))]
impl<FS: AsyncFileSystem + Send + Sync> AsyncFileSystem for EventEmittingFs<FS> {
    fn read<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        self.inner.read(path)
    }

    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        self.inner.read_to_string(path)
    }

    fn read_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<DirEntry>>> {
        self.inner.read_dir(path)
    }

    fn write<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Only emit events for valid UTF-8 content (text files). Binary
            // writes pass through silently, matching the previous
            // `write_binary` behavior.
            let Ok(text) = std::str::from_utf8(contents) else {
                return self.inner.write(path, contents).await;
            };

            // Detect create vs update before writing.
            let old_frontmatter = if self.is_enabled() {
                self.inner
                    .read_to_string(path)
                    .await
                    .ok()
                    .and_then(|old| self.extract_frontmatter(&old))
            } else {
                None
            };
            let existed = old_frontmatter.is_some()
                || (!self.is_enabled() && self.inner.try_exists(path).await.unwrap_or(false));

            let result = self.inner.write(path, contents).await;

            if result.is_ok() {
                let new_frontmatter = self.extract_frontmatter(text);
                let parent_path = self.get_parent_from_content(path, text);

                if existed {
                    if let Some(new_fm) = new_frontmatter {
                        let changed = match &old_frontmatter {
                            Some(old_fm) => old_fm != &new_fm,
                            None => true,
                        };
                        if changed {
                            self.emit(FileSystemEvent::metadata_changed(
                                path.to_path_buf(),
                                new_fm,
                            ));
                        }
                    }
                } else {
                    self.emit(FileSystemEvent::file_created_with_metadata(
                        path.to_path_buf(),
                        new_frontmatter,
                        parent_path,
                    ));
                }
            }

            result
        })
    }

    fn create_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        self.inner.create_dir(path)
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        self.inner.create_dir_all(path)
    }

    fn remove_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Read parent_of from frontmatter before deletion.
            let parent_path = if self.is_enabled() {
                self.inner
                    .read_to_string(path)
                    .await
                    .ok()
                    .and_then(|content| self.get_parent_from_content(path, &content))
            } else {
                None
            };

            let result = self.inner.remove_file(path).await;

            if result.is_ok() {
                self.emit(FileSystemEvent::file_deleted_with_parent(
                    path.to_path_buf(),
                    parent_path,
                ));
            }

            result
        })
    }

    fn remove_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        self.inner.remove_dir(path)
    }

    fn remove_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        self.inner.remove_dir_all(path)
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let result = self.inner.rename(from, to).await;

            if result.is_ok() {
                let from_parent = from.parent();
                let to_parent = to.parent();

                if from_parent == to_parent {
                    self.emit(FileSystemEvent::file_renamed(
                        from.to_path_buf(),
                        to.to_path_buf(),
                    ));
                } else {
                    self.emit(FileSystemEvent::file_moved(
                        to.to_path_buf(),
                        from_parent.map(PathBuf::from),
                        to_parent.map(PathBuf::from),
                    ));
                }
            }

            result
        })
    }

    fn metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Metadata>> {
        self.inner.metadata(path)
    }

    fn symlink_metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Metadata>> {
        self.inner.symlink_metadata(path)
    }

    fn create_new<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let result = self.inner.create_new(path, contents).await;

            if result.is_ok() {
                if let Ok(text) = std::str::from_utf8(contents) {
                    let frontmatter = self.extract_frontmatter(text);
                    let parent_path = self.get_parent_from_content(path, text);

                    self.emit(FileSystemEvent::file_created_with_metadata(
                        path.to_path_buf(),
                        frontmatter,
                        parent_path,
                    ));
                } else {
                    self.emit(FileSystemEvent::file_created_with_metadata(
                        path.to_path_buf(),
                        None,
                        None,
                    ));
                }
            }

            result
        })
    }
}

// WASM impl (no Send + Sync)
#[cfg(target_arch = "wasm32")]
impl<FS: AsyncFileSystem> AsyncFileSystem for EventEmittingFs<FS> {
    fn read<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        self.inner.read(path)
    }

    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        self.inner.read_to_string(path)
    }

    fn read_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<DirEntry>>> {
        self.inner.read_dir(path)
    }

    fn write<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let Ok(text) = std::str::from_utf8(contents) else {
                return self.inner.write(path, contents).await;
            };

            let old_frontmatter = if self.is_enabled() {
                self.inner
                    .read_to_string(path)
                    .await
                    .ok()
                    .and_then(|old| self.extract_frontmatter(&old))
            } else {
                None
            };
            let existed = old_frontmatter.is_some()
                || (!self.is_enabled() && self.inner.try_exists(path).await.unwrap_or(false));

            let result = self.inner.write(path, contents).await;

            if result.is_ok() {
                let new_frontmatter = self.extract_frontmatter(text);
                let parent_path = self.get_parent_from_content(path, text);

                if existed {
                    if let Some(new_fm) = new_frontmatter {
                        let changed = match &old_frontmatter {
                            Some(old_fm) => old_fm != &new_fm,
                            None => true,
                        };
                        if changed {
                            self.emit(FileSystemEvent::metadata_changed(
                                path.to_path_buf(),
                                new_fm,
                            ));
                        }
                    }
                } else {
                    self.emit(FileSystemEvent::file_created_with_metadata(
                        path.to_path_buf(),
                        new_frontmatter,
                        parent_path,
                    ));
                }
            }

            result
        })
    }

    fn create_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        self.inner.create_dir(path)
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        self.inner.create_dir_all(path)
    }

    fn remove_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let parent_path = if self.is_enabled() {
                self.inner
                    .read_to_string(path)
                    .await
                    .ok()
                    .and_then(|content| self.get_parent_from_content(path, &content))
            } else {
                None
            };

            let result = self.inner.remove_file(path).await;

            if result.is_ok() {
                self.emit(FileSystemEvent::file_deleted_with_parent(
                    path.to_path_buf(),
                    parent_path,
                ));
            }

            result
        })
    }

    fn remove_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        self.inner.remove_dir(path)
    }

    fn remove_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        self.inner.remove_dir_all(path)
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let result = self.inner.rename(from, to).await;

            if result.is_ok() {
                let from_parent = from.parent();
                let to_parent = to.parent();

                if from_parent == to_parent {
                    self.emit(FileSystemEvent::file_renamed(
                        from.to_path_buf(),
                        to.to_path_buf(),
                    ));
                } else {
                    self.emit(FileSystemEvent::file_moved(
                        to.to_path_buf(),
                        from_parent.map(PathBuf::from),
                        to_parent.map(PathBuf::from),
                    ));
                }
            }

            result
        })
    }

    fn metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Metadata>> {
        self.inner.metadata(path)
    }

    fn symlink_metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Metadata>> {
        self.inner.symlink_metadata(path)
    }

    fn create_new<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let result = self.inner.create_new(path, contents).await;

            if result.is_ok() {
                if let Ok(text) = std::str::from_utf8(contents) {
                    let frontmatter = self.extract_frontmatter(text);
                    let parent_path = self.get_parent_from_content(path, text);

                    self.emit(FileSystemEvent::file_created_with_metadata(
                        path.to_path_buf(),
                        frontmatter,
                        parent_path,
                    ));
                } else {
                    self.emit(FileSystemEvent::file_created_with_metadata(
                        path.to_path_buf(),
                        None,
                        None,
                    ));
                }
            }

            result
        })
    }
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;
    use crate::fs::{InMemoryFileSystem, SyncToAsyncFs};
    use std::sync::atomic::AtomicUsize;

    fn create_test_event_fs() -> EventEmittingFs<SyncToAsyncFs<InMemoryFileSystem>> {
        let inner = SyncToAsyncFs::new(InMemoryFileSystem::new());
        EventEmittingFs::new(inner)
    }

    #[test]
    fn test_write_emits_file_created() {
        let fs = create_test_event_fs();
        let created_count = Arc::new(AtomicUsize::new(0));

        let counter = Arc::clone(&created_count);
        fs.on_event(Arc::new(move |event| {
            if matches!(event, FileSystemEvent::FileCreated { .. }) {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), "---\ntitle: Test\n---\nBody")
                .await
                .unwrap();
        });

        assert_eq!(created_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_write_existing_emits_metadata_changed_only_when_frontmatter_changes() {
        let fs = create_test_event_fs();
        let changed_count = Arc::new(AtomicUsize::new(0));

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), "---\ntitle: First\n---\nBody")
                .await
                .unwrap();
        });

        let counter = Arc::clone(&changed_count);
        fs.on_event(Arc::new(move |event| {
            if matches!(event, FileSystemEvent::MetadataChanged { .. }) {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), "---\ntitle: Updated\n---\nBody")
                .await
                .unwrap();
        });
        assert_eq!(changed_count.load(Ordering::SeqCst), 1);

        futures_lite::future::block_on(async {
            fs.write_file(
                Path::new("test.md"),
                "---\ntitle: Updated\n---\nBody changed!",
            )
            .await
            .unwrap();
        });
        assert_eq!(changed_count.load(Ordering::SeqCst), 1);

        futures_lite::future::block_on(async {
            fs.write_file(
                Path::new("test.md"),
                "---\ntitle: Final Title\n---\nBody changed!",
            )
            .await
            .unwrap();
        });
        assert_eq!(changed_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_delete_emits_file_deleted() {
        let fs = create_test_event_fs();
        let deleted_count = Arc::new(AtomicUsize::new(0));

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), "content")
                .await
                .unwrap();
        });

        let counter = Arc::clone(&deleted_count);
        fs.on_event(Arc::new(move |event| {
            if matches!(event, FileSystemEvent::FileDeleted { .. }) {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        futures_lite::future::block_on(async {
            fs.delete_file(Path::new("test.md")).await.unwrap();
        });

        assert_eq!(deleted_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_disabled_skips_events() {
        let fs = create_test_event_fs();
        fs.set_enabled(false);

        let event_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&event_count);
        fs.on_event(Arc::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test.md"), "content")
                .await
                .unwrap();
        });

        assert_eq!(event_count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_unsubscribe() {
        let fs = create_test_event_fs();
        let event_count = Arc::new(AtomicUsize::new(0));

        let counter = Arc::clone(&event_count);
        let id = fs.on_event(Arc::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test1.md"), "content")
                .await
                .unwrap();
        });
        assert_eq!(event_count.load(Ordering::SeqCst), 1);

        assert!(fs.off_event(id));

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("test2.md"), "content")
                .await
                .unwrap();
        });

        assert_eq!(event_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_move_same_parent_emits_renamed() {
        let fs = create_test_event_fs();
        let renamed_count = Arc::new(AtomicUsize::new(0));

        futures_lite::future::block_on(async {
            fs.write_file(Path::new("dir/old.md"), "content")
                .await
                .unwrap();
        });

        let counter = Arc::clone(&renamed_count);
        fs.on_event(Arc::new(move |event| {
            if matches!(event, FileSystemEvent::FileRenamed { .. }) {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        futures_lite::future::block_on(async {
            fs.move_file(Path::new("dir/old.md"), Path::new("dir/new.md"))
                .await
                .unwrap();
        });

        assert_eq!(renamed_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_move_different_parent_emits_moved() {
        let fs = create_test_event_fs();
        let moved_count = Arc::new(AtomicUsize::new(0));

        futures_lite::future::block_on(async {
            fs.create_dir_all(Path::new("dir1")).await.unwrap();
            fs.create_dir_all(Path::new("dir2")).await.unwrap();
            fs.write_file(Path::new("dir1/file.md"), "content")
                .await
                .unwrap();
        });

        let counter = Arc::clone(&moved_count);
        fs.on_event(Arc::new(move |event| {
            if matches!(event, FileSystemEvent::FileMoved { .. }) {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));

        futures_lite::future::block_on(async {
            fs.move_file(Path::new("dir1/file.md"), Path::new("dir2/file.md"))
                .await
                .unwrap();
        });

        assert_eq!(moved_count.load(Ordering::SeqCst), 1);
    }
}
