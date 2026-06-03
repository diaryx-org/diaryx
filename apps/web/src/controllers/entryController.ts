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


import type { EntryData, TreeNode, Api, CreateChildResult } from '../lib/backend';
import { getBackend } from '../lib/backend';
import type { JsonValue } from '../lib/backend/generated/serde_json/JsonValue';
import { entryStore, uiStore } from '../models/stores';
import {
  revokeBlobUrls,
  reverseBlobUrlsToAttachmentPaths,
} from '../models/services';
import { dispatchFileOpenedEvent } from '../lib/plugins/browserPluginManager.svelte';
import { toWorkspaceRelativePath } from '../lib/utils/path';

/**
 * Notify plugins that an entry was opened.
 */
async function notifyEntryOpened(path: string): Promise<void> {
  const backend = await getBackend();
  const syncPath = toWorkspaceRelativePath(backend.getWorkspacePath(), path);
  await dispatchFileOpenedEvent(syncPath);
}

type EditorMarkdownRef = {
  getMarkdown?: () => string | undefined;
  acknowledgeSavedContent?: (markdown: string) => void;
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
  _tree: TreeNode | null,
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

  // Non-blocking: notify plugins that entry was opened.
  try {
    await notifyEntryOpened(path);
  } catch (e) {
    console.warn('[EntryController] Plugin notification failed:', e);
  }
}

/**
 * Save an entry.
 *
 * @param api - API instance
 * @param currentEntry - The current entry being saved
 * @param editorRef - Reference to the editor component
 * @param rootIndexPath - Workspace root index path
 * @param detectH1Title - When true, detect H1→title sync (manual save/blur only, not auto-save)
 * @returns Object with newPath if H1 sync caused a rename
 */
export async function saveEntry(
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

    // Save to backend
    const newPath = await saveEntryWithRetry(api, currentEntry.path, markdown, rootIndexPath, detectH1Title);
    // Pre-acknowledge so the Editor's content-sync effect won't re-apply
    // the content we just read from it (which would reset the cursor).
    editorRef?.acknowledgeSavedContent?.(markdown);
    entryStore.setDisplayContent(markdown);
    entryStore.markClean();

    if (newPath && newPath !== currentEntry.path) {
      // H1 sync caused a rename
      await notifyEntryOpened(newPath);
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

/**
 * Create a child entry under a parent.
 *
 * @param api - API instance
 * @param parentPath - Path of the parent entry
 * @param onSuccess - Callback after successful creation
 * @returns The CreateChildResult with paths and conversion info, or null on failure
 */
export async function createChildEntry(
  api: Api,
  parentPath: string,
  onSuccess?: (result: CreateChildResult) => Promise<void>
): Promise<CreateChildResult | null> {
  try {
    const result = await api.createChildEntry(parentPath);

    // Ensure plugin notification is triggered for the new file
    await notifyEntryOpened(result.child_path);

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
 * Create a new entry at a specific path.
 *
 * @param api - API instance
 * @param path - Path for the new entry
 * @param options - Options including title and template
 * @param onSuccess - Callback after successful creation
 * @returns The path of the new entry, or null on failure
 */
export async function createEntry(
  api: Api,
  path: string,
  options: { title: string; template?: string; rootIndexPath?: string },
  onSuccess?: () => Promise<void>
): Promise<string | null> {
  try {
    const newPath = await api.createEntry(path, { ...options, rootIndexPath: options.rootIndexPath });

    // Ensure plugin notification is triggered for the new file
    await notifyEntryOpened(newPath);

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
 *
 * @param api - API instance
 * @param path - Path of the entry to delete
 * @param currentEntryPath - Path of the currently open entry (to clear if same)
 * @param onSuccess - Callback after successful deletion (e.g., refresh tree)
 * @returns True if deletion completed successfully
 */
export async function deleteEntry(
  api: Api,
  path: string,
  currentEntryPath: string | null,
  onSuccess?: () => Promise<void>
): Promise<boolean> {
  try {
    // Delete via Rust
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
  const newPath = await api.renameEntry(path, newFilename);

  // Notify plugins about new path
  await notifyEntryOpened(newPath);

  if (onSuccess) {
    await onSuccess();
  }

  return newPath;
}

/**
 * Duplicate an entry.
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

  // Notify plugins about new path
  await notifyEntryOpened(newPath);

  if (onSuccess) {
    await onSuccess();
  }

  return newPath;
}
