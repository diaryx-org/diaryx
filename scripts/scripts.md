---
title: Scripts
part_of: '[Diaryx](/Diaryx.md)'
attachments:
- build-wasm.sh
- publish-ios.sh
- publish-macos.sh
- sync-marketplace.sh
- sync-versions.sh
- test-sync.sh
- update-agents-index.sh
audience:
- developers
- agents
---

# Scripts

- `build-wasm.sh`: Builds `crates/diaryx_wasm` for the web app via wasm-pack. Used in `apps/web/package.json`'s build script. On macOS it also exports `DEVELOPER_DIR`/`SDKROOT` from Xcode tools to reduce host-toolchain SDK warnings.
- `publish-ios.sh`: Builds the iOS App Store export with the Tauri `apple` feature enabled, then uploads the IPA to App Store Connect using credentials from `scripts/.env.publish`.
- `publish-macos.sh`: Builds the macOS App Store bundle with the Tauri `apple` feature enabled, then signs, packages, and uploads the `.pkg` to App Store Connect using credentials from `scripts/.env.publish`.
- `sync-marketplace.sh`: Fetches marketplace registry files from the production CDN into `apps/web/marketplace-dist/`. Run once after cloning to populate plugin, template, and starter workspace registries for local development. WASM artifacts are not downloaded — they are fetched at install time from the CDN.
- `sync-versions.sh`: Using the root README.md as a source of truth, updates every version number in the repository.
- `test-sync.sh`: Opens a tmux window with the diaryx_sync_server and two web app dev servers.
- `update-agents-index.sh`: Updates the workspace index in AGENTS.md using `diaryx workspace info`.

Plugin builds, registry assembly, and CDN uploads are handled by the [plugin-registry](https://github.com/diaryx-org/plugin-registry) repo and each plugin's own CI.

&nbsp;
