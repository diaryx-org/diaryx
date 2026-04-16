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

// ---------------------------------------------------------------------------
// Reactive sync state (Svelte 5 runes — module-level $state)
// ---------------------------------------------------------------------------

/** Reactive sync-in-progress flag. Use getSyncState() to read from components. */
let syncing = $state(false);

/** Accessor for the reactive sync state. */
export function getSyncState(): { readonly syncing: boolean } {
  return {
    get syncing() {
      return syncing;
    },
  };
}

let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let teardown: (() => void) | null = null;
let visibilityListenerInstalled = false;
let onlineListenerInstalled = false;
let syncRequestedAfterCurrentRun = false;
let tauriFsEventTeardown: (() => void) | null = null;
let schedulerLifecycleToken = 0;

/**
 * Dispatch a sync command to a provider plugin, using the native backend API
 * on Tauri or the browser plugin manager on web.
 */
async function dispatchSyncCommand(
  pluginId: string,
): Promise<{ success: boolean; error?: string }> {
  let result: { success: boolean; error?: string; deferred?: string[] };

  if (isTauri()) {
    try {
      const { getBackend, createApi } = await import("$lib/backend");
      const backend = await getBackend();
      const api = createApi(backend);
      const raw = await api.executePluginCommand(pluginId, "Sync", {
        provider_id: pluginId,
      });
      const deferred = (raw as Record<string, unknown> | null)?.deferred as
        | string[]
        | undefined;
      result = { success: true, deferred };
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      result = { success: false, error: message };
    }
  } else {
    const cmdResult = await dispatchCommand(pluginId, "Sync", {
      provider_id: pluginId,
    });
    const deferred = (cmdResult.data as Record<string, unknown> | null)
      ?.deferred as string[] | undefined;
    result = { ...cmdResult, deferred };
  }

  // Enqueue any deferred (non-markdown) files for background download.
  if (result.deferred && result.deferred.length > 0) {
    try {
      const { getServerUrl, getToken } = await import("$lib/auth");
      const { getBackend, createApi } = await import("$lib/backend");
      const { initDeferredQueue, enqueueDeferredFiles } = await import(
        "$lib/sync/deferredFileQueue"
      );
      const serverUrl = getServerUrl();
      const token = getToken();
      if (serverUrl && token) {
        const backend = await getBackend();
        initDeferredQueue(createApi(backend), serverUrl, token);
        // Derive nsId from the provider link's remoteWorkspaceId.
        const wsId = getCurrentWorkspaceId();
        const link = wsId
          ? getWorkspaceProviderLinks(wsId).find((l) => l.pluginId === pluginId)
          : null;
        const nsId = link?.remoteWorkspaceId;
        if (nsId) {
          enqueueDeferredFiles(nsId, result.deferred);
        }
      }
    } catch (e) {
      console.warn("[syncScheduler] Failed to enqueue deferred files:", e);
    }
  }

  return result;
}

async function triggerSync(): Promise<void> {
  if (syncing) {
    syncRequestedAfterCurrentRun = true;
    return;
  }

  while (true) {
    syncRequestedAfterCurrentRun = false;

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

    if (!syncRequestedAfterCurrentRun) {
      return;
    }
  }
}

function requestSyncNow(): void {
  void triggerSync();
}

function scheduleSync(): void {
  if (debounceTimer !== null) {
    clearTimeout(debounceTimer);
  }
  debounceTimer = setTimeout(() => {
    debounceTimer = null;
    requestSyncNow();
  }, SYNC_DEBOUNCE_MS);
}

function handleVisibilityChange(): void {
  if (document.visibilityState !== "visible") {
    return;
  }
  requestSyncNow();
}

function handleOnline(): void {
  requestSyncNow();
}

const observer: PluginEventObserver = (event) => {
  if (FILE_MUTATION_EVENTS.has(event.event_type)) {
    scheduleSync();
  }
};

/** Start listening for file events and scheduling sync pushes. */
export function startSyncScheduler(): void {
  if (teardown) return; // already running
  const lifecycleToken = ++schedulerLifecycleToken;

  // Browser plugin events (web/WASM)
  teardown = onPluginEventDispatched(observer);

  // Tauri native file system events
  if (isTauri()) {
    void (async () => {
      try {
        const { getBackend } = await import("$lib/backend");
        const backend = await getBackend();
        if (schedulerLifecycleToken !== lifecycleToken || !teardown) {
          return;
        }
        if (backend.onFileSystemEvent) {
          const subscriptionId = backend.onFileSystemEvent((event: { type: string }) => {
            if (TAURI_FILE_MUTATION_TYPES.has(event.type)) {
              scheduleSync();
            }
          });
          if (schedulerLifecycleToken !== lifecycleToken || !teardown) {
            backend.offFileSystemEvent?.(subscriptionId);
            return;
          }
          tauriFsEventTeardown = () => {
            backend.offFileSystemEvent?.(subscriptionId);
          };
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
  if (!onlineListenerInstalled) {
    window.addEventListener("online", handleOnline);
    onlineListenerInstalled = true;
  }
  requestSyncNow();
}

/** Stop the scheduler and cancel any pending sync. */
export function stopSyncScheduler(): void {
  schedulerLifecycleToken += 1;
  syncRequestedAfterCurrentRun = false;
  if (debounceTimer !== null) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
  teardown?.();
  teardown = null;

  tauriFsEventTeardown?.();
  tauriFsEventTeardown = null;

  if (visibilityListenerInstalled) {
    document.removeEventListener("visibilitychange", handleVisibilityChange);
    visibilityListenerInstalled = false;
  }
  if (onlineListenerInstalled) {
    window.removeEventListener("online", handleOnline);
    onlineListenerInstalled = false;
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
