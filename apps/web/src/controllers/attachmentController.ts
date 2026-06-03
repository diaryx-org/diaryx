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
 */

import type { Api, EntryData } from '../lib/backend';
import { entryStore } from '../models/stores';
import {
  trackBlobUrl,
  computeRelativeAttachmentPath,
  formatMarkdownDestination,
  getMimeType,
  getAttachmentMediaKind,
  isPreviewableAttachmentKind,
  isHtmlFile,
  type AttachmentMediaKind,
} from '../models/services/attachmentService';
import {
  indexAttachmentRefs,
  sha256Hex,
} from '$lib/sync/attachmentSyncService';
import { getCurrentWorkspaceId } from '$lib/storage/localWorkspaceRegistry.svelte';
import { toast } from 'svelte-sonner';

// ============================================================================
// Constants
// ============================================================================

const MAX_FILE_SIZE = 10 * 1024 * 1024; // 10MB

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

function indexAttachmentMetadata(
  entryPath: string,
  attachmentPath: string,
  hash: string,
  mimeType: string,
  sizeBytes: number,
  workspaceId: string,
): void {
  indexAttachmentRefs(
    entryPath,
    [
      {
        path: attachmentPath,
        source: 'local',
        hash,
        mime_type: mimeType || getMimeType(attachmentPath),
        size: BigInt(sizeBytes),
        uploaded_at: BigInt(Date.now()),
        deleted: false,
      },
    ],
    workspaceId,
  );
}

function createAttachmentPreviewBlobUrl(
  file: File,
  bytes: Uint8Array,
  kind: AttachmentMediaKind,
): string | undefined {
  if (!isPreviewableAttachmentKind(kind)) return undefined;
  const mimeType = file.type || getMimeType(file.name);
  const blob = new Blob([bytes as unknown as BlobPart], { type: mimeType });
  return URL.createObjectURL(blob);
}

export async function enqueueIncrementalAttachmentUpload(
  entryPath: string,
  attachmentMetadataPath: string,
  file: File,
  bytes?: Uint8Array,
): Promise<void> {
  const workspaceId = getCurrentWorkspaceId();
  if (!workspaceId) return;
  const resolvedBytes = bytes ?? new Uint8Array(await file.arrayBuffer());
  const hash = await sha256Hex(resolvedBytes);
  indexAttachmentMetadata(
    entryPath,
    attachmentMetadataPath,
    hash,
    file.type || getMimeType(file.name),
    file.size,
    workspaceId,
  );
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
    const bytes = new Uint8Array(await file.arrayBuffer());
    const mediaKind = getAttachmentMediaKind(file.name, file.type);

    const attachmentPath = await api.uploadAttachment(
      pendingAttachmentPath,
      file.name,
      bytes
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
      bytes,
    );

    // Refresh the entry if it's currently open
    if (currentEntry?.path === pendingAttachmentPath) {
      const entry = await api.getEntry(pendingAttachmentPath);
      entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
      entryStore.setCurrentEntry(entry);
      if (onEntryUpdate) {
        onEntryUpdate(entry);
      }

      // Insert previewable media immediately at the cursor using the bytes
      // already in memory, so we do not have to re-read the uploaded file.
      if (editorRef && isPreviewableAttachmentKind(mediaKind)) {
        const blobUrl = createAttachmentPreviewBlobUrl(file, bytes, mediaKind);
        if (blobUrl) {
          trackBlobUrl(entryRelativePath, blobUrl);
          editorRef.insertImage(blobUrl, file.name);
        }
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
): Promise<{ blobUrl: string; attachmentPath: string; kind: AttachmentMediaKind } | null> {
  if (!currentEntry) return null;

  // Check size limit
  if (file.size > MAX_FILE_SIZE) {
    attachmentError = `File too large (${(file.size / 1024 / 1024).toFixed(1)}MB). Maximum is 10MB.`;
    return null;
  }

  try {
    const bytes = new Uint8Array(await file.arrayBuffer());
    const mediaKind = getAttachmentMediaKind(file.name, file.type);
    const attachmentPath = await api.uploadAttachment(
      currentEntry.path,
      file.name,
      bytes
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
      bytes,
    );

    // Refresh the entry to update attachments list
    const entry = await api.getEntry(currentEntry.path);
    entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
    entryStore.setCurrentEntry(entry);
    if (onEntryUpdate) {
      onEntryUpdate(entry);
    }

    // For previewable media, create a blob URL for immediate display in editor.
    const blobUrl = createAttachmentPreviewBlobUrl(file, bytes, mediaKind);
    if (blobUrl) {
      trackBlobUrl(entryRelativePath, blobUrl);
      return { blobUrl, attachmentPath: entryRelativePath, kind: mediaKind };
    }

    // Non-previewable files still upload successfully, but the editor does not
    // have an inline representation for them.
    return { blobUrl: '', attachmentPath: entryRelativePath, kind: mediaKind };
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
    kind: AttachmentMediaKind;
    blobUrl?: string;
    filename?: string;
    sourceEntryPath: string;
  },
  editorRef: any,
  currentEntry: EntryData | null
): void {
  if (!selection || !editorRef || !currentEntry) return;

  const filename = selection.filename || selection.path.split('/').pop() || selection.path;

  // Calculate relative path from current entry to the attachment
  const relativePath = computeRelativeAttachmentPath(
    currentEntry.path,
    selection.sourceEntryPath,
    selection.path
  );

  // Always embed mode
  const isHtml = isHtmlFile(selection.filename ?? selection.path);
  if ((isPreviewableAttachmentKind(selection.kind) || isHtml) && selection.blobUrl) {
    // Track the blob URL for reverse transformation on save
    trackBlobUrl(relativePath, selection.blobUrl);
    editorRef.insertImage(selection.blobUrl, filename);
  } else if (isPreviewableAttachmentKind(selection.kind) || isHtml) {
    editorRef.insertImage(relativePath, filename);
  } else {
    // Preserve the legacy markdown-embed fallback for non-previewable files.
    const markdown = `![${filename}](${formatMarkdownDestination(relativePath)})`;
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
    await api.moveAttachment(sourceEntryPath, targetEntryPath, attachmentPath);

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

    toast.success('Attachment moved successfully');
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    toast.error(`Failed to move attachment: ${message}`);
  }
}

/**
 * Deprecated no-op maintained for API compatibility with legacy share flows.
 *
 * @param api - API instance
 * Share/session orchestration is sync-plugin-owned.
 */
