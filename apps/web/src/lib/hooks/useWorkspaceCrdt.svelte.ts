/**
 * Svelte 5 reactive hook for workspace CRDT integration.
 *
 * Provides reactive state bindings for the workspace CRDT,
 * making it easy to use in Svelte components.
 *
 * Usage:
 * ```svelte
 * <script lang="ts">
 *   import { useWorkspaceCrdt } from '$lib/hooks/useWorkspaceCrdt.svelte';
 *
 *   const workspace = useWorkspaceCrdt();
 *
 *   // Initialize when backend is ready
 *   $effect(() => {
 *     if (backend) {
 *       workspace.init({ serverUrl: 'ws://localhost:1234' });
 *     }
 *   });
 *
 *   // Reactive access to files
 *   const files = $derived(workspace.files);
 *   const tree = $derived(workspace.tree);
 * </script>
 * ```
 */

import { onDestroy } from "svelte";
import {
  initWorkspace,
  destroyWorkspace,
  disconnectWorkspace,
  reconnectWorkspace,
  isWorkspaceInitialized,
  isWorkspaceConnected,
  getAllFiles,
  getFileMetadata,
  setFileMetadata,
  updateFileMetadata,
  deleteFile,
  restoreFile,
  addToContents,
  removeFromContents,
  setPartOf,
  moveFile,
  renameFile,
  addAttachment,
  removeAttachment,
  getAttachments,
  updateAttachmentSource,
  syncFromBackend,
  metadataToFrontmatter,
  buildTreeFromCrdt,
  waitForSync,
  getWorkspaceStats,
  garbageCollect,
  type FileMetadata,
  type BinaryRef,
  type WorkspaceInitOptions,
} from "../workspaceCrdt";
import type { Backend, TreeNode } from "../backend";

/**
 * Reactive workspace CRDT state.
 */
export interface WorkspaceCrdtState {
  /** Whether the workspace CRDT is initialized */
  initialized: boolean;
  /** Whether connected to the collaboration server */
  connected: boolean;
  /** All files in the workspace (reactive Map) */
  files: Map<string, FileMetadata>;
  /** Tree structure built from CRDT (reactive) */
  tree: TreeNode | null;
  /** Last error that occurred */
  error: string | null;
  /** Whether currently syncing with backend */
  syncing: boolean;
}

/**
 * Workspace CRDT hook actions.
 */
export interface WorkspaceCrdtActions {
  /** Initialize the workspace CRDT */
  init: (options?: WorkspaceInitOptions) => Promise<void>;
  /** Destroy the workspace session */
  destroy: () => Promise<void>;
  /** Disconnect from server (keeps local state) */
  disconnect: () => void;
  /** Reconnect to server */
  reconnect: () => void;
  /** Sync workspace CRDT from backend data */
  syncFromBackend: (backend: Backend) => Promise<void>;
  /** Get metadata for a specific file */
  getFile: (path: string) => FileMetadata | null;
  /** Set metadata for a file */
  setFile: (path: string, metadata: FileMetadata) => void;
  /** Update specific fields of a file */
  updateFile: (path: string, updates: Partial<FileMetadata>) => void;
  /** Mark a file as deleted */
  deleteFile: (path: string) => void;
  /** Restore a deleted file */
  restoreFile: (path: string) => void;
  /** Add a child to parent's contents */
  addToContents: (parentPath: string, childPath: string) => void;
  /** Remove a child from parent's contents */
  removeFromContents: (parentPath: string, childPath: string) => void;
  /** Set part_of relationship */
  setPartOf: (childPath: string, parentPath: string | null) => void;
  /** Move a file to a new parent */
  moveFile: (filePath: string, oldParent: string | null, newParent: string | null) => void;
  /** Rename a file */
  renameFile: (oldPath: string, newPath: string) => void;
  /** Add an attachment to a file */
  addAttachment: (filePath: string, attachment: BinaryRef) => void;
  /** Remove an attachment from a file */
  removeAttachment: (filePath: string, attachmentPath: string) => void;
  /** Get attachments for a file */
  getAttachments: (filePath: string) => BinaryRef[];
  /** Update attachment source URL */
  updateAttachmentSource: (filePath: string, attachmentPath: string, source: string) => void;
  /** Convert metadata to frontmatter format */
  toFrontmatter: (metadata: FileMetadata) => Record<string, unknown>;
  /** Wait for sync to complete */
  waitForSync: (timeoutMs?: number) => Promise<boolean>;
  /** Get workspace statistics */
  getStats: () => ReturnType<typeof getWorkspaceStats>;
  /** Run garbage collection */
  garbageCollect: (olderThanMs?: number) => number;
  /** Manually refresh the reactive state */
  refresh: () => void;
}

/**
 * Create a reactive workspace CRDT hook.
 *
 * @returns Reactive state and actions for the workspace CRDT
 */
export function useWorkspaceCrdt(): WorkspaceCrdtState & WorkspaceCrdtActions {
  // Reactive state using Svelte 5 runes
  let initialized = $state(false);
  let connected = $state(false);
  let files = $state<Map<string, FileMetadata>>(new Map());
  let tree = $state<TreeNode | null>(null);
  let error = $state<string | null>(null);
  let syncing = $state(false);

  // Update reactive state from CRDT
  function refresh(): void {
    if (!isWorkspaceInitialized()) {
      initialized = false;
      connected = false;
      files = new Map();
      tree = null;
      return;
    }

    initialized = true;
    connected = isWorkspaceConnected();
    files = getAllFiles();
    tree = buildTreeFromCrdt() as TreeNode | null;
  }

  // Initialize workspace
  async function init(options: WorkspaceInitOptions = {}): Promise<void> {
    try {
      error = null;
      await initWorkspace({
        ...options,
        onFilesChange: (newFiles) => {
          files = newFiles;
          tree = buildTreeFromCrdt() as TreeNode | null;
        },
        onConnectionChange: (isConnected) => {
          connected = isConnected;
        },
      });
      initialized = true;
      refresh();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      throw e;
    }
  }

  // Destroy workspace
  async function destroy(): Promise<void> {
    await destroyWorkspace();
    initialized = false;
    connected = false;
    files = new Map();
    tree = null;
  }

  // Disconnect (keep local state)
  function disconnect(): void {
    disconnectWorkspace();
    connected = false;
  }

  // Reconnect
  function reconnect(): void {
    reconnectWorkspace();
    // Connection status will update via callback
  }

  // Sync from backend
  async function doSyncFromBackend(backend: Backend): Promise<void> {
    if (!initialized) {
      throw new Error("Workspace CRDT not initialized");
    }

    try {
      syncing = true;
      error = null;
      await syncFromBackend(backend);
      refresh();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      throw e;
    } finally {
      syncing = false;
    }
  }

  // File operations (wrapped to trigger refresh)
  function setFile(path: string, metadata: FileMetadata): void {
    setFileMetadata(path, metadata);
    // Note: refresh happens via onFilesChange callback
  }

  function updateFile(path: string, updates: Partial<FileMetadata>): void {
    updateFileMetadata(path, updates);
  }

  function doDeleteFile(path: string): void {
    deleteFile(path);
  }

  function doRestoreFile(path: string): void {
    restoreFile(path);
  }

  function doAddToContents(parentPath: string, childPath: string): void {
    addToContents(parentPath, childPath);
  }

  function doRemoveFromContents(parentPath: string, childPath: string): void {
    removeFromContents(parentPath, childPath);
  }

  function doSetPartOf(childPath: string, parentPath: string | null): void {
    setPartOf(childPath, parentPath);
  }

  function doMoveFile(
    filePath: string,
    oldParent: string | null,
    newParent: string | null
  ): void {
    moveFile(filePath, oldParent, newParent);
  }

  function doRenameFile(oldPath: string, newPath: string): void {
    renameFile(oldPath, newPath);
  }

  function doAddAttachment(filePath: string, attachment: BinaryRef): void {
    addAttachment(filePath, attachment);
  }

  function doRemoveAttachment(filePath: string, attachmentPath: string): void {
    removeAttachment(filePath, attachmentPath);
  }

  function doGetAttachments(filePath: string): BinaryRef[] {
    return getAttachments(filePath);
  }

  function doUpdateAttachmentSource(
    filePath: string,
    attachmentPath: string,
    source: string
  ): void {
    updateAttachmentSource(filePath, attachmentPath, source);
  }

  // Cleanup on component destroy
  onDestroy(() => {
    // Don't destroy the workspace - just disconnect
    // This allows the workspace to persist across navigation
    disconnect();
  });

  // Return reactive state and actions
  return {
    // Reactive state (getters)
    get initialized() {
      return initialized;
    },
    get connected() {
      return connected;
    },
    get files() {
      return files;
    },
    get tree() {
      return tree;
    },
    get error() {
      return error;
    },
    get syncing() {
      return syncing;
    },

    // Actions
    init,
    destroy,
    disconnect,
    reconnect,
    syncFromBackend: doSyncFromBackend,
    getFile: getFileMetadata,
    setFile,
    updateFile,
    deleteFile: doDeleteFile,
    restoreFile: doRestoreFile,
    addToContents: doAddToContents,
    removeFromContents: doRemoveFromContents,
    setPartOf: doSetPartOf,
    moveFile: doMoveFile,
    renameFile: doRenameFile,
    addAttachment: doAddAttachment,
    removeAttachment: doRemoveAttachment,
    getAttachments: doGetAttachments,
    updateAttachmentSource: doUpdateAttachmentSource,
    toFrontmatter: metadataToFrontmatter,
    waitForSync,
    getStats: getWorkspaceStats,
    garbageCollect,
    refresh,
  };
}

/**
 * Type for the return value of useWorkspaceCrdt.
 */
export type WorkspaceCrdtHook = ReturnType<typeof useWorkspaceCrdt>;
