---
title: "Import"
description: "Import entries from Day One, Markdown directories, and other formats"
id: "diaryx.import"
version: "0.1.2"
author: "Diaryx Team"
license: "PolyForm Shield 1.0.0"
repository: "https://github.com/diaryx-org/diaryx"
categories: ["import", "migration"]
tags: ["import", "day-one", "markdown"]
capabilities: ["custom_commands"]
artifact:
  url: ""
  sha256: ""
  size: 0
  published_at: ""
cli:
  - name: import
    about: "Import entries from external formats"
requested_permissions:
  defaults:
    read_files:
      include: ["all"]
    edit_files:
      include: ["all"]
    create_files:
      include: ["all"]
  reasons:
    read_files: "Read existing entries during import."
    edit_files: "Update entry metadata during import."
    create_files: "Create new entries from imported data."
---

# diaryx_import_extism

Extism guest plugin that provides import orchestration for Diaryx. Parses Day One and Markdown exports using `diaryx_core` parsers, then writes entries into the workspace via host bridge functions.

## Plugin ID

`diaryx.import`

## Commands

| Command | Description |
|---------|-------------|
| `ParseDayOne` | Parse a Day One export (ZIP or JSON). Input: `{ data: "<base64>" }` or `{ file_key: "dayone_export" }` for host-requested file bytes. New hosts pass raw bytes directly for `host_request_file`; the guest keeps a legacy fallback for older base64/JSON hosts. Returns parsed entries, errors, and journal name. |
| `ImportDayOne` | Parse and write a Day One export in one step. Input: `{ folder, parent_path, data? }` or `{ folder, parent_path, file_key }`. Returns `ImportResult` with parse/write errors combined, avoiding a giant intermediate `entries_json` payload. |
| `ParseMarkdownFile` | Parse a single markdown file. Input: `{ data: "<base64>", filename: "..." }`. Returns a serialized `ImportedEntry`. Available only when built with the `markdown-import` feature. |
| `ImportEntries` | Write parsed entries into workspace with date-based hierarchy. Input: `{ entries_json, folder, parent_path }`. Returns `ImportResult`. |
| `ImportDirectoryInPlace` | Add hierarchy metadata to an already-written directory of files. Input: `{ path }`. Returns `ImportResult`. |

## CLI Commands

Declared in the plugin manifest and discovered at runtime:

```
diaryx import email <source> [--folder] [--dry-run] [--verbose]   # native handler (mbox needs mmap)
diaryx import dayone <source> [--folder] [--dry-run] [--verbose]  # native handler
diaryx import markdown <source> [--folder] [--dry-run] [--verbose] # native handler (requires `markdown-import`)
```

All CLI import subcommands use `native_handler` — the CLI binary reads source files from the filesystem and delegates to `diaryx_core` parsers directly, since source files live outside the workspace.

`markdown-import` is enabled by default. Build with `--no-default-features` to exclude Markdown parser support.

## Architecture

- **Parsers**: `diaryx_core::import::{dayone, markdown}` — pure functions, no I/O
- **Orchestration**: `orchestrate.rs` — writes entries into date-based hierarchy via host bridge, used both by legacy `ImportEntries` and the direct `ImportDayOne` path
- **Directory import**: `directory.rs` — adds `part_of`/`contents` metadata to existing files via host bridge
- **Host bridge**: `host_bridge.rs` — wraps Extism host functions (`host_read_file`, `host_write_file`, `host_request_file`, etc.). `host_request_file` now prefers raw bytes over JSON/base64 for selected-file handoff.
- **Command exports**: supports both legacy JSON `handle_command` requests and typed `execute_typed_command` dispatch used by frontend `backend.execute({ type: "PluginCommand", ... })` routing

## Build

```bash
cargo build -p diaryx_import_extism --target wasm32-unknown-unknown --release

# Exclude Markdown parser support (smaller WASM, ParseMarkdownFile disabled)
cargo build -p diaryx_import_extism --target wasm32-unknown-unknown --release --no-default-features
```

The CI plugin pipeline auto-discovers this crate (cdylib + extism-pdk).
