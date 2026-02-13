#![cfg(all(target_arch = "wasm32", feature = "browser"))]

use std::io::ErrorKind;
use std::path::Path;

use diaryx_core::fs::AsyncFileSystem;
use diaryx_wasm::OpfsFileSystem;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

async fn create_isolated_fs(test_name: &str) -> OpfsFileSystem {
    let now_ms = js_sys::Date::now() as u64;
    let rand_suffix = (js_sys::Math::random() * 1_000_000_000.0) as u64;
    let root_name = format!("diaryx-opfs-test-{test_name}-{now_ms}-{rand_suffix}");
    OpfsFileSystem::create_with_name(&root_name)
        .await
        .expect("OPFS test filesystem should be created")
}

fn sorted_paths(paths: Vec<std::path::PathBuf>) -> Vec<String> {
    let mut values: Vec<String> = paths
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    values.sort();
    values
}

#[wasm_bindgen_test(async)]
async fn opfs_exists_covers_directories_and_files() {
    let fs = create_isolated_fs("exists").await;

    fs.create_dir_all(Path::new("notes/2026"))
        .await
        .expect("directory creation should succeed");
    fs.write_file(Path::new("notes/2026/today.md"), "# Today")
        .await
        .expect("file write should succeed");

    assert!(fs.exists(Path::new("notes")).await);
    assert!(fs.exists(Path::new("notes/2026")).await);
    assert!(fs.exists(Path::new("notes/2026/today.md")).await);
    assert!(fs.is_dir(Path::new("notes")).await);
    assert!(fs.is_dir(Path::new("notes/2026")).await);
}

#[wasm_bindgen_test(async)]
async fn opfs_move_file_errors_when_destination_exists() {
    let fs = create_isolated_fs("move-conflict").await;

    fs.write_file(Path::new("a/from.md"), "from")
        .await
        .expect("source write should succeed");
    fs.write_file(Path::new("a/to.md"), "to")
        .await
        .expect("destination write should succeed");

    let err = fs
        .move_file(Path::new("a/from.md"), Path::new("a/to.md"))
        .await
        .expect_err("move_file should fail when destination exists");
    assert_eq!(err.kind(), ErrorKind::AlreadyExists);

    let from_content = fs
        .read_to_string(Path::new("a/from.md"))
        .await
        .expect("source should remain unchanged");
    let to_content = fs
        .read_to_string(Path::new("a/to.md"))
        .await
        .expect("destination should remain unchanged");
    assert_eq!(from_content, "from");
    assert_eq!(to_content, "to");
}

#[wasm_bindgen_test(async)]
async fn opfs_move_file_happy_path_and_missing_source() {
    let fs = create_isolated_fs("move-happy").await;

    let missing_err = fs
        .move_file(Path::new("missing.md"), Path::new("dest.md"))
        .await
        .expect_err("moving a missing source should fail");
    assert_eq!(missing_err.kind(), ErrorKind::NotFound);

    fs.write_file(Path::new("entry.md"), "entry")
        .await
        .expect("source write should succeed");
    fs.move_file(Path::new("entry.md"), Path::new("archive/entry.md"))
        .await
        .expect("move should succeed");

    assert!(!fs.exists(Path::new("entry.md")).await);
    assert!(fs.exists(Path::new("archive/entry.md")).await);
    let moved = fs
        .read_to_string(Path::new("archive/entry.md"))
        .await
        .expect("destination should be readable");
    assert_eq!(moved, "entry");
}

#[wasm_bindgen_test(async)]
async fn opfs_create_new_reports_already_exists() {
    let fs = create_isolated_fs("create-new").await;

    fs.create_new(Path::new("notes/new.md"), "# First")
        .await
        .expect("initial create_new should succeed");
    let err = fs
        .create_new(Path::new("notes/new.md"), "# Second")
        .await
        .expect_err("second create_new should fail");

    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
    let content = fs
        .read_to_string(Path::new("notes/new.md"))
        .await
        .expect("existing file should remain");
    assert_eq!(content, "# First");
}

#[wasm_bindgen_test(async)]
async fn opfs_list_files_and_md_files_match_direct_children() {
    let fs = create_isolated_fs("list-files").await;

    fs.create_dir_all(Path::new("workspace/sub"))
        .await
        .expect("subdir create should succeed");
    fs.write_file(Path::new("workspace/alpha.md"), "# Alpha")
        .await
        .expect("write should succeed");
    fs.write_file(Path::new("workspace/note.txt"), "note")
        .await
        .expect("write should succeed");
    fs.write_file(Path::new("workspace/sub/deep.md"), "# Deep")
        .await
        .expect("write should succeed");

    let files = sorted_paths(
        fs.list_files(Path::new("workspace"))
            .await
            .expect("list_files should succeed"),
    );
    assert_eq!(
        files,
        vec!["workspace/alpha.md", "workspace/note.txt", "workspace/sub"]
    );

    let md_files = sorted_paths(
        fs.list_md_files(Path::new("workspace"))
            .await
            .expect("list_md_files should succeed"),
    );
    assert_eq!(md_files, vec!["workspace/alpha.md"]);
}

#[wasm_bindgen_test(async)]
async fn opfs_binary_roundtrip() {
    let fs = create_isolated_fs("binary").await;
    let data = vec![0_u8, 1, 2, 255, 128, 42];

    fs.write_binary(Path::new("assets/image.bin"), &data)
        .await
        .expect("binary write should succeed");

    let loaded = fs
        .read_binary(Path::new("assets/image.bin"))
        .await
        .expect("binary read should succeed");
    assert_eq!(loaded, data);
}
