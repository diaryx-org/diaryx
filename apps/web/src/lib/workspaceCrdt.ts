/**
 * Workspace CRDT module for Diaryx.
 *
 * Provides real-time synchronization of the workspace hierarchy (file tree,
 * frontmatter metadata, and relationships) using Y.js CRDTs.
 *
 * Architecture:
 * - Single workspace Y.Doc containing a "files" Y.Map
 * - Each file entry contains metadata (title, part_of, contents, attachments, etc.)
 * - Separate from per-file body CRDTs (handled by collaborationUtils.ts)
 * - Changes propagate to all connected peers in real-time
 *
 * Room naming convention:
 * - Workspace CRDT: "{workspaceId}:workspace" or just "workspace" for local
 * - File body CRDTs: "{workspaceId}:doc:{path}" (existing TipTap docs)
 */

import * as Y from "yjs";
import { HocuspocusProvider } from "@hocuspocus/provider";
import { IndexeddbPersistence } from "y-indexeddb";
import type { Backend, BackendEvent } from "./backend/interface";

// Origin marker for local changes (to distinguish from remote)
const LOCAL_ORIGIN = "local";

// ============================================================================
// Types
// ============================================================================

/**
 * Reference to a binary attachment (stored externally, not in CRDT).
 */
export interface BinaryRef {
  /** Relative path within the workspace (e.g., "_attachments/photo.jpg") */
  path: string;
  /** External storage URL or "local" for not-yet-synced, "pending" for uploading */
  source: string;
  /** Content hash (SHA-256) for deduplication and integrity */
  hash: string;
  /** MIME type of the file */
  mimeType: string;
  /** File size in bytes */
  size: number;
  /** When the attachment was uploaded (Unix timestamp) */
  uploadedAt?: number;
  /** Tombstone for soft deletion */
  deleted: boolean;
}

/**
 * Metadata for a single file in the workspace.
 * This is stored in the workspace CRDT, NOT the file body.
 */
export interface FileMetadata {
  /** Display title (from frontmatter) */
  title: string | null;
  /** Parent index path (relative), null for root */
  partOf: string | null;
  /** Child paths for index files, null for leaf files */
  contents: string[] | null;
  /** Attachment references */
  attachments: BinaryRef[];
  /** Soft deletion tombstone */
  deleted: boolean;
  /** Audience tags for visibility filtering */
  audience: string[] | null;
  /** Description (from frontmatter) */
  description: string | null;
  /** Any additional frontmatter properties */
  extra: Record<string, unknown>;
  /** Last modified timestamp (for conflict resolution hints) */
  modifiedAt: number;
}

/**
 * Workspace session state.
 */
interface WorkspaceSession {
  ydoc: Y.Doc;
  provider: HocuspocusProvider | null;
  persistence: IndexeddbPersistence;
  filesMap: Y.Map<FileMetadata>;
  backend: Backend | null;
  onFilesChange?: (files: Map<string, FileMetadata>) => void;
  /** Callback when remote sync creates/deletes files */
  onRemoteFileSync?: (created: string[], deleted: string[]) => void;
}

/**
 * Options for initializing the workspace CRDT.
 */
export interface WorkspaceInitOptions {
  /** Workspace identifier (used in room name for multi-tenant scenarios) */
  workspaceId?: string;
  /** Collaboration server URL (null to disable remote sync) */
  serverUrl?: string | null;
  /** Backend instance for file operations */
  backend?: Backend | null;
  /** Callback when files map changes */
  onFilesChange?: (files: Map<string, FileMetadata>) => void;
  /** Callback when a specific file's metadata changes */
  onFileChange?: (path: string, metadata: FileMetadata | null) => void;
  /** Callback when connection status changes */
  onConnectionChange?: (connected: boolean) => void;
  /** Callback when remote sync creates/deletes files locally */
  onRemoteFileSync?: (created: string[], deleted: string[]) => void;
  /** Callback when server sync completes (for background delta sync) */
  onServerSynced?: () => Promise<void>;
}

// ============================================================================
// Module State
// ============================================================================

let workspaceSession: WorkspaceSession | null = null;
let connectionChangeCallback: ((connected: boolean) => void) | null = null;
let fileChangeCallback:
  | ((path: string, metadata: FileMetadata | null) => void)
  | null = null;
let serverSyncedCallback: (() => Promise<void>) | null = null;

// IndexedDB sync tracking
let indexedDBSynced = false;
let indexedDBSyncResolvers: (() => void)[] = [];

// Default server URL (can be overridden) - load from localStorage
const SYNC_SERVER_KEY = "diaryx-sync-server";
const DEFAULT_SERVER_URL = "ws://localhost:1234";
let defaultServerUrl: string | null = typeof window !== "undefined"
  ? localStorage.getItem(SYNC_SERVER_KEY) || DEFAULT_SERVER_URL
  : DEFAULT_SERVER_URL;

// Debounce state for file change callbacks
let filesChangeDebounceTimer: ReturnType<typeof setTimeout> | null = null;
const FILES_CHANGE_DEBOUNCE_MS = 100; // Debounce rapid changes

// Remote file sync callback
let remoteFileSyncCallback: ((created: string[], deleted: string[]) => void) | null = null;

// Track paths currently being processed to avoid duplicate operations
let pathsBeingProcessed = new Set<string>();

// Flag to prevent remote sync handling during initial syncFromBackend
// Set to true before syncing, false after. This prevents race conditions.
let isInitializing = false;

// ============================================================================
// Configuration
// ============================================================================

/**
 * Set the default collaboration server URL for workspace sync.
 * Set to null to disable remote sync (local-only mode).
 * Persists to localStorage for future sessions.
 */
export function setWorkspaceServer(url: string | null): void {
  defaultServerUrl = url;
  if (typeof window !== "undefined") {
    if (url) {
      localStorage.setItem(SYNC_SERVER_KEY, url);
    } else {
      localStorage.removeItem(SYNC_SERVER_KEY);
    }
  }
}

/**
 * Get the current default server URL.
 */
export function getWorkspaceServer(): string | null {
  return defaultServerUrl;
}

/**
 * Set the initializing flag to prevent remote sync handling during syncFromBackend.
 * Call with true before syncing from backend, false after.
 */
export function setInitializing(initializing: boolean): void {
  isInitializing = initializing;
  if (initializing) {
    console.log("[WorkspaceCRDT] Entering initialization mode - remote sync paused");
  } else {
    console.log("[WorkspaceCRDT] Exiting initialization mode - remote sync resumed");
  }
}

// ============================================================================
// Initialization & Lifecycle
// ============================================================================

/**
 * Initialize the workspace CRDT.
 *
 * This creates a Y.Doc for the workspace hierarchy with:
 * - IndexedDB persistence for offline support
 * - Optional Hocuspocus provider for real-time sync
 *
 * @param options Initialization options
 * @returns The workspace Y.Doc and files map
 */
export async function initWorkspace(
  options: WorkspaceInitOptions = {},
): Promise<{
  ydoc: Y.Doc;
  filesMap: Y.Map<FileMetadata>;
}> {
  // Clean up existing session if any
  if (workspaceSession) {
    await destroyWorkspace();
  }

  const {
    workspaceId = "default",
    serverUrl = defaultServerUrl,
    backend = null,
    onFilesChange,
    onFileChange,
    onConnectionChange,
    onRemoteFileSync,
    onServerSynced,
  } = options;

  // Store callbacks
  connectionChangeCallback = onConnectionChange ?? null;
  fileChangeCallback = onFileChange ?? null;
  remoteFileSyncCallback = onRemoteFileSync ?? null;
  serverSyncedCallback = onServerSynced ?? null;
  
  // Reset IndexedDB sync state
  indexedDBSynced = false;
  indexedDBSyncResolvers = [];

  // Create Y.Doc
  const ydoc = new Y.Doc();

  // Get or create the files map
  const filesMap = ydoc.getMap<FileMetadata>("files");

  // Room name for this workspace
  const roomName = workspaceId ? `${workspaceId}:workspace` : "workspace";

  // Create IndexedDB persistence for offline support
  const persistence = new IndexeddbPersistence(
    `diaryx-workspace-${workspaceId}`,
    ydoc,
  );

  persistence.on("synced", () => {
    console.log(
      `[WorkspaceCRDT] IndexedDB synced for workspace ${workspaceId}`,
    );
    
    // Mark as synced and resolve any pending waiters
    indexedDBSynced = true;
    indexedDBSyncResolvers.forEach(resolve => resolve());
    indexedDBSyncResolvers = [];
  });

  // Create Hocuspocus provider if server URL is provided
  let provider: HocuspocusProvider | null = null;

  if (serverUrl) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    provider = new HocuspocusProvider({
      url: serverUrl,
      name: roomName,
      document: ydoc,
      onConnect: () => {
        console.log(`[WorkspaceCRDT] Connected to ${roomName}`);
        connectionChangeCallback?.(true);
      },
      onDisconnect: () => {
        console.log(`[WorkspaceCRDT] Disconnected from ${roomName}`);
        connectionChangeCallback?.(false);
      },
      onSynced: () => {
        console.log(`[WorkspaceCRDT] Server synced ${roomName}`);
        
        // Call serverSyncedCallback for background delta sync
        // This allows the service to run syncToLocal() in the background
        if (serverSyncedCallback) {
          serverSyncedCallback().catch(e => {
            console.error('[WorkspaceCRDT] Error in serverSyncedCallback:', e);
          });
        }
      },
    } as any);
  }

  // Create session
  workspaceSession = {
    ydoc,
    provider,
    persistence,
    filesMap,
    backend,
    onFilesChange,
    onRemoteFileSync,
  };

  // Set up change observers with error handling
  filesMap.observe((event) => {
    try {
      handleFilesMapChange(event);
    } catch (e) {
      console.error("[WorkspaceCRDT] Error in filesMap observer:", e);
    }
  });

  // Deep observe for changes to individual file metadata
  filesMap.observeDeep((events) => {
    try {
      handleFilesDeepChange(events);
    } catch (e) {
      console.error("[WorkspaceCRDT] Error in filesMap deep observer:", e);
    }
  });

  // Set up backend event subscriptions for automatic CRDT updates
  if (backend) {
    setupBackendEventSubscriptions(backend);
  }

  console.log(`[WorkspaceCRDT] Initialized workspace ${workspaceId}`);

  return { ydoc, filesMap };
}

/**
 * Set up subscriptions to backend events for automatic CRDT updates.
 * This is the core of the event-driven architecture - backend operations
 * automatically trigger CRDT updates without manual intervention.
 */
function setupBackendEventSubscriptions(backend: Backend): void {
  /**
   * Compute relative path from parent's directory to child file.
   * This matches how the Rust backend stores paths in contents arrays.
   * Example: parentPath="workspace/README.md", childPath="workspace/test.md" -> "test.md"
   */
  const getRelativePathForContents = (parentPath: string, childPath: string): string => {
    // Get parent directory (remove filename from parent path)
    const parentDir = parentPath.substring(0, parentPath.lastIndexOf('/') + 1);
    
    // If child path starts with parent directory, return relative portion
    if (childPath.startsWith(parentDir)) {
      return childPath.substring(parentDir.length);
    }
    
    // Fallback: just return the filename
    const lastSlash = childPath.lastIndexOf('/');
    return lastSlash >= 0 ? childPath.substring(lastSlash + 1) : childPath;
  };

  // Handler for file created events
  const handleEvent = (event: BackendEvent) => {
    if (!workspaceSession) return;
    
    switch (event.type) {
      case 'file:created': {
        console.log(`[WorkspaceCRDT] Event: file:created ${event.path}`);
        
        // Build metadata from frontmatter
        const metadata: FileMetadata = {
          title: (event.frontmatter.title as string) ?? null,
          partOf: (event.frontmatter.part_of as string) ?? null,
          contents: (event.frontmatter.contents as string[]) ?? null,
          attachments: [], // Will be populated later if needed
          deleted: false,
          audience: (event.frontmatter.audience as string[]) ?? null,
          description: (event.frontmatter.description as string) ?? null,
          extra: {},
          modifiedAt: Date.now(),
        };
        
        // Add to CRDT
        setFileMetadata(event.path, metadata);
        
        // Update parent's contents if we have a parent
        // Note: contents should store relative paths from parent directory
        if (event.parentPath) {
          const relativePath = getRelativePathForContents(event.parentPath, event.path);
          addToContents(event.parentPath, relativePath);
        }
        break;
      }
      
      case 'file:deleted': {
        console.log(`[WorkspaceCRDT] Event: file:deleted ${event.path}`);
        
        // Mark as deleted in CRDT
        deleteFile(event.path);
        
        // Remove from parent's contents
        // Note: contents stores relative paths, so we need to compute what was stored
        if (event.parentPath) {
          const relativePath = getRelativePathForContents(event.parentPath, event.path);
          removeFromContents(event.parentPath, relativePath);
        }
        break;
      }
      
      case 'file:moved': {
        console.log(`[WorkspaceCRDT] Event: file:moved ${event.path}`);
        
        // Update partOf in the file's metadata
        if (event.newParent) {
          updateFileMetadata(event.path, { partOf: event.newParent });
        }
        
        // Remove from old parent's contents
        if (event.oldParent) {
          const oldRelativePath = getRelativePathForContents(event.oldParent, event.path);
          removeFromContents(event.oldParent, oldRelativePath);
        }
        
        // Add to new parent's contents
        if (event.newParent) {
          const newRelativePath = getRelativePathForContents(event.newParent, event.path);
          addToContents(event.newParent, newRelativePath);
        }
        break;
      }
      
      case 'file:renamed': {
        console.log(`[WorkspaceCRDT] Event: file:renamed ${event.oldPath} -> ${event.newPath}`);
        
        // Get the old metadata
        const oldMetadata = workspaceSession.filesMap.get(event.oldPath);
        if (oldMetadata) {
          // Create new entry with same metadata
          setFileMetadata(event.newPath, {
            ...oldMetadata,
            modifiedAt: Date.now(),
          });
          
          // Remove old entry
          purgeFile(event.oldPath);
          
          // Update parent's contents to reference new path
          if (oldMetadata.partOf) {
            const oldRelative = getRelativePathForContents(oldMetadata.partOf, event.oldPath);
            const newRelative = getRelativePathForContents(oldMetadata.partOf, event.newPath);
            removeFromContents(oldMetadata.partOf, oldRelative);
            addToContents(oldMetadata.partOf, newRelative);
          }
        }
        break;
      }
      
      case 'metadata:changed': {
        console.log(`[WorkspaceCRDT] Event: metadata:changed ${event.path}`);
        
        // Convert frontmatter to CRDT metadata format and update
        const fm = event.frontmatter;
        const updates: Partial<FileMetadata> = {
          title: (fm.title as string) ?? null,
          partOf: (fm.part_of as string) ?? null,
          contents: (fm.contents as string[]) ?? null,
          audience: (fm.audience as string[]) ?? null,
          description: (fm.description as string) ?? null,
          modifiedAt: Date.now(),
        };
        updateFileMetadata(event.path, updates);
        break;
      }
      
      case 'contents:changed': {
        console.log(`[WorkspaceCRDT] Event: contents:changed ${event.path}`);
        updateFileMetadata(event.path, { contents: event.contents });
        break;
      }
    }
  };
  
  // Subscribe to all event types
  backend.on('file:created', handleEvent);
  backend.on('file:deleted', handleEvent);
  backend.on('file:moved', handleEvent);
  backend.on('file:renamed', handleEvent);
  backend.on('metadata:changed', handleEvent);
  backend.on('contents:changed', handleEvent);
  
  console.log('[WorkspaceCRDT] Subscribed to backend events');
}

/**
 * Disconnect the workspace CRDT (keeps local state).
 */
export function disconnectWorkspace(): void {
  if (!workspaceSession) return;

  workspaceSession.provider?.disconnect();
  console.log("[WorkspaceCRDT] Disconnected (kept local state)");
}

/**
 * Reconnect the workspace CRDT.
 */
export function reconnectWorkspace(): void {
  if (!workspaceSession?.provider) return;

  workspaceSession.provider.connect();
  console.log("[WorkspaceCRDT] Reconnecting...");
}

/**
 * Fully destroy the workspace session.
 */
export async function destroyWorkspace(): Promise<void> {
  if (!workspaceSession) return;

  // Clear any pending debounce timer
  if (filesChangeDebounceTimer) {
    clearTimeout(filesChangeDebounceTimer);
    filesChangeDebounceTimer = null;
  }

  workspaceSession.provider?.disconnect();
  workspaceSession.ydoc.destroy();

  workspaceSession = null;
  connectionChangeCallback = null;
  fileChangeCallback = null;

  console.log("[WorkspaceCRDT] Destroyed workspace session");
}

/**
 * Check if workspace is initialized.
 */
export function isWorkspaceInitialized(): boolean {
  return workspaceSession !== null;
}

/**
 * Check if workspace is connected to server.
 */
export function isWorkspaceConnected(): boolean {
  return workspaceSession?.provider?.synced ?? false;
}

/**
 * Get the workspace Y.Doc (for advanced use cases).
 */
export function getWorkspaceDoc(): Y.Doc | null {
  return workspaceSession?.ydoc ?? null;
}

/**
 * Get the files map directly (for advanced use cases).
 */
export function getFilesMap(): Y.Map<FileMetadata> | null {
  return workspaceSession?.filesMap ?? null;
}

// ============================================================================
// File Operations
// ============================================================================

/**
 * Get metadata for a file.
 */
export function getFileMetadata(path: string): FileMetadata | null {
  if (!workspaceSession) return null;
  return workspaceSession.filesMap.get(path) ?? null;
}

/**
 * Get all files in the workspace.
 */
export function getAllFiles(): Map<string, FileMetadata> {
  if (!workspaceSession) return new Map();

  const result = new Map<string, FileMetadata>();
  workspaceSession.filesMap.forEach((value, key) => {
    if (!value.deleted) {
      result.set(key, value);
    }
  });
  return result;
}

/**
 * Get all files including deleted (tombstoned) ones.
 */
export function getAllFilesIncludingDeleted(): Map<string, FileMetadata> {
  if (!workspaceSession) return new Map();

  const result = new Map<string, FileMetadata>();
  workspaceSession.filesMap.forEach((value, key) => {
    result.set(key, value);
  });
  return result;
}

/**
 * Set metadata for a file.
 * This creates the file entry if it doesn't exist.
 */
export function setFileMetadata(path: string, metadata: FileMetadata): void {
  if (!workspaceSession) {
    console.warn(
      "[WorkspaceCRDT] Cannot set file metadata: workspace not initialized",
    );
    return;
  }

  try {
    // Use LOCAL_ORIGIN transaction so observers can distinguish local vs remote changes
    workspaceSession.ydoc.transact(() => {
      workspaceSession!.filesMap.set(path, {
        ...metadata,
        modifiedAt: Date.now(),
      });
    }, LOCAL_ORIGIN);
    console.log(`[WorkspaceCRDT] Set metadata for ${path}`);
  } catch (e) {
    console.error(`[WorkspaceCRDT] Failed to set metadata for ${path}:`, e);
  }
}

/**
 * Update specific fields of a file's metadata.
 * Creates the file entry with defaults if it doesn't exist.
 */
export function updateFileMetadata(
  path: string,
  updates: Partial<FileMetadata>,
): void {
  if (!workspaceSession) {
    console.warn(
      "[WorkspaceCRDT] Cannot update file metadata: workspace not initialized",
    );
    return;
  }

  try {
    const existing = workspaceSession.filesMap.get(path);
    const newMetadata: FileMetadata = {
      title: existing?.title ?? null,
      partOf: existing?.partOf ?? null,
      contents: existing?.contents ?? null,
      attachments: existing?.attachments ?? [],
      deleted: existing?.deleted ?? false,
      audience: existing?.audience ?? null,
      description: existing?.description ?? null,
      extra: existing?.extra ?? {},
      modifiedAt: Date.now(),
      ...updates,
    };

    // Use LOCAL_ORIGIN transaction so observers can distinguish local vs remote changes
    workspaceSession.ydoc.transact(() => {
      workspaceSession!.filesMap.set(path, newMetadata);
    }, LOCAL_ORIGIN);
    console.log(
      `[WorkspaceCRDT] Updated metadata for ${path}`,
      Object.keys(updates),
    );
  } catch (e) {
    console.error(`[WorkspaceCRDT] Failed to update metadata for ${path}:`, e);
  }
}

/**
 * Mark a file as deleted (soft delete).
 */
export function deleteFile(path: string): void {
  updateFileMetadata(path, { deleted: true });
  console.log(`[WorkspaceCRDT] Marked ${path} as deleted`);
}

/**
 * Restore a deleted file.
 */
export function restoreFile(path: string): void {
  updateFileMetadata(path, { deleted: false });
  console.log(`[WorkspaceCRDT] Restored ${path}`);
}

/**
 * Permanently remove a file from the CRDT.
 * Use sparingly - prefer soft delete for sync consistency.
 */
export function purgeFile(path: string): void {
  if (!workspaceSession) return;

  // Use LOCAL_ORIGIN transaction so observers can distinguish local vs remote changes
  workspaceSession.ydoc.transact(() => {
    workspaceSession!.filesMap.delete(path);
  }, LOCAL_ORIGIN);
  console.log(`[WorkspaceCRDT] Purged ${path} from CRDT`);
}

// ============================================================================
// Relationship Operations
// ============================================================================

/**
 * Add a child to a parent index's contents.
 */
export function addToContents(parentPath: string, childPath: string): void {
  if (!workspaceSession) return;

  const parent = workspaceSession.filesMap.get(parentPath);
  if (!parent) {
    console.warn(`[WorkspaceCRDT] Parent ${parentPath} not found`);
    return;
  }

  const contents = parent.contents ?? [];
  if (!contents.includes(childPath)) {
    updateFileMetadata(parentPath, {
      contents: [...contents, childPath].sort(),
    });
  }
}

/**
 * Remove a child from a parent index's contents.
 */
export function removeFromContents(
  parentPath: string,
  childPath: string,
): void {
  if (!workspaceSession) return;

  const parent = workspaceSession.filesMap.get(parentPath);
  if (!parent?.contents) return;

  const newContents = parent.contents.filter((c) => c !== childPath);
  updateFileMetadata(parentPath, {
    contents: newContents.length > 0 ? newContents : null,
  });
}

/**
 * Set the part_of relationship for a file.
 */
export function setPartOf(childPath: string, parentPath: string | null): void {
  updateFileMetadata(childPath, { partOf: parentPath });
}

/**
 * Move a file to a new parent.
 * Updates both the old parent's contents and new parent's contents,
 * as well as the file's part_of.
 */
export function moveFile(
  filePath: string,
  oldParentPath: string | null,
  newParentPath: string | null,
): void {
  if (!workspaceSession) return;

  // Remove from old parent
  if (oldParentPath) {
    removeFromContents(oldParentPath, filePath);
  }

  // Add to new parent
  if (newParentPath) {
    addToContents(newParentPath, filePath);
  }

  // Update file's part_of
  setPartOf(filePath, newParentPath);

  console.log(
    `[WorkspaceCRDT] Moved ${filePath} from ${oldParentPath} to ${newParentPath}`,
  );
}

/**
 * Rename a file (update its path in the CRDT).
 * This creates a new entry and marks the old one as deleted.
 */
export function renameFile(oldPath: string, newPath: string): void {
  if (!workspaceSession) return;

  const metadata = workspaceSession.filesMap.get(oldPath);
  if (!metadata) {
    console.warn(`[WorkspaceCRDT] Cannot rename: ${oldPath} not found`);
    return;
  }

  // Create new entry
  setFileMetadata(newPath, { ...metadata, deleted: false });

  // Mark old entry as deleted
  deleteFile(oldPath);

  // Update parent's contents if it exists
  if (metadata.partOf) {
    const parent = workspaceSession.filesMap.get(metadata.partOf);
    if (parent?.contents) {
      const newContents = parent.contents
        .map((c) => (c === oldPath ? newPath : c))
        .sort();
      updateFileMetadata(metadata.partOf, { contents: newContents });
    }
  }

  // Update children's part_of if this is an index
  if (metadata.contents) {
    for (const childPath of metadata.contents) {
      const child = workspaceSession.filesMap.get(childPath);
      if (child?.partOf === oldPath) {
        updateFileMetadata(childPath, { partOf: newPath });
      }
    }
  }

  console.log(`[WorkspaceCRDT] Renamed ${oldPath} to ${newPath}`);
}

// ============================================================================
// Attachment Operations
// ============================================================================

/**
 * Add an attachment reference to a file.
 */
export function addAttachment(filePath: string, attachment: BinaryRef): void {
  if (!workspaceSession) return;

  const metadata = workspaceSession.filesMap.get(filePath);
  const attachments = metadata?.attachments ?? [];

  // Check if attachment already exists (by path or hash)
  const existingIndex = attachments.findIndex(
    (a) => a.path === attachment.path || a.hash === attachment.hash,
  );

  if (existingIndex >= 0) {
    // Update existing
    const newAttachments = [...attachments];
    newAttachments[existingIndex] = { ...attachment, deleted: false };
    updateFileMetadata(filePath, { attachments: newAttachments });
  } else {
    // Add new
    updateFileMetadata(filePath, {
      attachments: [...attachments, attachment],
    });
  }

  console.log(
    `[WorkspaceCRDT] Added attachment ${attachment.path} to ${filePath}`,
  );
}

/**
 * Remove an attachment from a file (soft delete).
 */
export function removeAttachment(
  filePath: string,
  attachmentPath: string,
): void {
  if (!workspaceSession) return;

  const metadata = workspaceSession.filesMap.get(filePath);
  if (!metadata?.attachments) return;

  const newAttachments = metadata.attachments.map((a) =>
    a.path === attachmentPath ? { ...a, deleted: true } : a,
  );

  updateFileMetadata(filePath, { attachments: newAttachments });
  console.log(
    `[WorkspaceCRDT] Removed attachment ${attachmentPath} from ${filePath}`,
  );
}

/**
 * Get attachments for a file (excluding deleted).
 */
export function getAttachments(filePath: string): BinaryRef[] {
  const metadata = getFileMetadata(filePath);
  return metadata?.attachments?.filter((a) => !a.deleted) ?? [];
}

/**
 * Update attachment source URL (e.g., after upload completes).
 */
export function updateAttachmentSource(
  filePath: string,
  attachmentPath: string,
  source: string,
): void {
  if (!workspaceSession) return;

  const metadata = workspaceSession.filesMap.get(filePath);
  if (!metadata?.attachments) return;

  const newAttachments = metadata.attachments.map((a) =>
    a.path === attachmentPath ? { ...a, source, uploadedAt: Date.now() } : a,
  );

  updateFileMetadata(filePath, { attachments: newAttachments });
  console.log(
    `[WorkspaceCRDT] Updated source for ${attachmentPath} to ${source}`,
  );
}

// ============================================================================
// Sync with Backend
// ============================================================================

/**
 * Populate the workspace CRDT from backend data.
 * Call this after loading workspace from disk to sync CRDT state.
 */
export async function syncFromBackend(
  backend: {
    getWorkspaceTree: () => Promise<{
      name: string;
      path: string;
      description?: string;
      children: unknown[];
    }>;
    getEntry: (path: string) => Promise<{
      path: string;
      frontmatter: Record<string, unknown>;
    }>;
    getFrontmatter: (path: string) => Promise<Record<string, unknown>>;
  },
  _rootPath?: string,
): Promise<void> {
  if (!workspaceSession) {
    console.warn("[WorkspaceCRDT] Cannot sync: workspace not initialized");
    return;
  }

  console.log("[WorkspaceCRDT] Syncing from backend...");

  try {
    const tree = await backend.getWorkspaceTree();
    await syncTreeNode(backend, tree, null);
    console.log("[WorkspaceCRDT] Sync from backend complete");
  } catch (error) {
    console.error("[WorkspaceCRDT] Sync from backend failed:", error);
    throw error;
  }
}

/**
 * Recursively sync a tree node and its children.
 * MERGE-ONLY: Only adds files that don't exist in CRDT yet.
 * This prevents local state from overwriting newer remote data.
 */
async function syncTreeNode(
  backend: {
    getFrontmatter: (path: string) => Promise<Record<string, unknown>>;
  },
  node: {
    path: string;
    name: string;
    description?: string;
    children: unknown[];
  },
  parentPath: string | null,
): Promise<void> {
  if (!workspaceSession) return;

  try {
    // Check if file already exists in CRDT
    const existingMetadata = workspaceSession.filesMap.get(node.path);
    
    if (existingMetadata) {
      // File already exists in CRDT - merge local contents into CRDT contents
      // This ensures children added locally get into the CRDT
      const localContents = node.children.length > 0 
        ? node.children.map((c: any) => c.path) 
        : [];
      const crdtContents = existingMetadata.contents ?? [];
      
      // Merge local children into CRDT
      const mergedContents = [...new Set([...crdtContents, ...localContents])].sort();
      
      if (mergedContents.length > crdtContents.length) {
        console.log(`[WorkspaceCRDT] Merging local contents into CRDT for ${node.path}: ${crdtContents.length} -> ${mergedContents.length}`);
        workspaceSession.ydoc.transact(() => {
          workspaceSession!.filesMap.set(node.path, {
            ...existingMetadata,
            contents: mergedContents.length > 0 ? mergedContents : null,
            modifiedAt: Date.now(),
          });
        }, LOCAL_ORIGIN);
      } else {
        console.log(`[WorkspaceCRDT] Skipping existing file (no new contents): ${node.path}`);
      }
    } else {
      // File doesn't exist in CRDT - add it from local filesystem
      const frontmatter = await backend.getFrontmatter(node.path);

      const metadata: FileMetadata = {
        title: (frontmatter.title as string) ?? node.name ?? null,
        partOf: (frontmatter.part_of as string) ?? parentPath,
        contents: frontmatter.contents
          ? (frontmatter.contents as string[])
          : node.children.length > 0
            ? node.children.map((c: any) => c.path)
            : null,
        attachments: ((frontmatter.attachments as string[]) ?? []).map(
          (path) => ({
            path,
            source: "local",
            hash: "",
            mimeType: "",
            size: 0,
            deleted: false,
          }),
        ),
        deleted: false,
        audience: (frontmatter.audience as string[]) ?? null,
        description:
          (frontmatter.description as string) ?? node.description ?? null,
        extra: Object.fromEntries(
          Object.entries(frontmatter).filter(
            ([key]) =>
              ![
                "title",
                "part_of",
                "contents",
                "attachments",
                "audience",
                "description",
              ].includes(key),
          ),
        ),
        modifiedAt: Date.now(),
      };

      // Add to CRDT using LOCAL_ORIGIN transaction
      workspaceSession.ydoc.transact(() => {
        workspaceSession!.filesMap.set(node.path, metadata);
      }, LOCAL_ORIGIN);
      
      console.log(`[WorkspaceCRDT] Added local file to CRDT: ${node.path}`);
    }

    // Recursively sync children
    for (const child of node.children as any[]) {
      await syncTreeNode(backend, child, node.path);
    }
  } catch (error) {
    console.warn(`[WorkspaceCRDT] Failed to sync ${node.path}:`, error);
  }
}

/**
 * Convert CRDT metadata back to frontmatter format for saving.
 */
export function metadataToFrontmatter(
  metadata: FileMetadata,
): Record<string, unknown> {
  const frontmatter: Record<string, unknown> = {};

  if (metadata.title) {
    frontmatter.title = metadata.title;
  }

  if (metadata.description) {
    frontmatter.description = metadata.description;
  }

  if (metadata.partOf) {
    frontmatter.part_of = metadata.partOf;
  }

  if (metadata.contents && metadata.contents.length > 0) {
    frontmatter.contents = metadata.contents;
  }

  if (metadata.audience && metadata.audience.length > 0) {
    frontmatter.audience = metadata.audience;
  }

  const activeAttachments = metadata.attachments.filter((a) => !a.deleted);
  if (activeAttachments.length > 0) {
    frontmatter.attachments = activeAttachments.map((a) => a.path);
  }

  // Merge in extra properties
  Object.assign(frontmatter, metadata.extra);

  return frontmatter;
}

/**
 * Build a tree structure from the CRDT files map.
 * Useful for rendering the sidebar.
 */
export function buildTreeFromCrdt(rootPath?: string): {
  name: string;
  path: string;
  description?: string;
  children: any[];
} | null {
  if (!workspaceSession) return null;

  const files = getAllFiles();

  // Find root (file with contents but no part_of)
  let root: [string, FileMetadata] | undefined;

  if (rootPath) {
    const metadata = files.get(rootPath);
    if (metadata) {
      root = [rootPath, metadata];
    }
  } else {
    for (const [path, metadata] of files) {
      if (metadata.contents !== null && metadata.partOf === null) {
        root = [path, metadata];
        break;
      }
    }
  }

  if (!root) return null;

  return buildTreeNode(root[0], root[1], files);
}

function buildTreeNode(
  path: string,
  metadata: FileMetadata,
  files: Map<string, FileMetadata>,
): {
  name: string;
  path: string;
  description?: string;
  children: any[];
} {
  const children: any[] = [];

  if (metadata.contents) {
    for (const childPath of metadata.contents) {
      const childMetadata = files.get(childPath);
      if (childMetadata && !childMetadata.deleted) {
        children.push(buildTreeNode(childPath, childMetadata, files));
      }
    }
  }

  return {
    name: metadata.title ?? path.split("/").pop() ?? path,
    path,
    description: metadata.description ?? undefined,
    children,
  };
}

// ============================================================================
// Change Handlers
// ============================================================================

/**
 * Debounced notification for files change.
 * Prevents rapid updates from overwhelming the UI or causing re-render loops.
 */
function notifyFilesChangeDebounced(): void {
  if (!workspaceSession) return;

  // Clear existing timer
  if (filesChangeDebounceTimer) {
    clearTimeout(filesChangeDebounceTimer);
  }

  // Schedule debounced callback
  filesChangeDebounceTimer = setTimeout(() => {
    filesChangeDebounceTimer = null;
    if (workspaceSession?.onFilesChange) {
      try {
        workspaceSession.onFilesChange(getAllFiles());
      } catch (e) {
        console.error("[WorkspaceCRDT] Error in onFilesChange callback:", e);
      }
    }
  }, FILES_CHANGE_DEBOUNCE_MS);
}



/**
 * Create a local file from CRDT metadata.
 */
async function createLocalFile(
  backend: Backend,
  path: string,
  metadata: FileMetadata,
): Promise<void> {
  try {
    // Create the entry with frontmatter from metadata
    await backend.createEntry(path, {
      title: metadata.title ?? undefined,
      partOf: metadata.partOf ?? undefined,
    });
    console.log(`[WorkspaceCRDT] Created local file from remote: ${path}`);
  } catch (e) {
    console.warn(`[WorkspaceCRDT] Failed to create local file ${path}:`, e);
    throw e;
  }
}

/**
 * Sync CRDT state to local filesystem.
 * Call this when connecting to a new server to pull down all files.
 * Uses batching to prevent overwhelming the browser.
 */
export async function syncToLocal(): Promise<{
  created: string[];
  deleted: string[];
}> {
  if (!workspaceSession?.backend) {
    console.warn("[WorkspaceCRDT] Cannot sync to local: no backend");
    return { created: [], deleted: [] };
  }

  // Wait for provider to sync if available
  if (workspaceSession.provider) {
    await waitForSync(10000);
  }

  const backend = workspaceSession.backend;
  const filesMap = workspaceSession.filesMap;
  const created: string[] = [];
  const deleted: string[] = [];
  const BATCH_SIZE = 5;
  const BATCH_DELAY_MS = 100;

  console.log(`[WorkspaceCRDT] Syncing CRDT (${filesMap.size} files) to local filesystem...`);

  try {
    // Collect files to process
    const filesToProcess: Array<[string, FileMetadata]> = [];
    for (const [path, metadata] of filesMap.entries()) {
      if (pathsBeingProcessed.has(path)) continue;
      filesToProcess.push([path, metadata]);
    }

    // Process in batches
    for (let i = 0; i < filesToProcess.length; i += BATCH_SIZE) {
      const batch = filesToProcess.slice(i, i + BATCH_SIZE);
      
      await Promise.all(
        batch.map(async ([path, metadata]) => {
          if (pathsBeingProcessed.has(path)) return;
          pathsBeingProcessed.add(path);

          try {
            if (metadata.deleted) {
              // Delete local file if it exists
              try {
                await backend.deleteEntry(path);
                deleted.push(path);
              } catch {
                // File doesn't exist, nothing to delete
              }
            } else {
              // Create file if it doesn't exist
              try {
                await backend.getEntry(path);
                // File exists locally
              } catch {
                // File doesn't exist, create it
                await createLocalFile(backend, path, metadata);
                created.push(path);
              }
            }
          } finally {
            pathsBeingProcessed.delete(path);
          }
        })
      );

      // Delay between batches
      if (i + BATCH_SIZE < filesToProcess.length) {
        await new Promise((resolve) => setTimeout(resolve, BATCH_DELAY_MS));
      }
    }

    console.log(
      `[WorkspaceCRDT] Synced to local: created ${created.length}, deleted ${deleted.length} files`,
    );

    if (created.length > 0 || deleted.length > 0) {
      remoteFileSyncCallback?.(created, deleted);
      workspaceSession.onRemoteFileSync?.(created, deleted);
    }

    return { created, deleted };
  } catch (e) {
    console.error("[WorkspaceCRDT] Error syncing to local:", e);
    return { created, deleted };
  }
}


/**
 * Check if an event origin indicates a remote change.
 * Local changes use LOCAL_ORIGIN, remote changes come from other sources.
 */
function isRemoteChange(transaction: Y.Transaction): boolean {
  return transaction.origin !== LOCAL_ORIGIN && transaction.origin !== null;
}

function handleFilesMapChange(event: Y.YMapEvent<FileMetadata>): void {
  if (!workspaceSession) return;

  try {
    const isRemote = isRemoteChange(event.transaction);

    // Process file changes
    event.keysChanged.forEach((key) => {
      try {
        const metadata = workspaceSession!.filesMap.get(key) ?? null;

        // Notify callback about file change
        fileChangeCallback?.(key, metadata);

        // Handle remote file sync (create/delete actual files)
        // Skip during initialization to prevent race conditions with syncFromBackend
        if (isRemote && !isInitializing && workspaceSession?.backend && !pathsBeingProcessed.has(key)) {
          handleRemoteFileChange(key, metadata);
        }
      } catch (e) {
        console.error(
          `[WorkspaceCRDT] Error processing file change for ${key}:`,
          e,
        );
      }
    });

    // Notify about overall files change (debounced to prevent rapid updates)
    notifyFilesChangeDebounced();
  } catch (e) {
    console.error("[WorkspaceCRDT] Error handling files map change:", e);
  }
}

/**
 * Handle a remote file change - create or delete the file locally.
 */
async function handleRemoteFileChange(
  path: string,
  metadata: FileMetadata | null,
): Promise<void> {
  if (!workspaceSession?.backend) return;

  const backend = workspaceSession.backend;
  pathsBeingProcessed.add(path);

  try {
    if (metadata === null || metadata.deleted) {
      // File was deleted remotely
      try {
        // First, get the partOf before deletion to know the parent
        // Use the previous metadata if available (before it was marked deleted)
        const previousMetadata = workspaceSession!.filesMap.get(path);
        const parentPath = previousMetadata?.partOf ?? metadata?.partOf;
        
        await backend.deleteEntry(path);
        console.log(`[WorkspaceCRDT] Deleted local file from remote: ${path}`);
        
        // Remove from parent's contents if we know the parent
        if (parentPath) {
          removeFromContents(parentPath, path);
          console.log(`[WorkspaceCRDT] Removed ${path} from parent ${parentPath} contents`);
        }
        
        remoteFileSyncCallback?.([], [path]);
        workspaceSession.onRemoteFileSync?.([], [path]);
      } catch {
        // File might not exist locally
      }
    } else {
      // File was created or updated remotely
      try {
        // Check if file exists locally
        const existingEntry = await backend.getEntry(path);
        
        // File exists - sync metadata from CRDT to local frontmatter
        // Only update if there are meaningful differences
        const updates: Record<string, unknown> = {};
        
        if (metadata.title && metadata.title !== existingEntry.frontmatter?.title) {
          updates.title = metadata.title;
        }
        if (metadata.partOf !== undefined && metadata.partOf !== existingEntry.frontmatter?.part_of) {
          updates.part_of = metadata.partOf;
        }
        
        // For contents array, MERGE instead of overwrite to preserve files added by either client
        const localContents = (existingEntry.frontmatter?.contents as string[]) ?? [];
        const remoteContents = metadata.contents ?? [];
        
        // Union of both arrays (preserves additions from both sides)
        const mergedContents = [...new Set([...localContents, ...remoteContents])].sort();
        
        if (JSON.stringify(mergedContents) !== JSON.stringify(localContents)) {
          updates.contents = mergedContents;
          
          // Also update the CRDT with merged contents so other clients get the full list
          if (JSON.stringify(mergedContents) !== JSON.stringify(remoteContents)) {
            console.log(`[WorkspaceCRDT] Merging contents for ${path}: local=${localContents.length}, remote=${remoteContents.length}, merged=${mergedContents.length}`);
            // Update CRDT with merged contents using LOCAL_ORIGIN
            workspaceSession!.ydoc.transact(() => {
              const current = workspaceSession!.filesMap.get(path);
              if (current) {
                workspaceSession!.filesMap.set(path, {
                  ...current,
                  contents: mergedContents,
                  modifiedAt: Date.now(),
                });
              }
            }, LOCAL_ORIGIN);
          }
        }
        
        if (metadata.audience && JSON.stringify(metadata.audience) !== JSON.stringify(existingEntry.frontmatter?.audience)) {
          updates.audience = metadata.audience;
        }
        if (metadata.description && metadata.description !== existingEntry.frontmatter?.description) {
          updates.description = metadata.description;
        }
        
        // Apply updates if there are any
        if (Object.keys(updates).length > 0) {
          for (const [key, value] of Object.entries(updates)) {
            await backend.setFrontmatterProperty(path, key, value);
          }
          console.log(`[WorkspaceCRDT] Synced remote metadata to local: ${path}`, Object.keys(updates));
        }
      } catch {
        // File doesn't exist locally, create it
        await createLocalFile(backend, path, metadata);
        remoteFileSyncCallback?.([path], []);
        workspaceSession.onRemoteFileSync?.([path], []);
      }
    }
  } finally {
    pathsBeingProcessed.delete(path);
  }
}

function handleFilesDeepChange(_events: Y.YEvent<any>[]): void {
  if (!workspaceSession) return;

  // For deep changes, use debounced notification
  // Individual file changes are handled by the map observer
  notifyFilesChangeDebounced();
}


// ============================================================================
// Utilities
// ============================================================================

/**
 * Wait for IndexedDB persistence to sync (load local data).
 * This is fast if data is already cached locally (typically instant).
 * Returns immediately if already synced.
 */
export function waitForIndexedDBSync(): Promise<void> {
  return new Promise((resolve) => {
    if (indexedDBSynced) {
      resolve();
      return;
    }
    
    // Add to pending resolvers - will be called when persistence syncs
    indexedDBSyncResolvers.push(resolve);
  });
}

/**
 * Wait for the workspace to sync with the server.
 * Returns immediately if no server is configured.
 */
export function waitForSync(timeoutMs = 5000): Promise<boolean> {
  return new Promise((resolve) => {
    if (!workspaceSession?.provider) {
      resolve(true);
      return;
    }

    if (workspaceSession.provider.synced) {
      resolve(true);
      return;
    }

    const timeout = setTimeout(() => {
      resolve(false);
    }, timeoutMs);

    const handler = () => {
      clearTimeout(timeout);
      resolve(true);
    };

    workspaceSession.provider.on("sync", handler);
  });
}

/**
 * Get statistics about the workspace CRDT.
 */
export function getWorkspaceStats(): {
  totalFiles: number;
  activeFiles: number;
  deletedFiles: number;
  indexFiles: number;
  leafFiles: number;
  totalAttachments: number;
} {
  if (!workspaceSession) {
    return {
      totalFiles: 0,
      activeFiles: 0,
      deletedFiles: 0,
      indexFiles: 0,
      leafFiles: 0,
      totalAttachments: 0,
    };
  }

  let totalFiles = 0;
  let activeFiles = 0;
  let deletedFiles = 0;
  let indexFiles = 0;
  let leafFiles = 0;
  let totalAttachments = 0;

  workspaceSession.filesMap.forEach((metadata) => {
    totalFiles++;
    if (metadata.deleted) {
      deletedFiles++;
    } else {
      activeFiles++;
      if (metadata.contents !== null) {
        indexFiles++;
      } else {
        leafFiles++;
      }
      totalAttachments += metadata.attachments.filter((a) => !a.deleted).length;
    }
  });

  return {
    totalFiles,
    activeFiles,
    deletedFiles,
    indexFiles,
    leafFiles,
    totalAttachments,
  };
}

/**
 * Garbage collect deleted files older than a threshold.
 * @param olderThanMs Only purge files deleted more than this many ms ago
 */
export function garbageCollect(olderThanMs = 7 * 24 * 60 * 60 * 1000): number {
  if (!workspaceSession) return 0;

  const now = Date.now();
  const toPurge: string[] = [];

  workspaceSession.filesMap.forEach((metadata, path) => {
    if (metadata.deleted && now - metadata.modifiedAt > olderThanMs) {
      toPurge.push(path);
    }
  });

  for (const path of toPurge) {
    workspaceSession.filesMap.delete(path);
  }

  if (toPurge.length > 0) {
    console.log(`[WorkspaceCRDT] Garbage collected ${toPurge.length} files`);
  }

  return toPurge.length;
}
