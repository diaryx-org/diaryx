---
title: Contributing to Diaryx
description: A guide for making contributions in the Diaryx repo
part_of: '[Diaryx](/Diaryx.md)'
audience:
- developers
updated: 2026-06-05T17:06:16-07:00
---
# Contributing to Diaryx

Welcome to the Diaryx project! This document will help you understand the codebase structure, identify areas for improvement, and find good first issues to work on.

Note that much of the documentation in this repo is NOT complete, though it is mostly up-to-date.

## Repository Structure

Diaryx is organized as a Rust workspace with multiple crates:

```
diaryx/
├── crates/
│   ├── diaryx_core/     # Core library - shared logic for all frontends
│   ├── diaryx/          # CLI application
│   └── diaryx_wasm/     # WebAssembly bindings for web frontend
├── apps/
│   ├── tauri/           # Desktop application (Tauri)
│   └── web/             # Web application
└── Cargo.toml           # Workspace configuration
```

### Crate Overview

#### `diaryx_core` - Core Library

The heart of the project. Contains all business logic that should be shared across frontends.

See [more information here](crates/diaryx_core/README.md).

#### `diaryx` - CLI Application

Command-line interface built on top of `diaryx_core`.

See [more information here](crates/diaryx/README.md).

#### `diaryx_wasm` - WebAssembly Bindings

WASM bindings that expose `diaryx_core` functionality to JavaScript. Uses an in-memory filesystem that syncs with IndexedDB.

See [more information here](crates/diaryx_wasm/README.md).

---

## Development Setup

```bash
# Clone the repository
git clone https://github.com/diaryx-org/diaryx-core.git
cd diaryx-core

# Install the pre-commit hook (one-time, per clone)
cargo xtask install-hooks

# Build all crates
cargo build

# Run tests
cargo test

# Install the CLI locally
cargo install --path crates/diaryx

# Build WASM (requires wasm-pack)
wasm-pack build crates/diaryx_wasm --target web
```

## xtask Commands

Project automation lives in `cargo xtask`. Run `cargo xtask help` for full usage and flags.


| Command                            | Purpose                                                                                                     |
| ---------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `build-wasm [--panic-hook]`        | Build `crates/diaryx_wasm` for `apps/web` (wasm-pack + wasm-opt).                                           |
| `build-plugin <name>`              | Build a plugin WASM (`--release` for size-optimized).                                                       |
| `check [--fix]`                    | Run `cargo fmt` + `cargo clippy` concurrently with `svelte-check`.                                          |
| `clean [--dry-run]`                | `cargo clean` plus removal of stray nested `target/` dirs.                                                  |
| `install-hooks [--force]`          | Install `.git/hooks/pre-commit` → `cargo xtask pre-commit`.                                                 |
| `pre-commit [--all]`               | Run the pre-commit checks (invoked by the git hook).                                                        |
| `publish-ios`                      | Build the iOS App Store export and upload via altool (macOS only).                                          |
| `publish-macos <build>`            | Build, sign, package, and upload the macOS App Store `.pkg` (macOS only).                                   |
| `release-plugin <name> [--upload]` | Build a release WASM + `dist/` artifact; with `--upload`, cut a GitHub Release + open a plugin-registry PR. |
| `sync-bindings`                    | Sync ts-rs bindings into `apps/web/src/lib/backend/generated/`.                                             |
| `sync-marketplace`                 | Fetch marketplace registries from the production CDN.                                                       |
| `sync-versions`                    | Propagate `README.md` version → `Cargo.toml` / `tauri.conf.json` / `package.json` / `flake.nix`.            |
| `tauri <subcommand>`               | Tauri builds: `macos`, `ios`, `render-updater-config`.                                                      |
| `update-agents-index`              | Refresh the workspace tree in `AGENTS.md`.                                                                  |


Thank you for contributing to Diaryx!
