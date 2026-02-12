#![cfg(all(target_arch = "wasm32", feature = "browser"))]

use std::io::ErrorKind;
use std::path::PathBuf;

use diaryx_core::fs::AsyncFileSystem;
use diaryx_wasm::{FsaFileSystem, IndexedDbFileSystem, OpfsFileSystem};
use js_sys::Reflect;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn unique_suffix(label: &str) -> String {
    let now_ms = js_sys::Date::now() as u64;
    let rand_suffix = (js_sys::Math::random() * 1_000_000_000.0) as u64;
    format!("{label}-{now_ms}-{rand_suffix}")
}

fn sorted_paths(paths: Vec<PathBuf>) -> Vec<String> {
    let mut values: Vec<String> = paths
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    values.sort();
    values
}

async fn get_opfs_root() -> web_sys::FileSystemDirectoryHandle {
    let global = js_sys::global();
    let navigator = Reflect::get(&global, &JsValue::from_str("navigator"))
        .expect("navigator should exist in browser");
    let storage =
        Reflect::get(&navigator, &JsValue::from_str("storage")).expect("navigator.storage");
    let get_directory = Reflect::get(&storage, &JsValue::from_str("getDirectory"))
        .expect("storage.getDirectory should exist");
    let get_directory_fn = get_directory
        .dyn_ref::<js_sys::Function>()
        .expect("getDirectory should be a function");

    let promise = get_directory_fn
        .call0(&storage)
        .expect("getDirectory() call should succeed")
        .dyn_into::<js_sys::Promise>()
        .expect("getDirectory() should return a Promise");

    JsFuture::from(promise)
        .await
        .expect("getDirectory() promise should resolve")
        .dyn_into::<web_sys::FileSystemDirectoryHandle>()
        .expect("resolved value should be a FileSystemDirectoryHandle")
}

async fn create_fsa_root_handle(name: &str) -> web_sys::FileSystemDirectoryHandle {
    let root = get_opfs_root().await;
    let options = js_sys::Object::new();
    Reflect::set(
        &options,
        &JsValue::from_str("create"),
        &JsValue::from_bool(true),
    )
    .expect("options.create should be set");

    let get_directory_handle =
        Reflect::get(root.as_ref(), &JsValue::from_str("getDirectoryHandle"))
            .expect("directory handle should expose getDirectoryHandle");
    let get_directory_handle_fn = get_directory_handle
        .dyn_ref::<js_sys::Function>()
        .expect("getDirectoryHandle should be a function");

    let promise = get_directory_handle_fn
        .call2(root.as_ref(), &JsValue::from_str(name), &options.into())
        .expect("getDirectoryHandle() call should succeed")
        .dyn_into::<js_sys::Promise>()
        .expect("getDirectoryHandle() should return a Promise");

    JsFuture::from(promise)
        .await
        .expect("getDirectoryHandle() promise should resolve")
        .dyn_into::<web_sys::FileSystemDirectoryHandle>()
        .expect("resolved value should be a FileSystemDirectoryHandle")
}

async fn assert_fs_contract<FS: AsyncFileSystem>(fs: &FS, root: &str) {
    let root_dir = PathBuf::from(root);
    let dirs = root_dir.join("dirs/a");

    fs.create_dir_all(&dirs)
        .await
        .expect("create_dir_all should succeed");
    assert!(fs.exists(&root_dir.join("dirs")).await);
    assert!(fs.exists(&dirs).await);
    assert!(fs.is_dir(&root_dir.join("dirs")).await);

    let create_new_path = root_dir.join("dup.md");
    fs.create_new(&create_new_path, "# First")
        .await
        .expect("first create_new should succeed");
    let create_new_err = fs
        .create_new(&create_new_path, "# Second")
        .await
        .expect_err("second create_new should fail");
    assert_eq!(create_new_err.kind(), ErrorKind::AlreadyExists);

    fs.write_file(&root_dir.join("list/one.md"), "# One")
        .await
        .expect("write should succeed");
    fs.write_file(&root_dir.join("list/two.txt"), "two")
        .await
        .expect("write should succeed");
    fs.write_file(&root_dir.join("list/sub/three.md"), "# Three")
        .await
        .expect("write should succeed");

    let list_files = sorted_paths(
        fs.list_files(&root_dir.join("list"))
            .await
            .expect("list_files should succeed"),
    );
    let expected_files = sorted_paths(vec![
        root_dir.join("list/one.md"),
        root_dir.join("list/two.txt"),
        root_dir.join("list/sub"),
    ]);
    assert_eq!(list_files, expected_files);

    let list_md_files = sorted_paths(
        fs.list_md_files(&root_dir.join("list"))
            .await
            .expect("list_md_files should succeed"),
    );
    let expected_md = sorted_paths(vec![root_dir.join("list/one.md")]);
    assert_eq!(list_md_files, expected_md);

    let binary_data = vec![0_u8, 7, 64, 128, 255];
    let binary_path = root_dir.join("bin/blob.dat");
    fs.write_binary(&binary_path, &binary_data)
        .await
        .expect("binary write should succeed");
    let loaded_binary = fs
        .read_binary(&binary_path)
        .await
        .expect("binary read should succeed");
    assert_eq!(loaded_binary, binary_data);

    let move_from = root_dir.join("move/from.md");
    let move_to = root_dir.join("move/to.md");
    fs.write_file(&move_from, "from")
        .await
        .expect("source should be created");
    fs.write_file(&move_to, "to")
        .await
        .expect("destination should be created");

    let conflict_err = fs
        .move_file(&move_from, &move_to)
        .await
        .expect_err("move should fail when destination exists");
    assert_eq!(conflict_err.kind(), ErrorKind::AlreadyExists);
    assert_eq!(
        fs.read_to_string(&move_from)
            .await
            .expect("source should remain after failed move"),
        "from"
    );
    assert_eq!(
        fs.read_to_string(&move_to)
            .await
            .expect("destination should remain after failed move"),
        "to"
    );

    let missing_err = fs
        .move_file(
            &root_dir.join("move/missing.md"),
            &root_dir.join("move/new.md"),
        )
        .await
        .expect_err("move should fail when source is missing");
    assert_eq!(missing_err.kind(), ErrorKind::NotFound);
}

#[wasm_bindgen_test(async)]
async fn opfs_matches_async_fs_contract() {
    let fs = OpfsFileSystem::create_with_name(&format!("diaryx-opfs-{}", unique_suffix("parity")))
        .await
        .expect("OPFS filesystem should be created");
    assert_fs_contract(&fs, "workspace").await;
}

#[wasm_bindgen_test(async)]
async fn indexeddb_matches_async_fs_contract() {
    let fs = IndexedDbFileSystem::create()
        .await
        .expect("IndexedDB filesystem should be created");
    assert_fs_contract(&fs, &format!("workspace-{}", unique_suffix("idb"))).await;
}

#[wasm_bindgen_test(async)]
async fn fsa_matches_async_fs_contract() {
    let handle = create_fsa_root_handle(&format!("diaryx-fsa-{}", unique_suffix("parity"))).await;
    let fs = FsaFileSystem::from_handle(handle);
    assert_fs_contract(&fs, "workspace").await;
}
