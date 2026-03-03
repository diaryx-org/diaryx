# diaryx_storage_gdrive_extism

Extism WASM guest plugin that implements Google Drive as an `AsyncFileSystem` backend.

## Overview

This plugin exposes Google Drive REST API operations as commands that map 1:1 to the `AsyncFileSystem` trait methods. Frontend (browser) or native (CLI/Tauri) adapters dispatch these commands to create a Google Drive-backed filesystem.

**Plugin ID**: `diaryx.storage.gdrive`

## Architecture

```
Browser:  pluginFileSystem.ts → Extism plugin → host_http_request → Google Drive API
Native:   PluginFileSystem    → Extism plugin → host_http_request → Google Drive API
```

OAuth sign-in stays in the browser (requires user interaction). After sign-in, the frontend passes tokens to the plugin via `SetConfig`. The plugin handles token refresh internally via `RefreshToken`.

## Commands

| Command | AsyncFileSystem method | Google Drive operation |
|---------|----------------------|----------------------|
| `ReadFile` | `read_to_string(path)` | GET files/{id}?alt=media |
| `WriteFile` | `write_file(path, content)` | Multipart upload / PATCH |
| `DeleteFile` | `delete_file(path)` | DELETE files/{id} |
| `Exists` | `exists(path)` | Search by name+parent |
| `ListFiles` | `list_files(dir)` | List children |
| `ListMdFiles` | `list_md_files(dir)` | List + filter *.md |
| `CreateDirAll` | `create_dir_all(path)` | Create folder hierarchy |
| `IsDir` | `is_dir(path)` | Check mimeType |
| `MoveFile` | `move_file(from, to)` | PATCH parents + name |
| `ReadBinary` | `read_binary(path)` | GET (binary) |
| `WriteBinary` | `write_binary(path, data)` | Multipart upload (binary) |
| `GetModifiedTime` | `get_modified_time(path)` | GET modifiedTime field |
| `RefreshToken` | — | OAuth token refresh |
| `GetConfig` / `SetConfig` | — | OAuth credentials |

## Build

```bash
cargo build -p diaryx_storage_gdrive_extism --target wasm32-unknown-unknown --release
```

## Source Files

- `src/lib.rs` — Plugin lifecycle, command dispatch, manifest
- `src/host_bridge.rs` — Host function wrappers (HTTP, storage, logging)
- `src/gdrive_ops.rs` — Google Drive REST API operations
- `src/multipart.rs` — Manual multipart/related encoding for uploads
