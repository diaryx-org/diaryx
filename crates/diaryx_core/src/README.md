---
title: diaryx_core src
description: Source code for the core Diaryx library
part_of: "[README](/crates/diaryx_core/README.md)"
contents:
  - "[README](/crates/diaryx_core/src/crdt/README.md)"
  - "[README](/crates/diaryx_core/src/cloud/README.md)"
  - "[README](/crates/diaryx_core/src/entry/README.md)"
  - "[README](/crates/diaryx_core/src/fs/README.md)"
  - "[README](/crates/diaryx_core/src/publish/README.md)"
  - "[README](/crates/diaryx_core/src/utils/README.md)"
  - "[README](/crates/diaryx_core/src/workspace/README.md)"
attachments:
  - "[lib.rs](/crates/diaryx_core/src/lib.rs)"
  - "[backup.rs](/crates/diaryx_core/src/backup.rs)"
  - "[command.rs](/crates/diaryx_core/src/command.rs)"
  - "[command_handler.rs](/crates/diaryx_core/src/command_handler.rs)"
  - "[config.rs](/crates/diaryx_core/src/config.rs)"
  - "[diaryx.rs](/crates/diaryx_core/src/diaryx.rs)"
  - "[error.rs](/crates/diaryx_core/src/error.rs)"
  - "[export.rs](/crates/diaryx_core/src/export.rs)"
  - "[frontmatter.rs](/crates/diaryx_core/src/frontmatter.rs)"
  - "[link_parser.rs](/crates/diaryx_core/src/link_parser.rs)"
  - "[metadata_writer.rs](/crates/diaryx_core/src/metadata_writer.rs)"
  - "[search.rs](/crates/diaryx_core/src/search.rs)"
  - "[template.rs](/crates/diaryx_core/src/template.rs)"
  - "[test_utils.rs](/crates/diaryx_core/src/test_utils.rs)"
  - "[validate.rs](/crates/diaryx_core/src/validate.rs)"
exclude:
  - "*.lock"
---

# diaryx_core Source

This directory contains the source code for the core Diaryx library.

## Structure

| File                 | Purpose                                                |
| -------------------- | ------------------------------------------------------ |
| `lib.rs`             | Library entry point and public API exports             |
| `backup.rs`          | Create ZIP backups of markdown files                   |
| `command.rs`         | Command pattern API for unified WASM/Tauri operations  |
| `command_handler.rs` | Command execution implementation                       |
| `config.rs`          | Configuration management                               |
| `diaryx.rs`          | Central Diaryx data structure                          |
| `error.rs`           | Shared error types                                     |
| `export.rs`          | Export with audience filtering                         |
| `frontmatter.rs`     | Frontmatter parsing and manipulation                   |
| `link_parser.rs`     | Parse markdown links                                   |
| `metadata_writer.rs` | Write frontmatter metadata (temp + backup safe writes) |
| `search.rs`          | Search functionality                                   |
| `template.rs`        | Template management                                    |
| `test_utils.rs`      | Feature-gated test utilities                           |
| `validate.rs`        | Workspace validation and fixing                        |

## CRDT Metadata Notes

- `Command::SetCrdtFile` preserves existing attachment `BinaryRef` metadata
  when incoming metadata omits attachments or includes refs with empty hashes.
  This avoids dropping cloud attachment references during frontmatter-driven
  metadata refreshes.
