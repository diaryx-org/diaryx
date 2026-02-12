#![cfg(feature = "crdt")]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use diaryx_core::command::{Command, Response};
use diaryx_core::crdt::{BodyDocManager, FileMetadata, WorkspaceCrdt, parse_snapshot_markdown};
use diaryx_core::diaryx::Diaryx;
use diaryx_core::fs::{
    AsyncFileSystem, CrdtFs, DecoratedFsBuilder, EventEmittingFs, FileSystem, InMemoryFileSystem,
    SyncToAsyncFs,
};
use diaryx_core::path_utils::normalize_sync_path;
use futures_lite::future::block_on;

type BaseFs = SyncToAsyncFs<InMemoryFileSystem>;
type StackFs = EventEmittingFs<CrdtFs<BaseFs>>;

struct TestNode {
    diaryx: Diaryx<StackFs>,
    fs: StackFs,
    base_fs: BaseFs,
    workspace_crdt: Arc<WorkspaceCrdt>,
    body_docs: Arc<BodyDocManager>,
}

impl TestNode {
    fn new() -> Self {
        let base = SyncToAsyncFs::new(InMemoryFileSystem::new());
        let decorated = DecoratedFsBuilder::new(base).build();
        let fs = decorated.fs.clone();
        let base_fs = decorated.base_fs().clone();
        let workspace_crdt = Arc::clone(&decorated.workspace_crdt);
        let body_docs = Arc::clone(&decorated.body_doc_manager);
        let diaryx = Diaryx::with_crdt_instances(
            fs.clone(),
            Arc::clone(&workspace_crdt),
            Arc::clone(&body_docs),
        );

        Self {
            diaryx,
            fs,
            base_fs,
            workspace_crdt,
            body_docs,
        }
    }
}

fn execute(node: &TestNode, command: Command) -> Response {
    block_on(node.diaryx.execute(command)).expect("command should succeed")
}

fn expect_binary(response: Response, context: &str) -> Vec<u8> {
    match response {
        Response::Binary(bytes) => bytes,
        other => panic!("expected binary response for {context}, got {other:?}"),
    }
}

fn assert_metadata_matches(path: &str, actual: &FileMetadata, expected: &FileMetadata) {
    assert_eq!(
        actual.filename, expected.filename,
        "filename mismatch for {path}"
    );
    assert_eq!(actual.title, expected.title, "title mismatch for {path}");
    assert_eq!(
        actual.part_of, expected.part_of,
        "part_of mismatch for {path}"
    );
    assert_eq!(
        actual.contents, expected.contents,
        "contents mismatch for {path}"
    );
    assert_eq!(
        actual.audience, expected.audience,
        "audience mismatch for {path}"
    );
    assert_eq!(
        actual.description, expected.description,
        "description mismatch for {path}"
    );
    assert_eq!(
        actual.deleted, expected.deleted,
        "deleted mismatch for {path}"
    );
    assert_eq!(
        actual.modified_at, expected.modified_at,
        "modified_at mismatch for {path}"
    );

    let mut actual_attachment_paths = actual
        .attachments
        .iter()
        .map(|a| normalize_sync_path(&a.path))
        .collect::<Vec<_>>();
    let mut expected_attachment_paths = expected
        .attachments
        .iter()
        .map(|a| normalize_sync_path(&a.path))
        .collect::<Vec<_>>();
    actual_attachment_paths.sort();
    expected_attachment_paths.sort();
    assert_eq!(
        actual_attachment_paths, expected_attachment_paths,
        "attachment paths mismatch for {path}"
    );
}

fn disk_markdown_files(node: &TestNode) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();

    for path in node.base_fs.inner().list_all_files() {
        if path.extension().is_some_and(|ext| ext == "md") {
            let normalized = normalize_sync_path(&path.to_string_lossy());
            let content = node
                .base_fs
                .inner()
                .read_to_string(&path)
                .expect("disk file should be readable");
            files.insert(normalized, content);
        }
    }

    files
}

fn active_crdt_paths(node: &TestNode) -> BTreeSet<String> {
    node.workspace_crdt
        .list_active_files()
        .into_iter()
        .map(|(path, _)| normalize_sync_path(&path))
        .collect()
}

fn assert_disk_matches_crdt(node: &TestNode) {
    let disk_files = disk_markdown_files(node);
    let disk_paths: BTreeSet<String> = disk_files.keys().cloned().collect();
    let crdt_paths = active_crdt_paths(node);

    assert_eq!(
        disk_paths, crdt_paths,
        "active CRDT paths must match markdown files on disk"
    );

    for (path, disk_content) in disk_files {
        let crdt_metadata = node
            .workspace_crdt
            .get_file(&path)
            .expect("CRDT metadata should exist for disk file");
        assert!(
            !crdt_metadata.deleted,
            "active disk file should not be tombstoned in CRDT: {path}"
        );

        let (disk_metadata, disk_body) =
            parse_snapshot_markdown(&path, &disk_content).expect("disk markdown should parse");
        assert_metadata_matches(&path, &crdt_metadata, &disk_metadata);

        let body_doc = node
            .body_docs
            .get(&path)
            .expect("body doc should exist for active file");
        assert_eq!(
            body_doc.get_body(),
            disk_body,
            "body doc mismatch for {path}"
        );
    }
}

fn assert_replicas_match(source: &TestNode, target: &TestNode) {
    let source_paths = active_crdt_paths(source);
    let target_paths = active_crdt_paths(target);
    assert_eq!(
        source_paths, target_paths,
        "replicas should have same paths"
    );

    for path in source_paths {
        let source_meta = source
            .workspace_crdt
            .get_file(&path)
            .expect("source metadata missing");
        let target_meta = target
            .workspace_crdt
            .get_file(&path)
            .expect("target metadata missing");
        assert_metadata_matches(&path, &source_meta, &target_meta);

        let source_body = source
            .body_docs
            .get(&path)
            .expect("source body doc missing")
            .get_body();
        let target_body = target
            .body_docs
            .get(&path)
            .expect("target body doc missing")
            .get_body();
        assert_eq!(source_body, target_body, "body mismatch for {path}");
    }
}

fn sync_source_to_target(source: &TestNode, target: &TestNode) {
    let target_state = expect_binary(
        execute(
            target,
            Command::GetSyncState {
                doc_name: "workspace".to_string(),
            },
        ),
        "GetSyncState",
    );

    let workspace_update = expect_binary(
        execute(
            source,
            Command::GetMissingUpdates {
                doc_name: "workspace".to_string(),
                remote_state_vector: target_state,
            },
        ),
        "GetMissingUpdates",
    );

    if !workspace_update.is_empty() {
        match execute(
            target,
            Command::ApplyRemoteWorkspaceUpdateWithEffects {
                update: workspace_update,
                write_to_disk: true,
            },
        ) {
            Response::UpdateId(_) => {}
            other => panic!(
                "expected update-id response for ApplyRemoteWorkspaceUpdateWithEffects, got {other:?}"
            ),
        }
    }

    let mut active_paths = source
        .workspace_crdt
        .list_active_files()
        .into_iter()
        .map(|(path, _)| normalize_sync_path(&path))
        .collect::<Vec<_>>();
    active_paths.sort();

    for path in active_paths {
        let body_state = expect_binary(
            execute(
                source,
                Command::GetBodyFullState {
                    doc_name: path.clone(),
                },
            ),
            "GetBodyFullState",
        );

        if body_state.is_empty() {
            continue;
        }

        match execute(
            target,
            Command::ApplyRemoteBodyUpdateWithEffects {
                doc_name: path,
                update: body_state,
                write_to_disk: true,
            },
        ) {
            Response::UpdateId(_) => {}
            other => panic!(
                "expected update-id response for ApplyRemoteBodyUpdateWithEffects, got {other:?}"
            ),
        }
    }
}

#[test]
fn local_file_ops_keep_disk_and_crdt_in_sync() {
    let node = TestNode::new();

    block_on(async {
        node.fs
            .write_file(
                Path::new("README.md"),
                "---\ntitle: Root\nupdated: 1700000000000\n---\n\nRoot body",
            )
            .await
            .unwrap();
        node.fs
            .write_file(
                Path::new("notes/today.md"),
                "---\ntitle: Today\npart_of: ../README.md\nupdated: 1700000001000\n---\n\nToday body",
            )
            .await
            .unwrap();
        node.fs
            .write_file(
                Path::new("notes/keep.md"),
                "---\ntitle: Keep\npart_of: ../README.md\nupdated: 1700000002000\n---\n\nKeep body",
            )
            .await
            .unwrap();
    });
    assert_disk_matches_crdt(&node);

    block_on(async {
        node.fs
            .move_file(Path::new("notes/today.md"), Path::new("archive/today.md"))
            .await
            .unwrap();
        node.fs
            .write_file(
                Path::new("archive/today.md"),
                "---\ntitle: Today (Moved)\npart_of: ../README.md\nupdated: 1700000003000\n---\n\nUpdated body after move",
            )
            .await
            .unwrap();
        node.fs
            .delete_file(Path::new("notes/keep.md"))
            .await
            .unwrap();
    });

    assert_disk_matches_crdt(&node);
    assert!(
        !node.base_fs.inner().exists(Path::new("notes/today.md")),
        "old path should not exist on disk after move"
    );
    assert!(
        node.workspace_crdt
            .get_file("notes/today.md")
            .map(|m| m.deleted)
            .unwrap_or(false),
        "old path should be tombstoned in CRDT after move"
    );
    assert!(
        node.workspace_crdt
            .get_file("notes/keep.md")
            .map(|m| m.deleted)
            .unwrap_or(false),
        "deleted file should be tombstoned in CRDT"
    );
}

#[test]
fn remote_sync_with_effects_preserves_disk_crdt_parity() {
    let source = TestNode::new();
    let target = TestNode::new();

    block_on(async {
        source
            .fs
            .write_file(
                Path::new("README.md"),
                "---\ntitle: Root\nupdated: 1700000100000\n---\n\nShared root",
            )
            .await
            .unwrap();
        source
            .fs
            .write_file(
                Path::new("notes/today.md"),
                "---\ntitle: Today\npart_of: ../README.md\nupdated: 1700000101000\n---\n\nSource body",
            )
            .await
            .unwrap();
        source
            .fs
            .write_file(
                Path::new("notes/keep.md"),
                "---\ntitle: Keep\npart_of: ../README.md\nupdated: 1700000102000\n---\n\nKeep me",
            )
            .await
            .unwrap();
    });

    sync_source_to_target(&source, &target);
    assert_disk_matches_crdt(&source);
    assert_disk_matches_crdt(&target);
    assert_replicas_match(&source, &target);

    block_on(async {
        source
            .fs
            .move_file(Path::new("notes/today.md"), Path::new("archive/today.md"))
            .await
            .unwrap();
        source
            .fs
            .write_file(
                Path::new("archive/today.md"),
                "---\ntitle: Today (Renamed)\npart_of: ../README.md\nupdated: 1700000103000\n---\n\nRenamed body",
            )
            .await
            .unwrap();
        source
            .fs
            .delete_file(Path::new("notes/keep.md"))
            .await
            .unwrap();
    });

    sync_source_to_target(&source, &target);
    assert_disk_matches_crdt(&source);
    assert_disk_matches_crdt(&target);
    assert_replicas_match(&source, &target);

    assert!(
        !target.base_fs.inner().exists(Path::new("notes/today.md")),
        "target should not retain old renamed path"
    );
    assert!(
        !target.base_fs.inner().exists(Path::new("notes/keep.md")),
        "target should delete files removed from source"
    );
    assert!(
        target.base_fs.inner().exists(Path::new("archive/today.md")),
        "target should contain renamed path"
    );
}
