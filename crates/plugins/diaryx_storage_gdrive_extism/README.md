---
title: "Google Drive Storage"
description: "Google Drive as a filesystem backend"
id: "diaryx.storage.gdrive"
version: "0.1.2"
author: "Diaryx Team"
license: "PolyForm Shield 1.0.0"
repository: "https://github.com/diaryx-org/diaryx"
categories: ["storage", "integration"]
tags: ["google-drive", "storage", "cloud"]
capabilities: ["custom_commands"]
artifact:
  url: ""
  sha256: ""
  size: 0
  published_at: ""
ui:
  - slot: StorageProvider
    id: diaryx.storage.gdrive
    label: "Google Drive"
  - slot: SettingsTab
    id: gdrive-storage-settings
    label: "Google Drive"
requested_permissions:
  defaults:
    http_requests:
      include: ["googleapis.com"]
    plugin_storage:
      include: ["all"]
  reasons:
    http_requests: "Communicate with Google Drive and Google OAuth API endpoints."
    plugin_storage: "Persist Google Drive settings and cached workspace metadata."
---

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

OAuth sign-in uses a declarative settings tab plus PKCE. Diaryx provides the Google client ID at runtime, opens the OAuth window, and the plugin stores tokens outside normal config via host secret storage. The plugin handles token refresh internally via `RefreshToken`.

Runtime client IDs are expected from the host app via `VITE_GOOGLE_DRIVE_CLIENT_ID` or the platform-specific `VITE_GOOGLE_DRIVE_WEB_CLIENT_ID` / `VITE_GOOGLE_DRIVE_DESKTOP_CLIENT_ID` environment variables.

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
| `BeginOAuth` / `CompleteOAuth` | — | PKCE auth URL + code exchange |
| `Disconnect` | — | Clear stored tokens |
| `RefreshToken` | — | OAuth token refresh |
| `GetConfig` / `SetConfig` | — | Non-secret plugin settings |

## Build

```bash
cargo build -p diaryx_storage_gdrive_extism --target wasm32-unknown-unknown --release
```

## Source Files

- `src/lib.rs` — Plugin lifecycle, command dispatch, manifest
- `src/host_bridge.rs` — Host function wrappers (HTTP, storage, logging)
- `src/gdrive_ops.rs` — Google Drive REST API operations
- `src/multipart.rs` — Manual multipart/related encoding for uploads
