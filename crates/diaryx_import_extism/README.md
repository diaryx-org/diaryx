---
title: diaryx_import_extism
description: Extism guest plugin for Diaryx import functionality
part_of: '[README](/crates/README.md)'
---

# diaryx_import_extism

Extism guest plugin that provides import orchestration for Diaryx. Parses Day One and Markdown exports using `diaryx_core` parsers, then writes entries into the workspace via host bridge functions.

## Plugin ID

`diaryx.import`

## Commands

| Command | Description |
|---------|-------------|
| `ParseDayOne` | Parse a Day One export (ZIP or JSON). Input: `{ data: "<base64>" }`. Returns parsed entries, errors, and journal name. |
| `ParseMarkdownFile` | Parse a single markdown file. Input: `{ data: "<base64>", filename: "..." }`. Returns a serialized `ImportedEntry`. |
| `ImportEntries` | Write parsed entries into workspace with date-based hierarchy. Input: `{ entries_json, folder, parent_path }`. Returns `ImportResult`. |
| `ImportDirectoryInPlace` | Add hierarchy metadata to an already-written directory of files. Input: `{ path }`. Returns `ImportResult`. |

## CLI Commands

Declared in the plugin manifest and discovered at runtime:

```
diaryx import email <source> [--folder] [--dry-run] [--verbose]   # native handler (mbox needs mmap)
diaryx import dayone <source> [--folder] [--dry-run] [--verbose]  # native handler
diaryx import markdown <source> [--folder] [--dry-run] [--verbose] # native handler
```

All CLI import subcommands use `native_handler` — the CLI binary reads source files from the filesystem and delegates to `diaryx_core` parsers directly, since source files live outside the workspace.

## Architecture

- **Parsers**: `diaryx_core::import::{dayone, markdown}` — pure functions, no I/O
- **Orchestration**: `orchestrate.rs` — writes entries into date-based hierarchy via host bridge
- **Directory import**: `directory.rs` — adds `part_of`/`contents` metadata to existing files via host bridge
- **Host bridge**: `host_bridge.rs` — wraps Extism host functions (`host_read_file`, `host_write_file`, `host_request_file`, etc.)

## Build

```bash
cargo build -p diaryx_import_extism --target wasm32-unknown-unknown --release
```

The CI plugin pipeline auto-discovers this crate (cdylib + extism-pdk).
