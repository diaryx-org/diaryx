---
title: Backend
description: Backend abstraction layer for WASM and Tauri
part_of: '[README](/apps/web/src/lib/README.md)'
attachments:
  - '[index.ts](/apps/web/src/lib/backend/index.ts)'
  - '[api.ts](/apps/web/src/lib/backend/api.ts)'
  - '[eventEmitter.ts](/apps/web/src/lib/backend/eventEmitter.ts)'
  - '[interface.ts](/apps/web/src/lib/backend/interface.ts)'
  - '[storageType.ts](/apps/web/src/lib/backend/storageType.ts)'
  - '[tauri.ts](/apps/web/src/lib/backend/tauri.ts)'
  - '[wasmWorkerNew.ts](/apps/web/src/lib/backend/wasmWorkerNew.ts)'
  - '[workerBackendNew.ts](/apps/web/src/lib/backend/workerBackendNew.ts)'
exclude:
  - '*.lock'
  - 'generated/**'
  - 'serde_json/**'
  - '*.test.ts'
---

# Backend

Backend abstraction layer supporting both WASM and Tauri environments.

## Files

| File | Purpose |
|------|---------|
| `api.ts` | High-level backend API |
| `eventEmitter.ts` | Event emission for file changes |
| `interface.ts` | Backend interface definition |
| `storageType.ts` | Storage type detection |
| `tauri.ts` | Tauri IPC implementation |
| `wasmWorkerNew.ts` | WASM worker implementation |
| `workerBackendNew.ts` | Worker-based backend |

The `generated/` directory contains TypeScript types generated from Rust.
