/**
 * Host-side debounced sync scheduler.
 *
 * After file mutation events, waits for a quiet period (no new mutations)
 * then triggers a full Sync command on each linked workspace provider plugin.
 * The same sync path is reused for workspace startup, tab visibility resume,
 * and explicit manual sync requests from the footer.
 */

import {
  getCurrentWorkspaceId,
  getWorkspaceProviderLinks,
} from "$lib/storage/localWorkspaceRegistry.svelte";
import { dispatchCommand } from "$lib/plugins/browserPluginManager.svelte";
import {
  onPluginEventDispatched,
  type PluginEventObserver,
} from "$lib/plugins/browserPluginManager.svelte";

const SYNC_DEBOUNCE_MS = 3_000;

const FILE_MUTATION_EVENTS = new Set([
  "file_saved",
  "file_created",
  "file_deleted",
  "file_moved",
  "file_renamed",
]);

let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let syncing = false;
let teardown: (() => void) | null = null;
let visibilityListenerInstalled = false;

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
          const result = await dispatchCommand(link.pluginId, "Sync", {
            provider_id: link.pluginId,
          });
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
  teardown = onPluginEventDispatched(observer);
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
