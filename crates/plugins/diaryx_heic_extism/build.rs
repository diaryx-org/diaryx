//! Build script: compiles vendored libde265 + libheif C sources for wasm32-wasip1.
//!
//! Requires wasi-sdk to be installed (provides clang targeting wasm32-wasip1).
//! Set `WASI_SDK_PATH` to the SDK root if it's not at `/opt/wasi-sdk`.

use std::path::PathBuf;

fn wasi_sdk_path() -> PathBuf {
    std::env::var("WASI_SDK_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/opt/wasi-sdk"))
}

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    let is_wasm = target.contains("wasm32");

    let sdk = wasi_sdk_path();
    let wasi_clang = sdk.join("bin/clang");

    // ── libde265 ────────────────────────────────────────────────────────
    let de265_dir = PathBuf::from("vendor/libde265");
    if de265_dir.exists() {
        let mut build = cc::Build::new();
        build
            .warnings(false)
            .define("HAVE_STDINT_H", "1")
            .define("HAVE_STDBOOL_H", "1")
            .include(&de265_dir);

        if is_wasm {
            build.compiler(&wasi_clang);
        }

        // Collect .c files (non-recursive — libde265 ships a flat directory of sources)
        let sources: Vec<PathBuf> = std::fs::read_dir(&de265_dir)
            .expect("vendor/libde265 directory missing — run vendor script first")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext == "c" || ext == "cc")
            })
            .map(|e| e.path())
            .collect();

        if sources.is_empty() {
            println!(
                "cargo::warning=vendor/libde265 has no C/C++ sources — skipping. Populate with vendored libde265 sources."
            );
        } else {
            for src in &sources {
                build.file(src);
            }
            build.compile("de265");
        }
    } else {
        println!(
            "cargo::warning=vendor/libde265 not found — link will fail unless sources are vendored"
        );
    }

    // ── libheif ─────────────────────────────────────────────────────────
    let heif_dir = PathBuf::from("vendor/libheif");
    if heif_dir.exists() {
        let mut build = cc::Build::new();
        build
            .warnings(false)
            .define("HAVE_LIBDE265", "1")
            .include(&heif_dir)
            .include(&de265_dir);

        if is_wasm {
            build.compiler(&wasi_clang);
        }

        let sources: Vec<PathBuf> = std::fs::read_dir(&heif_dir)
            .expect("vendor/libheif directory missing — run vendor script first")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext == "c" || ext == "cc")
            })
            .map(|e| e.path())
            .collect();

        if sources.is_empty() {
            println!(
                "cargo::warning=vendor/libheif has no C/C++ sources — skipping. Populate with vendored libheif sources."
            );
        } else {
            for src in &sources {
                build.file(src);
            }
            build.compile("heif");
        }
    } else {
        println!(
            "cargo::warning=vendor/libheif not found — link will fail unless sources are vendored"
        );
    }
}
