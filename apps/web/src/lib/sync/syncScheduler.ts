/**
 * Host-side debounced sync scheduler.
 *
 * After file mutation events, waits for a quiet period (no new mutations)
 * then triggers a full Sync command on each linked workspace provider plugin.
 * The same sync path is reused for workspace startup, tab visibility resume,
 * and explicit manual sync requests from the footer.
 *
 * Works on both browser (WASM plugins via dispatchCommand) and Tauri (native
 * plugins via the backend API).
 */

import {
  getCurrentWorkspaceId,
  getWorkspaceProviderLinks,
} from "$lib/storage/localWorkspaceRegistry.svelte";
import {
  dispatchCommand,
  onPluginEventDispatched,
  type PluginEventObserver,
} from "$lib/plugins/browserPluginManager.svelte";
import { isTauri } from "$lib/backend/interface";

const SYNC_DEBOUNCE_MS = 3_000;

const FILE_MUTATION_EVENTS = new Set([
  "file_saved",
  "file_created",
  "file_deleted",
  "file_moved",
  "file_renamed",
]);

/** Tauri FileSystemEvent types that correspond to file mutations. */
const TAURI_FILE_MUTATION_TYPES = new Set([
  "FileCreated",
  "FileDeleted",
  "FileRenamed",
  "FileMoved",
  "ContentsChanged",
]);

let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let syncing = false;
let teardown: (() => void) | null = null;
let visibilityListenerInstalled = false;
let tauriFsEventSubId: number | undefined;

/**
 * Dispatch a sync command to a provider plugin, using the native backend API
 * on Tauri or the browser plugin manager on web.
 */
async function dispatchSyncCommand(
  pluginId: string,
): Promise<{ success: boolean; error?: string }> {
  if (isTauri()) {
    try {
      const { getBackend, createApi } = await import("$lib/backend");
      const backend = await getBackend();
      const api = createApi(backend);
      await api.executePluginCommand(pluginId, "Sync", {
        provider_id: pluginId,
      });
      return { success: true };
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      return { success: false, error: message };
    }
  }

  return await dispatchCommand(pluginId, "Sync", {
    provider_id: pluginId,
  });
}

async function triggerSync(): Promise<void> {
  const wsId = getCurrentWorkspaceId();
  if (!wsId) return;

  const links = getWorkspaceProviderLinks(wsId).filter((l) => l.syncEnabled);
  if (links.length === 0) return;

  syncing = true;
  try {
    await Promise.allSettled(
      links.map(async (link) => {
        try {
          const result = await dispatchSyncCommand(link.pluginId);
          if (!result.success) {
            console.warn("[syncScheduler] Sync failed:", link.pluginId, result.error);
          }
        } catch (e) {
          console.warn("[syncScheduler] Sync error:", link.pluginId, e);
        }
      }),
    );
  } finally {
    syncing = false;
  }
}

function scheduleSync(): void {
  if (debounceTimer !== null) {
    clearTimeout(debounceTimer);
  }
  debounceTimer = setTimeout(() => {
    debounceTimer = null;
    if (!syncing) {
      void triggerSync();
    }
  }, SYNC_DEBOUNCE_MS);
}

function handleVisibilityChange(): void {
  if (document.visibilityState !== "visible" || syncing) {
    return;
  }
  void triggerSync();
}

const observer: PluginEventObserver = (event) => {
  if (FILE_MUTATION_EVENTS.has(event.event_type)) {
    scheduleSync();
  }
};

/** Start listening for file events and scheduling sync pushes. */
export function startSyncScheduler(): void {
  if (teardown) return; // already running

  // Browser plugin events (web/WASM)
  teardown = onPluginEventDispatched(observer);

  // Tauri native file system events
  if (isTauri()) {
    void (async () => {
      try {
        const { getBackend } = await import("$lib/backend");
        const backend = await getBackend();
        if (backend.onFileSystemEvent) {
          tauriFsEventSubId = backend.onFileSystemEvent((event: { type: string }) => {
            if (TAURI_FILE_MUTATION_TYPES.has(event.type)) {
              scheduleSync();
            }
          });
        }
      } catch (e) {
        console.warn("[syncScheduler] Failed to subscribe to Tauri FS events:", e);
      }
    })();
  }

  if (!visibilityListenerInstalled) {
    document.addEventListener("visibilitychange", handleVisibilityChange);
    visibilityListenerInstalled = true;
  }
  void triggerSync();
}

/** Stop the scheduler and cancel any pending sync. */
export function stopSyncScheduler(): void {
  if (debounceTimer !== null) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
  teardown?.();
  teardown = null;

  // Unsubscribe from Tauri FS events
  if (tauriFsEventSubId !== undefined) {
    void (async () => {
      try {
        const { getBackend } = await import("$lib/backend");
        const backend = await getBackend();
        backend.offFileSystemEvent?.(tauriFsEventSubId!);
      } catch { /* cleanup best-effort */ }
    })();
    tauriFsEventSubId = undefined;
  }

  if (visibilityListenerInstalled) {
    document.removeEventListener("visibilitychange", handleVisibilityChange);
    visibilityListenerInstalled = false;
  }
}

/** Trigger an immediate manual sync for all linked providers in the current workspace. */
export async function runManualSyncNow(): Promise<void> {
  if (debounceTimer !== null) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
  if (syncing) {
    return;
  }
  await triggerSync();
}
