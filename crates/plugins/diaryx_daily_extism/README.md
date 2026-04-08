---
title: "Daily"
description: "Daily entry plugin with date hierarchy, navigation, and CLI surface"
id: "diaryx.daily"
version: "0.1.3"
author: "Diaryx Team"
license: "PolyForm Shield 1.0.0"
repository: "https://github.com/diaryx-org/diaryx"
categories: ["productivity", "journaling"]
tags: ["daily", "journal", "calendar"]
capabilities: ["workspace_events", "custom_commands"]
artifact:
  url: ""
  sha256: ""
  size: 0
  published_at: ""
ui:
  - slot: SidebarTab
    id: daily-panel
    label: "Daily"
  - slot: CommandPaletteItem
    id: daily-open-today
    label: "Open Today's Entry"
  - slot: CommandPaletteItem
    id: daily-open-yesterday
    label: "Open Yesterday's Entry"
cli:
  - name: daily
    about: "Daily entry commands"
requested_permissions:
  defaults:
    read_files:
      include: ["all"]
    edit_files:
      include: ["all"]
    create_files:
      include: ["all"]
    plugin_storage:
      include: ["all"]
  reasons:
    read_files: "Read daily entries, index files, and optional templates from the workspace."
    edit_files: "Update existing year, month, and daily entry files when navigating and organizing the daily hierarchy."
    create_files: "Create missing year, month, and daily entry files for new dates."
    plugin_storage: "Persist daily plugin configuration for the current workspace."
---

# diaryx_daily_extism

Extism WASM guest plugin that provides all daily-entry functionality for Diaryx.

## Overview

This plugin owns daily behavior end-to-end:

- ensure/create daily entries
- date-adjacent navigation (prev/next)
- daily entry state checks
- plugin-declared CLI command (`diaryx daily`)
- plugin-owned sidebar iframe UI (`daily.panel`)
- one-time migration of legacy workspace keys (`daily_entry_folder`, `daily_template`)

No daily logic is required in vanilla `diaryx_core` or `apps/web`.

## Exports

- `manifest`
- `init`
- `shutdown`
- `handle_command`
- `execute_typed_command`
- `get_config`
- `set_config`
- `get_component_html`
- `on_event`

## Build

```bash
cargo build -p diaryx_daily_extism --target wasm32-unknown-unknown --release
```
