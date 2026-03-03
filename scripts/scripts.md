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

This folder contains core scripts plus plugin registry pipeline helpers:

- `build-wasm.sh`: Builds `crates/diaryx_wasm` for the web app via wasm-pack. Used in `apps/web/package.json`'s build script. On macOS it also exports `DEVELOPER_DIR`/`SDKROOT` from Xcode tools to reduce host-toolchain SDK warnings. Extism plugins (sync, AI, publish, math) are built separately in CI.
- `sync-versions.sh`: Using the root README.md as a source of truth, updates every version number in the repository.
- `test-sync.sh`: Opens a tmux window with the diaryx_sync_server and two web app dev servers.
- `update-agents-index.sh`: Updates the workspace index in AGENTS.md using `diaryx workspace info`.

Plugin registry/CI helpers live under `scripts/plugins/`:

- `discover-internal-plugins.sh`: Uses `cargo metadata` to discover Extism plugin crates (`cdylib` + `extism-pdk`).
- `build-internal-plugin.sh`: Builds a plugin crate to WASM, inspects its manifest, and emits metadata (`sha256`, `sizeBytes`, manifest fields).
- `assemble-registry-v2.sh`: Merges internal build metadata with curated catalogs and produces `registry-v2.json` plus upload plan.
- `upload-plugin-artifacts.sh`: Uploads immutable plugin artifacts and `plugins/registry-v2.json` to CDN (R2/S3-compatible).

&nbsp;
