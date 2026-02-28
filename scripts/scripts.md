---
title: Scripts
part_of: '[README](/README.md)'
attachments:
- build-wasm.sh
- sync-versions.sh
- test-sync.sh
- update-agents-index.sh
---
# Scripts

This folder contains four scripts:

- `build-wasm.sh`: Builds crates/diaryx_wasm for the web app and also builds/copies the Extism sync plugin (`crates/diaryx_sync_extism -> apps/web/public/plugins/diaryx_sync.wasm`). Used in apps/web/package.json's build script. On macOS it also exports `DEVELOPER_DIR`/`SDKROOT` from Xcode tools to reduce host-toolchain SDK warnings. The script validates that the sync plugin does not contain wasm-bindgen placeholder imports.
- `sync-versions.sh`: Using the root README.md as a source of truth, updates every version number in the repository.
- `test-sync.sh`: Opens a tmux window with the diaryx_sync_server and two web app dev servers.
- `update-agents-index.sh`: Updates the workspace index in AGENTS.md using `diaryx workspace info`.

&nbsp;
