---
title: Command-line module
description: The main CLI command implementation module
part_of: '[README](/crates/diaryx/src/README.md)'
author: adammharris
contents:
- '[README](/crates/diaryx/src/cli/nav/README.md)'
exclude:
- '*.lock'
---
# Command-line module

In the Diaryx CLI, this module provides the majority of the functionality.

## Optional Cargo Features

- `plugins` enables Extism plugin management, plugin manifest discovery, and plugin-native helpers such as `publish` and `preview`.
- `edit` enables `diaryx edit`, which starts the local sync server used by the web editor.

## Plugin Management Commands

Available only when the CLI is compiled with the `plugins` feature.

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
- `--author <value>`
- `--installed` (search only)
- `--json` (machine-readable output)

## Dynamic Plugin Commands

Available only when the CLI is compiled with the `plugins` feature.

Installed plugins declare their own CLI subcommands via `CliCommand` in their manifest.
At startup, the CLI scans `~/.diaryx/plugins/*.diaryx/manifest.json` and dynamically
adds plugin-declared commands to the clap parser. Commands are dispatched to either
a native handler (for commands needing native resources like WebSocket or HTTP) or
routed to the plugin's WASM `handle_command` export.

Built-in import and sync modules have been removed from the CLI. Those workflows
now come entirely from installed plugins such as `diaryx.sync` and
format-specific import plugins.

CLI auth and plugin-provided sync commands preserve server error bodies for
non-2xx HTTP responses. This is important for actionable server-side failures
such as expired sessions and device-limit login blocks, which include useful
JSON messages even when the status code is 4xx.

## Account and Device Commands

- `diaryx login <email>` requests a magic link, prompts for the 6-digit code,
  and registers the CLI as a device named `Diaryx CLI` by default.
- `diaryx login <email> --device-name "Work Mac"` registers a custom device
  name.
- `diaryx login <email> --replace-device <device_id>` replaces a known device
  while verifying. If verification hits the device limit without this flag, the
  CLI lists the server-provided devices and prompts for one to replace, then
  retries verification with the same code.
- `diaryx devices list [--json]` lists registered account devices.
- `diaryx devices rename <device_id> <name>` renames a registered device.
- `diaryx devices remove <device_id> [-y]` removes a registered device.

`publish` and `preview` remain native helper implementations, but they are
reached through plugin-declared commands rather than top-level built-in clap
subcommands.

&nbsp;
