---
title: AGENTS
description: Instructions for AI Agents
author: adammharris
updated: 2026-02-01T01:53:29Z
part_of: '[README](/README.md)'
---

# Instructions for AI agents

## Workspace Overview

<!-- BEGIN:WORKSPACE_INDEX -->
README - Repository for the Diaryx project
├── crates - Cargo crates for Diaryx
│   ├── diaryx - CLI frontend
│   │   └── diaryx src - Source code for the Diaryx CLI application
│   │       └── Command-line module - The main CLI command implementation module
│   │           └── Sync CLI module - CLI commands for workspace synchronization
│   ├── diaryx_core - Core library shared by Diaryx clients
│   │   └── diaryx_core src - Source code for the core Diaryx library
│   │       ├── CRDT Synchronization - Conflict-free replicated data types for real-time collaboration
│   │       ├── Cloud Sync - Bidirectional file synchronization with cloud storage
│   │       ├── Entry module - Entry manipulation functionality
│   │       ├── Filesystem module - Filesystem abstraction layer
│   │       ├── Publish module - HTML publishing using comrak
│   │       ├── Utils module - Utility functions for date and path handling
│   │       └── Workspace module - Workspace tree organization
│   ├── diaryx_wasm - WASM bindings for diaryx_core
│   │   └── diaryx_wasm src - Source code for WASM bindings
│   └── diaryx_sync_server - Sync server used by frontends
│       └── diaryx_sync_server src - Source code for the sync server
│           ├── Auth module - Authentication middleware and magic link handling
│           ├── Database module - SQLite database schema and repository
│           ├── Email module - SMTP email sending for magic links
│           ├── Handlers module - HTTP route handlers
│           └── Sync module - WebSocket sync room management
├── apps - GUI frontends for Diaryx
│   ├── web - Svelte + Tiptap frontend for Diaryx
│   │   ├── web src - Source code for the Diaryx web application
│   │   │   ├── Controllers - Controller logic for UI actions
│   │   │   ├── lib - Shared libraries and components
│   │   │   │   ├── Auth - Authentication services and stores
│   │   │   │   ├── Backend - Backend abstraction layer for WASM and Tauri
│   │   │   │   ├── Components - Reusable Svelte components
│   │   │   │   │   └── UI Components - shadcn-svelte based UI primitives
│   │   │   │   ├── CRDT - CRDT synchronization bridge
│   │   │   │   ├── Device - Device identification
│   │   │   │   ├── Extensions - TipTap editor extensions
│   │   │   │   ├── History - Version history components
│   │   │   │   ├── Hooks - Svelte hooks
│   │   │   │   ├── Settings - Settings panel components
│   │   │   │   ├── Share - Share session components
│   │   │   │   ├── Storage - Storage abstraction layer
│   │   │   │   ├── Lib Stores - Svelte stores for UI preferences
│   │   │   │   └── diaryx_wasm - WASM bindings for diaryx_core
│   │   │   ├── Models - Stores and services for application state
│   │   │   │   ├── Services - Business logic services
│   │   │   │   └── Stores - Svelte stores for reactive state
│   │   │   ├── Views - View components
│   │   │   │   ├── Editor Views - Editor-related view components
│   │   │   │   ├── Layout Views - Layout components
│   │   │   │   ├── Shared Views - Shared view components
│   │   │   │   └── Sidebar Views - Sidebar components
│   │   │   └── License - PolyForm Shield License 1.0.0
│   │   └── TipTap Custom Extensions - Guide to creating custom TipTap extensions with markdown support
│   └── tauri - Web app + native backend
├── LICENSE - PolyForm Shield License 1.0.0
├── ROADMAP - The plan for future Diaryx features
├── AGENTS - Instructions for AI Agents
├── CONTRIBUTING - A guide for making contributions in the Diaryx repo
└── Scripts
<!-- END:WORKSPACE_INDEX -->

## Entry Points

Read the root README.md first. For specific projects, use these entry points:

| Project | Entry point |
|---------|-------------|
| Entire workspace | README.md |
| Core library | crates/diaryx_core/README.md |
| CLI | crates/diaryx/README.md |
| Web app | apps/web/README.md |
| Tauri app | apps/tauri/README.md |
| WASM bindings | crates/diaryx_wasm/README.md |
| Sync server | crates/diaryx_sync_server/README.md |

## Commands

```bash
# Build all crates
cargo build

# Run tests
cargo test

# Run CLI
cargo run --bin diaryx -- <args>

# Build WASM
./scripts/build-wasm.sh
# Or, since this script is included in package.json...
cd apps/web && bun run build

# Web dev server
cd apps/web && bun run dev

# Tauri dev
cd apps/tauri && cargo tauri dev
```

## Not Documented

Read these files directly when needed:
- CI/workflows: `.github/workflows/*.yml`
- Pre-commit config: `.pre-commit-config.yaml`
