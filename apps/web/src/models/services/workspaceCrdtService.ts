import type { RustCrdtApi } from '$lib/crdt/rustCrdtApi';
import {
  initWorkspace,
  setWorkspaceId,
  setInitializing,
  updateFileMetadata as updateFileInCrdt,
  getWorkspaceStats,
  setCollaborationWorkspaceId,
  type FileMetadata,
  type BinaryRef,
} from '$lib/crdt';
import type { JsonValue } from '$lib/backend/generated/serde_json/JsonValue';

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
 * @param _workspacePath - Path to workspace root (for display, currently unused)
 * @param serverUrl - Collaboration server URL (null for offline mode)
 * @param collaborationEnabled - Whether collaboration is enabled
 * @param rustApi - Rust CRDT API instance
 * @param callbacks - Event callbacks
 * @returns Whether initialization succeeded
 */
export async function initializeWorkspaceCrdt(
  workspaceId: string | null,
  _workspacePath: string | null,
  serverUrl: string | null,
  collaborationEnabled: boolean,
  rustApi: RustCrdtApi,
  _callbacks: WorkspaceCrdtCallbacks,
): Promise<boolean> {
  try {
    // Set workspace ID for per-file document room naming
    setWorkspaceId(workspaceId);
    setCollaborationWorkspaceId(workspaceId);

    // Initialize workspace CRDT (bridge is created internally if collaboration is enabled)
    setInitializing(true);
    try {
      await initWorkspace({
        rustApi,
        serverUrl: collaborationEnabled && serverUrl ? serverUrl : undefined,
        workspaceId: workspaceId ?? undefined,
        onReady: () => {
          console.log('[WorkspaceCrdtService] Workspace CRDT ready');
        },
      });
    } finally {
      setInitializing(false);
    }

    const stats = await getWorkspaceStats();
    console.log(
      `[WorkspaceCrdtService] Initialized: ${stats.activeFiles} active, ${stats.deletedFiles} deleted files`,
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
 *
 * NOTE: This function does NOT update `part_of` or `contents` in the CRDT.
 * Those fields are handled exclusively by Rust commands (SetFrontmatterProperty, etc.)
 * which parse markdown links and store canonical paths. If we passed frontmatter values
 * directly, we'd overwrite the canonical paths with unparsed markdown links.
 */
export async function updateCrdtFileMetadata(
  path: string,
  frontmatter: Record<string, unknown>,
): Promise<void> {
  if (!isInitialized) return;

  try {
    // Build extra fields with proper typing
    // Exclude part_of, contents, attachments - these are handled by Rust
    const extraFields: Record<string, JsonValue | undefined> = {};
    for (const [key, value] of Object.entries(frontmatter)) {
      if (!['title', 'part_of', 'contents', 'attachments', 'audience', 'description'].includes(key)) {
        extraFields[key] = value as JsonValue;
      }
    }

    // Update CRDT with metadata fields that don't require link parsing.
    // part_of and contents are intentionally omitted - Rust handles those with proper
    // markdown link parsing to extract canonical paths for CRDT storage.
    await updateFileInCrdt(path, {
      title: (frontmatter.title as string) ?? null,
      // part_of: intentionally omitted - Rust handles via SetFrontmatterProperty
      // contents: intentionally omitted - Rust handles via SetFrontmatterProperty
      audience: (frontmatter.audience as string[]) ?? null,
      description: (frontmatter.description as string) ?? null,
      extra: extraFields,
    });
  } catch (e) {
    console.error('[WorkspaceCrdtService] Failed to update metadata:', e);
  }
}

/**
 * Add a new file to the CRDT.
 *
 * NOTE: This function is primarily for test compatibility. In production,
 * file creation should go through Rust commands (CreateEntry, CreateChildEntry)
 * which handle part_of/contents with proper markdown link formatting.
 *
 * This function does NOT set part_of or contents in the CRDT to avoid
 * conflicts with Rust's link parsing. Use Rust commands for hierarchy operations.
 */
export async function addFileToCrdt(
  path: string,
  frontmatter: Record<string, unknown>,
  _parentPath: string | null, // Ignored - Rust handles hierarchy via commands
): Promise<void> {
  if (!isInitialized) return;

  try {
    // Build extra fields with proper typing
    // Exclude part_of, contents - these should be set by Rust commands
    const extraFields: Record<string, JsonValue | undefined> = {};
    for (const [key, value] of Object.entries(frontmatter)) {
      if (!['title', 'part_of', 'contents', 'attachments', 'audience', 'description'].includes(key)) {
        extraFields[key] = value as JsonValue;
      }
    }

    // Only update non-hierarchy metadata.
    // part_of and contents are handled by Rust commands with proper link formatting.
    await updateFileInCrdt(path, {
      title: (frontmatter.title as string) ?? null,
      // part_of: intentionally omitted - Rust handles
      // contents: intentionally omitted - Rust handles
      audience: (frontmatter.audience as string[]) ?? null,
      description: (frontmatter.description as string) ?? null,
      extra: extraFields,
    });

    // NOTE: We intentionally do NOT call addToContents here.
    // Parent-child relationships should be established via Rust commands
    // (CreateEntry, CreateChildEntry, AttachEntryToParent) which handle
    // markdown link formatting for part_of/contents properties.
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
    source: 'local',
    hash: '',
    mime_type: file.type,
    size: BigInt(file.size),
    uploaded_at: BigInt(Date.now()),
    deleted: false,
  };
}

/**
 * Get workspace statistics.
 */
export async function getCrdtStats(): Promise<WorkspaceCrdtStats> {
  const stats = await getWorkspaceStats();
  return {
    activeFiles: stats.activeFiles,
    totalAttachments: 0, // TODO: count attachments from all files
  };
}
