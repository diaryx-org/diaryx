<script lang="ts">
  import { onMount } from "svelte";
  import type { Api } from "$lib/backend/api";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";
  import { getWorkspaceStore } from "@/models/stores/workspaceStore.svelte";
  import { runManualSyncNow } from "$lib/sync/syncScheduler.svelte";

  interface Props {
    api: Api | null;
  }

  interface SyncStatusLike {
    state?: string;
    label?: string;
    detail?: string | null;
    progress?: { completed: number; total: number } | null;
  }

  let { api }: Props = $props();

  const pluginStore = getPluginStore();
  const workspaceStore = getWorkspaceStore();
  const statusItems = $derived(pluginStore.statusBarItems);

  let dataByItemId = $state<Record<string, SyncStatusLike>>({});
  let syncingItemIds = $state<Record<string, boolean>>({});

  async function refreshStatusItems() {
    if (!api) return;

    const next: Record<string, SyncStatusLike> = {};
    for (const item of statusItems) {
      const command = item.contribution.plugin_command;
      if (!command) continue;

      try {
        const result = await api.executePluginCommand(
          item.pluginId as unknown as string,
          command,
          null,
        );
        if (result && typeof result === "object") {
          next[item.contribution.id] = result as SyncStatusLike;
        }
      } catch {
        next[item.contribution.id] = {
          state: "error",
          label: item.contribution.label,
          detail: "Status unavailable",
          progress: null,
        };
      }
    }

    dataByItemId = next;
  }

  onMount(() => {
    // Initial fetch
    void refreshStatusItems();

    // Subscribe to filesystem events for event-driven updates instead of polling.
    // SyncStatusChanged / SyncProgress events are emitted by the sync plugin
    // when state actually changes, so we only re-fetch on real transitions.
    const backend = workspaceStore.backend;
    let fsEventSubId: number | undefined;
    if (backend?.onFileSystemEvent) {
      fsEventSubId = backend.onFileSystemEvent((event: any) => {
        if (
          event?.type === "SyncStatusChanged" ||
          event?.type === "SyncProgress"
        ) {
          void refreshStatusItems();
        }
      });
    }

    return () => {
      if (fsEventSubId !== undefined && backend?.offFileSystemEvent) {
        backend.offFileSystemEvent(fsEventSubId);
      }
    };
  });

  $effect(() => {
    statusItems;
    void refreshStatusItems();
  });

  function itemText(itemId: string, fallbackLabel: string): string {
    const status = dataByItemId[itemId];
    if (!status) return fallbackLabel;

    const base = status.label ?? fallbackLabel;
    if (status.progress && status.progress.total > 0) {
      return `${base} ${status.progress.completed}/${status.progress.total}`;
    }
    return base;
  }

  function itemTitle(itemId: string, fallbackLabel: string): string {
    const status = dataByItemId[itemId];
    if (!status) return fallbackLabel;

    const text = itemText(itemId, fallbackLabel);
    if (status.detail) {
      return `${text} - ${status.detail}`;
    }
    return text;
  }

  function stateClass(itemId: string): string {
    const state = dataByItemId[itemId]?.state;
    if (state === "error") return "text-destructive";
    if (state === "synced") return "text-green-600 dark:text-green-400";
    if (state === "syncing" || state === "connecting") {
      return "text-amber-600 dark:text-amber-400";
    }
    return "text-muted-foreground";
  }

  function isManualSyncItem(_pluginId: string, itemId: string): boolean {
    return itemId === "sync-status";
  }

  function itemAriaLabel(pluginId: string, itemId: string, fallbackLabel: string): string {
    if (isManualSyncItem(pluginId, itemId)) {
      return "Sync now";
    }
    return fallbackLabel;
  }

  async function handleItemClick(pluginId: string, itemId: string): Promise<void> {
    if (!isManualSyncItem(pluginId, itemId) || syncingItemIds[itemId]) {
      return;
    }

    syncingItemIds = { ...syncingItemIds, [itemId]: true };
    try {
      await runManualSyncNow();
      await refreshStatusItems();
    } finally {
      syncingItemIds = { ...syncingItemIds, [itemId]: false };
    }
  }
</script>

<div class="flex items-center gap-2">
  {#each statusItems as item (item.pluginId + ':' + item.contribution.id)}
    {#if isManualSyncItem(String(item.pluginId), item.contribution.id)}
      <button
        type="button"
        class={`text-xs whitespace-nowrap rounded px-1 py-0.5 transition-colors hover:bg-accent disabled:cursor-wait disabled:opacity-60 ${stateClass(item.contribution.id)}`}
        title={`${itemTitle(item.contribution.id, item.contribution.label)} - Click to sync now`}
        aria-label={itemAriaLabel(String(item.pluginId), item.contribution.id, item.contribution.label)}
        disabled={syncingItemIds[item.contribution.id] === true}
        onclick={() => void handleItemClick(String(item.pluginId), item.contribution.id)}
      >
        {itemText(item.contribution.id, item.contribution.label)}
      </button>
    {:else}
      <span
        class={`text-xs whitespace-nowrap ${stateClass(item.contribution.id)}`}
        title={itemTitle(item.contribution.id, item.contribution.label)}
      >
        {itemText(item.contribution.id, item.contribution.label)}
      </span>
    {/if}
  {/each}
</div>
