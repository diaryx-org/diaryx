---
title: Scripts
part_of: "[README](/README.md)"
attachments:
  - build-wasm.sh
  - sync-versions.sh
  - test-sync.sh
  - update-agents-index.sh
---

# Scripts

This folder contains four scripts:

- `build-wasm.sh`: Builds crates/diaryx*wasm for the web app and also builds/copies Extism plugins for sync, AI, publish, and math (`crates/diaryx*_\_extism -> apps/web/public/plugins/_.wasm`). Used in apps/web/package.json's build script. On macOS it also exports `DEVELOPER_DIR`/`SDKROOT` from Xcode tools to reduce host-toolchain SDK warnings. The script validates that plugin artifacts do not contain wasm-bindgen placeholder imports.
- `sync-versions.sh`: Using the root README.md as a source of truth, updates every version number in the repository.
- `test-sync.sh`: Opens a tmux window with the diaryx_sync_server and two web app dev servers.
- `update-agents-index.sh`: Updates the workspace index in AGENTS.md using `diaryx workspace info`.

&nbsp;
