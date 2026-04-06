<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import {
    RefreshCw,
    CloudDownload,
    CloudUpload,
    Cloud,
    CloudOff,
    Loader2,
    AlertCircle,
    CheckCircle2,
  } from "@lucide/svelte";
  import { collaborationStore } from "@/models/stores/collaborationStore.svelte";
  import { getSyncState } from "$lib/sync/syncScheduler.svelte";

  interface Props {
    open: boolean;
    onSync: () => Promise<void>;
    onPull: () => Promise<void>;
    onPush: () => Promise<void>;
    onRefreshStatus: () => Promise<void>;
  }

  let { open = $bindable(), onSync, onPull, onPush, onRefreshStatus }: Props =
    $props();

  const syncState = getSyncState();

  let refreshing = $state(false);

  // Fetch current status from the plugin whenever the dialog opens.
  $effect(() => {
    if (open) {
      void handleRefresh();
    }
  });

  async function handleRefresh() {
    refreshing = true;
    try {
      await onRefreshStatus();
    } finally {
      refreshing = false;
    }
  }

  async function handleSync() {
    await onSync();
  }

  async function handlePull() {
    await onPull();
  }

  async function handlePush() {
    await onPush();
  }

  const statusLabel = $derived.by(() => {
    const s = collaborationStore.effectiveSyncStatus;
    if (s === "synced") return "Synced";
    if (s === "syncing") return "Syncing...";
    if (s === "connecting") return "Connecting...";
    if (s === "error") return "Error";
    if (s === "not_configured") return "Not configured";
    return "Idle";
  });

  const statusColor = $derived.by(() => {
    const s = collaborationStore.effectiveSyncStatus;
    if (s === "synced") return "text-green-600 dark:text-green-400";
    if (s === "syncing" || s === "connecting")
      return "text-amber-600 dark:text-amber-400";
    if (s === "error") return "text-destructive";
    return "text-muted-foreground";
  });

  const lastSyncLabel = $derived.by(() => {
    // The collaborationStore doesn't track last_sync_at directly, but we
    // can derive it from the store's sync status transitions.  For now,
    // show the error message if present, otherwise just the status.
    if (collaborationStore.syncError) return collaborationStore.syncError;
    return null;
  });

  const progress = $derived(collaborationStore.syncProgress);

  const busy = $derived(syncState.syncing);
</script>

<Dialog.Root bind:open>
  <Dialog.Content
    class="flex w-[calc(100%-2rem)] max-w-[420px] flex-col gap-4 p-6"
  >
    <Dialog.Header>
      <Dialog.Title>Sync</Dialog.Title>
      <Dialog.Description class="sr-only"
        >Workspace sync status and actions</Dialog.Description
      >
    </Dialog.Header>

    <!-- Status display -->
    <div class="flex items-center gap-3 rounded-lg border p-4">
      <div class="shrink-0">
        {#if collaborationStore.effectiveSyncStatus === "syncing" || collaborationStore.effectiveSyncStatus === "connecting"}
          <Loader2 class="size-5 animate-spin text-amber-500" />
        {:else if collaborationStore.effectiveSyncStatus === "error"}
          <AlertCircle class="size-5 text-destructive" />
        {:else if collaborationStore.effectiveSyncStatus === "synced"}
          <CheckCircle2 class="size-5 text-green-500" />
        {:else if collaborationStore.serverOffline}
          <CloudOff class="size-5 text-muted-foreground" />
        {:else}
          <Cloud class="size-5 text-muted-foreground" />
        {/if}
      </div>
      <div class="flex-1 min-w-0">
        <p class={`text-sm font-medium ${statusColor}`}>{statusLabel}</p>
        {#if progress && progress.total > 0}
          <p class="text-xs text-muted-foreground">
            {progress.completed}/{progress.total} files
          </p>
        {/if}
        {#if lastSyncLabel}
          <p class="text-xs text-muted-foreground truncate">{lastSyncLabel}</p>
        {/if}
      </div>
      <Button
        variant="ghost"
        size="icon"
        class="size-8 shrink-0"
        onclick={handleRefresh}
        disabled={refreshing}
        aria-label="Refresh status"
      >
        <RefreshCw
          class={`size-4 ${refreshing ? "animate-spin" : ""}`}
        />
      </Button>
    </div>

    <!-- Action buttons -->
    <div class="flex gap-2">
      <Button
        variant="default"
        class="flex-1"
        onclick={handleSync}
        disabled={busy}
      >
        {#if busy}
          <Loader2 class="size-4 mr-2 animate-spin" />
        {:else}
          <RefreshCw class="size-4 mr-2" />
        {/if}
        Sync
      </Button>
      <Button
        variant="outline"
        class="flex-1"
        onclick={handlePull}
        disabled={busy}
      >
        <CloudDownload class="size-4 mr-2" />
        Pull
      </Button>
      <Button
        variant="outline"
        class="flex-1"
        onclick={handlePush}
        disabled={busy}
      >
        <CloudUpload class="size-4 mr-2" />
        Push
      </Button>
    </div>
  </Dialog.Content>
</Dialog.Root>
