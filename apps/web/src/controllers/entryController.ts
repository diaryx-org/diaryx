/**
 * Entry Controller
 *
 * Handles entry-level operations including:
 * - Opening entries
 * - Saving entries
 * - Creating entries
 * - Deleting entries
 * - Renaming entries
 * - Property changes
 */

import { tick } from 'svelte';
import type { EntryData, TreeNode, Api, CreateChildResult } from '../lib/backend';
import { getBackend } from '../lib/backend';
import type { JsonValue } from '../lib/backend/generated/serde_json/JsonValue';
import { entryStore, uiStore, collaborationStore } from '../models/stores';
import {
  revokeBlobUrls,
  reverseBlobUrlsToAttachmentPaths,
} from '../models/services';
import { dispatchFileOpenedEvent } from '../lib/plugins/browserPluginManager.svelte';

function toSyncOpenPath(workspacePath: string, path: string): string {
  const workspaceDir = workspacePath
    .replace(/\/index\.md$/, '')
    .replace(/\/README\.md$/, '');
  const normalizedPath = path.replace(/\\/g, '/');
  if (workspaceDir && normalizedPath.startsWith(`${workspaceDir}/`)) {
    return normalizedPath.substring(workspaceDir.length + 1);
  }
  return normalizedPath.replace(/^\/+/, '');
}

// Sync/body orchestration is plugin-owned; host keeps local filesystem workflows.
async function ensureBodySync(path: string): Promise<void> {
  const backend = await getBackend();
  const syncPath = toSyncOpenPath(backend.getWorkspacePath(), path);
  await dispatchFileOpenedEvent(syncPath);
}
function closeBodySync(_path: string): void {}

type EditorMarkdownRef = {
  getMarkdown?: () => string | undefined;
} | null | undefined;

const SAVE_RETRY_DELAYS_MS = [100, 200, 400, 800, 1600, 3200];

function isTransientSaveError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return (
    message.includes('NotFoundError') ||
    message.includes('NoModificationAllowedError') ||
    message.includes('Failed to write file') ||
    message.includes('A requested file or directory could not be found') ||
    message.includes('An attempt was made to modify an object where modifications are not allowed')
  );
}

async function saveEntryWithRetry(
  api: Api,
  path: string,
  markdown: string,
  rootIndexPath?: string,
  detectH1Title?: boolean
): Promise<string | null> {
  let lastError: unknown = null;
  for (let attempt = 0; attempt <= SAVE_RETRY_DELAYS_MS.length; attempt++) {
    try {
      return await api.saveEntry(path, markdown, rootIndexPath, detectH1Title);
    } catch (e) {
      lastError = e;
      const shouldRetry = isTransientSaveError(e) && attempt < SAVE_RETRY_DELAYS_MS.length;
      if (!shouldRetry) break;
      const delayMs = SAVE_RETRY_DELAYS_MS[attempt];
      console.log(`[EntryController] saveEntry transient failure for '${path}', retrying in ${delayMs}ms`);
      await new Promise((resolve) => setTimeout(resolve, delayMs));
    }
  }
  console.warn(`[EntryController] saveEntry failed for '${path}' after retries:`, lastError);
  throw lastError;
}

/**
 * Get the current editor body markdown, reversing any transient blob URLs so
 * callers receive the same attachment paths that persist to disk.
 */
export function getEditorBodyMarkdown(editorRef: EditorMarkdownRef): string {
  const markdownWithBlobUrls = editorRef?.getMarkdown?.() ?? '';
  return reverseBlobUrlsToAttachmentPaths(markdownWithBlobUrls);
}

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
 * Open an entry for editing.
 */
export async function openEntry(
  api: Api,
  path: string,
  tree: TreeNode | null,
  collaborationEnabled: boolean,
  options?: {
    onBeforeOpen?: () => Promise<void>;
    isCurrentRequest?: () => boolean;
  }
): Promise<void> {
  const isCurrentRequest = options?.isCurrentRequest ?? (() => true);

  // Call before open callback (e.g., save current entry)
  if (options?.onBeforeOpen) {
    await options.onBeforeOpen();
  }
  if (!isCurrentRequest()) return;

  try {
    entryStore.setLoading(true);

    // Cleanup previous blob URLs
    revokeBlobUrls();

    const entry = await api.getEntry(path);
    if (!isCurrentRequest()) return;
    // Normalize frontmatter to Object
    entry.frontmatter = normalizeFrontmatter(entry.frontmatter);
    entryStore.setCurrentEntry(entry);
    entryStore.setTitleError(null); // Clear any title error when switching files

    console.log('[EntryController] Loaded entry:', entry);

    // Show content immediately — NodeViews resolve attachments lazily.
    // Clear loading state so the editor is visible before sync setup.
    if (entry) {
      entryStore.setDisplayContent(entry.content);
    } else {
      entryStore.setDisplayContent('');
    }
    entryStore.markClean();
    uiStore.clearError();
  } catch (e) {
    if (isCurrentRequest()) {
      uiStore.setError(e instanceof Error ? e.message : String(e));
    }
  } finally {
    if (isCurrentRequest()) {
      entryStore.setLoading(false);
    }
  }
  if (!isCurrentRequest()) return;

  // Non-blocking: set up body sync bridge and collaboration tracking.
  // The bridge must exist to receive remote body updates (onBodyChange callback),
  // but it doesn't need to complete before the editor is visible.
  try {
    await ensureBodySync(path);
  } catch (e) {
    console.warn('[EntryController] Body sync setup failed:', e);
  }
  if (!isCurrentRequest()) return;

  // Collaboration path tracking (doesn't affect content display)
  try {
    const entry = entryStore.currentEntry;
    if (entry && entry.path === path) {
      let workspaceDir = tree?.path || '';
      if (workspaceDir.endsWith('/')) {
        workspaceDir = workspaceDir.slice(0, -1);
      }
      if (
        workspaceDir.endsWith('README.md') ||
        workspaceDir.endsWith('index.md')
      ) {
        workspaceDir = workspaceDir.substring(0, workspaceDir.lastIndexOf('/'));
      }
      let newRelativePath = entry.path;
      if (workspaceDir && entry.path.startsWith(workspaceDir)) {
        newRelativePath = entry.path.substring(workspaceDir.length + 1);
      }

      const currentCollaborationPath = collaborationStore.currentCollaborationPath;
      if (currentCollaborationPath !== newRelativePath) {
        collaborationStore.clearCollaborationSession();
        await tick();
      }

      if (collaborationEnabled) {
        collaborationStore.setCollaborationPath(newRelativePath);
        console.log('[EntryController] Collaboration path:', newRelativePath);
      }
    } else if (!entry) {
      collaborationStore.clearCollaborationSession();
    }
  } catch (e) {
    console.warn('[EntryController] Collaboration setup failed:', e);
  }
}

/**
 * Save the current entry.
 */
export async function saveEntry(
  api: Api,
  currentEntry: EntryData | null,
  editorRef: any,
  rootIndexPath?: string
): Promise<void> {
  if (!currentEntry || !editorRef) return;
  if (entryStore.isSaving) return; // Prevent concurrent saves

  try {
    entryStore.setSaving(true);
    const markdown = getEditorBodyMarkdown(editorRef);

    // Note: saveEntry expects only the body content, not frontmatter.
    // Frontmatter is preserved by the backend's save_content() method.
    await saveEntryWithRetry(api, currentEntry.path, markdown, rootIndexPath);
    entryStore.setDisplayContent(markdown);
    entryStore.markClean();
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
  } finally {
    entryStore.setSaving(false);
  }
}

/**
 * Create a child entry under a parent.
 * Note: CRDT sync is handled by Rust CreateEntry command.
 */
export async function createChildEntry(
  api: Api,
  parentPath: string,
  onSuccess?: () => Promise<void>
): Promise<CreateChildResult | null> {
  try {
    const result = await api.createChildEntry(parentPath);

    if (onSuccess) {
      await onSuccess();
    }

    return result;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return null;
  }
}

/**
 * Create a new entry at a specific path.
 * Note: CRDT sync is handled by Rust CreateEntry command.
 */
export async function createEntry(
  api: Api,
  path: string,
  options: { title: string; rootIndexPath?: string },
  onSuccess?: () => Promise<void>
): Promise<string | null> {
  try {
    const newPath = await api.createEntry(path, { ...options, rootIndexPath: options.rootIndexPath });

    if (onSuccess) {
      await onSuccess();
    }

    return newPath;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return null;
  } finally {
    uiStore.closeNewEntryModal();
  }
}

/**
 * Delete an entry.
 *
 * Callers are responsible for showing a confirmation dialog before calling this.
 */
export async function deleteEntry(
  api: Api,
  path: string,
  currentEntryPath: string | null,
  onSuccess?: () => Promise<void>
): Promise<boolean> {
  try {
    await api.deleteEntry(path);

    // If we deleted the currently open entry, clear it
    if (currentEntryPath === path) {
      entryStore.setCurrentEntry(null);
      entryStore.markClean();
    }

    if (onSuccess) {
      // Try to refresh - might fail if workspace state is temporarily inconsistent
      try {
        await onSuccess();
      } catch (refreshError) {
        console.warn('[EntryController] Error refreshing after delete:', refreshError);
        // Try again after a short delay
        setTimeout(async () => {
          try {
            if (onSuccess) await onSuccess();
          } catch (e) {
            console.error('[EntryController] Retry refresh failed:', e);
          }
        }, 500);
      }
    }

    return true;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return false;
  }
}

/**
 * Move an entry to a new parent (attach entry to parent).
 */
export async function moveEntry(
  api: Api,
  entryPath: string,
  newParentPath: string,
  onSuccess?: () => Promise<void>
): Promise<boolean> {
  if (entryPath === newParentPath) return false; // Can't attach to self

  console.log(
    `[EntryController] entryPath="${entryPath}" -> newParentPath="${newParentPath}"`
  );

  try {
    await api.attachEntryToParent(entryPath, newParentPath);

    if (onSuccess) {
      await onSuccess();
    }

    return true;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return false;
  }
}

/**
 * Handle property change on the current entry.
 * Title changes with auto-rename are handled atomically by the backend.
 */
export async function handlePropertyChange(
  api: Api,
  currentEntry: EntryData,
  key: string,
  value: unknown,
  expandedNodes: Set<string>,
  onRefreshTree?: () => Promise<void>,
  options?: { rootIndexPath?: string; onRootIndexRenamed?: (newTitle: string) => void }
): Promise<{ success: boolean; newPath?: string }> {
  try {
    const normalizedFrontmatter = normalizeFrontmatter(currentEntry.frontmatter);

    if (key === 'title' && typeof value === 'string' && value.trim()) {
      const nextTitle = value.trim();
      const currentTitle =
        typeof normalizedFrontmatter.title === 'string'
          ? normalizedFrontmatter.title
          : '';

      if (nextTitle === currentTitle) {
        entryStore.setTitleError(null);
        return { success: true };
      }

      try {
        // Backend handles: workspace config read, filename style, rename, title set, H1 sync
        // Returns new path string if rename occurred, null otherwise
        const newPath = await api.setFrontmatterProperty(
          currentEntry.path, key, value, options?.rootIndexPath
        );

        if (newPath) {
          // Rename happened — update UI state to new path
          if (expandedNodes.has(currentEntry.path)) {
            expandedNodes.delete(currentEntry.path);
            expandedNodes.add(newPath);
          }

          entryStore.setCurrentEntry({
            ...currentEntry,
            path: newPath,
            frontmatter: { ...normalizedFrontmatter, [key]: value },
          });
        } else {
          // No rename — just update frontmatter
          entryStore.setCurrentEntry({
            ...currentEntry,
            frontmatter: { ...normalizedFrontmatter, [key]: value },
          });
        }

        // If this entry is the root index, notify caller to sync workspace name
        if (
          options?.onRootIndexRenamed &&
          options?.rootIndexPath &&
          (currentEntry.path === options.rootIndexPath ||
            (newPath ?? currentEntry.path) === options.rootIndexPath)
        ) {
          options.onRootIndexRenamed(nextTitle);
        }

        entryStore.setTitleError(null);
        if (onRefreshTree) await onRefreshTree();
        return { success: true, newPath: newPath ?? undefined };
      } catch (renameError) {
        const errorMsg =
          renameError instanceof Error
            ? renameError.message
            : String(renameError);
        if (
          errorMsg.includes('already exists') ||
          errorMsg.includes('Destination')
        ) {
          entryStore.setTitleError(
            `A file with that name already exists. Choose a different title.`
          );
        } else {
          entryStore.setTitleError(`Could not rename: ${errorMsg}`);
        }
        return { success: false };
      }
    } else {
      // Non-title properties: update normally
      const newFrontmatter = { ...normalizedFrontmatter, [key]: value };
      await api.setFrontmatterProperty(currentEntry.path, key, value as JsonValue, options?.rootIndexPath);
      entryStore.setCurrentEntry({
        ...currentEntry,
        frontmatter: newFrontmatter,
      });

      if ((key === 'contents' || key === 'part_of') && onRefreshTree) {
        await onRefreshTree();
      }

      return { success: true };
    }
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return { success: false };
  }
}

/**
 * Remove a property from the current entry.
 * Note: CRDT sync is handled by Rust RemoveFrontmatterProperty command.
 */
export async function removeProperty(
  api: Api,
  currentEntry: EntryData,
  key: string
): Promise<boolean> {
  try {
    await api.removeFrontmatterProperty(currentEntry.path, key);

    // Update local state
    const newFrontmatter = normalizeFrontmatter(currentEntry.frontmatter);
    delete newFrontmatter[key];
    entryStore.setCurrentEntry({ ...currentEntry, frontmatter: newFrontmatter });

    return true;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return false;
  }
}

/**
 * Add a property to the current entry.
 * Note: CRDT sync is handled by Rust SetFrontmatterProperty command.
 */
export async function addProperty(
  api: Api,
  currentEntry: EntryData,
  key: string,
  value: unknown,
  rootIndexPath?: string
): Promise<boolean> {
  try {
    await api.setFrontmatterProperty(currentEntry.path, key, value as JsonValue, rootIndexPath);
    const normalizedFrontmatter = normalizeFrontmatter(currentEntry.frontmatter);

    // Update local state
    entryStore.setCurrentEntry({
      ...currentEntry,
      frontmatter: { ...normalizedFrontmatter, [key]: value },
    });

    return true;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return false;
  }
}

/**
 * Rename an entry.
 * Note: CRDT sync is handled by Rust RenameEntry command.
 *
 * @param api - API instance
 * @param path - Current path of the entry
 * @param newFilename - New filename (with .md extension)
 * @param onSuccess - Callback after successful rename (e.g., refresh tree)
 * @returns The new path of the renamed entry
 */
export async function renameEntry(
  api: Api,
  path: string,
  newFilename: string,
  onSuccess?: () => Promise<void>
): Promise<string> {
  // Close old body sync bridge before rename
  closeBodySync(path);

  const newPath = await api.renameEntry(path, newFilename);

  // Create new body sync bridge for the new path
  await ensureBodySync(newPath);

  if (onSuccess) {
    await onSuccess();
  }

  return newPath;
}

/**
 * Duplicate an entry.
 * Note: CRDT sync for the new file should be added via Rust DuplicateEntry command.
 *
 * @param api - API instance
 * @param path - Path of the entry to duplicate
 * @param onSuccess - Callback after successful duplication (e.g., refresh tree)
 * @returns The path of the new duplicated entry
 */
export async function duplicateEntry(
  api: Api,
  path: string,
  onSuccess?: () => Promise<void>
): Promise<string> {
  const newPath = await api.duplicateEntry(path);

  // Ensure body sync bridge is created for the new file
  await ensureBodySync(newPath);

  if (onSuccess) {
    await onSuccess();
  }

  return newPath;
}

/**
 * Delete an entry with CRDT sync support.
 * Note: CRDT sync (soft delete) is now handled by Rust DeleteEntry command.
 *
 * Callers are responsible for showing a confirmation dialog before calling this.
 *
 * @param api - API instance
 * @param path - Path of the entry to delete
 * @param currentEntryPath - Path of the currently open entry (to clear if same)
 * @param onSuccess - Callback after successful deletion (e.g., refresh tree)
 * @returns True if deletion completed successfully
 */
export async function deleteEntryWithSync(
  api: Api,
  path: string,
  currentEntryPath: string | null,
  onSuccess?: () => Promise<void>
): Promise<boolean> {
  try {
    // Close body sync bridge for the deleted file
    closeBodySync(path);

    // Delete via Rust - handles CRDT soft delete automatically
    await api.deleteEntry(path);

    // If we deleted the currently open entry, clear it
    if (currentEntryPath === path) {
      entryStore.setCurrentEntry(null);
      entryStore.markClean();
    }

    if (onSuccess) {
      // Try to refresh - might fail if workspace state is temporarily inconsistent
      try {
        await onSuccess();
      } catch (refreshError) {
        console.warn('[EntryController] Error refreshing after delete:', refreshError);
        // Try again after a short delay
        setTimeout(async () => {
          try {
            if (onSuccess) await onSuccess();
          } catch (e) {
            console.error('[EntryController] Retry refresh failed:', e);
          }
        }, 500);
      }
    }

    return true;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return false;
  }
}

/**
 * Create a child entry with CRDT sync support.
 * Note: CRDT sync is now handled by Rust CreateEntry command.
 *
 * @param api - API instance
 * @param parentPath - Path of the parent entry
 * @param onSuccess - Callback after successful creation. Receives the full result
 *                    including both child path and (possibly new) parent path.
 * @returns The CreateChildResult with paths and conversion info, or null on failure
 */
export async function createChildEntryWithSync(
  api: Api,
  parentPath: string,
  onSuccess?: (result: CreateChildResult) => Promise<void>
): Promise<CreateChildResult | null> {
  try {
    // Create via Rust - handles CRDT sync automatically
    // Returns detailed result with child path, parent path, and conversion info
    const result = await api.createChildEntry(parentPath);

    // Ensure body sync bridge is created for the new file
    await ensureBodySync(result.child_path);

    if (onSuccess) {
      await onSuccess(result);
    }

    return result;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return null;
  }
}

/**
 * Create a new entry with CRDT sync support.
 * Note: CRDT sync is now handled by Rust CreateEntry command.
 *
 * @param api - API instance
 * @param path - Path for the new entry
 * @param options - Options including title and template
 * @param onSuccess - Callback after successful creation
 * @returns The path of the new entry, or null on failure
 */
export async function createEntryWithSync(
  api: Api,
  path: string,
  options: { title: string; template?: string; rootIndexPath?: string },
  onSuccess?: () => Promise<void>
): Promise<string | null> {
  try {
    // Create via Rust - handles CRDT sync automatically
    const newPath = await api.createEntry(path, { ...options, rootIndexPath: options.rootIndexPath });

    // Ensure body sync bridge is created for the new file
    await ensureBodySync(newPath);

    if (onSuccess) {
      await onSuccess();
    }

    return newPath;
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
    return null;
  } finally {
    uiStore.closeNewEntryModal();
  }
}

/**
 * Save an entry with CRDT sync support.
 * Note: CRDT sync is now handled by Rust SaveEntry command.
 *
 * @param api - API instance
 * @param currentEntry - The current entry being saved
 * @param editorRef - Reference to the editor component
 * @param rootIndexPath - Workspace root index path
 * @param detectH1Title - When true, detect H1→title sync (manual save/blur only, not auto-save)
 * @returns Object with newPath if H1 sync caused a rename
 */
export async function saveEntryWithSync(
  api: Api,
  currentEntry: EntryData | null,
  editorRef: any,
  rootIndexPath?: string,
  detectH1Title?: boolean
): Promise<{ newPath?: string } | void> {
  if (!currentEntry || !editorRef) return;
  if (entryStore.isSaving) return; // Prevent concurrent saves

  try {
    entryStore.setSaving(true);
    const markdown = getEditorBodyMarkdown(editorRef);

    // Save to backend - Rust handles CRDT sync automatically
    const newPath = await saveEntryWithRetry(api, currentEntry.path, markdown, rootIndexPath, detectH1Title);
    // Mirror the saved markdown into displayContent so it tracks the editor's
    // live state. Without this, displayContent permanently lags behind the editor
    // until the user switches entries — and any later code path that re-syncs the
    // editor from displayContent (e.g. plugin-triggered editor rebuild) will
    // silently overwrite unsaved-since-load edits, causing data loss.
    entryStore.setDisplayContent(markdown);
    entryStore.markClean();

    if (newPath && newPath !== currentEntry.path) {
      // H1 sync caused a rename — update body sync bridges
      closeBodySync(currentEntry.path);
      await ensureBodySync(newPath);
      return { newPath };
    }

    if (newPath) {
      // Title changed but path didn't — return so caller can refresh UI
      return { newPath };
    }
  } catch (e) {
    uiStore.setError(e instanceof Error ? e.message : String(e));
  } finally {
    entryStore.setSaving(false);
  }
}
