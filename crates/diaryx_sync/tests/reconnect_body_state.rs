use std::sync::Arc;

use diaryx_core::fs::{InMemoryFileSystem, SyncToAsyncFs};
use diaryx_sync::{
    BodyDocManager, CrdtStorage, MemoryStorage, RustSyncManager, SyncHandler, WorkspaceCrdt,
};

fn create_test_manager() -> RustSyncManager<SyncToAsyncFs<InMemoryFileSystem>> {
    let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
    let workspace_crdt = Arc::new(WorkspaceCrdt::new(Arc::clone(&storage)));
    let body_manager = Arc::new(BodyDocManager::new(Arc::clone(&storage)));
    let fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
    let sync_handler = Arc::new(SyncHandler::new(fs));

    RustSyncManager::new(workspace_crdt, body_manager, sync_handler)
}

#[tokio::test]
async fn remote_update_marks_body_as_synced() {
    let source = create_test_manager();
    let target = create_test_manager();
    target.set_focused_files(&["test.md".to_string()]);

    let update = source
        .create_body_update("test.md", "hello from source")
        .unwrap();

    target
        .handle_body_message("test.md", &update, false)
        .await
        .unwrap();

    assert!(
        !target.body_state_changed("test.md"),
        "remote Update messages should advance the synced baseline"
    );
}

#[test]
fn reconnect_resend_clears_body_dirty_flag() {
    let manager = create_test_manager();

    let initial = manager
        .create_body_update("test.md", "local offline edit")
        .unwrap();
    assert!(!initial.is_empty());

    assert!(manager.body_state_changed("test.md"));

    let update = manager.encode_full_body_update("test.md");
    assert!(update.is_some());
    assert!(
        !manager.body_state_changed("test.md"),
        "reconnect resend should not leave the same body marked dirty forever"
    );
}
