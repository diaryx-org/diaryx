/**
 * Attachment Controller
 *
 * Handles attachment-related operations including:
 * - Adding attachments via file picker
 * - Processing file selection
 * - Drag-drop in editor
 * - Deleting attachments
 * - Inserting attachments into editor
 * - Moving attachments between entries
 * - Populating CRDT before hosting
 */

import type { Api, EntryData } from '../lib/backend';
import { entryStore } from '../models/stores';
import {
  trackBlobUrl,
  computeRelativeAttachmentPath,
  getMimeType,
  bytesToBase64,
} from '../models/services/attachmentService';
import {
  enqueueAttachmentUpload,
  indexAttachmentRefs,
  sha256Hex,
  isAttachmentSyncEnabled,
  onQueueItemStateChange,
} from '../models/services/attachmentSyncService';
import type { QueueItemEvent } from '../models/services/attachmentSyncService';
import { showLoading } from '../models/services/toastService';
import { getCurrentWorkspace } from '../lib/auth/authStore.svelte';
import { getFileMetadata, getWorkspaceId, setFileMetadata } from '../lib/crdt';
import { toast } from 'svelte-sonner';

// ============================================================================
// Constants
// ============================================================================

const MAX_FILE_SIZE = 10 * 1024 * 1024; // 10MB

// ============================================================================
// Upload progress toast
// ============================================================================

const activeUploadIds = new Set<string>();
let uploadToast: ReturnType<typeof showLoading> | null = null;
let uploadListenerInstalled = false;

function uploadToastMessage(): string {
  const count = activeUploadIds.size;
  if (count === 1) return 'Syncing attachment to cloud...';
  return `Syncing ${count} attachments to cloud...`;
}

function handleUploadEvent(event: QueueItemEvent): void {
  if (event.kind !== 'upload' || !activeUploadIds.has(event.id)) return;

  if (event.state === 'complete' || event.state === 'failed') {
    const failed = event.state === 'failed';
    activeUploadIds.delete(event.id);
    if (activeUploadIds.size === 0 && uploadToast) {
      if (failed) {
        const filename = event.attachmentPath.split('/').pop() || 'attachment';
        uploadToast.error(`Failed to sync ${filename}`);
      } else {
        uploadToast.success('Attachment synced to cloud');
      }
      uploadToast = null;
    } else if (uploadToast) {
      uploadToast.update(uploadToastMessage());
    }
  }
}

function trackUpload(queueItemId: string): void {
  if (!uploadListenerInstalled) {
    uploadListenerInstalled = true;
    onQueueItemStateChange(handleUploadEvent);
  }
  activeUploadIds.add(queueItemId);
  if (!uploadToast) {
    uploadToast = showLoading(uploadToastMessage());
  } else {
    uploadToast.update(uploadToastMessage());
  }
}

// ============================================================================
// Helpers
// ============================================================================

/**
 * Helper to handle mixed frontmatter types (Map from WASM vs Object from JSON/Tauri).
 */
function normalizeFrontmatter(frontmatter: any): Record<string, any> {
  if (!frontmatter) return {};
  if (frontmatter instanceof Map) {
    return Object.fromEntries(frontmatter.entries());
  }
  return frontmatter;
}

/**
 * Convert a File to base64 string.
 */
export function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result as string;
      // Extract base64 part from data URL
      const base64 = result.split(',')[1];
      resolve(base64);
    };
    reader.onerror = reject;
    reader.readAsDataURL(file);
  });
}

async function updateAttachmentRefMetadata(
  entryPath: string,
  attachmentPath: string,
  hash: string,
  mimeType: string,
  sizeBytes: number,
): Promise<void> {
  const metadata = await getFileMetadata(entryPath);
  if (!metadata) {
    console.warn('[AttachmentController] Missing CRDT metadata while updating attachment hash:', entryPath);
    return;
  }

  let matchedAttachmentRef = false;
  const updatedAttachments = metadata.attachments.map((attachment) => {
    if (attachment.path !== attachmentPath) return attachment;
    matchedAttachmentRef = true;
    return {
      ...attachment,
      hash,
      mime_type: mimeType || attachment.mime_type,
      size: BigInt(sizeBytes),
      uploaded_at: BigInt(Date.now()),
      deleted: false,
    };
  });
  if (!matchedAttachmentRef) {
    // Ensure a BinaryRef exists for newly uploaded attachments before syncing hash metadata.
    updatedAttachments.push({
      path: attachmentPath,
      source: 'local',
      hash,
      mime_type: mimeType || getMimeType(attachmentPath),
      size: BigInt(sizeBytes),
      uploaded_at: BigInt(Date.now()),
      deleted: false,
    });
  }

  const updatedMetadata = {
    ...metadata,
    attachments: updatedAttachments,
    modified_at: BigInt(Date.now()),
  };
  await setFileMetadata(entryPath, updatedMetadata);

  const workspaceId = getWorkspaceId() ?? getCurrentWorkspace()?.id;
  if (workspaceId) {
    indexAttachmentRefs(entryPath, updatedAttachments, workspaceId);
  }
}

export async function enqueueIncrementalAttachmentUpload(
  entryPath: string,
  attachmentMetadataPath: string,
  file: File,
): Promise<void> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  const hash = await sha256Hex(bytes);
  await updateAttachmentRefMetadata(
    entryPath,
    attachmentMetadataPath,
    hash,
    file.type || getMimeType(file.name),
    file.size,
  );
  const workspaceId = getWorkspaceId() ?? getCurrentWorkspace()?.id;
  if (!workspaceId) return;
  const syncEnabled = isAttachmentSyncEnabled();
  console.log('[AttachmentController] enqueue: workspaceId=', workspaceId, 'syncEnabled=', syncEnabled);
  const queueId = enqueueAttachmentUpload({
    workspaceId,
    entryPath,
    attachmentPath: attachmentMetadataPath,
    hash,
    mimeType: file.type || getMimeType(file.name),
    sizeBytes: file.size,
  });
  console.log('[AttachmentController] enqueued queueId=', queueId);
  if (syncEnabled) {
    trackUpload(queueId);
  }
}

async function formatSourceRelativeAttachmentPath(
  api: Api,
  sourceEntryPath: string,
  canonicalAttachmentPath: string,
  fallbackPath?: string,
): Promise<string> {
  try {
    return await api.formatLink(
      canonicalAttachmentPath,
      canonicalAttachmentPath.split('/').pop() || 'attachment',
      'plain_relative',
      sourceEntryPath,
    );
  } catch {
    return fallbackPath ?? canonicalAttachmentPath;
  }
}

// ============================================================================
// State for pending attachment
// ============================================================================

let pendingAttachmentPath: string = '';
let attachmentError: string | null = null;

/**
 * Get the pending attachment path.
 */
export function getPendingAttachmentPath(): string {
  return pendingAttachmentPath;
}

/**
 * Set the pending attachment path.
 */
export function setPendingAttachmentPath(path: string): void {
  pendingAttachmentPath = path;
}

/**
 * Get the current attachment error.
 */
export function getAttachmentError(): string | null {
  return attachmentError;
}

/**
 * Set the attachment error.
 */
export function setAttachmentError(error: string | null): void {
  attachmentError = error;
}

/**
 * Clear the attachment error.
 */
export function clearAttachmentError(): void {
  attachmentError = null;
}

// ============================================================================
// Main Functions
// ============================================================================

/**
 * Handle add attachment from context menu - triggers file picker.
 */
export function handleAddAttachment(
  entryPath: string,
  fileInput: HTMLInputElement | null
): void {
  pendingAttachmentPath = entryPath;
  attachmentError = null;
  fileInput?.click();
}

/**
 * Handle file selection from file input for attachment.
 */
export async function handleAttachmentFileSelect(
  event: Event,
  api: Api,
  currentEntry: EntryData | null,
  editorRef: any,
  onEntryUpdate?: (entry: EntryData) => void
): Promise<void> {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file || !pendingAttachmentPath) return;

  // Check size limit
  if (file.size > MAX_FILE_SIZE) {
    attachmentError = `File too large (${(file.size / 1024 / 1024).toFixed(1)}MB). Maximum is 10MB.`;
    input.value = '';
    return;
  }

  try {
    // Convert file to base64
    const bytes = new Uint8Array(await file.arrayBuffer());
    const dataBase64 = bytesToBase64(bytes);

    // Upload attachment
    const attachmentPath = await api.uploadAttachment(
      pendingAttachmentPath,
      file.name,
      dataBase64
    );
    const canonicalAttachmentPath = await api.canonicalizeLink(
      attachmentPath,
      pendingAttachmentPath
    );
    const entryRelativePath = await formatSourceRelativeAttachmentPath(
      api,
      pendingAttachmentPath,
      canonicalAttachmentPath,
      attachmentPath,
    );
    await enqueueIncrementalAttachmentUpload(
      pendingAttachmentPath,
      canonicalAttachmentPath,
      file,
    );

    // Refresh the entry if it's currently open
    if (currentEntry?.path === pendingAttachmentPath) {
      const entry = await api.getEntry(pendingAttachmentPath);
      entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
      entryStore.setCurrentEntry(entry);
      if (onEntryUpdate) {
        onEntryUpdate(entry);
      }

      // If it's an image, also insert it into the editor at cursor
      if (file.type.startsWith('image/') && editorRef) {
        const data = await api.getAttachmentData(
          currentEntry.path,
          entryRelativePath
        );
        const blob = new Blob([new Uint8Array(data)], { type: file.type });
        const blobUrl = URL.createObjectURL(blob);

        // Track for cleanup
        trackBlobUrl(entryRelativePath, blobUrl);

        // Insert image at cursor
        editorRef.insertImage(blobUrl, file.name);
      }
    }

    attachmentError = null;
  } catch (e) {
    attachmentError = e instanceof Error ? e.message : String(e);
  }

  input.value = '';
  pendingAttachmentPath = '';
}

/**
 * Handle file drop in Editor - upload and return blob URL for images.
 */
export async function handleEditorFileDrop(
  file: File,
  api: Api,
  currentEntry: EntryData | null,
  onEntryUpdate?: (entry: EntryData) => void
): Promise<{ blobUrl: string; attachmentPath: string } | null> {
  if (!currentEntry) return null;

  // Check size limit
  if (file.size > MAX_FILE_SIZE) {
    attachmentError = `File too large (${(file.size / 1024 / 1024).toFixed(1)}MB). Maximum is 10MB.`;
    return null;
  }

  try {
    const bytes = new Uint8Array(await file.arrayBuffer());
    const dataBase64 = bytesToBase64(bytes);
    const attachmentPath = await api.uploadAttachment(
      currentEntry.path,
      file.name,
      dataBase64
    );
    const canonicalAttachmentPath = await api.canonicalizeLink(
      attachmentPath,
      currentEntry.path
    );
    const entryRelativePath = await formatSourceRelativeAttachmentPath(
      api,
      currentEntry.path,
      canonicalAttachmentPath,
      attachmentPath,
    );
    await enqueueIncrementalAttachmentUpload(
      currentEntry.path,
      canonicalAttachmentPath,
      file,
    );

    // Refresh the entry to update attachments list
    const entry = await api.getEntry(currentEntry.path);
    entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
    entryStore.setCurrentEntry(entry);
    if (onEntryUpdate) {
      onEntryUpdate(entry);
    }

    // For images, create blob URL for display in editor
    if (file.type.startsWith('image/')) {
      const data = await api.getAttachmentData(
        currentEntry.path,
        entryRelativePath
      );
      // Use the file's actual MIME type when available, fall back to extension-based lookup
      const mimeType = file.type || getMimeType(file.name);
      const blob = new Blob([new Uint8Array(data)], { type: mimeType });
      const blobUrl = URL.createObjectURL(blob);

      // Track for cleanup
      trackBlobUrl(entryRelativePath, blobUrl);

      return { blobUrl, attachmentPath: entryRelativePath };
    }

    // For non-image files, just return the path (no blob URL for editor display)
    return { blobUrl: '', attachmentPath: entryRelativePath };
  } catch (e) {
    console.error('[AttachmentController] handleEditorFileDrop failed:', e);
    attachmentError = e instanceof Error ? e.message : String(e);
    return null;
  }
}

/**
 * Handle delete attachment.
 */
export async function handleDeleteAttachment(
  attachmentPath: string,
  api: Api,
  currentEntry: EntryData | null,
  onEntryUpdate?: (entry: EntryData) => void
): Promise<void> {
  if (!currentEntry) return;

  try {
    await api.deleteAttachment(currentEntry.path, attachmentPath);

    // Refresh current entry to update attachments list
    const entry = await api.getEntry(currentEntry.path);
    entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
    entryStore.setCurrentEntry(entry);
    if (onEntryUpdate) {
      onEntryUpdate(entry);
    }
    const workspaceId = getCurrentWorkspace()?.id;
    if (workspaceId) {
      indexAttachmentRefs(currentEntry.path, (entry.frontmatter.attachments as any[]) ?? [], workspaceId);
    }
    attachmentError = null;
  } catch (e) {
    attachmentError = e instanceof Error ? e.message : String(e);
  }
}

/**
 * Handle attachment selection from inline picker node.
 */
export function handleAttachmentInsert(
  selection: {
    path: string;
    isImage: boolean;
    blobUrl?: string;
    sourceEntryPath: string;
  },
  editorRef: any,
  currentEntry: EntryData | null
): void {
  if (!selection || !editorRef || !currentEntry) return;

  const filename = selection.path.split('/').pop() || selection.path;

  // Calculate relative path from current entry to the attachment
  const relativePath = computeRelativeAttachmentPath(
    currentEntry.path,
    selection.sourceEntryPath,
    selection.path
  );

  // Always embed mode
  if (selection.isImage && selection.blobUrl) {
    // Track the blob URL for reverse transformation on save
    trackBlobUrl(relativePath, selection.blobUrl);
    // Insert image with blob URL
    editorRef.insertImage(selection.blobUrl, filename);
  } else {
    // For non-images or images without blob URL, insert markdown syntax
    const markdown = `![${filename}](${relativePath})`;
    editorRef.setContent(editorRef.getMarkdown() + `\n${markdown}\n`);
  }
}

/**
 * Handle moving an attachment from one entry to another.
 */
export async function handleMoveAttachment(
  sourceEntryPath: string,
  targetEntryPath: string,
  attachmentPath: string,
  api: Api,
  currentEntry: EntryData | null,
  onEntryUpdate?: (entry: EntryData) => void
): Promise<void> {
  if (sourceEntryPath === targetEntryPath) return;

  try {
    const movedPath = await api.moveAttachment(sourceEntryPath, targetEntryPath, attachmentPath);

    // Refresh current entry if it was affected
    if (
      currentEntry?.path === sourceEntryPath ||
      currentEntry?.path === targetEntryPath
    ) {
      const entry = await api.getEntry(currentEntry.path);
      entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
      entryStore.setCurrentEntry(entry);
      if (onEntryUpdate) {
        onEntryUpdate(entry);
      }
    }

    const workspaceId = getCurrentWorkspace()?.id;
    if (workspaceId) {
      const sourceMetadata = await getFileMetadata(sourceEntryPath);
      if (sourceMetadata) {
        indexAttachmentRefs(sourceEntryPath, sourceMetadata.attachments, workspaceId);
      }
      const targetMetadata = await getFileMetadata(targetEntryPath);
      if (targetMetadata) {
        indexAttachmentRefs(targetEntryPath, targetMetadata.attachments, workspaceId);
      }
      if (movedPath && targetMetadata) {
        const movedRef = targetMetadata.attachments.find((attachment) => attachment.path === movedPath);
        if (movedRef?.hash) {
          const queueId = enqueueAttachmentUpload({
            workspaceId,
            entryPath: targetEntryPath,
            attachmentPath: movedPath,
            hash: movedRef.hash,
            mimeType: movedRef.mime_type || getMimeType(movedPath),
            sizeBytes: Number(movedRef.size ?? 0n),
          });
          if (isAttachmentSyncEnabled()) {
            trackUpload(queueId);
          }
        }
      }
    }

    toast.success('Attachment moved successfully');
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    toast.error(`Failed to move attachment: ${message}`);
  }
}

/**
 * Populate the CRDT with files from the filesystem.
 * Called before hosting a share session to ensure all files are available.
 *
 * @param api - API instance
 * @param treePath - Path to the workspace root index
 * @param audience - If provided, only include files accessible to this audience
 */
export async function populateCrdtBeforeHost(
  api: Api,
  treePath: string | null,
  audience: string | null = null
): Promise<void> {
  if (!treePath) {
    console.warn('[AttachmentController] Cannot populate CRDT: treePath not available');
    return;
  }

  console.log(
    '[AttachmentController] Populating CRDT from filesystem before hosting, audience:',
    audience
  );

  try {
    // Use Rust command which handles audience filtering internally
    const result = await api.initializeWorkspaceCrdt(
      treePath,
      audience ?? undefined
    );
    console.log('[AttachmentController] CRDT populated:', result);
  } catch (e) {
    console.error('[AttachmentController] Failed to populate CRDT:', e);
  }
}
