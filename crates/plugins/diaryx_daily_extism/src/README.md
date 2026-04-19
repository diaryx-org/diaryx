---
title: diaryx_daily_extism src
description: Source for the Daily Extism guest plugin
part_of: '[README](/crates/diaryx_daily_extism/README.md)'
---

# diaryx_daily_extism src

- `lib.rs` — `#[plugin_fn]` entry points and module declarations only
- `commands.rs` — command dispatch (`EnsureDailyEntry`, `GetAdjacentDailyEntry`, `CliDaily`, etc.)
- `daily_logic.rs` — pure date/path/template domain (no host deps, fully unit-testable)
- `indices.rs` — daily index files, `contents`/`part_of` maintenance, tree walks
- `links.rs` — link format discovery and formatting
- `markdown_io.rs` — frontmatter read/write helpers over `host::fs`
- `migration.rs` — one-time migration of legacy workspace frontmatter keys
- `paths.rs` — workspace/filesystem path conversion helpers
- `permissions.rs` — manifest permissions and runtime permission patches
- `state.rs` — plugin thread-local state and workspace lifecycle
- `storage.rs` — workspace-scoped plugin config persistence via `host::storage`
- `ui/panel.html` — plugin-owned daily sidebar iframe UI
