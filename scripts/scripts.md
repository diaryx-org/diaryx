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

- `build-wasm.sh`: Builds `crates/diaryx_wasm` for the web app via wasm-pack. Used in `apps/web/package.json`'s build script. On macOS it also exports `DEVELOPER_DIR`/`SDKROOT` from Xcode tools to reduce host-toolchain SDK warnings.
- `sync-versions.sh`: Using the root README.md as a source of truth, updates every version number in the repository.
- `test-sync.sh`: Opens a tmux window with the diaryx_sync_server and two web app dev servers.
- `update-agents-index.sh`: Updates the workspace index in AGENTS.md using `diaryx workspace info`.

Plugin builds, registry assembly, and CDN uploads are handled by the [plugin-registry](https://github.com/diaryx-org/plugin-registry) repo and each plugin's own CI.

&nbsp;
