#![cfg(all(feature = "crdt", not(target_arch = "wasm32")))]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use diaryx_core::crdt::{BodyDocManager, FileMetadata, WorkspaceCrdt, parse_snapshot_markdown};
use diaryx_core::fs::{
    AsyncFileSystem, CrdtFs, DecoratedFsBuilder, EventEmittingFs, InMemoryFileSystem,
    RealFileSystem, SyncToAsyncFs,
};
use diaryx_core::path_utils::normalize_sync_path;
use futures_lite::future::block_on;

type InMemoryAsyncFs = SyncToAsyncFs<InMemoryFileSystem>;
type NativeAsyncFs = SyncToAsyncFs<RealFileSystem>;

#[derive(Debug, Clone, PartialEq)]
struct FileState {
    metadata: FileMetadata,
    body: String,
}

#[derive(Debug, Clone, PartialEq)]
struct BackendSnapshot {
    active: BTreeMap<String, FileState>,
    tombstones: BTreeSet<String>,
    disk_markdown: BTreeMap<String, String>,
    sync_suppressed_exists_on_disk: bool,
}

fn relativize_sync_path(value: &str, root_prefix: &str) -> String {
    let normalized = normalize_sync_path(value);
    if normalized == root_prefix {
        String::new()
    } else if let Some(rest) = normalized.strip_prefix(&format!("{root_prefix}/")) {
        rest.to_string()
    } else {
        normalized
    }
}

fn normalize_metadata_paths(metadata: &FileMetadata, root_prefix: &str) -> FileMetadata {
    let mut normalized = metadata.clone();

    normalized.part_of = normalized
        .part_of
        .as_ref()
        .map(|p| relativize_sync_path(p, root_prefix));

    if let Some(contents) = &mut normalized.contents {
        for item in contents {
            *item = relativize_sync_path(item, root_prefix);
        }
    }

    for attachment in &mut normalized.attachments {
        attachment.path = relativize_sync_path(&attachment.path, root_prefix);
    }

    normalized
}

fn relative_disk_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| normalize_sync_path(&p.to_string_lossy()))
        .unwrap_or_else(|_| normalize_sync_path(&path.to_string_lossy()))
}

fn collect_snapshot<FS: AsyncFileSystem + Clone + Send + Sync>(
    fs: &EventEmittingFs<CrdtFs<FS>>,
    workspace_crdt: &Arc<WorkspaceCrdt>,
    body_docs: &Arc<BodyDocManager>,
    root: &Path,
    sync_suppressed_rel: &str,
) -> BackendSnapshot {
    let root_prefix = normalize_sync_path(&root.to_string_lossy());
    let mut disk_markdown = BTreeMap::new();
    let mut disk_parsed = BTreeMap::new();

    let entries = block_on(fs.list_all_files_recursive(root)).expect("recursive listing failed");
    for entry in entries {
        if entry.extension().is_some_and(|ext| ext == "md") {
            let rel = relative_disk_path(root, &entry);
            let content = block_on(fs.read_to_string(&entry)).expect("disk read failed");
            let (metadata, body) = parse_snapshot_markdown(&entry.to_string_lossy(), &content)
                .expect("markdown parse failed");
            disk_markdown.insert(rel.clone(), content);
            disk_parsed.insert(
                rel,
                FileState {
                    metadata: normalize_metadata_paths(&metadata, &root_prefix),
                    body,
                },
            );
        }
    }

    let mut active = BTreeMap::new();
    for (key, metadata) in workspace_crdt.list_active_files() {
        let rel = relativize_sync_path(&key, &root_prefix);
        let body = body_docs
            .get(&key)
            .expect("body doc should exist for active key")
            .get_body();
        active.insert(
            rel,
            FileState {
                metadata: normalize_metadata_paths(&metadata, &root_prefix),
                body,
            },
        );
    }

    let tombstones = workspace_crdt
        .list_files()
        .into_iter()
        .filter(|(_, metadata)| metadata.deleted)
        .map(|(key, _)| relativize_sync_path(&key, &root_prefix))
        .collect::<BTreeSet<_>>();

    let sync_suppressed_exists_on_disk = disk_markdown.contains_key(sync_suppressed_rel);
    assert!(
        sync_suppressed_exists_on_disk,
        "sync-suppressed file should exist on disk"
    );
    assert!(
        !active.contains_key(sync_suppressed_rel),
        "sync-suppressed file should not be present in active CRDT state"
    );

    let disk_active_paths = disk_markdown
        .keys()
        .filter(|path| path.as_str() != sync_suppressed_rel)
        .cloned()
        .collect::<BTreeSet<_>>();
    let active_paths = active.keys().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        active_paths, disk_active_paths,
        "active CRDT paths must match disk markdown paths (excluding sync-suppressed path)"
    );

    for path in &active_paths {
        let disk_state = disk_parsed
            .get(path)
            .expect("disk parsed state missing for active path");
        let crdt_state = active.get(path).expect("CRDT active state missing");
        assert_eq!(
            disk_state, crdt_state,
            "disk metadata/body should match CRDT state for {path}"
        );
    }

    BackendSnapshot {
        active,
        tombstones,
        disk_markdown,
        sync_suppressed_exists_on_disk,
    }
}

fn run_crdt_scenario<FS: AsyncFileSystem + Clone + Send + Sync>(
    base: FS,
    root: PathBuf,
) -> BackendSnapshot {
    let decorated = DecoratedFsBuilder::new(base).crdt_enabled(true).build();
    let fs = decorated.fs.clone();
    let workspace_crdt = Arc::clone(&decorated.workspace_crdt);
    let body_docs = Arc::clone(&decorated.body_doc_manager);

    let root_index = root.join("README.md");
    let today = root.join("notes/today.md");
    let keep = root.join("notes/keep.md");
    let moved = root.join("archive/today.md");
    let sync_suppressed = root.join("remote-only.md");
    let sync_suppressed_rel = "remote-only.md";

    block_on(async {
        fs.create_dir_all(&root).await.unwrap();

        fs.create_new(
            &root_index,
            "---\ntitle: Root\nupdated: 1700100000000\n---\n\nRoot body",
        )
        .await
        .unwrap();

        fs.write_file(
            &today,
            "---\ntitle: Today\npart_of: ../README.md\nupdated: 1700100001000\n---\n\nToday body",
        )
        .await
        .unwrap();

        fs.write_file(
            &keep,
            "---\ntitle: Keep\npart_of: ../README.md\nupdated: 1700100002000\n---\n\nKeep body",
        )
        .await
        .unwrap();

        fs.move_file(&today, &moved).await.unwrap();

        fs.write_file(
            &moved,
            "---\ntitle: Today (Moved)\npart_of: ../README.md\nupdated: 1700100003000\n---\n\nMoved body",
        )
        .await
        .unwrap();

        fs.delete_file(&keep).await.unwrap();

        fs.mark_sync_write_start(&sync_suppressed);
        fs.write_file(
            &sync_suppressed,
            "---\ntitle: Remote Only\nupdated: 1700100004000\n---\n\nsuppressed",
        )
        .await
        .unwrap();
        fs.mark_sync_write_end(&sync_suppressed);
    });

    collect_snapshot(&fs, &workspace_crdt, &body_docs, &root, sync_suppressed_rel)
}

#[test]
fn crdt_fs_behavior_matches_between_inmemory_and_native_backends() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let root = tempdir.path().join("workspace");

    let in_memory_snapshot = run_crdt_scenario(
        InMemoryAsyncFs::new(InMemoryFileSystem::new()),
        root.clone(),
    );
    let native_snapshot = run_crdt_scenario(NativeAsyncFs::new(RealFileSystem), root);

    assert_eq!(
        in_memory_snapshot.active, native_snapshot.active,
        "active metadata/body state should match across filesystem backends"
    );
    assert_eq!(
        in_memory_snapshot.tombstones, native_snapshot.tombstones,
        "deleted/tombstoned entries should match across filesystem backends"
    );
    assert_eq!(
        in_memory_snapshot.disk_markdown, native_snapshot.disk_markdown,
        "final markdown content on disk should match across filesystem backends"
    );
    assert!(
        in_memory_snapshot.sync_suppressed_exists_on_disk
            && native_snapshot.sync_suppressed_exists_on_disk,
        "sync-suppressed path should exist on disk in both backends"
    );
}
