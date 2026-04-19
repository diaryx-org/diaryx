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

## Cargo xtasks

Build and maintenance tasks live in `xtask/src/` and are invoked as `cargo xtask <task>`:

- `cargo xtask build-wasm`: Builds `crates/diaryx_wasm` for the web app via wasm-pack, runs `wasm-opt -Oz`, and trims trailing whitespace from the ts-rs generated bindings. Used in `apps/web/package.json`'s build script. On macOS it exports `DEVELOPER_DIR`/`SDKROOT` from Xcode tools to reduce host-toolchain SDK warnings.
- `cargo xtask build-plugin <crate> [--release]`: Builds a plugin WASM binary. On `--release`, also runs `wasm-opt -Oz`. Run without args to list available plugins.
- `cargo xtask release-plugin <crate> [--upload]`: Builds a release WASM and prepares a versioned artifact in `dist/plugins/<crate>/`. The GitHub Release asset keeps the crate WASM filename, with a local `plugin.wasm` copy retained for manual installs. With `--upload`, it also creates a GitHub Release on `diaryx-org/diaryx` and opens a PR against the plugin-registry repo. Requires `gh` CLI with push access.
- `cargo xtask sync-bindings`: Syncs ts-rs bindings from `crates/diaryx_core/bindings/` into `apps/web/src/lib/backend/generated/` as symlinks and auto-generates the barrel `index.ts`. Idempotent — correct existing symlinks are left in place.
- `cargo xtask sync-marketplace`: Fetches marketplace registry files from the production CDN into `apps/web/marketplace-dist/`. Run once after cloning to populate plugin, template, and starter workspace registries for local development. WASM artifacts are not downloaded — they are fetched at install time from the CDN. Override the source with `CDN_ORIGIN`.
- `cargo xtask sync-versions`: Using the root `README.md`'s `version` frontmatter as the source of truth, updates every version number in the repository (Cargo.toml workspace + the two version-locked deps, `apps/tauri/src-tauri/tauri.conf.json`, `apps/web/package.json`, and `flake.nix`'s Diaryx packages).
- `cargo xtask update-agents-index`: Updates the workspace index in `AGENTS.md` using `diaryx workspace info`. Bumps the `updated` frontmatter timestamp only when the tree actually changes.

## Shell scripts

App Store publishing (in `scripts/`, macOS-only):

- `publish-ios.sh`: Builds the iOS App Store export with the Tauri `apple` feature enabled, then uploads the IPA to App Store Connect using credentials from `scripts/.env.publish`.
- `publish-macos.sh`: Builds the macOS App Store bundle with the Tauri `apple` feature enabled, then signs, packages, and uploads the `.pkg` to App Store Connect using credentials from `scripts/.env.publish`.

Tauri dev signing (in `apps/tauri/scripts/`):

- `setup-macos-dev-signing.sh`: One-time setup for macOS. Validates that an Apple-anchored code-signing identity (default: first `Apple Development: ...` in your login keychain; override with `DIARYX_DEV_SIGN_IDENTITY`) is available and authorizes `codesign` to use its key without prompting. This lets macOS keychain "Always Allow" grants persist across `bun tauri dev` rebuilds instead of re-prompting every build. Safe to re-run.
- `macos-dev-sign.sh`: Cargo `runner` wired up by `apps/tauri/src-tauri/.cargo/config.toml`. When `DIARYX_DEV_SIGN=1` is set, re-signs the debug binary with the configured identity before executing. Reads `DIARYX_DEV_SIGN_IDENTITY`, or auto-detects the first Apple Development identity. No-op passthrough when the env var isn't set.

Self-hosted sync server (in `deploy/`):

- `deploy-sync-server.sh`: Example deploy script for a single-VPS self-hosted `diaryx_sync_server`. Used by `.github/workflows/deploy-sync-server.yml` after SCP'ing the binary and config. Shown as a reference for anyone mirroring the setup on their own infrastructure. Sibling files (`Caddyfile`, `diaryx-sync.service`) define the Caddy reverse-proxy and systemd unit.

Registry metadata lives in the [plugin-registry](https://github.com/diaryx-org/plugin-registry) repo.

&nbsp;
