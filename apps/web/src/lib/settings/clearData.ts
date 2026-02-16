/**
 * Shared utilities for clearing all local data.
 *
 * Used by ClearDataSettings (manual clear) and AccountSettings (post-logout prompt).
 */

import { getLocalWorkspaces } from "$lib/storage/localWorkspaceRegistry";

/**
 * Delete a single workspace's OPFS directory by its ID and/or name.
 * Tries both since directories may be UUID-named (legacy) or name-based (current).
 */
export async function deleteLocalWorkspaceData(workspaceId: string, workspaceName?: string): Promise<void> {
  if (!navigator.storage?.getDirectory) return;
  const root = await navigator.storage.getDirectory();

  // Try deleting by ID (legacy UUID-named dirs)
  try {
    await root.removeEntry(workspaceId, { recursive: true });
    console.log(`[ClearData] Deleted OPFS workspace directory: ${workspaceId}`);
  } catch (e) {
    if ((e as Error).name !== "NotFoundError") {
      console.warn(`[ClearData] Failed to delete OPFS workspace ${workspaceId}:`, e);
    }
  }

  // Also try deleting by name (current name-based dirs)
  if (workspaceName && workspaceName !== workspaceId) {
    try {
      await root.removeEntry(workspaceName, { recursive: true });
      console.log(`[ClearData] Deleted OPFS workspace directory by name: ${workspaceName}`);
    } catch (e) {
      if ((e as Error).name !== "NotFoundError") {
        console.warn(`[ClearData] Failed to delete OPFS workspace ${workspaceName}:`, e);
      }
    }
  }
}

/**
 * Clear all OPFS data by enumerating and deleting all entries in the OPFS root.
 * This catches any orphaned directories (UUID-named, name-based, legacy, etc.)
 */
export async function clearOpfs(): Promise<void> {
  if (!navigator.storage?.getDirectory) return;

  const root = await navigator.storage.getDirectory();

  // Enumerate all entries in the OPFS root and delete them
  try {
    for await (const [name] of (root as any).entries()) {
      try {
        await root.removeEntry(name, { recursive: true });
        console.log(`[ClearData] Deleted OPFS entry: ${name}`);
      } catch (e) {
        if ((e as Error).name !== "NotFoundError") {
          console.warn(`[ClearData] Failed to delete OPFS ${name}:`, e);
        }
      }
    }
  } catch (e) {
    console.warn("[ClearData] Failed to enumerate OPFS root:", e);

    // Fallback: try known directory names
    const workspaceName = localStorage.getItem("diaryx-workspace-name") || "My Journal";
    const dirsToDelete = [...new Set(["diaryx", ".diaryx", "guest", workspaceName])];
    for (const ws of getLocalWorkspaces()) {
      dirsToDelete.push(ws.id);
      dirsToDelete.push(ws.name);
    }
    for (const dir of dirsToDelete) {
      try {
        await root.removeEntry(dir, { recursive: true });
        console.log(`[ClearData] Deleted OPFS directory: ${dir}`);
      } catch (e) {
        if ((e as Error).name !== "NotFoundError") {
          console.warn(`[ClearData] Failed to delete OPFS ${dir}:`, e);
        }
      }
    }
  }
}

/**
 * Clear all IndexedDB databases used by the app.
 */
export async function clearIndexedDb(): Promise<void> {
  const dbNames = [
    "diaryx-fs-handles",
  ];

  for (const name of dbNames) {
    try {
      await new Promise<void>((resolve, reject) => {
        const request = indexedDB.deleteDatabase(name);
        request.onsuccess = () => resolve();
        request.onerror = () => reject(request.error);
        request.onblocked = () => {
          console.warn(`[ClearData] Database ${name} is blocked`);
          resolve();
        };
      });
    } catch (e) {
      console.warn(`[ClearData] Failed to delete IndexedDB ${name}:`, e);
    }
  }
}

/**
 * Clear all localStorage keys used by the app.
 */
export function clearLocalStorage(): void {
  const keysToRemove = [
    "diaryx-storage-type",
    "diaryx_auth_token",
    "diaryx_sync_server_url",
    "diaryx_user",
    "diaryx-sync-server",
    "diaryx-show-unlinked-files",
    "diaryx-show-hidden-files",
    "diaryx-show-editor-title",
    "diaryx-show-editor-path",
    "diaryx-readable-line-length",
    "diaryx-focus-mode",
    "diaryx-device-id",
    "diaryx-device-name",
    "diaryx-theme",
    "diaryx-enable-spoilers",
    "diaryx_s3_access_key",
    "diaryx_s3_secret_key",
    "diaryx_s3_config",
    "diaryx_gd_refresh_token",
    "diaryx_gd_folder_id",
    "diaryx_gd_client_id",
    "diaryx_gd_client_secret",
    "diaryx_sync_enabled",
    "diaryx-workspace-name",
  ];

  for (const key of keysToRemove) {
    try {
      localStorage.removeItem(key);
    } catch (e) {
      console.warn(`[ClearData] Failed to remove localStorage key ${key}:`, e);
    }
  }
}

/**
 * Clear all local data (OPFS + IndexedDB + localStorage) and reload.
 */
export async function clearAllLocalData(): Promise<void> {
  await clearOpfs();
  await clearIndexedDb();
  clearLocalStorage();
  await new Promise((resolve) => setTimeout(resolve, 100));
  window.location.reload();
}
