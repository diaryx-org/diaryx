/**
 * Host-side debounced sync scheduler.
 *
 * After file mutation events, waits for a quiet period (no new mutations)
 * then triggers a SyncPush command on each linked workspace provider plugin.
 * This keeps sync timing in the host — plugins just track dirty state and
 * execute sync when told.
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

async function triggerSyncPush(): Promise<void> {
  const wsId = getCurrentWorkspaceId();
  if (!wsId) return;

  const links = getWorkspaceProviderLinks(wsId).filter((l) => l.syncEnabled);
  if (links.length === 0) return;

  syncing = true;
  try {
    await Promise.allSettled(
      links.map(async (link) => {
        try {
          const result = await dispatchCommand(link.pluginId, "SyncPush", {
            provider_id: link.pluginId,
          });
          if (!result.success) {
            console.warn("[syncScheduler] SyncPush failed:", link.pluginId, result.error);
          }
        } catch (e) {
          console.warn("[syncScheduler] SyncPush error:", link.pluginId, e);
        }
      }),
    );
  } finally {
    syncing = false;
  }
}

function scheduleSyncPush(): void {
  if (debounceTimer !== null) {
    clearTimeout(debounceTimer);
  }
  debounceTimer = setTimeout(() => {
    debounceTimer = null;
    if (!syncing) {
      triggerSyncPush();
    }
  }, SYNC_DEBOUNCE_MS);
}

const observer: PluginEventObserver = (event) => {
  if (FILE_MUTATION_EVENTS.has(event.event_type)) {
    scheduleSyncPush();
  }
};

/** Start listening for file events and scheduling sync pushes. */
export function startSyncScheduler(): void {
  if (teardown) return; // already running
  teardown = onPluginEventDispatched(observer);
}

/** Stop the scheduler and cancel any pending sync. */
export function stopSyncScheduler(): void {
  if (debounceTimer !== null) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
  teardown?.();
  teardown = null;
}
