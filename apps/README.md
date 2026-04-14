---
title: apps
description: GUI frontends for Diaryx
author: adammharris
audience:
- developers
- agents
contents:
- '[README](/apps/web/README.md)'
- '[README](/apps/tauri/README.md)'
part_of: '[Diaryx](/Diaryx.md)'
updated: 2026-03-29T19:37:18-06:00
---
# Diaryx Frontend Apps

This directory contains the frontend applications for Diaryx.

## Architecture

```
apps/
├── web/                    # Shared frontend code (source of truth)
│   ├── src/
│   │   ├── lib/
│   │   │   ├── backend/    # Backend abstraction layer
│   │   │   │   ├── index.ts      # Factory & exports
│   │   │   │   ├── interface.ts  # Backend interface definition
│   │   │   │   ├── tauri.ts      # Tauri IPC implementation
│   │   │   │   └── wasm.ts       # WASM + IndexedDB implementation
│   │   │   └── Editor.svelte
│   │   ├── App.svelte
│   │   └── main.ts
│   ├── index.html
│   ├── vite.config.ts
│   └── package.json
│
└── tauri/                  # Tauri desktop wrapper
    ├── src-tauri/          # Rust Tauri backend
    │   ├── src/
    │   │   ├── commands.rs # Tauri IPC command handlers
    │   │   └── main.rs
    │   └── Cargo.toml
    ├── vite.config.ts      # Points to ../web as root
    └── package.json
```

## Backend Abstraction

The key to supporting both Tauri (desktop) and pure web targets is the **Backend interface** in `web/src/lib/backend/`.

### How It Works

1. `interface.ts` - Defines the `Backend` interface with all operations (getConfig, getEntry, saveEntry, search, etc.)
2. `tauri.ts` - Implements `Backend` using Tauri's `invoke()` IPC to call Rust backend
3. `wasm.ts` - Implements `Backend` using:
  - JavaScript fallbacks (or WASM module) for parsing/rendering
4. `index.ts` - Factory that auto-detects the runtime environment:
  ```typescript
   import { getBackend } from "./lib/backend";

   const backend = await getBackend();
   // Returns TauriBackend if window.__TAURI__ exists
   // Returns WasmBackend otherwise
  ```

### Runtime Detection

```typescript
// Tauri injects __TAURI__ into the window object
function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI__" in window;
}
```

## Development

### Web App (standalone browser)

```bash
cd apps/web
bun install
bun run dev      # Starts on http://localhost:5174
```

Uses WASM backend with IndexedDB for persistence.

### Tauri App (desktop)

```bash
cd apps/tauri
bun tauri dev
```

Uses Tauri IPC backend with real filesystem.

## Building

### Web App

```bash
cd apps/web
bun run build    # Output: apps/web/dist/
```

### Tauri App

```bash
cd apps/tauri
bun tauri build
```

## Adding New Backend Operations

1. Add the method signature to `Backend` interface in `interface.ts`
2. Implement in `TauriBackend` (calls `invoke()`)
3. Implement in `WasmBackend` (uses in-memory FS + WASM)
4. Add corresponding Tauri command in `src-tauri/src/commands.rs`
