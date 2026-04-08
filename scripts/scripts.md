---
title: Scripts
part_of: '[Diaryx](/Diaryx.md)'
exclude:
- '*.sh'
audience:
- developers
- agents
contents: []
---

# Scripts

- `build-wasm.sh`: Builds `crates/diaryx_wasm` for the web app via wasm-pack. Used in `apps/web/package.json`'s build script. On macOS it also exports `DEVELOPER_DIR`/`SDKROOT` from Xcode tools to reduce host-toolchain SDK warnings.
- `publish-ios.sh`: Builds the iOS App Store export with the Tauri `apple` feature enabled, then uploads the IPA to App Store Connect using credentials from `scripts/.env.publish`.
- `publish-macos.sh`: Builds the macOS App Store bundle with the Tauri `apple` feature enabled, then signs, packages, and uploads the `.pkg` to App Store Connect using credentials from `scripts/.env.publish`.
- `sync-marketplace.sh`: Fetches marketplace registry files from the production CDN into `apps/web/marketplace-dist/`. Run once after cloning to populate plugin, template, and starter workspace registries for local development. WASM artifacts are not downloaded — they are fetched at install time from the CDN.
- `sync-versions.sh`: Using the root README.md as a source of truth, updates every version number in the repository.
- `test-sync.sh`: Opens a tmux window with the diaryx_sync_server and two web app dev servers.
- `update-agents-index.sh`: Updates the workspace index in AGENTS.md using `diaryx workspace info`.

- `build-plugin.sh`: Builds a plugin WASM binary. Usage: `./scripts/build-plugin.sh <crate-name> [--release]`.
- `release-plugin.sh`: Builds a release WASM, runs wasm-opt, and prepares a versioned artifact in `dist/plugins/` for CDN upload. The GitHub Release asset keeps the crate WASM filename, with a local `plugin.wasm` copy retained for manual installs. With `--upload`, it also creates a GitHub Release on diaryx-org/diaryx and opens a PR against plugin-registry to update the registry entry. Requires `gh` CLI with push access to plugin-registry.

Registry metadata lives in the [plugin-registry](https://github.com/diaryx-org/plugin-registry) repo.

&nbsp;
