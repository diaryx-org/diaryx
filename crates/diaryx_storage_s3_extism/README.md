# diaryx_storage_s3_extism

Extism WASM guest plugin that implements S3-compatible object storage as an `AsyncFileSystem` backend.

## Overview

This plugin exposes S3 REST API operations as commands that map 1:1 to the `AsyncFileSystem` trait methods. Frontend (browser) or native (CLI/Tauri) adapters dispatch these commands to create an S3-backed filesystem that works identically to OPFS, IndexedDB, or native filesystem backends.

**Plugin ID**: `diaryx.storage.s3`

## Supported Services

- Amazon S3
- Cloudflare R2
- MinIO
- Backblaze B2
- Any S3-compatible object storage

## Architecture

```
Browser:  pluginFileSystem.ts → Extism plugin → host_http_request → S3 API
Native:   PluginFileSystem    → Extism plugin → host_http_request → S3 API
```

The plugin uses AWS Signature V4 signing (pure Rust, WASM-compatible) to authenticate requests.

## Commands

| Command | AsyncFileSystem method | S3 operation |
|---------|----------------------|--------------|
| `ReadFile` | `read_to_string(path)` | GET object |
| `WriteFile` | `write_file(path, content)` | PUT object |
| `DeleteFile` | `delete_file(path)` | DELETE object |
| `Exists` | `exists(path)` | HEAD object |
| `ListFiles` | `list_files(dir)` | LIST (delimiter=/) |
| `ListMdFiles` | `list_md_files(dir)` | LIST + filter *.md |
| `CreateDirAll` | `create_dir_all(path)` | No-op (S3 uses key prefixes) |
| `IsDir` | `is_dir(path)` | Check prefix existence |
| `MoveFile` | `move_file(from, to)` | COPY + DELETE |
| `ReadBinary` | `read_binary(path)` | GET object (binary) |
| `WriteBinary` | `write_binary(path, data)` | PUT object (binary) |
| `GetModifiedTime` | `get_modified_time(path)` | HEAD → Last-Modified |
| `TestConnection` | — | HEAD bucket |
| `GetConfig` / `SetConfig` | — | Credentials + endpoint |

## Build

```bash
cargo build -p diaryx_storage_s3_extism --target wasm32-unknown-unknown --release
```

## Source Files

- `src/lib.rs` — Plugin lifecycle, command dispatch, manifest
- `src/host_bridge.rs` — Host function wrappers (HTTP, storage, logging)
- `src/sigv4.rs` — AWS Signature V4 signing (~180 lines, hmac+sha2)
- `src/s3_ops.rs` — S3 REST operations mapped to AsyncFileSystem methods
