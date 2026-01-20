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
} from '../models/services/attachmentService';
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
    const dataBase64 = await fileToBase64(file);

    // Upload attachment
    const attachmentPath = await api.uploadAttachment(
      pendingAttachmentPath,
      file.name,
      dataBase64
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
          attachmentPath
        );
        const blob = new Blob([new Uint8Array(data)], { type: file.type });
        const blobUrl = URL.createObjectURL(blob);

        // Track for cleanup
        trackBlobUrl(attachmentPath, blobUrl);

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
    const dataBase64 = await fileToBase64(file);
    const attachmentPath = await api.uploadAttachment(
      currentEntry.path,
      file.name,
      dataBase64
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
        attachmentPath
      );
      // Use the file's actual MIME type when available, fall back to extension-based lookup
      const mimeType = file.type || getMimeType(file.name);
      const blob = new Blob([new Uint8Array(data)], { type: mimeType });
      const blobUrl = URL.createObjectURL(blob);

      // Track for cleanup
      trackBlobUrl(attachmentPath, blobUrl);

      return { blobUrl, attachmentPath };
    }

    // For non-image files, just return the path (no blob URL for editor display)
    return { blobUrl: '', attachmentPath };
  } catch (e) {
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
