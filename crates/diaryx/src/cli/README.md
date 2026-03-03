---
title: Command-line module
description: The main CLI command implementation module
part_of: '[README](/crates/diaryx/src/README.md)'
author: adammharris
audience:
- public
contents:
- '[README](/crates/diaryx/src/cli/nav/README.md)'
- '[README](/crates/diaryx/src/cli/sync/README.md)'
attachments:
- '[mod.rs](/crates/diaryx/src/cli/mod.rs)'
- '[args.rs](/crates/diaryx/src/cli/args.rs)'
- '[attachment.rs](/crates/diaryx/src/cli/attachment.rs)'
- '[config.rs](/crates/diaryx/src/cli/config.rs)'
- '[content.rs](/crates/diaryx/src/cli/content.rs)'
- '[entry.rs](/crates/diaryx/src/cli/entry.rs)'
- '[export.rs](/crates/diaryx/src/cli/export.rs)'
- '[normalize.rs](/crates/diaryx/src/cli/normalize.rs)'
- '[property.rs](/crates/diaryx/src/cli/property.rs)'
- '[publish.rs](/crates/diaryx/src/cli/publish.rs)'
- '[search.rs](/crates/diaryx/src/cli/search.rs)'
- '[sort.rs](/crates/diaryx/src/cli/sort.rs)'
- '[template.rs](/crates/diaryx/src/cli/template.rs)'
- '[util.rs](/crates/diaryx/src/cli/util.rs)'
- '[workspace.rs](/crates/diaryx/src/cli/workspace.rs)'
- '[import.rs](/crates/diaryx/src/cli/import.rs)'
- '[plugin_loader.rs](/crates/diaryx/src/cli/plugin_loader.rs)'
- '[plugin_storage.rs](/crates/diaryx/src/cli/plugin_storage.rs)'
- '[plugin_manager.rs](/crates/diaryx/src/cli/plugin_manager.rs)'
- '[plugin_dispatch.rs](/crates/diaryx/src/cli/plugin_dispatch.rs)'
- '[preview.rs](/crates/diaryx/src/cli/preview.rs)'
- '[edit.rs](/crates/diaryx/src/cli/edit.rs)'
exclude:
- '*.lock'
---
# Command-line module

In the Diaryx CLI, this module provides the majority of the functionality.

## Plugin Management Commands

- `diaryx plugin list` — List installed plugins (supports metadata filters and `--json`).
- `diaryx plugin install <id>` — Install a plugin from the curated `registry-v2` by canonical ID.
- `diaryx plugin remove <id>` — Remove an installed plugin.
- `diaryx plugin search [query]` — Search the curated registry with filters.
- `diaryx plugin update [id]` — Update installed plugins.
- `diaryx plugin info <id>` — Show rich plugin metadata (`--json` supported).

Registry contract and behavior:

- CLI only accepts registry schema `v2` and fails fast on older schema payloads.
- Install verifies `artifact.sha256` and `artifact.sizeBytes` before persistence.
- Canonical plugin IDs are required (for example: `diaryx.sync`).
- Legacy `diaryx plugin install --defaults` behavior was removed.

Discovery filters:

- `--category <value>`
- `--tag <value>`
- `--source internal|external`
- `--creator <value>`
- `--installed` (search only)
- `--json` (machine-readable output)

## Dynamic Plugin Commands

Installed plugins declare their own CLI subcommands via `CliCommand` in their manifest.
At startup, the CLI scans `~/.diaryx/plugins/*.diaryx/manifest.json` and dynamically
adds plugin-declared commands to the clap parser. Commands are dispatched to either
a native handler (for commands needing native resources like WebSocket or HTTP) or
routed to the plugin's WASM `handle_command` export.

## Import Commands

- `diaryx import email <source>` — Import `.eml` files, directories of `.eml` files, or `.mbox` archives.
  Options: `--folder <name>` (default: "emails"), `--dry-run`, `--verbose`.

&nbsp;
