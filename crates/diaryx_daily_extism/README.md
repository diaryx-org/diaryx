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
