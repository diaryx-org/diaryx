---
title: Storage
description: Storage abstraction layer
part_of: '[README](/apps/web/src/lib/README.md)'
attachments:
  - '[index.ts](/apps/web/src/lib/storage/index.ts)'
  - '[sqliteStorage.ts](/apps/web/src/lib/storage/sqliteStorage.ts)'
  - '[sqliteStorageBridge.js](/apps/web/src/lib/storage/sqliteStorageBridge.js)'
exclude:
  - '*.lock'
---

# Storage

Storage abstraction layer for CRDT persistence.

## Files

| File | Purpose |
|------|---------|
| `sqliteStorage.ts` | SQLite-based CRDT storage |
| `sqliteStorageBridge.js` | Bridge to sql.js |
