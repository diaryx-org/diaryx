import type { Backend } from '$lib/backend';
import { setWorkspaceId } from '$lib/collaborationUtils';
import {
  initWorkspace,
  syncFromBackend,
  syncToLocal,
  garbageCollect,
  getWorkspaceStats,
  updateFileMetadata,
  addToContents,
  setInitializing,
  waitForIndexedDBSync,
  type FileMetadata,
  type BinaryRef,
} from '$lib/workspaceCrdt';

// ============================================================================
// Types
// ============================================================================

export interface WorkspaceCrdtCallbacks {
  onFilesChange?: (files: Map<string, FileMetadata>) => void;
  onConnectionChange?: (connected: boolean) => void;
  onRemoteFileSync?: (created: string[], deleted: string[]) => Promise<void>;
}

export interface WorkspaceCrdtStats {
  activeFiles: number;
  totalAttachments: number;
}

// ============================================================================
// State for tracking initialization
// ============================================================================

let isInitialized = false;

// ============================================================================
// Public API
// ============================================================================

/**
 * Initialize the workspace CRDT system.
 * 
 * @param workspaceId - Unique workspace identifier (null for simple room names)
 * @param serverUrl - Collaboration server URL (null for offline mode)
 * @param backend - Backend instance for syncing
 * @param callbacks - Event callbacks
 * @returns Whether initialization succeeded
 */
export async function initializeWorkspaceCrdt(
  workspaceId: string | null,
  serverUrl: string | null,
  collaborationEnabled: boolean,
  backend: Backend,
  callbacks: WorkspaceCrdtCallbacks,
): Promise<boolean> {
  try {
    // Set workspace ID for per-file document room naming
    setWorkspaceId(workspaceId);

    // Track if this is the first server sync (for syncToLocal)
    let hasRunSyncToLocal = false;

    // Initialize workspace CRDT with background server sync callback
    await initWorkspace({
      workspaceId: workspaceId ?? undefined,
      serverUrl: collaborationEnabled ? serverUrl : null,
      backend: backend,
      onFilesChange: callbacks.onFilesChange,
      onConnectionChange: callbacks.onConnectionChange,
      onRemoteFileSync: callbacks.onRemoteFileSync,
      // This callback runs when server sync completes (in background)
      onServerSynced: async () => {
        // Only run syncToLocal once per session
        if (hasRunSyncToLocal) return;
        hasRunSyncToLocal = true;
        
        console.log('[WorkspaceCrdtService] Background server sync complete, running syncToLocal...');
        try {
          const { created, deleted } = await syncToLocal();
          if (created.length > 0 || deleted.length > 0) {
            console.log(`[WorkspaceCrdtService] Background syncToLocal: created ${created.length}, deleted ${deleted.length}`);
          }
        } catch (e) {
          console.error('[WorkspaceCrdtService] Background syncToLocal failed:', e);
        }
      },
    });

    // STEP 1: Wait for LOCAL IndexedDB sync (instant if data cached)
    await waitForIndexedDBSync();
    console.log('[WorkspaceCrdtService] Local IndexedDB synced');

    // STEP 2: Sync Local → CRDT (user can start working immediately!)
    // Use setInitializing to prevent race conditions with incoming remote changes
    setInitializing(true);
    try {
      console.log('[WorkspaceCrdtService] Syncing local to CRDT...');
      await syncFromBackend(backend);
    } finally {
      setInitializing(false);
    }

    // STEP 3: Server sync happens in background via HocuspocusProvider
    // When it completes, onServerSynced callback runs syncToLocal()
    // (no blocking waitForSync!)

    // Garbage collect old deleted files (older than 7 days)
    const purged = garbageCollect(7 * 24 * 60 * 60 * 1000);
    if (purged > 0) {
      console.log(`[WorkspaceCrdtService] Garbage collected ${purged} old deleted files`);
    }

    const stats = getWorkspaceStats();
    console.log(
      `[WorkspaceCrdtService] Initialized: ${stats.activeFiles} files, ${stats.totalAttachments} attachments`,
    );

    isInitialized = true;
    return true;
  } catch (e) {
    console.error('[WorkspaceCrdtService] Failed to initialize:', e);
    isInitialized = false;
    return false;
  }
}

/**
 * Check if CRDT is initialized.
 */
export function isCrdtInitialized(): boolean {
  return isInitialized;
}

/**
 * Reset initialization state.
 */
export function resetCrdtState(): void {
  isInitialized = false;
}

/**
 * Update file metadata in the CRDT.
 */
export function updateCrdtFileMetadata(
  path: string,
  frontmatter: Record<string, unknown>,
): void {
  if (!isInitialized) return;

  try {
    updateFileMetadata(path, {
      title: (frontmatter.title as string) ?? null,
      partOf: (frontmatter.part_of as string) ?? null,
      contents: frontmatter.contents ? (frontmatter.contents as string[]) : null,
      audience: (frontmatter.audience as string[]) ?? null,
      description: (frontmatter.description as string) ?? null,
      extra: Object.fromEntries(
        Object.entries(frontmatter).filter(
          ([key]) =>
            !['title', 'part_of', 'contents', 'attachments', 'audience', 'description'].includes(key),
        ),
      ),
    });
  } catch (e) {
    console.error('[WorkspaceCrdtService] Failed to update metadata:', e);
  }
}

/**
 * Add a new file to the CRDT.
 */
export function addFileToCrdt(
  path: string,
  frontmatter: Record<string, unknown>,
  parentPath: string | null,
): void {
  if (!isInitialized) return;

  try {
    const metadata: FileMetadata = {
      title: (frontmatter.title as string) ?? null,
      partOf: parentPath ?? (frontmatter.part_of as string) ?? null,
      contents: frontmatter.contents ? (frontmatter.contents as string[]) : null,
      attachments: ((frontmatter.attachments as string[]) ?? []).map((p) => ({
        path: p,
        source: 'local' as const,
        hash: '',
        mimeType: '',
        size: 0,
        deleted: false,
      })),
      deleted: false,
      audience: (frontmatter.audience as string[]) ?? null,
      description: (frontmatter.description as string) ?? null,
      extra: Object.fromEntries(
        Object.entries(frontmatter).filter(
          ([key]) =>
            !['title', 'part_of', 'contents', 'attachments', 'audience', 'description'].includes(key),
        ),
      ),
      modifiedAt: Date.now(),
    };

    updateFileMetadata(path, metadata);

    // Add to parent's contents if parent exists
    if (parentPath) {
      addToContents(parentPath, path);
    }
  } catch (e) {
    console.error('[WorkspaceCrdtService] Failed to add file:', e);
  }
}

/**
 * Create an attachment reference for CRDT tracking.
 */
export function createAttachmentRef(
  attachmentPath: string,
  file: File,
): BinaryRef {
  return {
    path: attachmentPath,
    source: 'local' as const,
    hash: '',
    mimeType: file.type,
    size: file.size,
    deleted: false,
  };
}

/**
 * Get workspace statistics.
 */
export function getCrdtStats(): WorkspaceCrdtStats {
  return getWorkspaceStats();
}
