# diaryx_publish_extism

Extism guest plugin wrapping publish/export functionality for browser and native runtimes.

## Overview

This crate builds to a `.wasm` plugin loaded by:

- **Native** via `diaryx_extism` (wasmtime)
- **Web** via `@extism/extism`

The plugin handles:

- export command dispatch (`PlanExport`, `ExportToMemory`, etc.)
- converter lifecycle (`DownloadConverter`, `IsConverterAvailable`)
- format conversion (`ConvertFormat`, `ConvertToPdf`) by running converter WASI modules through `host_run_wasi_module`

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

```bash
cargo build --target wasm32-unknown-unknown -p diaryx_publish_extism --release
```

Or use:

```bash
./scripts/build-wasm.sh
```

which copies the artifact to `apps/web/public/plugins/diaryx_publish.wasm`.
