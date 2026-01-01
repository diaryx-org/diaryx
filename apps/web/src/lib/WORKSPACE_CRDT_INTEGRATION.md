# Workspace CRDT Integration Guide

This document explains how to integrate the workspace CRDT into the Diaryx web app for real-time synchronization of the file hierarchy, frontmatter, and relationships.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        Hocuspocus Server                         │
│                    (or local-only with null URL)                 │
└─────────────────────────────────────────────────────────────────┘
                    │                           │
                    ▼                           ▼
    ┌───────────────────────────┐  ┌───────────────────────────┐
    │    Workspace CRDT         │  │   Per-File Body CRDTs     │
    │  (workspaceCrdt.ts)       │  │  (collaborationUtils.ts)  │
    │                           │  │                           │
    │  Room: {id}:workspace     │  │  Room: {id}:doc:{path}    │
    │                           │  │                           │
    │  Contains:                │  │  Contains:                │
    │  - files Y.Map            │  │  - TipTap Y.XmlFragment   │
    │    - title                │  │                           │
    │    - part_of              │  │  Used by: Editor.svelte   │
    │    - contents             │  │                           │
    │    - attachments          │  │                           │
    │    - audience             │  │                           │
    │    - extra                │  │                           │
    └───────────────────────────┘  └───────────────────────────┘
                    │                           │
                    ▼                           ▼
    ┌───────────────────────────┐  ┌───────────────────────────┐
    │   IndexedDB Persistence   │  │   IndexedDB Persistence   │
    │  (offline support)        │  │  (offline support)        │
    └───────────────────────────┘  └───────────────────────────┘
```

## Files

- **`workspaceCrdt.ts`** - Core workspace CRDT module
- **`hooks/useWorkspaceCrdt.svelte.ts`** - Svelte 5 reactive hook
- **`collaborationUtils.ts`** - Per-file body CRDTs (existing, updated)

## Integration Steps

### 1. Import the Hook

```svelte
<script lang="ts">
  import { useWorkspaceCrdt } from '$lib/hooks/useWorkspaceCrdt.svelte';
  
  const workspace = useWorkspaceCrdt();
</script>
```

### 2. Initialize on Mount

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { getBackend } from '$lib/backend';
  
  let backend = $state(null);
  const workspace = useWorkspaceCrdt();
  
  onMount(async () => {
    backend = await getBackend();
    
    // Initialize workspace CRDT
    await workspace.init({
      workspaceId: 'my-workspace', // or derive from config
      serverUrl: 'ws://localhost:1234', // or null for local-only
      onFilesChange: (files) => {
        console.log('Files changed:', files.size);
      },
      onConnectionChange: (connected) => {
        console.log('Connection:', connected ? 'online' : 'offline');
      },
    });
    
    // Sync from backend (loads existing files into CRDT)
    await workspace.syncFromBackend(backend);
  });
</script>
```

### 3. Use Reactive Tree

Instead of calling `backend.getWorkspaceTree()`, use the CRDT tree:

```svelte
<script lang="ts">
  // Before (backend-only):
  // let tree = $state(null);
  // tree = await backend.getWorkspaceTree();
  
  // After (CRDT-powered):
  const tree = $derived(workspace.tree);
</script>

<LeftSidebar {tree} />
```

### 4. Handle File Operations

When creating, moving, or deleting files, update both backend AND CRDT:

```svelte
<script lang="ts">
  async function createEntry(parentPath: string) {
    // 1. Create on backend (writes to disk/IndexedDB)
    const newPath = await backend.createChildEntry(parentPath);
    const entry = await backend.getEntry(newPath);
    
    // 2. Update CRDT (syncs to other devices)
    workspace.setFile(newPath, {
      title: entry.frontmatter.title ?? null,
      partOf: parentPath,
      contents: null, // leaf file
      attachments: [],
      deleted: false,
      audience: null,
      description: null,
      extra: {},
      modifiedAt: Date.now(),
    });
    workspace.addToContents(parentPath, newPath);
  }
  
  async function deleteEntry(path: string) {
    const metadata = workspace.getFile(path);
    
    // 1. Delete from backend
    await backend.deleteEntry(path);
    
    // 2. Mark as deleted in CRDT
    workspace.deleteFile(path);
    
    // 3. Remove from parent's contents
    if (metadata?.partOf) {
      workspace.removeFromContents(metadata.partOf, path);
    }
  }
  
  async function moveEntry(fromPath: string, toPath: string, newParent: string | null) {
    const metadata = workspace.getFile(fromPath);
    const oldParent = metadata?.partOf ?? null;
    
    // 1. Move on backend
    await backend.moveEntry(fromPath, toPath);
    
    // 2. Update CRDT
    workspace.moveFile(fromPath, oldParent, newParent);
    workspace.renameFile(fromPath, toPath);
  }
</script>
```

### 5. Handle Frontmatter Changes

When the user edits frontmatter in the sidebar:

```svelte
<script lang="ts">
  async function handlePropertyChange(path: string, key: string, value: unknown) {
    // 1. Update backend
    await backend.setFrontmatterProperty(path, key, value);
    
    // 2. Update CRDT based on the property
    switch (key) {
      case 'title':
        workspace.updateFile(path, { title: value as string });
        break;
      case 'description':
        workspace.updateFile(path, { description: value as string });
        break;
      case 'audience':
        workspace.updateFile(path, { audience: value as string[] });
        break;
      default:
        // Update extra properties
        const metadata = workspace.getFile(path);
        if (metadata) {
          workspace.updateFile(path, {
            extra: { ...metadata.extra, [key]: value },
          });
        }
    }
  }
</script>
```

### 6. Handle Attachments

```svelte
<script lang="ts">
  import { type BinaryRef } from '$lib/workspaceCrdt';
  
  async function uploadAttachment(entryPath: string, file: File) {
    // 1. Hash the file content
    const buffer = await file.arrayBuffer();
    const hashBuffer = await crypto.subtle.digest('SHA-256', buffer);
    const hash = Array.from(new Uint8Array(hashBuffer))
      .map(b => b.toString(16).padStart(2, '0'))
      .join('');
    
    // 2. Create attachment ref (pending upload)
    const ref: BinaryRef = {
      path: `_attachments/${file.name}`,
      source: 'pending',
      hash,
      mimeType: file.type,
      size: file.size,
      deleted: false,
    };
    
    // 3. Add to CRDT immediately (shows in UI)
    workspace.addAttachment(entryPath, ref);
    
    // 4. Upload to backend
    const dataBase64 = btoa(String.fromCharCode(...new Uint8Array(buffer)));
    const attachmentPath = await backend.uploadAttachment(entryPath, file.name, dataBase64);
    
    // 5. Update source URL (for remote storage like S3)
    workspace.updateAttachmentSource(entryPath, ref.path, 'local');
    // Or if uploading to S3:
    // workspace.updateAttachmentSource(entryPath, ref.path, 's3://bucket/hash.ext');
  }
  
  async function deleteAttachment(entryPath: string, attachmentPath: string) {
    // 1. Delete from backend
    await backend.deleteAttachment(entryPath, attachmentPath);
    
    // 2. Mark as deleted in CRDT
    workspace.removeAttachment(entryPath, attachmentPath);
  }
</script>
```

### 7. Set Up Workspace ID for Multi-Tenant

```svelte
<script lang="ts">
  import { setWorkspaceId } from '$lib/collaborationUtils';
  
  onMount(async () => {
    const config = await backend.getConfig();
    
    // Derive workspace ID from the workspace path
    // This ensures different workspaces don't conflict
    const workspaceId = btoa(config.default_workspace).replace(/[+/=]/g, '');
    
    // Set for both workspace CRDT and per-file CRDTs
    setWorkspaceId(workspaceId);
    
    await workspace.init({
      workspaceId,
      serverUrl: 'wss://sync.yourapp.com',
    });
  });
</script>
```

## Offline Support

Both the workspace CRDT and per-file CRDTs automatically persist to IndexedDB:

- Changes made offline are stored locally
- When reconnected, Y.js automatically syncs with peers
- No special handling needed in app code

## Conflict Resolution

Y.js CRDTs handle conflicts automatically:

- **Last-writer-wins** for simple values (title, description)
- **Set union** for arrays (contents, attachments, audience)
- **Tombstones** for deletions (deleted files stay in CRDT with `deleted: true`)

## Garbage Collection

Periodically clean up old deleted files:

```typescript
// Run on startup or on a schedule
const purged = workspace.garbageCollect(7 * 24 * 60 * 60 * 1000); // 7 days
console.log(`Purged ${purged} old deleted files`);
```

## Statistics

```typescript
const stats = workspace.getStats();
console.log(`
  Total files: ${stats.totalFiles}
  Active: ${stats.activeFiles}
  Deleted: ${stats.deletedFiles}
  Index files: ${stats.indexFiles}
  Leaf files: ${stats.leafFiles}
  Attachments: ${stats.totalAttachments}
`);
```

## Example: Full Integration in App.svelte

```svelte
<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { getBackend } from '$lib/backend';
  import { useWorkspaceCrdt } from '$lib/hooks/useWorkspaceCrdt.svelte';
  import { setWorkspaceId, getCollaborativeDocument, disconnectDocument } from '$lib/collaborationUtils';
  
  let backend = $state(null);
  const workspace = useWorkspaceCrdt();
  
  // Use CRDT tree instead of backend tree
  const tree = $derived(workspace.tree);
  
  // Connection status indicator
  const isOnline = $derived(workspace.connected);
  
  onMount(async () => {
    backend = await getBackend();
    const config = await backend.getConfig();
    
    // Set up workspace ID
    const workspaceId = btoa(config.default_workspace).replace(/[+/=]/g, '');
    setWorkspaceId(workspaceId);
    
    // Initialize workspace CRDT
    await workspace.init({
      workspaceId,
      serverUrl: import.meta.env.VITE_COLLAB_SERVER ?? null,
    });
    
    // Sync existing files from backend
    await workspace.syncFromBackend(backend);
    
    // Garbage collect old deleted files
    workspace.garbageCollect();
  });
  
  onDestroy(() => {
    workspace.disconnect();
  });
</script>

{#if !workspace.initialized}
  <p>Loading workspace...</p>
{:else}
  <header>
    <span class:online={isOnline} class:offline={!isOnline}>
      {isOnline ? '●' : '○'}
    </span>
    <span>{workspace.getStats().activeFiles} files</span>
  </header>
  
  <LeftSidebar {tree} />
{/if}
```

## Server Configuration

For production, configure environment variables:

```env
# .env.production
VITE_COLLAB_SERVER=wss://sync.yourapp.com
```

For local development without a server:

```env
# .env.development
VITE_COLLAB_SERVER=
```

## Future Enhancements

1. **Attachment blob storage** - Upload binaries to S3/R2 and store URLs in CRDT
2. **Presence/awareness** - Show who's viewing which files
3. **Selective sync** - Only sync files the user has accessed
4. **Compression** - Compress Y.js updates before sending
5. **Authentication** - Add JWT tokens to Hocuspocus connections
