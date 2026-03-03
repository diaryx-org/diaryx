<script lang="ts">
  import { onMount } from "svelte";
  import type { Api } from "$lib/backend/api";
  import { getPluginStore } from "@/models/stores/pluginStore.svelte";

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
  const statusItems = $derived(pluginStore.statusBarItems);

  let dataByItemId = $state<Record<string, SyncStatusLike>>({});

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
    void refreshStatusItems();
    const intervalId = window.setInterval(() => {
      void refreshStatusItems();
    }, 3000);
    return () => window.clearInterval(intervalId);
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
</script>

<div class="flex items-center gap-2">
  {#each statusItems as item (item.pluginId + ':' + item.contribution.id)}
    <span
      class={`text-xs whitespace-nowrap ${stateClass(item.contribution.id)}`}
      title={itemTitle(item.contribution.id, item.contribution.label)}
    >
      {itemText(item.contribution.id, item.contribution.label)}
    </span>
  {/each}
</div>
