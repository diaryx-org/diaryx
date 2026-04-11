---
title: "Publish"
description: "Export and publish content with optional format conversion"
id: "diaryx.publish"
version: "0.1.4"
author: "Diaryx Team"
license: "PolyForm Shield 1.0.0"
repository: "https://github.com/diaryx-org/diaryx"
categories: ["publish", "export"]
tags: ["publish", "export", "html"]
capabilities: ["workspace_events", "custom_commands"]
artifact:
  url: ""
  sha256: ""
  size: 0
  published_at: ""
ui:
  - slot: SidebarTab
    id: publish-panel
    label: "Publish"
  - slot: CommandPaletteItem
    id: publish-export
    label: "Export..."
  - slot: CommandPaletteItem
    id: publish-site
    label: "Publish Site"
cli:
  - name: publish
    about: "Publish workspace as HTML"
  - name: preview
    about: "Preview published workspace"
requested_permissions:
  defaults:
    read_files:
      include: ["all"]
    edit_files:
      include: ["all"]
    create_files:
      include: ["all"]
    http_requests:
      include: ["unpkg.com"]
    plugin_storage:
      include: ["all"]
  reasons:
    read_files: "Read workspace entries and attachments while building export output."
    edit_files: "Update generated publish artifacts during export and preview workflows."
    create_files: "Create exported HTML, assets, and converted output files."
    http_requests: "Download optional converter WASM modules used for format conversion."
    plugin_storage: "Cache downloaded converter modules between runs."
---

# diaryx_publish_extism

Extism guest plugin wrapping publish/export functionality for browser and native runtimes.
The publish pipeline now comes from `diaryx_core::publish`; there is no separate
`diaryx_publish` crate dependency anymore.

This repo pins `diaryx_core` from the main
`https://github.com/diaryx-org/diaryx.git` repository via a git ref rather than
a local filesystem checkout, so standalone builds track a pushed Diaryx commit.

## Overview

This crate builds to a `.wasm` plugin loaded by:

- **Native** via `diaryx_extism` (wasmtime)
- **Web** via `@extism/extism`

The plugin handles:

- export command dispatch (`PlanExport`, `ExportToMemory`, etc.)
- converter lifecycle (`DownloadConverter`, `IsConverterAvailable`)
- format conversion (`ConvertFormat`, `ConvertToPdf`) by running converter WASI modules through `host_run_wasi_module`

Published attachment uploads preserve attachment MIME types as they are copied
into namespace/site storage, including `text/html` for standalone HTML embeds,
so published iframe attachments render as documents instead of downloading as
generic binary blobs. Published page wrappers now also listen for height
messages from embedded HTML attachments, and published `.html` / `.htm`
attachments receive a tiny injected bridge at publish time so sandboxed iframe
embeds can auto-size without modifying the source file in the workspace.

## Exports

| Export                          | Description                        |
| ------------------------------- | ---------------------------------- |
| `manifest()`                    | Plugin metadata + UI contributions |
| `init(params)`                  | Initialize plugin state            |
| `shutdown()`                    | Drop plugin state                  |
| `handle_command(request)`       | Command dispatcher                 |
| `on_event(event)`               | Workspace lifecycle events         |
| `get_config()` / `set_config()` | Plugin config passthrough          |

## Host Functions Required

| Function               |
| ---------------------- |
| `host_log`             |
| `host_read_file`       |
| `host_read_binary`     |
| `host_list_files`      |
| `host_file_exists`     |
| `host_write_file`      |
| `host_write_binary`    |
| `host_emit_event`      |
| `host_storage_get`     |
| `host_storage_set`     |
| `host_get_timestamp`   |
| `host_http_request`    |
| `host_run_wasi_module` |

## Build

This repository can be built standalone with Cargo:

```bash
cargo build --target wasm32-unknown-unknown -p diaryx_publish_extism --release
```

When vendored into the Diaryx monorepo, you can still use the monorepo helper:

```bash
cd ../diaryx && ./scripts/build-wasm.sh
```

which copies the artifact to `apps/web/public/plugins/diaryx_publish.wasm`.
