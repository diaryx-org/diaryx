---
title: web
description: Svelte + Tiptap frontend for Diaryx
author: adammharris
audience:
  - public
  - developers
part_of: "[README](/apps/README.md)"
contents:
  - "[README](/apps/web/src/README.md)"
  - "[Tiptap Custom Extensions](/apps/web/docs/tiptap-custom-extensions.md)"
attachments:
  - "[package.json](/apps/web/package.json)"
  - "[vite.config.ts](/apps/web/vite.config.ts)"
  - "[svelte.config.js](/apps/web/svelte.config.js)"
  - "[tsconfig.json](/apps/web/tsconfig.json)"
  - "[vitest.config.ts](/apps/web/vitest.config.ts)"
  - "[playwright.config.ts](/apps/web/playwright.config.ts)"
  - "[components.json](/apps/web/components.json)"
  - "[index.html](/apps/web/index.html)"
exclude:
  - "*.lock"
  - "node_modules/**"
  - "dist/**"
  - "e2e/**"
---

# Diaryx Web

The Svelte web frontend for Diaryx, supporting both WebAssembly and Tauri backends.

## Getting Started

```bash
# Install dependencies (uses Bun package manager)
bun install

# Development server
bun run dev

# Build for production
bun run build
```

## Architecture

This is a plain Svelte 5 app (not SvelteKit). It uses a backend abstraction layer to support two runtime environments:

### Backend Abstraction

The `src/lib/backend/` directory contains:

| File           | Purpose                                             |
| -------------- | --------------------------------------------------- |
| `interface.ts` | TypeScript interface defining all backend methods   |
| `wasm.ts`      | WebAssembly implementation (InMemoryFS + IndexedDB) |
| `tauri.ts`     | Tauri IPC implementation (native filesystem)        |
| `index.ts`     | Runtime detection and backend export                |

```typescript
import { backend } from "$lib/backend";

// Works identically in both WASM and Tauri environments
await backend.init();
const tree = await backend.getWorkspaceTree();
const entry = await backend.getEntry("workspace/notes/my-note.md");
```

### WASM Backend

When running in a browser without Tauri:

1. Files are stored in IndexedDB
2. Loaded into `InMemoryFileSystem` on startup
3. All operations happen in memory
4. Changes persisted back to IndexedDB via `backend.persist()`

### Tauri Backend

When running inside Tauri:

1. Commands sent via Tauri IPC
2. Rust backend uses `RealFileSystem`
3. Direct native filesystem access
4. No explicit persistence needed

## Validation

Both backends support comprehensive validation and automatic fixing:

```typescript
// Validate workspace
const result = await backend.validateWorkspace();

if (result.errors.length > 0 || result.warnings.length > 0) {
  // Fix all issues automatically
  const summary = await backend.fixAll(result);
  console.log(`Fixed: ${summary.total_fixed}, Failed: ${summary.total_failed}`);
}

// Or fix individual issues
await backend.fixBrokenPartOf("workspace/broken.md");
await backend.fixUnlistedFile("workspace/index.md", "workspace/new-file.md");
```

### Validation Errors

- `BrokenPartOf` - `part_of` points to non-existent file
- `BrokenContentsRef` - `contents` references non-existent file
- `BrokenAttachment` - `attachments` references non-existent file

### Validation Warnings

- `OrphanFile` - Markdown file not in any index's contents
- `UnlinkedEntry` - File/directory not in contents hierarchy
- `UnlistedFile` - File exists but not listed in index's contents
- `CircularReference` - Circular reference in hierarchy
- `NonPortablePath` - Path contains absolute or `.`/`..` components
- `MultipleIndexes` - Multiple index files in same directory
- `OrphanBinaryFile` - Binary file not in any attachments

## E2E Sync Test

The sync E2E test expects a running sync server (default `http://127.0.0.1:3030`).
The test relies on the dev-mode magic link response (`dev_link`), so the sync server
must be running without SMTP configured.

When initializing with **Load from server**, the wizard clears the local workspace
before downloading so local-only files are removed.
The sync workspace transfer test validates entries via the CRDT state first
(metadata + body content), then falls back to file APIs, because browser clients
may not materialize files on disk during a load-from-server flow.

When running E2E tests in parallel across browsers, each project uses its own sync
server port by default to avoid conflicts (chromium: base port, webkit: base+1,
firefox: base+2). Set `SYNC_SERVER_URL` to override this behavior.

Environment variables:

- `SYNC_SERVER_URL` (optional): override the sync server URL (disables per-project ports)
- `SYNC_SERVER_HOST` (optional): host for the auto-started sync server (default `127.0.0.1`)
- `SYNC_SERVER_PORT` (optional): base port for the auto-started sync server (default `3030`)
- `SYNC_E2E_START_SERVER` (optional): set to `0` to skip auto-starting the sync server
- `MissingPartOf` - Non-index file has no `part_of`

## Project Structure

```
src/
├── App.svelte           # Main app component
├── main.ts              # Entry point
├── app.css              # Global styles
└── lib/
    ├── backend/         # Backend abstraction layer
    │   ├── interface.ts # Backend interface definition
    │   ├── wasm.ts      # WebAssembly implementation
    │   ├── tauri.ts     # Tauri IPC implementation
    │   └── index.ts     # Runtime detection
    ├── components/      # Reusable Svelte components
    ├── stores/          # Svelte stores for state management
    ├── hooks/           # Custom Svelte hooks
    ├── wasm/            # Built WASM module (from diaryx_wasm)
    ├── Editor.svelte    # TipTap markdown editor
    ├── LeftSidebar.svelte
    ├── RightSidebar.svelte
    └── ...
```

## Testing

The web app includes comprehensive unit tests (Vitest) and E2E tests (Playwright).

### Running Tests

```bash
# Run unit tests
bun run test

# Run tests with coverage
bun run test:coverage

# Run tests with UI
bun run test:ui

# Run E2E tests
bun run test:e2e

# Run E2E tests with UI
bun run test:e2e:ui
```

### Test Structure

```
src/
├── test/
│   └── setup.ts                    # Test setup and mocks
├── models/
│   ├── services/
│   │   ├── attachmentService.test.ts
│   │   ├── shareService.test.ts
│   │   ├── toastService.test.ts
│   │   └── workspaceCrdtService.test.ts
│   └── stores/
│       ├── workspaceStore.test.ts
│       ├── entryStore.test.ts
│       ├── collaborationStore.test.ts
│       └── uiStore.test.ts
├── lib/
│   ├── backend/
│   │   └── api.test.ts
│   ├── crdt/
│   │   ├── workspaceCrdtBridge.test.ts
│   │   └── syncTransport.test.ts
│   └── components/
│       └── AttachmentPicker.test.ts
e2e/
├── workspace.spec.ts               # Workspace navigation tests
├── editor.spec.ts                  # Editor functionality tests
├── attachments.spec.ts             # Attachment handling tests
├── entry-switch.spec.ts            # Entry switching while attachments load
├── share.spec.ts                   # Share session tests
├── sync.spec.ts                    # Sync smoke test
└── sync-workspace.spec.ts          # Sync workspace transfer test
```

### Configuration

- `vitest.config.ts` - Vitest configuration with Svelte support and jsdom environment
- `playwright.config.ts` - Playwright configuration for E2E testing

## Building WASM

The WASM module is built from `crates/diaryx_wasm`:

```bash
cd ../../crates/diaryx_wasm
wasm-pack build --target web --out-dir ../../apps/web/src/lib/wasm
```

## Developer Guides

| Guide                                                        | Description                                             |
| ------------------------------------------------------------ | ------------------------------------------------------- |
| [TipTap Custom Extensions](docs/tiptap-custom-extensions.md) | Creating custom TipTap extensions with markdown support |

## Live Demo

Try the web frontend at: https://diaryx-org.github.io/diaryx/
